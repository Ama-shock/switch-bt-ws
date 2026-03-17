//! ipc.rs — サーバー ↔ ワーカー間 IPC プロトコル定義。
//!
//! サーバーとワーカーサブプロセスは stdin/stdout を通じて JSON 行（NDJSON）で通信する。
//! 各メッセージは 1 行の JSON 文字列で表現され、末尾に改行 (`\n`) が付く。
//!
//! データの流れ:
//!   サーバー → ワーカー stdin  : `WorkerCommand`
//!   ワーカー stdout → サーバー : `WorkerEvent`

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// サーバー → ワーカー
// ---------------------------------------------------------------------------

/// サーバーからワーカーへ送るコマンド。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum WorkerCommand {
    /// ボタン状態を送信する。`button_status` は btkeyLib のビットマップ。
    Button { button_status: u32 },
    /// 右スティックの状態を送信する。h/v は 0–4095（中立 = 2048）。
    StickR { h: u32, v: u32 },
    /// 左スティックの状態を送信する。h/v は 0–4095（中立 = 2048）。
    StickL { h: u32, v: u32 },
    /// ジャイロデータを送信する。
    Gyro { g1: i16, g2: i16, g3: i16 },
    /// 加速度データを送信する。
    Accel { x: i16, y: i16, z: i16 },
    /// コントローラーカラーを設定する。
    PadColor { pad: u32, btn: u32, lg: u32, rg: u32 },
    /// Rumble 監視キーを登録する。
    RumbleRegister { key: u32 },
    /// Amiibo データを送信する（ファイルパス）。
    Amiibo { path: String },
    /// HCI を再起動して再接続を試みる。リンクキーがあればインポートしてから再接続。
    Reconnect {
        #[serde(default)]
        link_keys: Option<String>,
    },
    /// リンクキー全削除 + HCI リセット（シンクロボタン1回押し相当）。
    Sync,
    /// ペアリングループ開始（接続されるまでシンクロを繰り返す）。
    SyncStart,
    /// ペアリングループ停止。
    SyncStop,
    /// Switch との HID 接続を切断する。
    Disconnect,
    /// リンクキーをインポートする（base64 エンコード）。
    SetLinkKeys { data: String },
    /// リンクキーのエクスポートを要求する。
    GetLinkKeys,
    /// ワーカーをシャットダウンする。
    Shutdown,
}

// ---------------------------------------------------------------------------
// ワーカー → サーバー
// ---------------------------------------------------------------------------

/// ワーカーからサーバーへ送るイベント。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum WorkerEvent {
    /// ワーカーの準備が完了した（BTStack 初期化済み）。
    Ready,
    /// 定期的なステータス更新。
    Status { paired: bool, rumble: bool, rumble_left: u8, rumble_right: u8, syncing: bool, player: u8 },
    /// ワーカーがシャットダウンした。
    Shutdown,
    /// リンクキーデータ（base64 エンコード）。ペアリング成功時に自動送信。
    LinkKeys { data: String },
    /// エラーが発生した。
    Error { message: String },
}
