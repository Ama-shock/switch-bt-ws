//! WebSocket メッセージ型（JSON / serde）。
//!
//! ## クライアント → サーバー（`ClientMessage`）
//!
//! ```json
//! // ゲームパッド入力（Web Gamepad API のポーリングループから毎フレーム送信）
//! {
//!   "type": "gamepad_state",
//!   "buttons": [0.0, 1.0, ...],         // ボタン値（0.0〜1.0）、最大 20 個
//!   "axes":    [-0.5, 0.3, 0.0, -1.0]   // [左X, 左Y, 右X, 右Y]（-1.0〜1.0）
//! }
//!
//! // コントローラーカラー変更（0x00RRGGBB）
//! {
//!   "type": "set_color",
//!   "pad_color": 16711680,
//!   "button_color": 0,
//!   "left_grip_color": 0,
//!   "right_grip_color": 0
//! }
//!
//! // Amiibo 送信（サーバープロセスからアクセス可能なパス）
//! { "type": "send_amiibo", "path": "C:\\amiibo\\pikachu.bin" }
//!
//! // モーションセンサー（Web DeviceMotion API のデータ、任意）
//! {
//!   "type": "motion",
//!   "gyro":  [0, 0, 0],
//!   "accel": [100, 100, 100]
//! }
//! ```
//!
//! ## サーバー → クライアント（`ServerMessage`）
//!
//! ```json
//! { "type": "status", "paired": true, "rumble": false }
//! { "type": "error",  "message": "..." }
//! ```

use serde::{Deserialize, Deserializer, Serialize};

// ---------------------------------------------------------------------------
// クライアント → サーバー
// ---------------------------------------------------------------------------

/// JSON 値（bool / number）を f32 に変換するカスタムデシリアライザ。
/// クライアントが `buttons` を `[false, true, ...]` で送る場合に対応する。
fn deserialize_flexible_f32_vec<'de, D>(deserializer: D) -> Result<Vec<f32>, D::Error>
where
    D: Deserializer<'de>,
{
    let values: Vec<serde_json::Value> = Vec::deserialize(deserializer)?;
    Ok(values
        .into_iter()
        .map(|v| match v {
            serde_json::Value::Bool(b) => if b { 1.0 } else { 0.0 },
            serde_json::Value::Number(n) => n.as_f64().unwrap_or(0.0) as f32,
            _ => 0.0,
        })
        .collect())
}

#[derive(Debug, Deserialize, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMessage {
    /// Web Gamepad API から取得したゲームパッド状態。
    GamepadState {
        /// ボタン値（インデックス 0〜17）。0.5 以上を押下とみなす。
        /// bool (true/false) または f32 (0.0〜1.0) のどちらでも受け付ける。
        #[serde(default, deserialize_with = "deserialize_flexible_f32_vec")]
        buttons: Vec<f32>,
        /// 軸値。-1.0〜1.0 または 0〜4095（マッピング済み）のどちらでも受け付ける。
        #[serde(default, deserialize_with = "deserialize_flexible_f32_vec")]
        axes: Vec<f32>,
    },

    /// 4 箇所のコントローラーカラーを変更する（各値 0x00RRGGBB）。
    SetColor {
        pad_color: u32,
        button_color: u32,
        left_grip_color: u32,
        right_grip_color: u32,
    },

    /// サーバー側ファイルパスから Amiibo を読み込んで注入する。
    SendAmiibo { path: String },

    /// モーションセンサーの値を上書きする（Web DeviceMotion API 利用時）。
    Motion {
        /// [ジャイロX, ジャイロY, ジャイロZ]（Switch の生値 i16）
        #[serde(default)]
        gyro: Vec<i16>,
        /// [加速度X, 加速度Y, 加速度Z]（Switch の生値 i16）
        #[serde(default)]
        accel: Vec<i16>,
    },

    /// 振動イベント時に押下するボタンマスクを登録する。
    RumbleRegister { key: u32 },
}

// ---------------------------------------------------------------------------
// サーバー → クライアント
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMessage {
    /// 約 100ms ごとにプッシュされる定期ステータス。
    Status {
        /// エミュレートしたコントローラーが Switch と接続中なら true。
        paired: bool,
        /// Switch が現在振動を要求しているなら true。
        rumble: bool,
    },

    /// クライアントからの不正・処理不能なメッセージへのエラー応答。
    Error { message: String },
}
