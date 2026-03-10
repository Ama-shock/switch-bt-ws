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

use crate::driver;
use crate::ipc::{WorkerCommand, WorkerEvent};

// ---------------------------------------------------------------------------
// グローバルイベント（全体状態同期 WS 用）
// ---------------------------------------------------------------------------

/// 全体状態同期用のイベント。グローバル WS ハンドラに通知される。
#[derive(Debug, Clone)]
pub enum GlobalEvent {
    /// コントローラーリストが変化した（追加・削除）。
    ControllersChanged,
    /// デバイスリストが変化した（ドライバ操作後）。
    DevicesChanged,
    /// 個別コントローラーの状態が変化した。
    ControllerStatus {
        id: u32,
        paired: bool,
        rumble: bool,
        syncing: bool,
        player: u8,
    },
    /// コントローラーからリンクキーが送信された。
    ControllerLinkKeys {
        id: u32,
        vid: String,
        pid: String,
        instance: u32,
        data: String,
    },
}

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
    /// BTStack メモリ上のリンクキー（base64）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub link_keys: Option<String>,
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
    link_keys: Option<String>,
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
            link_keys: state.link_keys.clone(),
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
    /// グローバルイベントブロードキャスト
    global_tx: broadcast::Sender<GlobalEvent>,
    /// デバイスリストのキャッシュ
    cached_devices: RwLock<Vec<driver::BtUsbDevice>>,
}

impl ControllerManager {
    pub fn new() -> Self {
        let (global_tx, _) = broadcast::channel::<GlobalEvent>(64);
        Self {
            controllers: Arc::new(RwLock::new(HashMap::new())),
            next_id: Mutex::new(0),
            global_tx,
            cached_devices: RwLock::new(Vec::new()),
        }
    }

    /// グローバルイベントの受信端を返す。
    pub fn subscribe_global(&self) -> broadcast::Receiver<GlobalEvent> {
        self.global_tx.subscribe()
    }

    /// キャッシュされたデバイスリストを返す。
    pub async fn get_cached_devices(&self) -> Vec<driver::BtUsbDevice> {
        self.cached_devices.read().await.clone()
    }

    /// デバイスリストのキャッシュを更新する。
    pub async fn set_cached_devices(&self, devices: Vec<driver::BtUsbDevice>) {
        *self.cached_devices.write().await = devices;
    }

    /// デバイスリストを再スキャンしてキャッシュを更新し、全クライアントに通知する。
    pub async fn refresh_and_notify_devices(&self) {
        let devices = driver::list_bt_usb_devices().await.unwrap_or_default();
        *self.cached_devices.write().await = devices;
        let _ = self.global_tx.send(GlobalEvent::DevicesChanged);
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
            link_keys: None,
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
        tokio::spawn(stdout_reader(
            child_stdout,
            state_clone,
            status_tx,
            self.global_tx.clone(),
            id,
        ));

        // プロセス終了監視タスク
        let controllers = Arc::clone(&self.controllers);
        let global_tx = self.global_tx.clone();
        tokio::spawn(async move {
            let _ = child.wait().await;
            tracing::warn!("ワーカープロセス id={id} が終了しました");
            controllers.write().await.remove(&id);
            let _ = global_tx.send(GlobalEvent::ControllersChanged);
        });

        self.controllers.write().await.insert(id, handle);
        tracing::info!("コントローラー id={id} を登録しました (vid={vid:04x} pid={pid:04x} inst={instance})");
        let _ = self.global_tx.send(GlobalEvent::ControllersChanged);

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
    global_tx: broadcast::Sender<GlobalEvent>,
    id: u32,
) {
    let mut reader = BufReader::new(stdout).lines();
    // 状態変化のみグローバルに通知するための前回値
    let mut prev_paired = false;
    let mut prev_syncing = false;
    let mut prev_player = 0u8;

    while let Ok(Some(line)) = reader.next_line().await {
        let line = line.trim().to_string();
        if line.is_empty() || !line.starts_with('{') {
            continue;
        }
        match serde_json::from_str::<WorkerEvent>(&line) {
            Ok(event) => {
                match &event {
                    WorkerEvent::Status { paired, rumble, syncing, player } => {
                        let mut s = state.write().await;
                        s.paired = *paired;
                        s.rumble = *rumble;
                        s.syncing = *syncing;
                        s.player = *player;

                        // 意味のある変化時のみグローバルイベントを送信
                        if *paired != prev_paired || *syncing != prev_syncing || *player != prev_player {
                            let _ = global_tx.send(GlobalEvent::ControllerStatus {
                                id,
                                paired: *paired,
                                rumble: *rumble,
                                syncing: *syncing,
                                player: *player,
                            });
                            prev_paired = *paired;
                            prev_syncing = *syncing;
                            prev_player = *player;
                        }
                    }
                    WorkerEvent::LinkKeys { data } => {
                        let mut s = state.write().await;
                        s.link_keys = Some(data.clone());
                        let _ = global_tx.send(GlobalEvent::ControllerLinkKeys {
                            id,
                            vid: format!("{:04x}", s.vid),
                            pid: format!("{:04x}", s.pid),
                            instance: s.instance,
                            data: data.clone(),
                        });
                    }
                    _ => {}
                }
                let _ = status_tx.send(event);
            }
            Err(e) => {
                tracing::warn!("[controller id={id}] JSON パースエラー: {e}: {line}");
            }
        }
    }
}
