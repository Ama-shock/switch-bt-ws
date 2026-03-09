//! controller.rs — コントローラーサブプロセスの管理。
//!
//! BTStack はグローバルな C 状態を持つため、1 プロセスで複数インスタンスを
//! 実行できない。このモジュールはドングルごとにワーカーサブプロセスを生成し、
//! stdin/stdout パイプ経由で JSON 行 IPC を行う。
//!
//! 主要な型:
//!   - `ControllerManager` — 複数ワーカーを統括する非同期マネージャー
//!   - `ControllerHandle`  — 個々のワーカーへのインターフェース
//!   - `ControllerInfo`    — REST API に返すコントローラー情報

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{Context, Result};
use serde::Serialize;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::{broadcast, Mutex, RwLock};

use crate::ipc::{WorkerCommand, WorkerEvent};

// ---------------------------------------------------------------------------
// 公開型
// ---------------------------------------------------------------------------

/// REST API に返すコントローラー情報。
#[derive(Debug, Clone, Serialize)]
pub struct ControllerInfo {
    /// コントローラー ID（0 始まりの連番）
    pub id: u32,
    /// USB ベンダー ID（小文字16進数）
    pub vid: String,
    /// USB プロダクト ID（小文字16進数）
    pub pid: String,
    /// 同一 VID/PID のデバイスが複数ある場合のインスタンス番号
    pub instance: u32,
    /// Switch が接続されているか
    pub paired: bool,
    /// Rumble 要求が来ているか
    pub rumble: bool,
    /// ペアリングループ中か
    pub syncing: bool,
    /// Switch が割り当てたプレイヤー番号（1〜4）。未割当なら 0。
    pub player: u8,
}

/// コントローラーの状態（内部用）。
#[derive(Debug, Clone)]
struct ControllerState {
    vid: u16,
    pid: u16,
    instance: u32,
    paired: bool,
    rumble: bool,
    syncing: bool,
    player: u8,
}

/// 個々のワーカープロセスへのハンドル。
pub struct ControllerHandle {
    id: u32,
    state: Arc<RwLock<ControllerState>>,
    stdin_tx: tokio::sync::mpsc::UnboundedSender<WorkerCommand>,
    /// Status ブロードキャスト受信用チャンネル
    status_tx: broadcast::Sender<WorkerEvent>,
}

impl ControllerHandle {
    /// コントローラー ID を返す。
    pub fn id(&self) -> u32 {
        self.id
    }

    /// コマンドをワーカーへ送信する。
    pub fn send(&self, cmd: WorkerCommand) {
        let _ = self.stdin_tx.send(cmd);
    }

    /// 現在のコントローラー情報を返す。
    pub async fn info(&self) -> ControllerInfo {
        let state = self.state.read().await;
        ControllerInfo {
            id: self.id,
            vid: format!("{:04x}", state.vid),
            pid: format!("{:04x}", state.pid),
            instance: state.instance,
            paired: state.paired,
            rumble: state.rumble,
            syncing: state.syncing,
            player: state.player,
        }
    }

    /// Status ブロードキャストの受信端を返す。
    pub fn subscribe_status(&self) -> broadcast::Receiver<WorkerEvent> {
        self.status_tx.subscribe()
    }
}

// ---------------------------------------------------------------------------
// ControllerManager
// ---------------------------------------------------------------------------

/// 複数のワーカーを管理するマネージャー。
pub struct ControllerManager {
    /// id → ControllerHandle
    controllers: Arc<RwLock<HashMap<u32, Arc<ControllerHandle>>>>,
    next_id: Mutex<u32>,
}

impl ControllerManager {
    pub fn new() -> Self {
        Self {
            controllers: Arc::new(RwLock::new(HashMap::new())),
            next_id: Mutex::new(0),
        }
    }

    /// 新しいコントローラーワーカーを起動して登録する。
    pub async fn add(&self, vid: u16, pid: u16, instance: u32) -> Result<u32> {
        let id = {
            let mut n = self.next_id.lock().await;
            let id = *n;
            *n += 1;
            id
        };

        let exe = std::env::current_exe().context("実行ファイルパスの取得に失敗")?;

        // ワーカーサブプロセスを起動
        let mut child: Child = Command::new(&exe)
            .arg("--worker")
            .arg(id.to_string())
            .arg(format!("{vid:04x}"))
            .arg(format!("{pid:04x}"))
            .arg(instance.to_string())
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::inherit())
            .spawn()
            .with_context(|| format!("ワーカープロセスの起動に失敗: id={id}"))?;

        let child_stdin: ChildStdin = child.stdin.take().context("stdin の取得に失敗")?;
        let child_stdout: ChildStdout = child.stdout.take().context("stdout の取得に失敗")?;

        let state = Arc::new(RwLock::new(ControllerState {
            vid,
            pid,
            instance,
            paired: false,
            rumble: false,
            syncing: false,
            player: 0,
        }));

        let (stdin_tx, stdin_rx) = tokio::sync::mpsc::unbounded_channel::<WorkerCommand>();
        let (status_tx, _) = broadcast::channel::<WorkerEvent>(32);

        let handle = Arc::new(ControllerHandle {
            id,
            state: Arc::clone(&state),
            stdin_tx,
            status_tx: status_tx.clone(),
        });

        // stdin 書き込みタスク
        let state_clone = Arc::clone(&state);
        tokio::spawn(stdin_writer(child_stdin, stdin_rx));

        // stdout 読み取りタスク（イベント処理）
        tokio::spawn(stdout_reader(child_stdout, state_clone, status_tx, id));

        // プロセス終了監視タスク
        let controllers = Arc::clone(&self.controllers);
        tokio::spawn(async move {
            let _ = child.wait().await;
            tracing::warn!("ワーカープロセス id={id} が終了しました");
            controllers.write().await.remove(&id);
        });

        self.controllers.write().await.insert(id, handle);
        tracing::info!("コントローラー id={id} を登録しました (vid={vid:04x} pid={pid:04x} inst={instance})");

        Ok(id)
    }

    /// コントローラーを停止して登録解除する。
    pub async fn remove(&self, id: u32) -> Result<()> {
        let handle = self
            .controllers
            .read()
            .await
            .get(&id)
            .cloned()
            .with_context(|| format!("コントローラー id={id} が見つかりません"))?;
        handle.send(WorkerCommand::Shutdown);
        Ok(())
    }

    /// 全コントローラーの情報リストを返す。
    pub async fn list(&self) -> Vec<ControllerInfo> {
        let controllers = self.controllers.read().await;
        let mut list = Vec::with_capacity(controllers.len());
        for handle in controllers.values() {
            list.push(handle.info().await);
        }
        list.sort_by_key(|c| c.id);
        list
    }

    /// 特定のコントローラーハンドルを返す。
    pub async fn get(&self, id: u32) -> Option<Arc<ControllerHandle>> {
        self.controllers.read().await.get(&id).cloned()
    }
}

// ---------------------------------------------------------------------------
// 非同期ヘルパータスク
// ---------------------------------------------------------------------------

/// ワーカーの stdin へ JSON コマンドを書き込むタスク。
async fn stdin_writer(
    mut stdin: ChildStdin,
    mut rx: tokio::sync::mpsc::UnboundedReceiver<WorkerCommand>,
) {
    while let Some(cmd) = rx.recv().await {
        let mut line = match serde_json::to_string(&cmd) {
            Ok(s) => s,
            Err(e) => {
                tracing::error!("WorkerCommand シリアライズエラー: {e}");
                continue;
            }
        };
        line.push('\n');
        if stdin.write_all(line.as_bytes()).await.is_err() {
            break;
        }
    }
}

/// ワーカーの stdout から JSON イベントを読み取るタスク。
async fn stdout_reader(
    stdout: ChildStdout,
    state: Arc<RwLock<ControllerState>>,
    status_tx: broadcast::Sender<WorkerEvent>,
    id: u32,
) {
    let mut reader = BufReader::new(stdout).lines();
    while let Ok(Some(line)) = reader.next_line().await {
        let line = line.trim().to_string();
        if line.is_empty() || !line.starts_with('{') {
            continue;
        }
        match serde_json::from_str::<WorkerEvent>(&line) {
            Ok(event) => {
                if let WorkerEvent::Status { paired, rumble, syncing, player } = &event {
                    let mut s = state.write().await;
                    s.paired = *paired;
                    s.rumble = *rumble;
                    s.syncing = *syncing;
                    s.player = *player;
                }
                let _ = status_tx.send(event);
            }
            Err(e) => {
                tracing::warn!("[controller id={id}] JSON パースエラー: {e}: {line}");
            }
        }
    }
}
