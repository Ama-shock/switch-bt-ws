//! HTTP REST API（axum ルーター）。
//!
//! ## エンドポイント一覧
//!
//! | メソッド | パス                       | 説明                                       |
//! |---------|---------------------------|--------------------------------------------|
//! | GET     | `/ws/:id`                 | WebSocket アップグレード（コントローラー指定）|
//! | GET     | `/api/controllers`        | 全コントローラーの情報リスト                |
//! | POST    | `/api/controllers`        | コントローラーを追加する                    |
//! | DELETE  | `/api/controllers/:id`    | コントローラーを削除する                    |
//! | GET     | `/api/driver/list`        | BT USB デバイス一覧                        |
//! | POST    | `/api/driver/install`     | WinUSB ドライバを導入                      |
//! | POST    | `/api/driver/restore`     | 元の Bluetooth ドライバに戻す              |
//!
//! reconnect / sync / link-keys 操作は WebSocket (`/ws/:id`) 経由で行う。

use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::{delete, get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};

use crate::{controller::ControllerManager, driver, ws_server};

// ---------------------------------------------------------------------------
// アプリケーション状態
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct AppState {
    pub controllers: Arc<ControllerManager>,
}

/// axum ルーターを構築して返す。
pub fn build_router(state: AppState) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    Router::new()
        // WebSocket（コントローラー ID を URL パスから取得）
        .route("/ws/:id", get(ws_server::ws_handler))
        // コントローラー管理
        .route("/api/controllers",              get(api_controllers_list).post(api_controllers_add))
        .route("/api/controllers/:id",          delete(api_controllers_remove))
        // ドライバ管理
        .route("/api/driver/list",    get(api_driver_list))
        .route("/api/driver/install", post(api_driver_install))
        .route("/api/driver/restore", post(api_driver_restore))
        .layer(cors)
        .with_state(state)
}

// ---------------------------------------------------------------------------
// コントローラー管理ハンドラ
// ---------------------------------------------------------------------------

async fn api_controllers_list(
    State(state): State<AppState>,
) -> Json<Vec<crate::controller::ControllerInfo>> {
    Json(state.controllers.list().await)
}

#[derive(Deserialize)]
struct AddControllerRequest {
    vid: u16,
    pid: u16,
    #[serde(default)]
    instance: u32,
}

#[derive(Serialize)]
struct AddControllerResponse {
    success: bool,
    id: Option<u32>,
    message: String,
}

async fn api_controllers_add(
    State(state): State<AppState>,
    Json(req): Json<AddControllerRequest>,
) -> (StatusCode, Json<AddControllerResponse>) {
    match state.controllers.add(req.vid, req.pid, req.instance).await {
        Ok(id) => (
            StatusCode::CREATED,
            Json(AddControllerResponse {
                success: true,
                id: Some(id),
                message: format!("コントローラー id={id} を追加しました"),
            }),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(AddControllerResponse {
                success: false,
                id: None,
                message: e.to_string(),
            }),
        ),
    }
}

#[derive(Serialize)]
struct SimpleResponse {
    success: bool,
    message: String,
}

async fn api_controllers_remove(
    State(state): State<AppState>,
    Path(id): Path<u32>,
) -> (StatusCode, Json<SimpleResponse>) {
    match state.controllers.remove(id).await {
        Ok(()) => (
            StatusCode::OK,
            Json(SimpleResponse {
                success: true,
                message: format!("コントローラー id={id} を削除しました"),
            }),
        ),
        Err(e) => (
            StatusCode::NOT_FOUND,
            Json(SimpleResponse {
                success: false,
                message: e.to_string(),
            }),
        ),
    }
}

// ---------------------------------------------------------------------------
// ドライバ管理ハンドラ
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct DriverListResponse {
    version: &'static str,
    devices: Vec<driver::BtUsbDevice>,
}

async fn api_driver_list(_: State<AppState>) -> Json<DriverListResponse> {
    Json(DriverListResponse {
        version: env!("CARGO_PKG_VERSION"),
        devices: driver::list_bt_usb_devices().await.unwrap_or_default(),
    })
}

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
        Ok(msg)  => Json(DriverResponse { success: true,  message: msg }),
        Err(e)   => Json(DriverResponse { success: false, message: e.to_string() }),
    }
}

async fn api_driver_restore(
    _: State<AppState>,
    Json(req): Json<DriverRequest>,
) -> Json<DriverResponse> {
    match driver::restore_driver(req.vid, req.pid).await {
        Ok(msg)  => Json(DriverResponse { success: true,  message: msg }),
        Err(e)   => Json(DriverResponse { success: false, message: e.to_string() }),
    }
}

