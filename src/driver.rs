//! Windows Bluetooth dongle driver management.
//!
//! BTStack needs the WinUSB driver instead of the native Windows Bluetooth
//! HCI driver.  This module provides three operations:
//!
//! 1. **list**    — enumerate USB devices that look like Bluetooth dongles.
//! 2. **install** — write a WinUSB INF and apply it with `pnputil.exe`.
//! 3. **restore** — remove the WinUSB INF so Windows reinstalls the original
//!                  Bluetooth driver on next plug-in (or immediately via
//!                  `devcon update`).
//!
//! All driver changes require an **Administrator** token.  The server should
//! be started with elevated privileges, or UAC prompts will appear.
//!
//! On non-Windows targets every function returns empty / no-op results so the
//! crate still compiles for development.

use anyhow::{Context, Result};
use serde::Serialize;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct BtUsbDevice {
    /// USB Vendor ID (hex string, e.g. "0a12")
    pub vid: String,
    /// USB Product ID (hex string, e.g. "0001")
    pub pid: String,
    /// Human-readable device description
    pub description: String,
    /// Currently active driver (e.g. "WinUSB", "BthUsb", "Unknown")
    pub driver: String,
}

// ---------------------------------------------------------------------------
// Windows implementation
// ---------------------------------------------------------------------------

#[cfg(windows)]
mod platform {
    use super::*;

    const INF_DIR: &str = r"C:\Windows\Temp";

    fn inf_name(vid: u16, pid: u16) -> String {
        format!("btstack_{vid:04x}_{pid:04x}.inf")
    }
    fn inf_path(vid: u16, pid: u16) -> String {
        format!(r"{INF_DIR}\{}", inf_name(vid, pid))
    }

    // ---- List ---------------------------------------------------------------

    pub async fn list_bt_usb_devices() -> Result<Vec<BtUsbDevice>> {
        // Use PowerShell + Win32_PnPEntity to find Bluetooth USB devices.
        // Filtering on Class == "Bluetooth" catches most dongles.
        let output = tokio::process::Command::new("powershell")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                r#"
Get-WmiObject Win32_PnPEntity |
  Where-Object { $_.PNPClass -eq 'Bluetooth' -or $_.Description -match 'Bluetooth' } |
  Select-Object DeviceID, Description, Service |
  ConvertTo-Json -Compress
"#,
            ])
            .output()
            .await
            .context("Failed to run PowerShell for device enumeration")?;

        if !output.status.success() {
            let msg = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow::anyhow!("PowerShell error: {msg}"));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stdout = stdout.trim();
        if stdout.is_empty() || stdout == "null" {
            return Ok(vec![]);
        }

        // PowerShell may return a single object or an array.
        // Normalise to array.
        let json_str = if stdout.starts_with('[') {
            stdout.to_string()
        } else {
            format!("[{stdout}]")
        };

        #[derive(serde::Deserialize)]
        struct PnpEntry {
            #[serde(rename = "DeviceID")]
            device_id: Option<String>,
            #[serde(rename = "Description")]
            description: Option<String>,
            #[serde(rename = "Service")]
            service: Option<String>,
        }

        let entries: Vec<PnpEntry> = serde_json::from_str(&json_str)
            .unwrap_or_default();

        let mut devices = Vec::new();
        for entry in entries {
            let device_id = entry.device_id.unwrap_or_default();
            // Parse VID/PID from device ID like "USB\VID_0A12&PID_0001\..."
            let (vid, pid) = parse_vid_pid(&device_id);
            devices.push(BtUsbDevice {
                vid,
                pid,
                description: entry.description.unwrap_or_else(|| "Unknown".into()),
                driver: entry.service.unwrap_or_else(|| "Unknown".into()),
            });
        }
        Ok(devices)
    }

    fn parse_vid_pid(device_id: &str) -> (String, String) {
        let upper = device_id.to_uppercase();
        let vid = extract_id_field(&upper, "VID_");
        let pid = extract_id_field(&upper, "PID_");
        (
            vid.to_lowercase(),
            pid.to_lowercase(),
        )
    }

    fn extract_id_field<'a>(s: &'a str, prefix: &str) -> &'a str {
        if let Some(pos) = s.find(prefix) {
            let start = pos + prefix.len();
            let end = s[start..]
                .find(|c: char| !c.is_ascii_hexdigit())
                .map(|i| start + i)
                .unwrap_or(s.len());
            &s[start..end]
        } else {
            "0000"
        }
    }

    // ---- Install WinUSB driver ----------------------------------------------

    pub async fn install_winusb(vid: u16, pid: u16) -> Result<String> {
        let inf_path = inf_path(vid, pid);

        // Write the INF file.
        write_winusb_inf(vid, pid, &inf_path)?;

        // Install via pnputil — requires administrator rights.
        let output = tokio::process::Command::new("pnputil")
            .args(["/add-driver", &inf_path, "/install"])
            .output()
            .await
            .context("Failed to run pnputil /add-driver")?;

        if output.status.success() {
            tracing::info!("WinUSB driver installed for {:04x}:{:04x}", vid, pid);
            Ok(format!("WinUSB driver installed for {:04x}:{:04x}", vid, pid))
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(anyhow::anyhow!("pnputil /add-driver failed: {stderr}"))
        }
    }

    fn write_winusb_inf(vid: u16, pid: u16, path: &str) -> Result<()> {
        // A minimal WinUSB INF.  Generated at runtime so no bundled files are
        // needed.  This matches what Zadig/libwdi produces for generic devices.
        let inf = format!(
            r#"[Version]
Signature   = "$Windows NT$"
Class       = "USBDevice"
ClassGUID   = {{88BAE032-5A81-49f0-BC3D-A4FF138216D6}}
Provider    = %ProviderName%
DriverVer   = 06/21/2006,6.1.7600.16385
CatalogFile = winusbcoinstaller.cat

[ClassInstall32]
AddReg = WinUSBDeviceClassReg

[WinUSBDeviceClassReg]
HKR,,,0,%ClassName%
HKR,,Icon,,-20

[Manufacturer]
%ProviderName% = libusbDevice_WinUSB,NTamd64

[libusbDevice_WinUSB.NTamd64]
%DeviceName% = USB_Install, USB\VID_{vid:04X}&PID_{pid:04X}

[USB_Install]
Include = winusb.inf
Needs   = WINUSB.NT

[USB_Install.Services]
Include    = winusb.inf
AddService = WinUSB, 0x00000002, WinUSB_ServiceInstall

[WinUSB_ServiceInstall]
DisplayName    = %WinUSB_SvcDesc%
ServiceType    = 1
StartType      = 3
ErrorControl   = 1
ServiceBinary  = %12%\WinUSB.sys

[USB_Install.Wdf]
KmdfService = WINUSB, WinUsb_wdfcoinstaller

[WinUsb_wdfcoinstaller]
KmdfLibraryVersion = 1.9

[USB_Install.HW]
AddReg = Dev_AddReg

[Dev_AddReg]
HKR,,DeviceInterfaceGUIDs,0x10000,"{{B9F765A9-E7D4-4B73-9ADC-F2F4E2C34E7B}}"

[Strings]
ProviderName   = "switch-bt-ws"
ClassName      = "USB Devices"
DeviceName     = "Bluetooth Dongle (WinUSB)"
WinUSB_SvcDesc = "WinUSB"
"#,
            vid = vid,
            pid = pid,
        );

        std::fs::write(path, inf)
            .with_context(|| format!("Failed to write INF to {path}"))?;
        Ok(())
    }

    // ---- Restore original driver --------------------------------------------

    pub async fn restore_driver(vid: u16, pid: u16) -> Result<String> {
        // Strategy 1: Remove our WinUSB INF from the driver store.
        // After removal, Windows will re-scan and apply the next-best driver
        // (usually the inbox BthUsb driver) on next plug-in or after devcon.
        let inf = inf_name(vid, pid);
        let del = tokio::process::Command::new("pnputil")
            .args(["/delete-driver", &inf, "/uninstall", "/force"])
            .output()
            .await;

        match del {
            Ok(out) if out.status.success() => {
                let msg = format!("WinUSB driver removed for {:04x}:{:04x}", vid, pid);
                tracing::info!("{msg}");
                // Also trigger immediate driver update via devcon (best-effort).
                update_via_devcon(vid, pid).await;
                return Ok(msg);
            }
            _ => {
                tracing::warn!("pnputil /delete-driver failed; trying devcon");
            }
        }

        // Strategy 2: Use devcon to apply the inbox Bluetooth driver directly.
        update_via_devcon(vid, pid).await;
        Ok(format!(
            "Driver restore attempted for {:04x}:{:04x} (check Device Manager)",
            vid, pid
        ))
    }

    async fn update_via_devcon(vid: u16, pid: u16) {
        let hwid = format!("USB\\VID_{:04X}&PID_{:04X}", vid, pid);
        // devcon.exe is not shipped with Windows but is part of the WDK /
        // Windows SDK.  We try it as a best-effort step.
        let result = tokio::process::Command::new("devcon")
            .args(["update", r"C:\Windows\INF\bth.inf", &hwid])
            .output()
            .await;
        match result {
            Ok(out) if out.status.success() => {
                tracing::info!("devcon update succeeded for {hwid}");
            }
            Ok(out) => {
                let stderr = String::from_utf8_lossy(&out.stderr);
                tracing::warn!("devcon update failed for {hwid}: {stderr}");
            }
            Err(e) => {
                tracing::warn!("devcon not found or failed ({e}); driver may need manual restore");
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Non-Windows stub
// ---------------------------------------------------------------------------

#[cfg(not(windows))]
mod platform {
    use super::*;

    pub async fn list_bt_usb_devices() -> Result<Vec<BtUsbDevice>> {
        Ok(vec![])
    }

    pub async fn install_winusb(_vid: u16, _pid: u16) -> Result<String> {
        Err(anyhow::anyhow!("Driver management is only supported on Windows"))
    }

    pub async fn restore_driver(_vid: u16, _pid: u16) -> Result<String> {
        Err(anyhow::anyhow!("Driver management is only supported on Windows"))
    }
}

// ---------------------------------------------------------------------------
// Public re-exports
// ---------------------------------------------------------------------------

pub use platform::{install_winusb, list_bt_usb_devices, restore_driver};
