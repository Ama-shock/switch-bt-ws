//! FFI bindings to the BTStack gamepad static library.
//!
//! The C symbols come from two source files compiled by build.rs:
//!   - `example/btkeyLib.c`  — Pro Controller emulation + exported control API
//!   - `csrc/btstack_platform.c` — BTStack platform init (WinUSB, run loop)
//!
//! On non-Windows builds the stub library is linked instead, so every
//! function is a safe no-op.

use std::ffi::CString;

// ---------------------------------------------------------------------------
// Raw C declarations
// ---------------------------------------------------------------------------
//
// All of these match the EXPORT_API signatures in btkeyLib.c and
// btstack_platform.c verbatim.  The `btstack_gamepad` static lib is produced
// by build.rs.
//
#[link(name = "btstack_gamepad", kind = "static")]
extern "C" {
    /// Initialise BTStack and start the run loop.  Blocks until shutdown.
    fn start_gamepad();

    /// Request a clean shutdown of BTStack.
    fn shutdown_gamepad();

    /// Returns true if the emulated controller is currently connected to a Switch.
    fn gamepad_paired() -> bool;

    /// Set the button state bitmask (see `crate::gamepad` for bit layout).
    /// `press_time` is unused in the current btkeyLib implementation.
    fn send_button(button_status: u32, press_time: u32);

    /// Set the right analog stick.
    /// `stick_horizontal` and `stick_vertical` are 12-bit values (0–4095,
    /// centre = 0x800 = 2048).
    fn send_stick_r(stick_horizontal: u32, stick_vertical: u32, press_time: u32);

    /// Set the left analog stick (same encoding as `send_stick_r`).
    fn send_stick_l(stick_horizontal: u32, stick_vertical: u32, press_time: u32);

    /// Set gyroscope readings.
    fn send_gyro(gyro1: i16, gyro2: i16, gyro3: i16);

    /// Set accelerometer readings.
    fn send_accel(accel_x: i16, accel_y: i16, accel_z: i16);

    /// Set the controller colours (0x00RRGGBB each).
    fn send_padcolor(
        pad_color: u32,
        button_color: u32,
        leftgrip_color: u32,
        rightgrip_color: u32,
    );

    /// Returns true if the Switch is currently requesting rumble.
    fn get_rumble() -> bool;

    /// Register which buttons to "hold" when rumble is active.
    fn rumble_register(key: u32);

    /// Send an Amiibo dump from the given file path (NUL-terminated C string).
    fn send_amiibo(path: *const std::os::raw::c_char);
}

// ---------------------------------------------------------------------------
// Safe Rust wrappers
// ---------------------------------------------------------------------------

/// Start the BTStack run loop.  **Blocks until shutdown.**
/// Must be called on a dedicated OS thread.
pub fn start() {
    // SAFETY: no Rust invariants are violated; the C function performs its
    // own initialisation and is safe to call once per process.
    unsafe { start_gamepad() }
}

/// Request a clean shutdown of BTStack.
pub fn shutdown() {
    unsafe { shutdown_gamepad() }
}

/// Returns `true` when a Switch has established an HID connection.
pub fn is_paired() -> bool {
    unsafe { gamepad_paired() }
}

/// Update the button state.  `button_status` uses the bit layout defined in
/// `crate::gamepad::SwitchButton`.
pub fn set_buttons(button_status: u32) {
    unsafe { send_button(button_status, 0) }
}

/// Update the right analog stick.
/// `h` and `v` are 12-bit values (0–4095, neutral = 2048).
pub fn set_stick_right(h: u32, v: u32) {
    unsafe { send_stick_r(h, v, 0) }
}

/// Update the left analog stick.
/// `h` and `v` are 12-bit values (0–4095, neutral = 2048).
pub fn set_stick_left(h: u32, v: u32) {
    unsafe { send_stick_l(h, v, 0) }
}

/// Update gyroscope values.
pub fn set_gyro(g1: i16, g2: i16, g3: i16) {
    unsafe { send_gyro(g1, g2, g3) }
}

/// Update accelerometer values.
pub fn set_accel(x: i16, y: i16, z: i16) {
    unsafe { send_accel(x, y, z) }
}

/// Change the visual colour of the controller body, buttons, and grips.
/// Each colour is `0x00RRGGBB`.
pub fn set_padcolor(pad: u32, buttons: u32, left_grip: u32, right_grip: u32) {
    unsafe { send_padcolor(pad, buttons, left_grip, right_grip) }
}

/// Returns `true` if the Switch is requesting rumble right now.
pub fn get_rumble_state() -> bool {
    unsafe { get_rumble() }
}

/// Register a button bitmask to be pressed when the Switch requests rumble.
pub fn set_rumble_response(key: u32) {
    unsafe { rumble_register(key) }
}

/// Load and send an Amiibo from a `.bin` dump file (540 bytes).
pub fn send_amiibo_file(path: &str) {
    match CString::new(path) {
        Ok(cstr) => unsafe { send_amiibo(cstr.as_ptr()) },
        Err(_) => tracing::warn!("send_amiibo_file: path contains a NUL byte: {path:?}"),
    }
}
