//! HTTP REST API（axum ルーター）。
//!
//! ## エンドポイント一覧
//!
//! | メソッド | パス                    | 説明                         |
//! |---------|------------------------|------------------------------|
//! | GET     | `/ws`                  | WebSocket アップグレード       |
//! | GET     | `/api/status`          | 接続・振動状態を返す           |
//! | GET     | `/api/driver/list`     | BT USB デバイス一覧           |
//! | POST    | `/api/driver/install`  | WinUSB ドライバを導入          |
//! | POST    | `/api/driver/restore`  | 元の Bluetooth ドライバに戻す  |

use axum::{
    extract::State,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::broadcast;
use tower_http::cors::{Any, CorsLayer};

use crate::{btstack, driver, protocol::ServerMessage, ws_server};

pub type AppState = Arc<broadcast::Sender<ServerMessage>>;

/// axum ルーターを構築して返す。
pub fn build_router(status_tx: AppState) -> Router {
    // フロントエンドが別オリジンから接続できるよう CORS を許可
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    Router::new()
        // WebSocket
        .route("/ws", get(ws_server::ws_handler))
        // REST API
        .route("/api/status",          get(api_status))
        .route("/api/driver/list",     get(api_driver_list))
        .route("/api/driver/install",  post(api_driver_install))
        .route("/api/driver/restore",  post(api_driver_restore))
        .layer(cors)
        .with_state(status_tx)
}

// ---------------------------------------------------------------------------
// ハンドラ
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct StatusResponse {
    /// Switch と接続中なら true
    paired: bool,
    /// Switch が振動を要求中なら true
    rumble: bool,
}

async fn api_status(_: State<AppState>) -> Json<StatusResponse> {
    Json(StatusResponse {
        paired: btstack::is_paired(),
        rumble: btstack::get_rumble_state(),
    })
}

// ---- デバイス一覧 ----------------------------------------------------------

async fn api_driver_list(_: State<AppState>) -> Json<Vec<driver::BtUsbDevice>> {
    let devices = driver::list_bt_usb_devices().await.unwrap_or_default();
    Json(devices)
}

// ---- ドライバ導入 / 復元 ---------------------------------------------------

/// ドライバ操作リクエスト（VID / PID 指定）
#[derive(Deserialize)]
struct DriverRequest {
    /// USB ベンダー ID（10進数）
    vid: u16,
    /// USB プロダクト ID（10進数）
    pid: u16,
}

#[derive(Serialize)]
struct DriverResponse {
    success: bool,
    message: String,
}

/// WinUSB ドライバを導入する（管理者権限が必要）。
async fn api_driver_install(
    _: State<AppState>,
    Json(req): Json<DriverRequest>,
) -> Json<DriverResponse> {
    match driver::install_winusb(req.vid, req.pid).await {
        Ok(msg)  => Json(DriverResponse { success: true,  message: msg }),
        Err(e)   => Json(DriverResponse { success: false, message: e.to_string() }),
    }
}

/// 元の Bluetooth ドライバに戻す（管理者権限が必要）。
async fn api_driver_restore(
    _: State<AppState>,
    Json(req): Json<DriverRequest>,
) -> Json<DriverResponse> {
    match driver::restore_driver(req.vid, req.pid).await {
        Ok(msg)  => Json(DriverResponse { success: true,  message: msg }),
        Err(e)   => Json(DriverResponse { success: false, message: e.to_string() }),
    }
}
