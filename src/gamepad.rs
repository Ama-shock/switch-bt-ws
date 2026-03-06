//! Web Gamepad API → Nintendo Switch Pro Controller マッピング。
//!
//! ## `button_flg` のビットレイアウト（btkeyLib.c）
//!
//! | ビット | Switch ボタン    |
//! |--------|-----------------|
//! |  0     | Y               |
//! |  1     | X               |
//! |  2     | B               |
//! |  3     | A               |
//! |  4     | SR（右 JC）     |
//! |  5     | SL（右 JC）     |
//! |  6     | R               |
//! |  7     | ZR              |
//! |  8     | −（マイナス）   |
//! |  9     | +（プラス）     |
//! | 10     | RS（右スティック押下）|
//! | 11     | LS（左スティック押下）|
//! | 12     | HOME            |
//! | 13     | キャプチャ      |
//! | 16     | 十字キー 下     |
//! | 17     | 十字キー 上     |
//! | 18     | 十字キー 右     |
//! | 19     | 十字キー 左     |
//! | 20     | SR（左 JC）     |
//! | 21     | SL（左 JC）     |
//! | 22     | L               |
//! | 23     | ZL              |
//!
//! ## Web Gamepad 標準ボタンインデックス
//!
//! | インデックス | ボタン（標準レイアウト）         |
//! |:-----------:|--------------------------------|
//! |    0        | A / Cross（下面ボタン）         |
//! |    1        | B / Circle（右面ボタン）        |
//! |    2        | X / Square（左面ボタン）        |
//! |    3        | Y / Triangle（上面ボタン）      |
//! |    4        | 左バンパー（LB / L1）           |
//! |    5        | 右バンパー（RB / R1）           |
//! |    6        | 左トリガー（LT / L2）           |
//! |    7        | 右トリガー（RT / R2）           |
//! |    8        | Back / Select / Share           |
//! |    9        | Start / Options                 |
//! |   10        | 左スティック押下（L3）           |
//! |   11        | 右スティック押下（R3）           |
//! |   12        | 十字キー 上                     |
//! |   13        | 十字キー 下                     |
//! |   14        | 十字キー 左                     |
//! |   15        | 十字キー 右                     |
//! |   16        | Home / Guide                    |
//! |   17        | Screenshot / Share / Touchpad   |
//!
//! ## スティックのエンコード
//!
//! `send_stick_l` / `send_stick_r` は 12bit 符号なし整数（0〜4095）を受け取ります。
//! 中立位置は **0x800 = 2048** です。
//! Web Gamepad API の軸値は -1.0〜+1.0 の `f32` です。

// ---------------------------------------------------------------------------
// Switch ボタンビット定数
// ---------------------------------------------------------------------------

pub struct SwitchButton;

#[allow(dead_code)]
impl SwitchButton {
    pub const Y: u32          = 1 << 0;
    pub const X: u32          = 1 << 1;
    pub const B: u32          = 1 << 2;
    pub const A: u32          = 1 << 3;
    pub const SR_RIGHT: u32   = 1 << 4;
    pub const SL_RIGHT: u32   = 1 << 5;
    pub const R: u32          = 1 << 6;
    pub const ZR: u32         = 1 << 7;
    pub const MINUS: u32      = 1 << 8;
    pub const PLUS: u32       = 1 << 9;
    pub const RS: u32         = 1 << 10;
    pub const LS: u32         = 1 << 11;
    pub const HOME: u32       = 1 << 12;
    pub const SCREENSHOT: u32 = 1 << 13;
    pub const DPAD_DOWN: u32  = 1 << 16;
    pub const DPAD_UP: u32    = 1 << 17;
    pub const DPAD_RIGHT: u32 = 1 << 18;
    pub const DPAD_LEFT: u32  = 1 << 19;
    pub const SR_LEFT: u32    = 1 << 20;
    pub const SL_LEFT: u32    = 1 << 21;
    pub const L: u32          = 1 << 22;
    pub const ZL: u32         = 1 << 23;
}

// ---------------------------------------------------------------------------
// ボタンマッピング
// ---------------------------------------------------------------------------

/// Web Gamepad ボタン配列を Switch の `button_flg` ビットマスクに変換する。
///
/// 0.5 以上を押下、0.5 未満を離しとみなす。
/// （デジタルボタンとアナログトリガーの両方に対応）
pub fn map_buttons(web_buttons: &[f32]) -> u32 {
    let pressed = |idx: usize| -> bool {
        web_buttons.get(idx).copied().unwrap_or(0.0) >= 0.5
    };

    let mut flags: u32 = 0;

    // ---- 面ボタン -----------------------------------------------------------
    // Web の A（下面）→ Switch の B
    // Web の B（右面）→ Switch の A
    // ※ 任天堂のボタン名は Sony/MS と比べて 90° 回転しています
    if pressed(0)  { flags |= SwitchButton::B; }
    if pressed(1)  { flags |= SwitchButton::A; }
    if pressed(2)  { flags |= SwitchButton::Y; }
    if pressed(3)  { flags |= SwitchButton::X; }

    // ---- ショルダー / トリガー ----------------------------------------------
    if pressed(4)  { flags |= SwitchButton::L; }
    if pressed(5)  { flags |= SwitchButton::R; }
    if pressed(6)  { flags |= SwitchButton::ZL; }
    if pressed(7)  { flags |= SwitchButton::ZR; }

    // ---- システムボタン -----------------------------------------------------
    if pressed(8)  { flags |= SwitchButton::MINUS; }
    if pressed(9)  { flags |= SwitchButton::PLUS; }

    // ---- スティック押下 -----------------------------------------------------
    if pressed(10) { flags |= SwitchButton::LS; }
    if pressed(11) { flags |= SwitchButton::RS; }

    // ---- 十字キー -----------------------------------------------------------
    if pressed(12) { flags |= SwitchButton::DPAD_UP; }
    if pressed(13) { flags |= SwitchButton::DPAD_DOWN; }
    if pressed(14) { flags |= SwitchButton::DPAD_LEFT; }
    if pressed(15) { flags |= SwitchButton::DPAD_RIGHT; }

    // ---- その他 -------------------------------------------------------------
    if pressed(16) { flags |= SwitchButton::HOME; }
    if pressed(17) { flags |= SwitchButton::SCREENSHOT; }

    flags
}

// ---------------------------------------------------------------------------
// 軸 / スティックマッピング
// ---------------------------------------------------------------------------

/// Web Gamepad の軸値（-1.0〜+1.0）を Switch の 12bit スティック値（0〜4095、中立 = 2048）に変換する。
pub fn axis_to_stick(v: f32) -> u32 {
    let clamped = v.clamp(-1.0, 1.0);
    ((clamped + 1.0) / 2.0 * 4095.0).round() as u32
}

/// Web Gamepad の軸配列 `[左X, 左Y, 右X, 右Y]` を
/// Switch の 12bit スティック値 `(左H, 左V, 右H, 右V)` に変換する。
///
/// Web Gamepad の Y 軸は**上が負**（Switch と同じ向き）のため、追加の反転は不要です。
pub fn map_axes(axes: &[f32]) -> (u32, u32, u32, u32) {
    let get = |i: usize| axes.get(i).copied().unwrap_or(0.0);
    (
        axis_to_stick(get(0)), // 左スティック 水平
        axis_to_stick(get(1)), // 左スティック 垂直
        axis_to_stick(get(2)), // 右スティック 水平
        axis_to_stick(get(3)), // 右スティック 垂直
    )
}

// ---------------------------------------------------------------------------
// テスト
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 中立スティック値は2048() {
        assert_eq!(axis_to_stick(0.0), 2048);
    }

    #[test]
    fn 端値のスティック変換() {
        assert_eq!(axis_to_stick(-1.0), 0);
        assert_eq!(axis_to_stick(1.0), 4095);
    }

    #[test]
    fn ボタン未押下でフラグゼロ() {
        let buttons = vec![0.0f32; 18];
        assert_eq!(map_buttons(&buttons), 0);
    }

    #[test]
    fn web_a_が_switch_b_にマップされる() {
        let mut buttons = vec![0.0f32; 18];
        buttons[0] = 1.0; // Web の「A」→ Switch の B
        assert_eq!(map_buttons(&buttons), SwitchButton::B);
    }

    #[test]
    fn 十字キー上のマッピング() {
        let mut buttons = vec![0.0f32; 18];
        buttons[12] = 1.0;
        assert_eq!(map_buttons(&buttons), SwitchButton::DPAD_UP);
    }
}
