//! WebSocket ハンドラ。
//!
//! 接続したブラウザごとに `handle_socket` タスクを生成し、以下を行います：
//!   - クライアントから受信した `ClientMessage` JSON フレームを
//!     BTStack への直接 FFI 呼び出しに変換する。
//!   - ステータスタスクからの `ServerMessage` ブロードキャストを受け取り、
//!     JSON テキストフレームとしてクライアントへ転送する。

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::response::Response;
use futures_util::{SinkExt, StreamExt};
use std::sync::Arc;
use tokio::sync::broadcast;

use crate::{btstack, gamepad, protocol::{ClientMessage, ServerMessage}};

pub type StatusTx = Arc<broadcast::Sender<ServerMessage>>;

/// axum ルートハンドラ — HTTP リクエストを WebSocket 接続にアップグレードする。
pub async fn ws_handler(
    ws: WebSocketUpgrade,
    State(status_tx): State<StatusTx>,
) -> Response {
    ws.on_upgrade(move |socket| handle_socket(socket, status_tx))
}

async fn handle_socket(socket: WebSocket, status_tx: StatusTx) {
    tracing::info!("WebSocket クライアント接続");

    let mut status_rx = status_tx.subscribe();
    let (mut ws_tx, mut ws_rx) = socket.split();

    loop {
        tokio::select! {
            // ----------------------------------------------------------------
            // サーバー → クライアント: ステータスブロードキャスト
            // ----------------------------------------------------------------
            result = status_rx.recv() => {
                match result {
                    Ok(msg) => {
                        match serde_json::to_string(&msg) {
                            Ok(json) => {
                                if ws_tx.send(Message::Text(json)).await.is_err() {
                                    break; // クライアント切断
                                }
                            }
                            Err(e) => tracing::error!("JSON シリアライズエラー: {e}"),
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!("ステータスブロードキャストが {n} メッセージ遅延");
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }

            // ----------------------------------------------------------------
            // クライアント → サーバー: ゲームパッド入力
            // ----------------------------------------------------------------
            msg = ws_rx.next() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        handle_client_message(&text, &mut ws_tx).await;
                    }
                    Some(Ok(Message::Close(_))) | None => {
                        tracing::info!("WebSocket クライアント切断");
                        break;
                    }
                    Some(Ok(_)) => { /* バイナリ / ping / pong は無視 */ }
                    Some(Err(e)) => {
                        tracing::warn!("WebSocket エラー: {e}");
                        break;
                    }
                }
            }
        }
    }
}

async fn handle_client_message<S>(text: &str, ws_tx: &mut S)
where
    S: SinkExt<Message> + Unpin,
    S::Error: std::fmt::Debug,
{
    let msg: ClientMessage = match serde_json::from_str(text) {
        Ok(m) => m,
        Err(e) => {
            tracing::warn!("クライアントメッセージのパース失敗: {e}  raw={text:?}");
            let err = ServerMessage::Error {
                message: format!("不正なメッセージ: {e}"),
            };
            if let Ok(json) = serde_json::to_string(&err) {
                let _ = ws_tx.send(Message::Text(json)).await;
            }
            return;
        }
    };

    match msg {
        // --------------------------------------------------------------------
        // メインの入力パス（ブラウザが毎アニメーションフレームに呼び出す）
        // --------------------------------------------------------------------
        ClientMessage::GamepadState { buttons, axes } => {
            let button_flags = gamepad::map_buttons(&buttons);
            btstack::set_buttons(button_flags);

            let (lh, lv, rh, rv) = gamepad::map_axes(&axes);
            btstack::set_stick_left(lh, lv);
            btstack::set_stick_right(rh, rv);
        }

        // --------------------------------------------------------------------
        // モーションセンサー（Web DeviceMotion API）
        // --------------------------------------------------------------------
        ClientMessage::Motion { gyro, accel } => {
            let g = |i: usize| gyro.get(i).copied().unwrap_or(0);
            let a = |i: usize| accel.get(i).copied().unwrap_or(100);
            btstack::set_gyro(g(0), g(1), g(2));
            btstack::set_accel(a(0), a(1), a(2));
        }

        // --------------------------------------------------------------------
        // コントローラー外観
        // --------------------------------------------------------------------
        ClientMessage::SetColor {
            pad_color,
            button_color,
            left_grip_color,
            right_grip_color,
        } => {
            btstack::set_padcolor(pad_color, button_color, left_grip_color, right_grip_color);
        }

        // --------------------------------------------------------------------
        // Amiibo
        // --------------------------------------------------------------------
        ClientMessage::SendAmiibo { path } => {
            btstack::send_amiibo_file(&path);
        }

        // --------------------------------------------------------------------
        // 振動応答設定
        // --------------------------------------------------------------------
        ClientMessage::RumbleRegister { key } => {
            btstack::set_rumble_response(key);
        }
    }
}
