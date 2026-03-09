//! BTStack ゲームパッド静的ライブラリへの FFI バインディング。
//!
//! C シンボルは build.rs でコンパイルされる 2 つのファイルに由来します：
//!   - `example/btkeyLib.c`         — Pro Controller エミュレーション本体 + 制御 API
//!   - `csrc/btstack_platform.c`    — BTStack プラットフォーム初期化（WinUSB・ランループ）
//!
//! Windows 以外のビルドではスタブライブラリがリンクされるため、
//! 全関数は安全な no-op になります（開発・CI 用）。
//!
//! **ワーカーモード専用**: サーバーモードでは直接 FFI を呼ばず
//! `ControllerManager` を介して IPC でワーカーに委譲します。

use std::ffi::CString;

// ---------------------------------------------------------------------------
// 生の C 宣言
// ---------------------------------------------------------------------------
//
// btkeyLib.c および btstack_platform.c の EXPORT_API シグネチャと完全に一致。
// build.rs が `btstack_gamepad` 静的ライブラリを生成します。
//
#[link(name = "btstack_gamepad", kind = "static")]
extern "C" {
    /// BTStack を初期化してランループを開始する。シャットダウンまでブロック。
    fn start_gamepad();

    /// BTStack のクリーンシャットダウンを要求する。
    fn shutdown_gamepad();

    /// エミュレートしたコントローラーが Switch と接続中なら true。
    fn gamepad_paired() -> bool;

    /// ボタン状態ビットマスクを設定する（ビットレイアウトは `crate::gamepad` 参照）。
    /// `press_time` は現在の btkeyLib 実装では未使用。
    fn send_button(button_status: u32, press_time: u32);

    /// 右アナログスティックを設定する。
    /// `stick_horizontal` / `stick_vertical` は 12bit 値（0〜4095、中央 = 0x800 = 2048）。
    fn send_stick_r(stick_horizontal: u32, stick_vertical: u32, press_time: u32);

    /// 左アナログスティックを設定する（エンコードは `send_stick_r` と同じ）。
    fn send_stick_l(stick_horizontal: u32, stick_vertical: u32, press_time: u32);

    /// ジャイロセンサー値を設定する。
    #[link_name = "send_gyro"]
    fn ffi_send_gyro(gyro1: i16, gyro2: i16, gyro3: i16);

    /// 加速度センサー値を設定する。
    #[link_name = "send_accel"]
    fn ffi_send_accel(accel_x: i16, accel_y: i16, accel_z: i16);

    /// コントローラーカラーを設定する（各値は 0x00RRGGBB）。
    fn send_padcolor(
        pad_color: u32,
        button_color: u32,
        leftgrip_color: u32,
        rightgrip_color: u32,
    );

    /// Switch が現在振動を要求しているなら true。
    fn get_rumble() -> bool;

    /// 振動中に押し続けるボタンのビットマスクを登録する。
    #[link_name = "rumble_register"]
    fn ffi_rumble_register(key: u32);

    /// 指定ファイルパスの Amiibo データを送信する（NUL 終端 C 文字列）。
    #[link_name = "send_amiibo"]
    fn ffi_send_amiibo(path: *const std::os::raw::c_char);

    /// HCI を OFF→ON サイクルして discoverable 状態をリセットする。
    /// apply_patches.sh によって btkeyLib.c に追加された関数。
    fn reconnect_gamepad();

    /// リンクキー全削除 + HCI リセット（シンクロボタン長押し相当）。
    /// apply_patches.sh によって btkeyLib.c に追加された関数。
    fn sync_gamepad();

    /// VID/PID+インスタンス番号でターゲット USB ドングルを指定する。
    /// apply_patches.sh によって hci_transport_h2_winusb.c に追加された関数。
    /// 非 Windows では btstack_stub.c の空スタブが使われる。
    fn hci_transport_usb_set_target(vid: u16, pid: u16, instance: i32);

    /// メモリ上のリンクキーをバイト列としてエクスポートする。
    /// 各エントリ 23 バイト: BD_ADDR(6) + link_key(16) + key_type(1)。
    /// 書き込んだバイト数を返す。
    fn export_link_keys(buf: *mut u8, buf_size: i32) -> i32;

    /// バイト列からリンクキーをインポートする（フォーマットは export_link_keys と同じ）。
    fn import_link_keys(buf: *const u8, len: i32);

    /// Switch が割り当てたプレイヤー LED のビットパターンを返す。
    /// P1=0x01, P2=0x03, P3=0x07, P4=0x0F（累積パターン）。
    fn get_player_leds() -> u8;
}

// ---------------------------------------------------------------------------
// 安全な Rust ラッパー
// ---------------------------------------------------------------------------

/// ターゲット USB ドングルを VID/PID+インスタンスで指定する。
/// `start()` を呼ぶ前に呼び出すこと。vid=0 / pid=0 の場合は最初の BT デバイスを使用。
pub fn set_target(vid: u16, pid: u16, instance: i32) {
    unsafe { hci_transport_usb_set_target(vid, pid, instance) }
}

/// BTStack ランループを開始する。**シャットダウンまでブロックします。**
/// 専用の OS スレッドから呼び出してください。
pub fn start() {
    // SAFETY: C 関数が自身の初期化を行う。プロセス内で 1 回だけ呼び出す。
    unsafe { start_gamepad() }
}

/// BTStack のクリーンシャットダウンを要求する。
pub fn shutdown() {
    unsafe { shutdown_gamepad() }
}

/// Switch が HID 接続を確立済みなら `true` を返す。
pub fn is_paired() -> bool {
    unsafe { gamepad_paired() }
}

/// ボタン状態を更新する。
/// `button_status` のビットレイアウトは `crate::gamepad::SwitchButton` を参照。
pub fn set_buttons(button_status: u32) {
    unsafe { send_button(button_status, 0) }
}

/// 右アナログスティックを更新する（h/v は 0–4095、中立 = 2048）。
pub fn set_stick_r(h: u32, v: u32) {
    unsafe { send_stick_r(h, v, 0) }
}

/// 左アナログスティックを更新する（h/v は 0–4095、中立 = 2048）。
pub fn set_stick_l(h: u32, v: u32) {
    unsafe { send_stick_l(h, v, 0) }
}

/// ジャイロセンサー値を更新する。
pub fn send_gyro(g1: i16, g2: i16, g3: i16) {
    unsafe { ffi_send_gyro(g1, g2, g3) }
}

/// 加速度センサー値を更新する。
pub fn send_accel(x: i16, y: i16, z: i16) {
    unsafe { ffi_send_accel(x, y, z) }
}

/// コントローラー本体・ボタン・グリップの色を変更する（各値 0x00RRGGBB）。
pub fn set_pad_color(pad: u32, btn: u32, lg: u32, rg: u32) {
    unsafe { send_padcolor(pad, btn, lg, rg) }
}

/// Switch が現在振動を要求しているなら `true` を返す。
pub fn get_rumble_state() -> bool {
    unsafe { get_rumble() }
}

/// 振動イベント時に押下するボタンのビットマスクを登録する。
pub fn rumble_register(key: u32) {
    unsafe { ffi_rumble_register(key) }
}

/// HCI 電源を OFF→ON サイクルして再接続を試みる。
pub fn reconnect() {
    unsafe { reconnect_gamepad() }
}

/// リンクキーを全削除して新規ペアリングモードに入る（シンクロボタン長押し相当）。
pub fn sync() {
    unsafe { sync_gamepad() }
}

/// Amiibo の `.bin` ダンプファイルを読み込んで送信する。
pub fn send_amiibo(path: &str) {
    match CString::new(path) {
        Ok(cstr) => unsafe { ffi_send_amiibo(cstr.as_ptr()) },
        Err(_) => tracing::warn!("send_amiibo: パスに NUL バイトが含まれています: {path:?}"),
    }
}

/// メモリ上のリンクキーを Vec<u8> としてエクスポートする。
pub fn get_link_keys() -> Vec<u8> {
    // 最大 16 エントリ分 (23 * 16 = 368 bytes)
    let mut buf = vec![0u8; 23 * 16];
    let len = unsafe { export_link_keys(buf.as_mut_ptr(), buf.len() as i32) };
    buf.truncate(len as usize);
    buf
}

/// バイト列からリンクキーをインポートする。
pub fn set_link_keys(data: &[u8]) {
    unsafe { import_link_keys(data.as_ptr(), data.len() as i32) }
}

/// Switch が割り当てたプレイヤー番号（1〜4）を返す。未割当なら 0。
/// LED ビットパターン: P1=0x01, P2=0x03, P3=0x07, P4=0x0F。
pub fn get_player_number() -> u8 {
    let leds = unsafe { get_player_leds() } & 0x0F;
    match leds {
        0x0F => 4,
        0x07 => 3,
        0x03 => 2,
        0x01 => 1,
        _ => 0,
    }
}
