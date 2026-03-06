//! WebSocket message types (JSON, serde).
//!
//! **Client → Server** (`ClientMessage`)
//!
//! ```json
//! // Gamepad input (sent every frame by the Web Gamepad API polling loop)
//! { "type": "gamepad_state",
//!   "buttons": [0.0, 1.0, ...],   // up to 20 button values, 0.0–1.0
//!   "axes":    [-0.5, 0.3, 0.0, -1.0] }  // [lx, ly, rx, ry], −1.0 to 1.0
//!
//! // Override controller colour
//! { "type": "set_color",
//!   "pad_color":        16711680,  // 0xFF0000 (red body)
//!   "button_color":     0,
//!   "left_grip_color":  0,
//!   "right_grip_color": 0 }
//!
//! // Send an Amiibo (path must be accessible to the server process)
//! { "type": "send_amiibo", "path": "C:\\amiibo\\pikachu.bin" }
//!
//! // Gyro + accel override (optional; Web DeviceMotion API data)
//! { "type": "motion",
//!   "gyro":  [0, 0, 0],
//!   "accel": [100, 100, 100] }
//! ```
//!
//! **Server → Client** (`ServerMessage`)
//!
//! ```json
//! { "type": "status", "paired": true, "rumble": false }
//! { "type": "error",  "message": "..." }
//! ```

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Client → Server
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMessage {
    /// Gamepad state from the Web Gamepad API.
    GamepadState {
        /// Button values (index 0–17).  Values > 0.5 are treated as pressed.
        #[serde(default)]
        buttons: Vec<f32>,
        /// Axis values [left_x, left_y, right_x, right_y], −1.0 to 1.0.
        #[serde(default)]
        axes: Vec<f32>,
    },

    /// Override the four controller colours (0x00RRGGBB each).
    SetColor {
        pad_color: u32,
        button_color: u32,
        left_grip_color: u32,
        right_grip_color: u32,
    },

    /// Load and inject an Amiibo from a server-side file path.
    SendAmiibo { path: String },

    /// Motion sensor override (Web DeviceMotion API values).
    Motion {
        /// [gyro_x, gyro_y, gyro_z] in raw Switch units (i16).
        #[serde(default)]
        gyro: Vec<i16>,
        /// [accel_x, accel_y, accel_z] in raw Switch units (i16).
        #[serde(default)]
        accel: Vec<i16>,
    },

    /// Register a button mask to hold during rumble events.
    RumbleRegister { key: u32 },
}

// ---------------------------------------------------------------------------
// Server → Client
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMessage {
    /// Periodic status update pushed every ~100 ms.
    Status {
        /// True when the emulated controller is connected to a Switch.
        paired: bool,
        /// True when the Switch is currently requesting rumble.
        rumble: bool,
    },

    /// Error response to a malformed or unprocessable client message.
    Error { message: String },
}
