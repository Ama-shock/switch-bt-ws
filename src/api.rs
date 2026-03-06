//! HTTP REST API (axum router).
//!
//! Endpoints
//! ---------
//! GET  /ws                     WebSocket upgrade (see ws_server.rs)
//! GET  /api/status             Current paired / rumble state (JSON)
//! GET  /api/driver/list        Enumerate Bluetooth USB dongles
//! POST /api/driver/install     Install WinUSB driver for a dongle
//! POST /api/driver/restore     Restore the original Bluetooth driver

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

/// Build the complete axum router.
pub fn build_router(status_tx: AppState) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    Router::new()
        // WebSocket
        .route("/ws", get(ws_server::ws_handler))
        // REST API
        .route("/api/status", get(api_status))
        .route("/api/driver/list", get(api_driver_list))
        .route("/api/driver/install", post(api_driver_install))
        .route("/api/driver/restore", post(api_driver_restore))
        .layer(cors)
        .with_state(status_tx)
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct StatusResponse {
    paired: bool,
    rumble: bool,
}

async fn api_status(_: State<AppState>) -> Json<StatusResponse> {
    Json(StatusResponse {
        paired: btstack::is_paired(),
        rumble: btstack::get_rumble_state(),
    })
}

// ---- Driver list -----------------------------------------------------------

async fn api_driver_list(_: State<AppState>) -> Json<Vec<driver::BtUsbDevice>> {
    let devices = driver::list_bt_usb_devices().await.unwrap_or_default();
    Json(devices)
}

// ---- Driver install / restore ----------------------------------------------

#[derive(Deserialize)]
struct DriverRequest {
    vid: u16,
    pid: u16,
}

#[derive(Serialize)]
struct DriverResponse {
    success: bool,
    message: String,
}

async fn api_driver_install(
    _: State<AppState>,
    Json(req): Json<DriverRequest>,
) -> Json<DriverResponse> {
    match driver::install_winusb(req.vid, req.pid).await {
        Ok(msg) => Json(DriverResponse { success: true, message: msg }),
        Err(e) => Json(DriverResponse { success: false, message: e.to_string() }),
    }
}

async fn api_driver_restore(
    _: State<AppState>,
    Json(req): Json<DriverRequest>,
) -> Json<DriverResponse> {
    match driver::restore_driver(req.vid, req.pid).await {
        Ok(msg) => Json(DriverResponse { success: true, message: msg }),
        Err(e) => Json(DriverResponse { success: false, message: e.to_string() }),
    }
}
