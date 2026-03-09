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
//! | POST    | `/api/controllers/:id/reconnect` | 再接続シグナル送信                   |
//! | POST    | `/api/controllers/:id/sync`      | シンクロ（リンクキー削除+新規ペアリング）|
//! | POST    | `/api/controllers/:id/sync-start`| ペアリングループ開始                    |
//! | POST    | `/api/controllers/:id/sync-stop` | ペアリングループ停止                    |
//! | GET     | `/api/driver/list`        | BT USB デバイス一覧                        |
//! | POST    | `/api/driver/install`     | WinUSB ドライバを導入                      |
//! | POST    | `/api/driver/restore`     | 元の Bluetooth ドライバに戻す              |
//! | GET     | `/api/tlv`                | TLV ファイル一覧                           |
//! | GET     | `/api/tlv/:filename`      | TLV ファイルダウンロード (binary)          |
//! | POST    | `/api/tlv/:filename`      | TLV ファイルアップロード (binary)          |

use axum::{
    body::Bytes,
    extract::{Path, State},
    http::{header, StatusCode},
    response::IntoResponse,
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
        .route("/api/controllers/:id/reconnect", post(api_controllers_reconnect))
        .route("/api/controllers/:id/sync",       post(api_controllers_sync))
        .route("/api/controllers/:id/sync-start", post(api_controllers_sync_start))
        .route("/api/controllers/:id/sync-stop",  post(api_controllers_sync_stop))
        // ドライバ管理
        .route("/api/driver/list",    get(api_driver_list))
        .route("/api/driver/install", post(api_driver_install))
        .route("/api/driver/restore", post(api_driver_restore))
        // TLV リンクキー管理
        .route("/api/tlv",           get(api_tlv_list))
        .route("/api/tlv/:filename", get(api_tlv_download).post(api_tlv_upload))
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

async fn api_controllers_reconnect(
    State(state): State<AppState>,
    Path(id): Path<u32>,
) -> (StatusCode, Json<SimpleResponse>) {
    match state.controllers.reconnect(id).await {
        Ok(()) => (
            StatusCode::OK,
            Json(SimpleResponse {
                success: true,
                message: format!("コントローラー id={id} に再接続シグナルを送信しました"),
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

async fn api_controllers_sync(
    State(state): State<AppState>,
    Path(id): Path<u32>,
) -> (StatusCode, Json<SimpleResponse>) {
    match state.controllers.sync(id).await {
        Ok(()) => (
            StatusCode::OK,
            Json(SimpleResponse {
                success: true,
                message: format!("コントローラー id={id} のリンクキーを削除し、新規ペアリングモードに入りました"),
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

async fn api_controllers_sync_start(
    State(state): State<AppState>,
    Path(id): Path<u32>,
) -> (StatusCode, Json<SimpleResponse>) {
    match state.controllers.sync_start(id).await {
        Ok(()) => (
            StatusCode::OK,
            Json(SimpleResponse {
                success: true,
                message: format!("コントローラー id={id} のペアリングループを開始しました"),
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

async fn api_controllers_sync_stop(
    State(state): State<AppState>,
    Path(id): Path<u32>,
) -> (StatusCode, Json<SimpleResponse>) {
    match state.controllers.sync_stop(id).await {
        Ok(()) => (
            StatusCode::OK,
            Json(SimpleResponse {
                success: true,
                message: format!("コントローラー id={id} のペアリングループを停止しました"),
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

// ---------------------------------------------------------------------------
// TLV リンクキー管理ハンドラ
// ---------------------------------------------------------------------------

/// .tlv ファイル名のバリデーション（パストラバーサル防止）
fn is_valid_tlv_filename(name: &str) -> bool {
    name.starts_with("btstack_")
        && name.ends_with(".tlv")
        && !name.contains('/')
        && !name.contains('\\')
        && !name.contains("..")
}

#[derive(Serialize)]
struct TlvFileInfo {
    filename: String,
    size: u64,
}

async fn api_tlv_list(_: State<AppState>) -> Json<Vec<TlvFileInfo>> {
    let mut files = Vec::new();
    if let Ok(entries) = std::fs::read_dir(".") {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with("btstack_") && name.ends_with(".tlv") {
                if let Ok(meta) = entry.metadata() {
                    files.push(TlvFileInfo {
                        filename: name,
                        size: meta.len(),
                    });
                }
            }
        }
    }
    Json(files)
}

async fn api_tlv_download(
    _: State<AppState>,
    Path(filename): Path<String>,
) -> impl IntoResponse {
    if !is_valid_tlv_filename(&filename) {
        return (
            StatusCode::BAD_REQUEST,
            [(header::CONTENT_TYPE, "application/json")],
            b"{\"error\":\"invalid filename\"}".to_vec(),
        );
    }
    match std::fs::read(&filename) {
        Ok(data) => (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "application/octet-stream")],
            data,
        ),
        Err(_) => (
            StatusCode::NOT_FOUND,
            [(header::CONTENT_TYPE, "application/json")],
            b"{\"error\":\"file not found\"}".to_vec(),
        ),
    }
}

async fn api_tlv_upload(
    _: State<AppState>,
    Path(filename): Path<String>,
    body: Bytes,
) -> (StatusCode, Json<SimpleResponse>) {
    if !is_valid_tlv_filename(&filename) {
        return (
            StatusCode::BAD_REQUEST,
            Json(SimpleResponse {
                success: false,
                message: "無効なファイル名です".into(),
            }),
        );
    }
    match std::fs::write(&filename, &body) {
        Ok(()) => (
            StatusCode::OK,
            Json(SimpleResponse {
                success: true,
                message: format!("{filename} を保存しました ({} bytes)", body.len()),
            }),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(SimpleResponse {
                success: false,
                message: format!("ファイル書き込みに失敗: {e}"),
            }),
        ),
    }
}
