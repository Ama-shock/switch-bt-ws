# switch-bt-ws

Nintendo Switch Pro Controller emulator controlled via WebSocket.

A single Rust binary that:
- Emulates a Pro Controller over Bluetooth Classic HID (using BTStack + btkeyLib)
- Exposes a WebSocket endpoint (`ws://localhost:8765/ws`) for real-time gamepad input
- Exposes a REST API for status queries and driver management
- Handles WinUSB driver install/restore for the Bluetooth dongle

---

## Architecture

```
Browser (Web Gamepad API)
       │  WebSocket JSON frames
       ▼
switch-bt-ws.exe  (Rust)
  ├── Tokio async runtime
  │   ├── axum HTTP server  :8765
  │   │   ├── GET  /ws                 WebSocket upgrade
  │   │   ├── GET  /api/status         { paired, rumble }
  │   │   ├── GET  /api/driver/list    BT USB device list
  │   │   ├── POST /api/driver/install install WinUSB
  │   │   └── POST /api/driver/restore restore BT driver
  │   └── WebSocket handler
  │       ├── recv gamepad_state → FFI calls
  │       └── send status broadcasts
  └── BTStack thread (blocking C)
      ├── btstack_platform_run()
      └── btkeyLib Pro Controller emulation
              │ WinUSB / BTStack HCI
              ▼
        Bluetooth dongle → Nintendo Switch
```

### Thread model

| Thread | Role |
|--------|------|
| `btstack` (OS thread) | Runs `btstack_run_loop_execute()` — blocks until shutdown |
| Tokio worker threads  | HTTP server, WebSocket handler, status broadcaster |

The BTStack C globals (`button_flg`, `stick_l_flg`, etc.) are written by the
Tokio WS handler and read by the BTStack thread at ~60 Hz.  On x86-64 these
aligned 32-bit stores are effectively atomic; a future improvement is to add
proper `_Atomic` qualifiers in btkeyLib.c.

---

## Prerequisites

### Hardware
- USB Bluetooth 4.0+ dongle (CSR/BCM/RTL chipsets work well)
- Nintendo Switch

### Software
- Windows 10/11 (64-bit)
- Rust toolchain (stable, `x86_64-pc-windows-msvc` or `x86_64-pc-windows-gnu`)
- Visual Studio Build Tools **or** mingw-w64 (for the C compilation step)
- Windows SDK (for WinUSB headers — usually bundled with VS Build Tools)

---

## Driver setup (one-time per dongle)

BTStack needs the WinUSB driver instead of the inbox `BthUsb.sys`.

### Option A — Zadig (recommended, GUI)

1. Download [Zadig](https://zadig.akeo.ie)
2. Options → "List all devices"
3. Select your Bluetooth dongle
4. Select **WinUSB** in the driver dropdown
5. Click **Replace Driver**

### Option B — REST API (programmatic)

First find your dongle's VID/PID in Device Manager, then:

```sh
# Install WinUSB
curl -X POST http://localhost:8765/api/driver/install \
  -H "Content-Type: application/json" \
  -d '{"vid": 2652, "pid": 1}'

# Restore original Bluetooth driver
curl -X POST http://localhost:8765/api/driver/restore \
  -H "Content-Type: application/json" \
  -d '{"vid": 2652, "pid": 1}'
```

> **Requires administrator privileges.** Run the server as Administrator.

---

## Build

```powershell
# Clone this repository alongside the BTStack fork
# Expected layout:
#   repos/btstack/windows/        ← mizuyoukanao/btstack clone
#   repos/btstack/switch-bt-ws/   ← this project

cd repos\btstack\switch-bt-ws
cargo build --release
```

The build script (`build.rs`) uses the `cc` crate to compile all BTStack C
sources and links them into the binary automatically.

---

## Run

```powershell
# Run as Administrator (required for WinUSB / BT driver access)
.\target\release\switch-bt-ws.exe
```

Set `RUST_LOG=debug` for verbose output:

```powershell
$env:RUST_LOG = "debug"
.\target\release\switch-bt-ws.exe
```

---

## WebSocket protocol

Connect to `ws://localhost:8765/ws`.

### Client → Server

```jsonc
// Gamepad state (send every animation frame)
{
  "type": "gamepad_state",
  "buttons": [0.0, 1.0, 0.0, ...],  // Web Gamepad button values (0.0–1.0)
  "axes":    [-0.5, 0.3, 0.0, 1.0]  // [left_x, left_y, right_x, right_y]
}

// Motion sensors (optional, from DeviceMotion API)
{ "type": "motion", "gyro": [0, 0, 0], "accel": [100, 100, 100] }

// Controller colour (0x00RRGGBB)
{ "type": "set_color", "pad_color": 16711680, "button_color": 0,
  "left_grip_color": 0, "right_grip_color": 0 }

// Amiibo (server-side path to a 540-byte .bin dump)
{ "type": "send_amiibo", "path": "C:\\amiibo\\pikachu.bin" }
```

### Server → Client

```jsonc
// Pushed every ~100 ms
{ "type": "status", "paired": true, "rumble": false }

// Error response
{ "type": "error", "message": "Invalid message: ..." }
```

---

## Button mapping

| Web Gamepad index | Web button       | Switch button |
|:-----------------:|------------------|---------------|
| 0                 | A (bottom face)  | B             |
| 1                 | B (right face)   | A             |
| 2                 | X (left face)    | Y             |
| 3                 | Y (top face)     | X             |
| 4                 | LB / L1          | L             |
| 5                 | RB / R1          | R             |
| 6                 | LT / L2          | ZL            |
| 7                 | RT / R2          | ZR            |
| 8                 | Back / Select    | −             |
| 9                 | Start            | +             |
| 10                | L3               | LS            |
| 11                | R3               | RS            |
| 12                | D-pad Up         | D-pad Up      |
| 13                | D-pad Down       | D-pad Down    |
| 14                | D-pad Left       | D-pad Left    |
| 15                | D-pad Right      | D-pad Right   |
| 16                | Home / Guide     | Home          |
| 17                | Screenshot       | Screenshot    |

Stick axes are mapped linearly: Web −1.0 → Switch 0x000, Web 0.0 → Switch 0x800, Web +1.0 → Switch 0xFFF.

---

## Project layout

```
switch-bt-ws/
├── Cargo.toml
├── build.rs                   Compiles BTStack C sources into libbtstack_gamepad
├── csrc/
│   ├── btstack_platform.c     Windows platform init (modified port/windows-winusb/main.c)
│   └── btstack_stub.c         No-op stubs for non-Windows builds
└── src/
    ├── main.rs                Entry point — spawns BTStack thread, starts axum
    ├── btstack.rs             FFI bindings + safe wrappers
    ├── protocol.rs            WebSocket message types (serde)
    ├── gamepad.rs             Web Gamepad → Switch button/stick mapping
    ├── ws_server.rs           WebSocket handler
    ├── api.rs                 HTTP REST API router
    └── driver.rs              Windows driver management (pnputil / devcon)
```

---

## Pairing with the Switch

1. Start the server (dongle must have WinUSB driver installed)
2. On the Switch: **Controllers** → **Change Grip/Order** → press the **Sync** button
3. Wait — the emulated "Pro Controller" should appear and pair automatically
4. The `/api/status` endpoint will show `"paired": true`

Re-pairing after the dongle is unplugged does not require repeating the Switch
pairing flow; the controller will reconnect automatically (link key is stored
in a `.tlv` file next to the executable).
