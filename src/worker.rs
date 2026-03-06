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
use std::time::Duration;

use crate::btstack;
use crate::ipc::{WorkerCommand, WorkerEvent};

/// ワーカーモードのメイン関数。
/// 引数: ["--worker", id, vid_hex, pid_hex, instance_str]
pub fn run(args: &[String]) {
    if args.len() < 5 {
        eprintln!("[worker] 使用方法: --worker <id> <vid_hex> <pid_hex> <instance>");
        std::process::exit(1);
    }

    let vid = u16::from_str_radix(&args[2], 16).unwrap_or(0);
    let pid = u16::from_str_radix(&args[3], 16).unwrap_or(0);
    let instance: i32 = args[4].parse().unwrap_or(0);

    eprintln!("[worker] 起動: vid={vid:04x} pid={pid:04x} instance={instance}");

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
    std::thread::Builder::new()
        .name("worker-status".into())
        .spawn(|| {
            let stdout = io::stdout();
            loop {
                std::thread::sleep(Duration::from_millis(100));
                let event = WorkerEvent::Status {
                    paired: btstack::is_paired(),
                    rumble: btstack::get_rumble_state(),
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
            Ok(cmd) => handle_command(cmd),
            Err(e) => {
                eprintln!("[worker] JSON パースエラー: {e}: {line}");
            }
        }
    }

    // stdin が閉じられたらシャットダウン
    btstack::shutdown();
    send_event(&WorkerEvent::Shutdown);
}

fn handle_command(cmd: WorkerCommand) {
    match cmd {
        WorkerCommand::Button { button_status } => btstack::set_buttons(button_status),
        WorkerCommand::StickR { h, v } => btstack::set_stick_r(h, v),
        WorkerCommand::StickL { h, v } => btstack::set_stick_l(h, v),
        WorkerCommand::Gyro { g1, g2, g3 } => btstack::send_gyro(g1, g2, g3),
        WorkerCommand::Accel { x, y, z } => btstack::send_accel(x, y, z),
        WorkerCommand::PadColor { pad, btn, lg, rg } => btstack::set_pad_color(pad, btn, lg, rg),
        WorkerCommand::RumbleRegister { key } => btstack::rumble_register(key),
        WorkerCommand::Amiibo { path } => btstack::send_amiibo(&path),
        WorkerCommand::Shutdown => {
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
