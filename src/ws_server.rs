//! WebSocket handler.
//!
//! Each connected browser gets a `handle_socket` task that:
//!   - Receives `ClientMessage` JSON frames and translates them into
//!     direct FFI calls into BTStack.
//!   - Receives `ServerMessage` broadcasts from the status task and
//!     forwards them to the client as JSON text frames.

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::response::Response;
use futures_util::{SinkExt, StreamExt};
use std::sync::Arc;
use tokio::sync::broadcast;

use crate::{btstack, gamepad, protocol::{ClientMessage, ServerMessage}};

pub type StatusTx = Arc<broadcast::Sender<ServerMessage>>;

/// Axum route handler — upgrades an HTTP request to a WebSocket connection.
pub async fn ws_handler(
    ws: WebSocketUpgrade,
    State(status_tx): State<StatusTx>,
) -> Response {
    ws.on_upgrade(move |socket| handle_socket(socket, status_tx))
}

async fn handle_socket(socket: WebSocket, status_tx: StatusTx) {
    let peer_addr = "WebSocket client"; // axum doesn't expose addr here easily
    tracing::info!("{peer_addr} connected");

    let mut status_rx = status_tx.subscribe();
    let (mut ws_tx, mut ws_rx) = socket.split();

    loop {
        tokio::select! {
            // ----------------------------------------------------------------
            // Server → client: status broadcast
            // ----------------------------------------------------------------
            result = status_rx.recv() => {
                match result {
                    Ok(msg) => {
                        match serde_json::to_string(&msg) {
                            Ok(json) => {
                                if ws_tx.send(Message::Text(json)).await.is_err() {
                                    break; // client disconnected
                                }
                            }
                            Err(e) => tracing::error!("JSON serialisation error: {e}"),
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!("Status broadcast lagged by {n} messages");
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }

            // ----------------------------------------------------------------
            // Client → server: gamepad input
            // ----------------------------------------------------------------
            msg = ws_rx.next() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        handle_client_message(&text, &mut ws_tx).await;
                    }
                    Some(Ok(Message::Close(_))) | None => {
                        tracing::info!("{peer_addr} disconnected");
                        break;
                    }
                    Some(Ok(_)) => { /* binary / ping / pong — ignore */ }
                    Some(Err(e)) => {
                        tracing::warn!("{peer_addr} WebSocket error: {e}");
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
            tracing::warn!("Bad client message: {e}  raw={text:?}");
            let err = ServerMessage::Error {
                message: format!("Invalid message: {e}"),
            };
            if let Ok(json) = serde_json::to_string(&err) {
                let _ = ws_tx.send(Message::Text(json)).await;
            }
            return;
        }
    };

    match msg {
        // --------------------------------------------------------------------
        // Main gamepad input path (called every animation frame by the browser)
        // --------------------------------------------------------------------
        ClientMessage::GamepadState { buttons, axes } => {
            let button_flags = gamepad::map_buttons(&buttons);
            btstack::set_buttons(button_flags);

            let (lh, lv, rh, rv) = gamepad::map_axes(&axes);
            btstack::set_stick_left(lh, lv);
            btstack::set_stick_right(rh, rv);
        }

        // --------------------------------------------------------------------
        // Motion sensors (Web DeviceMotion API)
        // --------------------------------------------------------------------
        ClientMessage::Motion { gyro, accel } => {
            let g = |i: usize| gyro.get(i).copied().unwrap_or(0);
            let a = |i: usize| accel.get(i).copied().unwrap_or(100);
            btstack::set_gyro(g(0), g(1), g(2));
            btstack::set_accel(a(0), a(1), a(2));
        }

        // --------------------------------------------------------------------
        // Controller appearance
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
        // Rumble response config
        // --------------------------------------------------------------------
        ClientMessage::RumbleRegister { key } => {
            btstack::set_rumble_response(key);
        }
    }
}
