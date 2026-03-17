//! worker.rs — ワーカーモードのエントリーポイント。
//!
//! `switch-bt-ws --worker <id> <vid_hex> <pid_hex> <instance>` で起動される。
//!
//! 起動シーケンス:
//!   1. `hci_transport_usb_set_target(vid, pid, instance)` を呼んでターゲットドングルを設定
//!   2. BTStack スレッドを専用 OS スレッドで起動
//!   3. 500ms 待機して BTStack が HCI 初期化を完了するのを待つ
//!   4. 準備完了イベントを stdout に送信
//!   5. stdin から JSON 行コマンドを読み取り btstack FFI 関数へ転送
//!   6. 100ms ごとに Status イベントを stdout へ送信

use std::io::{self, BufRead, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use base64::Engine;

use crate::btstack;
use crate::ipc::{WorkerCommand, WorkerEvent};

/// ワーカーモードのメイン関数。
/// 引数: ["--worker", id, vid_hex, pid_hex, instance_str]
pub fn run(args: &[String]) {
    // args: [exe, "--worker", id, vid_hex, pid_hex, instance, [--link-keys, base64]]
    if args.len() < 6 {
        eprintln!("[worker] 使用方法: --worker <id> <vid_hex> <pid_hex> <instance> [--link-keys <base64>]");
        std::process::exit(1);
    }

    let vid = u16::from_str_radix(&args[3], 16).unwrap_or(0);
    let pid = u16::from_str_radix(&args[4], 16).unwrap_or(0);
    let instance: i32 = args[5].parse().unwrap_or(0);

    // --link-keys オプションの解析
    let init_link_keys = args.windows(2)
        .find(|w| w[0] == "--link-keys")
        .and_then(|w| base64::engine::general_purpose::STANDARD.decode(&w[1]).ok());

    eprintln!("[worker:{vid:04x}:{pid:04x}] 起動 instance={instance} link_keys={}",
        init_link_keys.as_ref().map_or("none".to_string(), |k| format!("{} bytes", k.len())));

    // ペアリングループ中かどうか
    let syncing = Arc::new(AtomicBool::new(false));

    // ターゲットドングルを設定
    btstack::set_target(vid, pid, instance);

    // リンクキーを BTStack 起動前にインポート（メモリ DB に事前格納）
    if let Some(ref keys) = init_link_keys {
        btstack::set_link_keys(keys);
        eprintln!("[worker] link keys pre-imported before BTStack start");
    }

    // BTStack スレッドを起動（シャットダウンまでブロック）
    std::thread::Builder::new()
        .name("btstack".into())
        .spawn(|| {
            btstack::start();
        })
        .expect("BTStack スレッドの生成に失敗");

    // BTStack が HCI を初期化するまで待機
    std::thread::sleep(Duration::from_millis(500));

    // 準備完了を通知
    send_event(&WorkerEvent::Ready);

    // ステータス送信用スレッドを生成
    let syncing_status = Arc::clone(&syncing);
    std::thread::Builder::new()
        .name("worker-status".into())
        .spawn(move || {
            let stdout = io::stdout();
            let mut prev_paired = false;
            let mut link_keys_sent = false;
            loop {
                std::thread::sleep(Duration::from_millis(100));
                let paired = btstack::is_paired();
                let is_syncing = syncing_status.load(Ordering::Relaxed);

                // ペアリング成功検出: paired になったらシンクループを停止
                if paired && !prev_paired {
                    if is_syncing {
                        syncing_status.store(false, Ordering::Relaxed);
                        eprintln!("[worker] status: paired detected, stopping sync loop");
                    }
                    link_keys_sent = false;
                }

                // paired 中はリンクキーが送信できるまでリトライ
                if paired && !link_keys_sent {
                    link_keys_sent = send_link_keys();
                }

                // paired 解除時にリセット
                if !paired {
                    link_keys_sent = false;
                }

                prev_paired = paired;

                let event = WorkerEvent::Status {
                    paired,
                    rumble: btstack::get_rumble_state(),
                    syncing: syncing_status.load(Ordering::Relaxed),
                    player: btstack::get_player_number(),
                };
                let mut line = serde_json::to_string(&event).unwrap_or_default();
                line.push('\n');
                let _ = stdout.lock().write_all(line.as_bytes());
                let _ = stdout.lock().flush();
            }
        })
        .expect("ステータススレッドの生成に失敗");

    // stdin からコマンドを読み取るメインループ
    let stdin = io::stdin();
    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        match serde_json::from_str::<WorkerCommand>(line) {
            Ok(cmd) => handle_command(cmd, &syncing),
            Err(e) => {
                eprintln!("[worker] JSON パースエラー: {e}: {line}");
            }
        }
    }

    // stdin が閉じられたらシャットダウン
    btstack::shutdown();
    send_event(&WorkerEvent::Shutdown);
}

fn handle_command(cmd: WorkerCommand, syncing: &Arc<AtomicBool>) {
    match cmd {
        WorkerCommand::Button { button_status } => btstack::set_buttons(button_status),
        WorkerCommand::StickR { h, v } => btstack::set_stick_r(h, v),
        WorkerCommand::StickL { h, v } => btstack::set_stick_l(h, v),
        WorkerCommand::Gyro { g1, g2, g3 } => btstack::send_gyro(g1, g2, g3),
        WorkerCommand::Accel { x, y, z } => btstack::send_accel(x, y, z),
        WorkerCommand::PadColor { pad, btn, lg, rg } => btstack::set_pad_color(pad, btn, lg, rg),
        WorkerCommand::RumbleRegister { key } => btstack::rumble_register(key),
        WorkerCommand::Amiibo { path } => btstack::send_amiibo(&path),
        WorkerCommand::SetLinkKeys { data } => {
            match base64::engine::general_purpose::STANDARD.decode(&data) {
                Ok(bytes) => btstack::set_link_keys(&bytes),
                Err(e) => eprintln!("[worker] link key decode error: {e}"),
            }
        }
        WorkerCommand::GetLinkKeys => {
            send_link_keys();
        }
        WorkerCommand::Reconnect { link_keys } => {
            if let Some(data) = link_keys {
                match base64::engine::general_purpose::STANDARD.decode(&data) {
                    Ok(bytes) => {
                        btstack::set_link_keys(&bytes);
                        eprintln!("[worker] reconnect (imported link keys)");
                    }
                    Err(e) => eprintln!("[worker] reconnect link key decode error: {e}"),
                }
            } else {
                eprintln!("[worker] reconnect (no link keys)");
            }
            btstack::reconnect();
        }
        WorkerCommand::Sync => {
            eprintln!("[worker] sync (delete link keys + HCI reset)");
            btstack::sync();
        }
        WorkerCommand::SyncStart => {
            if syncing.load(Ordering::Relaxed) {
                return;
            }
            syncing.store(true, Ordering::Relaxed);
            let syncing_loop = Arc::clone(syncing);
            std::thread::Builder::new()
                .name("sync-loop".into())
                .spawn(move || {
                    eprintln!("[worker] pairing: start");
                    btstack::sync();
                    eprintln!("[worker] pairing: discoverable, waiting for Switch...");

                    // HCI OFF→ON がスキャン窓と合わない場合があるため定期的にリトライ
                    // 間隔が短いと接続試行中にリセットしてしまうため 60 秒に設定
                    const RETRY_INTERVAL_TICKS: u32 = 300; // 60秒 (200ms × 300)

                    let mut tick = 0u32;
                    loop {
                        std::thread::sleep(Duration::from_millis(200));
                        if !syncing_loop.load(Ordering::Relaxed) {
                            eprintln!("[worker] pairing: cancelled");
                            return;
                        }
                        if btstack::is_paired() {
                            eprintln!("[worker] pairing: success! ({:.1}s)", tick as f64 * 0.2);
                            syncing_loop.store(false, Ordering::Relaxed);
                            // ペアリング成功時にリンクキーを送信（ブラウザが IndexedDB に保存）
                            send_link_keys();
                            return;
                        }
                        // 15秒ごとに HCI リセットを再試行（スキャン窓合わせ）
                        // ただしリンクキーが存在する場合は接続進行中なのでスキップ
                        if tick > 0 && tick % RETRY_INTERVAL_TICKS == 0 {
                            let keys = btstack::get_link_keys();
                            if keys.is_empty() {
                                eprintln!("[worker] pairing: retry HCI reset ({:.0}s)", tick as f64 * 0.2);
                                btstack::sync();
                            } else {
                                eprintln!("[worker] pairing: connection in progress, skip retry ({:.0}s)", tick as f64 * 0.2);
                            }
                        }
                        // 10秒ごとにログ
                        if tick % 50 == 0 && tick > 0 {
                            eprintln!("[worker] pairing: waiting... ({:.0}s)", tick as f64 * 0.2);
                        }
                        tick += 1;
                    }
                })
                .expect("sync-loop thread spawn failed");
        }
        WorkerCommand::SyncStop => {
            eprintln!("[worker] pairing: stop");
            syncing.store(false, Ordering::Relaxed);
        }
        WorkerCommand::Shutdown => {
            syncing.store(false, Ordering::Relaxed);
            btstack::shutdown();
            send_event(&WorkerEvent::Shutdown);
            std::process::exit(0);
        }
    }
}

fn send_event(event: &WorkerEvent) {
    let mut line = serde_json::to_string(event).unwrap_or_default();
    line.push('\n');
    let stdout = io::stdout();
    let mut lock = stdout.lock();
    let _ = lock.write_all(line.as_bytes());
    let _ = lock.flush();
}

fn send_link_keys() -> bool {
    let keys = btstack::get_link_keys();
    if keys.is_empty() {
        return false;
    }
    eprintln!("[worker] export_link_keys: {} bytes", keys.len());
    let data = base64::engine::general_purpose::STANDARD.encode(&keys);
    send_event(&WorkerEvent::LinkKeys { data });
    true
}
