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
    // Initialise logging.
    // Set RUST_LOG=debug for verbose output, otherwise defaults to info.
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    // -----------------------------------------------------------------------
    // BTStack thread
    //
    // btstack::start() calls start_gamepad() in C, which initialises the
    // WinUSB HCI transport and the Pro Controller emulator, then blocks
    // inside btstack_run_loop_execute().  It must live on a dedicated OS
    // thread because it never returns until shutdown.
    // -----------------------------------------------------------------------
    std::thread::Builder::new()
        .name("btstack".into())
        .spawn(|| {
            tracing::info!("BTStack thread starting");
            btstack::start();
            tracing::info!("BTStack thread exiting");
        })?;

    // Brief pause so BTStack can initialise the HCI before the first status
    // broadcast is sent to clients.
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    // -----------------------------------------------------------------------
    // Broadcast channel — server → all connected WebSocket clients.
    // Used for periodic status pushes (paired state, rumble, etc.).
    // -----------------------------------------------------------------------
    let (status_tx, _) = broadcast::channel::<ServerMessage>(32);
    let status_tx = Arc::new(status_tx);

    // Background task: push status to every connected client every 100 ms.
    let status_tx_clone = Arc::clone(&status_tx);
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_millis(100));
        loop {
            interval.tick().await;
            let msg = ServerMessage::Status {
                paired: btstack::is_paired(),
                rumble: btstack::get_rumble_state(),
            };
            // Ignore send errors — they just mean no clients are connected.
            let _ = status_tx_clone.send(msg);
        }
    });

    // -----------------------------------------------------------------------
    // HTTP + WebSocket server (axum)
    // -----------------------------------------------------------------------
    let addr: SocketAddr = "127.0.0.1:8765".parse()?;
    let app = api::build_router(Arc::clone(&status_tx));

    tracing::info!("Listening on http://{addr}");
    tracing::info!("WebSocket endpoint: ws://{addr}/ws");

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
