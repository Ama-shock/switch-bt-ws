//! switch-bt-ws — エントリーポイント。
//!
//! ## 動作モード
//!
//! ### サーバーモード（引数なし）
//! HTTP + WebSocket サーバーを起動する。
//! ブラウザから WebSocket 経由でゲームパッド入力を受け付け、
//! ドングルごとのワーカーサブプロセスに IPC で転送する。
//!
//! ### ワーカーモード（`--worker <id> <vid_hex> <pid_hex> <instance>`）
//! BTStack を起動して Switch Pro Controller をエミュレートする。
//! 親プロセス（サーバーモード）との通信は stdin/stdout の JSON 行（NDJSON）で行う。
//! BTStack はグローバルな C 状態を持つため、ドングルごとに独立したプロセスとして動作する。

mod api;
mod btstack;
mod controller;
mod driver;
mod gamepad;
mod ipc;
mod protocol;
mod worker;
mod ws_server;

use std::net::SocketAddr;
use std::sync::Arc;
use tracing_subscriber::EnvFilter;

use crate::api::AppState;
use crate::controller::ControllerManager;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();

    // --worker フラグでワーカーモードに切り替える
    if args.len() > 1 && args[1] == "--worker" {
        // ワーカーモードは同期（Tokio ランタイム不要）
        init_logging();
        worker::run(&args);
        return Ok(());
    }

    // サーバーモード
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(run_server())
}

fn init_logging() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();
}

async fn run_server() -> anyhow::Result<()> {
    init_logging();

    let controllers = Arc::new(ControllerManager::new());

    let state = AppState {
        controllers: Arc::clone(&controllers),
    };

    let addr: SocketAddr = "127.0.0.1:8765".parse()?;
    let app = api::build_router(state);

    tracing::info!("HTTP サーバー起動: http://{addr}");
    tracing::info!("WebSocket エンドポイント: ws://{addr}/ws/<controller_id>");
    tracing::info!("コントローラー追加: POST http://{addr}/api/controllers {{\"vid\": 2578, \"pid\": 1}}");

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
