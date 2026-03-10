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
mod global_ws;
mod ipc;
mod protocol;
mod worker;
mod ws_server;

use std::net::SocketAddr;
use std::sync::Arc;
use tracing_subscriber::EnvFilter;

use crate::api::AppState;
use crate::controller::ControllerManager;

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();

    // --debug フラグがあれば RUST_LOG を debug に設定
    if args.iter().any(|a| a == "--debug") {
        std::env::set_var("RUST_LOG", "debug");
    }

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

fn print_banner() {
    println!("switch-bt-ws v{VERSION}");
    println!();
    println!("This software uses BTStack:");
    println!("  Copyright (C) 2009 BlueKitchen GmbH. All rights reserved.");
    println!("  Redistribution and use in source and binary forms, with or without");
    println!("  modification, are permitted provided that the following conditions are met:");
    println!("  1. Redistributions of source code must retain the above copyright notice.");
    println!("  2. Redistributions in binary form must reproduce the above copyright notice");
    println!("     in the documentation and/or other materials provided with the distribution.");
    println!("  3. Neither the name of the copyright holders nor the names of contributors");
    println!("     may be used to endorse or promote products derived from this software");
    println!("     without specific prior written permission.");
    println!("  4. Any redistribution, use, or modification is done solely for personal");
    println!("     benefit and not for any commercial purpose or for monetary gain.");
    println!("  See https://github.com/bluekitchen/btstack/blob/master/LICENSE");
    println!("      https://github.com/mizuyoukanao/btstack?tab=License-1-ov-file");
    println!();
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
    print_banner();

    let port: u16 = std::env::var("SWITCH_BT_WS_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(8765);

    let controllers = Arc::new(ControllerManager::new());

    let state = AppState {
        controllers: Arc::clone(&controllers),
    };

    let addr: SocketAddr = ([127, 0, 0, 1], port).into();
    let app = api::build_router(state);

    tracing::info!("HTTP サーバー起動: http://{addr}");
    tracing::info!("グローバル WS: ws://{addr}/ws");
    tracing::info!("コントローラー WS: ws://{addr}/ws/<controller_id>");
    tracing::info!("コントローラー追加: POST http://{addr}/api/controllers {{\"vid\": 2578, \"pid\": 1}}");

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
