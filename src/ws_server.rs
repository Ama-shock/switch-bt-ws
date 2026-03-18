//! WebSocket ハンドラ。
//!
//! 接続したブラウザごとに `handle_socket` タスクを生成し、以下を行います：
//!   - URL パスの `:id` からコントローラーを特定して `ControllerHandle` を取得
//!   - クライアントから受信した `ClientMessage` JSON フレームを `WorkerCommand` に変換して
//!     ワーカーサブプロセスの stdin に転送する
//!   - ワーカーの stdout ブロードキャストを受け取り、`ServerMessage` JSON として
//!     クライアントへ送信する

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use futures_util::{SinkExt, StreamExt};
use std::sync::Arc;

use crate::api::AppState;
use crate::controller::ControllerHandle;
use crate::gamepad;
use crate::ipc::{WorkerCommand, WorkerEvent};
use crate::protocol::{ClientMessage, ServerMessage};

/// axum ルートハンドラ — HTTP リクエストを WebSocket 接続にアップグレードする。
/// URL: `/ws/:id` の `:id` でコントローラーを指定する。
pub async fn ws_handler(
    ws: WebSocketUpgrade,
    Path(id): Path<u32>,
    State(state): State<AppState>,
) -> Response {
    match state.controllers.get(id).await {
        Some(handle) => ws.on_upgrade(move |socket| handle_socket(socket, handle)),
        None => (
            StatusCode::NOT_FOUND,
            format!("コントローラー id={id} が見つかりません"),
        )
            .into_response(),
    }
}

async fn handle_socket(socket: WebSocket, handle: Arc<ControllerHandle>) {
    tracing::info!("WebSocket クライアント接続 (controller id={})", handle.id());

    let mut event_rx = handle.subscribe_status();
    let (mut ws_tx, mut ws_rx) = socket.split();

    loop {
        tokio::select! {
            // ----------------------------------------------------------------
            // ワーカー → クライアント: WorkerEvent をブロードキャストから受け取る
            // ----------------------------------------------------------------
            result = event_rx.recv() => {
                match result {
                    Ok(event) => {
                        if let Some(msg) = worker_event_to_server_msg(event) {
                            match serde_json::to_string(&msg) {
                                Ok(json) => {
                                    if ws_tx.send(Message::Text(json)).await.is_err() {
                                        break;
                                    }
                                }
                                Err(e) => tracing::error!("JSON シリアライズエラー: {e}"),
                            }
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!("イベントブロードキャストが {n} メッセージ遅延");
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }

            // ----------------------------------------------------------------
            // クライアント → ワーカー: ゲームパッド入力
            // ----------------------------------------------------------------
            msg = ws_rx.next() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        dispatch_client_message(&text, &handle, &mut ws_tx).await;
                    }
                    Some(Ok(Message::Close(_))) | None => {
                        tracing::info!("WebSocket クライアント切断（Switch 接続は維持）");
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

fn worker_event_to_server_msg(event: WorkerEvent) -> Option<ServerMessage> {
    match event {
        WorkerEvent::Status { paired, rumble, rumble_left, rumble_right, syncing, player } =>
            Some(ServerMessage::Status { paired, rumble, rumble_left, rumble_right, syncing, player }),
        WorkerEvent::Rumble { left, right } => Some(ServerMessage::Rumble { left, right }),
        WorkerEvent::LinkKeys { data } => Some(ServerMessage::LinkKeys { data }),
        WorkerEvent::Error { message } => Some(ServerMessage::Error { message }),
        _ => None,
    }
}

/// クライアントの JSON メッセージを WorkerCommand に変換してワーカーへ送る。
/// 1 つのクライアントメッセージが複数の WorkerCommand を生成することがある（GamepadState など）。
async fn dispatch_client_message<S>(
    text: &str,
    handle: &Arc<ControllerHandle>,
    ws_tx: &mut S,
) where
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
        ClientMessage::GamepadState { button_status, buttons, axes } => {
            let button_flags = button_status.unwrap_or_else(|| gamepad::map_buttons(&buttons));
            handle.send(WorkerCommand::Button { button_status: button_flags });

            let (lh, lv, rh, rv) = gamepad::map_axes(&axes);
            handle.send(WorkerCommand::StickL { h: lh, v: lv });
            handle.send(WorkerCommand::StickR { h: rh, v: rv });
        }
        ClientMessage::Motion { gyro, accel } => {
            let g = |i: usize| gyro.get(i).copied().unwrap_or(0);
            let a = |i: usize| accel.get(i).copied().unwrap_or(100);
            handle.send(WorkerCommand::Gyro { g1: g(0), g2: g(1), g3: g(2) });
            handle.send(WorkerCommand::Accel { x: a(0), y: a(1), z: a(2) });
        }
        ClientMessage::SetColor { pad_color, button_color, left_grip_color, right_grip_color } => {
            handle.send(WorkerCommand::PadColor {
                pad: pad_color,
                btn: button_color,
                lg: left_grip_color,
                rg: right_grip_color,
            });
        }
        ClientMessage::SendAmiibo { path } => {
            handle.send(WorkerCommand::Amiibo { path });
        }
        ClientMessage::RumbleRegister { key } => {
            handle.send(WorkerCommand::RumbleRegister { key });
        }
        ClientMessage::Reconnect { link_keys } => {
            handle.send(WorkerCommand::Reconnect { link_keys });
        }
        ClientMessage::SyncStart => {
            handle.send(WorkerCommand::SyncStart);
        }
        ClientMessage::SyncStop => {
            handle.send(WorkerCommand::SyncStop);
        }
        ClientMessage::Disconnect => {
            handle.send(WorkerCommand::Disconnect);
        }
        ClientMessage::GetLinkKeys => {
            handle.send(WorkerCommand::GetLinkKeys);
        }
    }
}
