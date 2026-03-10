//! 全体状態同期用 WebSocket ハンドラ。
//!
//! `/ws` に接続すると、デバイス一覧・コントローラー一覧の初期スナップショットを送信し、
//! 以降はコントローラーの追加・削除・状態変化時にリアルタイムで更新を送信する。
//! クライアントから `{"type": "refresh"}` を送信すると、デバイス一覧を再スキャンして
//! 最新のスナップショットを送信する。

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::response::Response;
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};

use crate::api::AppState;
use crate::controller::{ControllerInfo, GlobalEvent};
use crate::driver;

// ---------------------------------------------------------------------------
// メッセージ型
// ---------------------------------------------------------------------------

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ServerMsg {
    /// デバイス・コントローラーの全体スナップショット。
    Snapshot {
        version: &'static str,
        devices: Vec<driver::BtUsbDevice>,
        controllers: Vec<ControllerInfo>,
    },
    /// 個別コントローラーの状態変化。
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

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ClientMsg {
    /// デバイス一覧を再スキャンしてスナップショットを送信する。
    Refresh,
}

// ---------------------------------------------------------------------------
// ハンドラ
// ---------------------------------------------------------------------------

pub async fn global_ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> Response {
    ws.on_upgrade(move |socket| handle_global_socket(socket, state))
}

async fn handle_global_socket(socket: WebSocket, state: AppState) {
    tracing::info!("グローバル WebSocket 接続");
    let manager = &state.controllers;
    let mut event_rx = manager.subscribe_global();
    let (mut ws_tx, mut ws_rx) = socket.split();

    // 初回スナップショットを送信（デバイス再スキャン付き）
    {
        let devices = driver::list_bt_usb_devices().await.unwrap_or_default();
        manager.set_cached_devices(devices.clone()).await;
        let controllers = manager.list().await;
        let msg = ServerMsg::Snapshot {
            version: env!("CARGO_PKG_VERSION"),
            devices,
            controllers,
        };
        if let Ok(json) = serde_json::to_string(&msg) {
            if ws_tx.send(Message::Text(json)).await.is_err() {
                return;
            }
        }
    }

    loop {
        tokio::select! {
            result = event_rx.recv() => {
                match result {
                    Ok(event) => {
                        let msg = match event {
                            GlobalEvent::ControllersChanged | GlobalEvent::DevicesChanged => {
                                let devices = manager.get_cached_devices().await;
                                let controllers = manager.list().await;
                                Some(ServerMsg::Snapshot {
                                    version: env!("CARGO_PKG_VERSION"),
                                    devices,
                                    controllers,
                                })
                            }
                            GlobalEvent::ControllerStatus { id, paired, rumble, syncing, player } => {
                                Some(ServerMsg::ControllerStatus { id, paired, rumble, syncing, player })
                            }
                            GlobalEvent::ControllerLinkKeys { id, vid, pid, instance, data } => {
                                Some(ServerMsg::ControllerLinkKeys { id, vid, pid, instance, data })
                            }
                        };
                        if let Some(msg) = msg {
                            if let Ok(json) = serde_json::to_string(&msg) {
                                if ws_tx.send(Message::Text(json)).await.is_err() {
                                    break;
                                }
                            }
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!("グローバルWS: {n} メッセージ遅延");
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }

            msg = ws_rx.next() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        if let Ok(client_msg) = serde_json::from_str::<ClientMsg>(&text) {
                            match client_msg {
                                ClientMsg::Refresh => {
                                    let devices = driver::list_bt_usb_devices().await.unwrap_or_default();
                                    manager.set_cached_devices(devices.clone()).await;
                                    let controllers = manager.list().await;
                                    let msg = ServerMsg::Snapshot {
                                        version: env!("CARGO_PKG_VERSION"),
                                        devices,
                                        controllers,
                                    };
                                    if let Ok(json) = serde_json::to_string(&msg) {
                                        if ws_tx.send(Message::Text(json)).await.is_err() {
                                            break;
                                        }
                                    }
                                }
                            }
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => {
                        tracing::info!("グローバル WebSocket 切断");
                        break;
                    }
                    _ => {}
                }
            }
        }
    }
}
