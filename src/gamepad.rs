//! Web Gamepad API → Nintendo Switch Pro Controller mapping.
//!
//! ## Button bit layout in `button_flg` (btkeyLib.c)
//!
//! | Bit | Switch button   |
//! |-----|-----------------|
//! |  0  | Y               |
//! |  1  | X               |
//! |  2  | B               |
//! |  3  | A               |
//! |  4  | SR (right JC)   |
//! |  5  | SL (right JC)   |
//! |  6  | R               |
//! |  7  | ZR              |
//! |  8  | −               |
//! |  9  | +               |
//! | 10  | RS (R-stick btn)|
//! | 11  | LS (L-stick btn)|
//! | 12  | Home            |
//! | 13  | Screenshot      |
//! | 16  | D-pad Down      |
//! | 17  | D-pad Up        |
//! | 18  | D-pad Right     |
//! | 19  | D-pad Left      |
//! | 20  | SR (left JC)    |
//! | 21  | SL (left JC)    |
//! | 22  | L               |
//! | 23  | ZL              |
//!
//! ## Web Gamepad Standard button indices
//!
//! | Index | Button (Standard Gamepad layout)       |
//! |-------|----------------------------------------|
//! |   0   | A / Cross (bottom face button)         |
//! |   1   | B / Circle (right face button)         |
//! |   2   | X / Square (left face button)          |
//! |   3   | Y / Triangle (top face button)         |
//! |   4   | Left Bumper (L1)                       |
//! |   5   | Right Bumper (R1)                      |
//! |   6   | Left Trigger (L2)                      |
//! |   7   | Right Trigger (R2)                     |
//! |   8   | Back / Select / Share                  |
//! |   9   | Start / Options                        |
//! |  10   | Left Stick press (L3)                  |
//! |  11   | Right Stick press (R3)                 |
//! |  12   | D-pad Up                               |
//! |  13   | D-pad Down                             |
//! |  14   | D-pad Left                             |
//! |  15   | D-pad Right                            |
//! |  16   | Home / Guide                           |
//! |  17   | Screenshot / Share / Touchpad          |
//!
//! ## Stick encoding
//!
//! Both `send_stick_l` and `send_stick_r` accept 12-bit unsigned integers
//! (0–4095).  The neutral / centre position is **0x800 = 2048**.
//! The Web Gamepad API reports axes as `f32` in the range −1.0 to +1.0.

// ---------------------------------------------------------------------------
// Switch button bit constants
// ---------------------------------------------------------------------------

pub struct SwitchButton;

#[allow(dead_code)]
impl SwitchButton {
    pub const Y: u32 = 1 << 0;
    pub const X: u32 = 1 << 1;
    pub const B: u32 = 1 << 2;
    pub const A: u32 = 1 << 3;
    pub const SR_RIGHT: u32 = 1 << 4;
    pub const SL_RIGHT: u32 = 1 << 5;
    pub const R: u32 = 1 << 6;
    pub const ZR: u32 = 1 << 7;
    pub const MINUS: u32 = 1 << 8;
    pub const PLUS: u32 = 1 << 9;
    pub const RS: u32 = 1 << 10;
    pub const LS: u32 = 1 << 11;
    pub const HOME: u32 = 1 << 12;
    pub const SCREENSHOT: u32 = 1 << 13;
    pub const DPAD_DOWN: u32 = 1 << 16;
    pub const DPAD_UP: u32 = 1 << 17;
    pub const DPAD_RIGHT: u32 = 1 << 18;
    pub const DPAD_LEFT: u32 = 1 << 19;
    pub const SR_LEFT: u32 = 1 << 20;
    pub const SL_LEFT: u32 = 1 << 21;
    pub const L: u32 = 1 << 22;
    pub const ZL: u32 = 1 << 23;
}

// ---------------------------------------------------------------------------
// Button mapping
// ---------------------------------------------------------------------------

/// Convert the Web Gamepad button array into a Switch `button_flg` bitmask.
///
/// Values ≥ 0.5 are treated as pressed; values < 0.5 as released.
/// (This threshold works for both digital buttons and analog triggers.)
pub fn map_buttons(web_buttons: &[f32]) -> u32 {
    let pressed = |idx: usize| -> bool {
        web_buttons.get(idx).copied().unwrap_or(0.0) >= 0.5
    };

    let mut flags: u32 = 0;

    // ---- Face buttons -------------------------------------------------------
    // Web Gamepad "A" (bottom/cross) → Switch B
    // Web Gamepad "B" (right/circle) → Switch A
    // Note: Nintendo's face-button naming is rotated 90° compared to Sony/MS.
    if pressed(0) { flags |= SwitchButton::B; }
    if pressed(1) { flags |= SwitchButton::A; }
    if pressed(2) { flags |= SwitchButton::Y; }
    if pressed(3) { flags |= SwitchButton::X; }

    // ---- Shoulder / trigger -------------------------------------------------
    if pressed(4) { flags |= SwitchButton::L; }
    if pressed(5) { flags |= SwitchButton::R; }
    if pressed(6) { flags |= SwitchButton::ZL; }
    if pressed(7) { flags |= SwitchButton::ZR; }

    // ---- System buttons -----------------------------------------------------
    if pressed(8)  { flags |= SwitchButton::MINUS; }
    if pressed(9)  { flags |= SwitchButton::PLUS; }

    // ---- Stick clicks -------------------------------------------------------
    if pressed(10) { flags |= SwitchButton::LS; }
    if pressed(11) { flags |= SwitchButton::RS; }

    // ---- D-pad --------------------------------------------------------------
    if pressed(12) { flags |= SwitchButton::DPAD_UP; }
    if pressed(13) { flags |= SwitchButton::DPAD_DOWN; }
    if pressed(14) { flags |= SwitchButton::DPAD_LEFT; }
    if pressed(15) { flags |= SwitchButton::DPAD_RIGHT; }

    // ---- Extra --------------------------------------------------------------
    if pressed(16) { flags |= SwitchButton::HOME; }
    if pressed(17) { flags |= SwitchButton::SCREENSHOT; }

    flags
}

// ---------------------------------------------------------------------------
// Axis / stick mapping
// ---------------------------------------------------------------------------

/// Convert a Web Gamepad axis value (−1.0 to +1.0) to a Switch 12-bit stick
/// value (0–4095, neutral = 2048).
pub fn axis_to_stick(v: f32) -> u32 {
    let clamped = v.clamp(-1.0, 1.0);
    ((clamped + 1.0) / 2.0 * 4095.0).round() as u32
}

/// Convert the Web Gamepad axes array `[left_x, left_y, right_x, right_y]`
/// into four Switch 12-bit stick values: `(l_h, l_v, r_h, r_v)`.
///
/// The Web Gamepad Y-axis is **inverted** (negative = up), which matches
/// the Switch convention, so no extra inversion is applied here.
pub fn map_axes(axes: &[f32]) -> (u32, u32, u32, u32) {
    let get = |i: usize| axes.get(i).copied().unwrap_or(0.0);
    (
        axis_to_stick(get(0)), // left  horizontal
        axis_to_stick(get(1)), // left  vertical
        axis_to_stick(get(2)), // right horizontal
        axis_to_stick(get(3)), // right vertical
    )
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn neutral_stick() {
        assert_eq!(axis_to_stick(0.0), 2048);
    }

    #[test]
    fn full_left_stick() {
        assert_eq!(axis_to_stick(-1.0), 0);
        assert_eq!(axis_to_stick(1.0), 4095);
    }

    #[test]
    fn no_buttons_pressed() {
        let buttons = vec![0.0f32; 18];
        assert_eq!(map_buttons(&buttons), 0);
    }

    #[test]
    fn a_button_maps_to_switch_b() {
        let mut buttons = vec![0.0f32; 18];
        buttons[0] = 1.0; // Web "A" → Switch B
        assert_eq!(map_buttons(&buttons), SwitchButton::B);
    }

    #[test]
    fn dpad_up() {
        let mut buttons = vec![0.0f32; 18];
        buttons[12] = 1.0;
        assert_eq!(map_buttons(&buttons), SwitchButton::DPAD_UP);
    }
}
