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

use crate::btstack;
use crate::ipc::{WorkerCommand, WorkerEvent};

/// ワーカーモードのメイン関数。
/// 引数: ["--worker", id, vid_hex, pid_hex, instance_str]
pub fn run(args: &[String]) {
    // args: [exe, "--worker", id, vid_hex, pid_hex, instance]
    if args.len() < 6 {
        eprintln!("[worker] 使用方法: --worker <id> <vid_hex> <pid_hex> <instance>");
        std::process::exit(1);
    }

    let vid = u16::from_str_radix(&args[3], 16).unwrap_or(0);
    let pid = u16::from_str_radix(&args[4], 16).unwrap_or(0);
    let instance: i32 = args[5].parse().unwrap_or(0);

    eprintln!("[worker] 起動: vid={vid:04x} pid={pid:04x} instance={instance}");

    // ペアリングループ中かどうか
    let syncing = Arc::new(AtomicBool::new(false));

    // ターゲットドングルを設定
    btstack::set_target(vid, pid, instance);

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
            loop {
                std::thread::sleep(Duration::from_millis(100));
                let event = WorkerEvent::Status {
                    paired: btstack::is_paired(),
                    rumble: btstack::get_rumble_state(),
                    syncing: syncing_status.load(Ordering::Relaxed),
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
        WorkerCommand::Reconnect => {
            eprintln!("[worker] 再接続シグナルを送信");
            btstack::reconnect();
        }
        WorkerCommand::Sync => {
            eprintln!("[worker] シンクロ（リンクキー削除 + HCI リセット）");
            btstack::sync();
        }
        WorkerCommand::SyncStart => {
            if syncing.load(Ordering::Relaxed) {
                eprintln!("[worker] ペアリングループは既に実行中");
                return;
            }
            syncing.store(true, Ordering::Relaxed);
            let syncing_loop = Arc::clone(syncing);
            std::thread::Builder::new()
                .name("sync-loop".into())
                .spawn(move || {
                    eprintln!("[worker] ペアリングループ開始: sync_gamepad() を呼び出します");
                    // リンクキー削除 + HCI リセット → discoverable モードに入る
                    // HCI OFF→ON 後、gap_discoverable_control(1) が自動で呼ばれる（パッチ済み）
                    btstack::sync();
                    eprintln!("[worker] sync_gamepad() 呼び出し完了、discoverable 待機開始");

                    // discoverable を維持したまま接続を待つ。
                    // 実機の Pro Controller と同様、HCI を再リセットせずに待機する。
                    // Switch が発見→接続→ペアリングハンドシェイク完了まで
                    // 数秒〜数十秒かかることがある。
                    let mut tick = 0u32;
                    loop {
                        std::thread::sleep(Duration::from_millis(200));
                        if !syncing_loop.load(Ordering::Relaxed) {
                            eprintln!("[worker] ペアリングループ中断 (tick={tick})");
                            return;
                        }
                        let paired = btstack::is_paired();
                        if tick % 25 == 0 {
                            // 5秒ごとにステータスログ
                            eprintln!("[worker] ペアリング待機中: tick={tick} paired={paired}");
                        }
                        if paired {
                            eprintln!("[worker] ペアリング成功！ループ終了 (tick={tick})");
                            syncing_loop.store(false, Ordering::Relaxed);
                            return;
                        }
                        tick += 1;
                    }
                })
                .expect("ペアリングループスレッドの生成に失敗");
        }
        WorkerCommand::SyncStop => {
            eprintln!("[worker] ペアリングループ停止");
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
