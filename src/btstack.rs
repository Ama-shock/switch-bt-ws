//! BTStack ゲームパッド静的ライブラリへの FFI バインディング。
//!
//! C シンボルは build.rs でコンパイルされる 2 つのファイルに由来します：
//!   - `example/btkeyLib.c`         — Pro Controller エミュレーション本体 + 制御 API
//!   - `csrc/btstack_platform.c`    — BTStack プラットフォーム初期化（WinUSB・ランループ）
//!
//! Windows 以外のビルドではスタブライブラリがリンクされるため、
//! 全関数は安全な no-op になります（開発・CI 用）。

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
    fn send_gyro(gyro1: i16, gyro2: i16, gyro3: i16);

    /// 加速度センサー値を設定する。
    fn send_accel(accel_x: i16, accel_y: i16, accel_z: i16);

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
    fn rumble_register(key: u32);

    /// 指定ファイルパスの Amiibo データを送信する（NUL 終端 C 文字列）。
    fn send_amiibo(path: *const std::os::raw::c_char);
}

// ---------------------------------------------------------------------------
// 安全な Rust ラッパー
// ---------------------------------------------------------------------------

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

/// 右アナログスティックを更新する。
/// `h`・`v` は 12bit 値（0〜4095、中立 = 2048）。
pub fn set_stick_right(h: u32, v: u32) {
    unsafe { send_stick_r(h, v, 0) }
}

/// 左アナログスティックを更新する。
/// `h`・`v` は 12bit 値（0〜4095、中立 = 2048）。
pub fn set_stick_left(h: u32, v: u32) {
    unsafe { send_stick_l(h, v, 0) }
}

/// ジャイロセンサー値を更新する。
pub fn set_gyro(g1: i16, g2: i16, g3: i16) {
    unsafe { send_gyro(g1, g2, g3) }
}

/// 加速度センサー値を更新する。
pub fn set_accel(x: i16, y: i16, z: i16) {
    unsafe { send_accel(x, y, z) }
}

/// コントローラー本体・ボタン・グリップの色を変更する。
/// 各色は `0x00RRGGBB` 形式。
pub fn set_padcolor(pad: u32, buttons: u32, left_grip: u32, right_grip: u32) {
    unsafe { send_padcolor(pad, buttons, left_grip, right_grip) }
}

/// Switch が現在振動を要求しているなら `true` を返す。
pub fn get_rumble_state() -> bool {
    unsafe { get_rumble() }
}

/// 振動イベント時に押下するボタンのビットマスクを登録する。
pub fn set_rumble_response(key: u32) {
    unsafe { rumble_register(key) }
}

/// Amiibo の `.bin` ダンプファイル（540バイト）を読み込んで送信する。
pub fn send_amiibo_file(path: &str) {
    match CString::new(path) {
        Ok(cstr) => unsafe { send_amiibo(cstr.as_ptr()) },
        Err(_) => tracing::warn!("send_amiibo_file: パスに NUL バイトが含まれています: {path:?}"),
    }
}
