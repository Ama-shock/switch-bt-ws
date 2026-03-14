//! HTTP REST API（axum ルーター）。
//!
//! ## エンドポイント一覧
//!
//! | メソッド | パス                       | 説明                                       |
//! |---------|---------------------------|--------------------------------------------|
//! | GET     | `/ws`                     | 全体状態同期 WebSocket                     |
//! | GET     | `/ws/:id`                 | WebSocket アップグレード（コントローラー指定）|
//! | POST    | `/api/controllers`        | コントローラーを追加する                    |
//! | DELETE  | `/api/controllers/:id`    | コントローラーを削除する                    |
//! | POST    | `/api/driver/install`     | WinUSB ドライバを導入                      |
//! | POST    | `/api/driver/restore`     | 元の Bluetooth ドライバに戻す              |
//!
//! デバイス一覧・コントローラー一覧はグローバル WS (`/ws`) で同期する。
//! reconnect / sync / link-keys 操作はコントローラー WS (`/ws/:id`) 経由で行う。

use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::{delete, get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};

use crate::{controller::ControllerManager, driver, global_ws, ws_server};

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
        // WebSocket
        .route("/ws", get(global_ws::global_ws_handler))
        .route("/ws/:id", get(ws_server::ws_handler))
        // コントローラー管理
        .route("/api/controllers", post(api_controllers_add))
        .route("/api/controllers/:id", delete(api_controllers_remove))
        // ドライバ管理
        .route("/api/driver/install", post(api_driver_install))
        .route("/api/driver/restore", post(api_driver_restore))
        .layer(cors)
        .with_state(state)
}

// ---------------------------------------------------------------------------
// コントローラー管理ハンドラ
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct AddControllerRequest {
    vid: u16,
    pid: u16,
    #[serde(default)]
    instance: u32,
    /// base64 エンコードされたリンクキー（再接続用）。
    /// 指定時はワーカー起動前にインポートし、起動後に reconnect を実行する。
    #[serde(default)]
    link_keys: Option<String>,
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
    match state.controllers.add(req.vid, req.pid, req.instance, req.link_keys).await {
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
    State(state): State<AppState>,
    Json(req): Json<DriverRequest>,
) -> Json<DriverResponse> {
    match driver::install_winusb(req.vid, req.pid).await {
        Ok(msg) => {
            state.controllers.refresh_and_notify_devices().await;
            Json(DriverResponse { success: true, message: msg })
        }
        Err(e) => Json(DriverResponse { success: false, message: e.to_string() }),
    }
}

async fn api_driver_restore(
    State(state): State<AppState>,
    Json(req): Json<DriverRequest>,
) -> Json<DriverResponse> {
    match driver::restore_driver(req.vid, req.pid).await {
        Ok(msg) => {
            state.controllers.refresh_and_notify_devices().await;
            Json(DriverResponse { success: true, message: msg })
        }
        Err(e) => Json(DriverResponse { success: false, message: e.to_string() }),
    }
}
