//! switch-bt-ws — エントリーポイント。
//!
//! 起動シーケンス：
//!   1. BTStack スレッドを生成し、C の `start_gamepad()` を呼び出す（ブロッキング）。
//!   2. ステータスブロードキャストタスクを起動する（100ms 間隔）。
//!   3. axum HTTP + WebSocket サーバーを `127.0.0.1:8765` で開始する。

mod api;
mod btstack;
mod driver;
mod gamepad;
mod protocol;
mod ws_server;

use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::broadcast;
use tracing_subscriber::EnvFilter;

use crate::protocol::ServerMessage;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // ロギング初期化。RUST_LOG 環境変数で制御（未設定時は info レベル）。
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    // -----------------------------------------------------------------------
    // BTStack スレッド
    //
    // `btstack::start()` は C の `start_gamepad()` を呼び出し、
    // WinUSB HCI トランスポートと Pro Controller エミュレーターを初期化した後、
    // `btstack_run_loop_execute()` 内でシャットダウンまでブロックします。
    // 専用の OS スレッドを使う必要があります。
    // -----------------------------------------------------------------------
    std::thread::Builder::new()
        .name("btstack".into())
        .spawn(|| {
            tracing::info!("BTStack スレッド開始");
            btstack::start();
            tracing::info!("BTStack スレッド終了");
        })?;

    // BTStack が HCI を初期化するまで少し待つ
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    // -----------------------------------------------------------------------
    // ブロードキャストチャンネル（サーバー → 全接続クライアント）
    // 定期ステータスプッシュに使用する。
    // -----------------------------------------------------------------------
    let (status_tx, _) = broadcast::channel::<ServerMessage>(32);
    let status_tx = Arc::new(status_tx);

    // バックグラウンドタスク: 接続中の全クライアントへ 100ms ごとにステータスを送信
    let status_tx_clone = Arc::clone(&status_tx);
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_millis(100));
        loop {
            interval.tick().await;
            let msg = ServerMessage::Status {
                paired: btstack::is_paired(),
                rumble: btstack::get_rumble_state(),
            };
            // クライアントが接続していない場合の send エラーは無視
            let _ = status_tx_clone.send(msg);
        }
    });

    // -----------------------------------------------------------------------
    // HTTP + WebSocket サーバー（axum）
    // -----------------------------------------------------------------------
    let addr: SocketAddr = "127.0.0.1:8765".parse()?;
    let app = api::build_router(Arc::clone(&status_tx));

    tracing::info!("HTTP サーバー起動: http://{addr}");
    tracing::info!("WebSocket エンドポイント: ws://{addr}/ws");

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
