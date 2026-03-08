//! Windows Bluetooth ドングルのドライバ管理。
//!
//! BTStack は OS 標準の `BthUsb.sys` の代わりに WinUSB ドライバが必要です。
//! このモジュールは以下の 3 操作を提供します：
//!
//! 1. **一覧取得** — Bluetooth ドングルと思われる USB デバイスを列挙する。
//! 2. **導入**     — WinUSB INF ファイルを生成し `pnputil.exe` で適用する。
//! 3. **復元**     — WinUSB INF を削除し、次回接続時に Windows が元の
//!                   Bluetooth ドライバを再適用できるようにする
//!                   （`devcon update` でその場で適用も試みる）。
//!
//! ドライバ変更にはすべて **管理者権限** が必要です。
//! サーバーを管理者として起動するか、UAC の確認ダイアログを受け入れてください。
//!
//! Windows 以外のビルドでは全関数が空のスタブになるため、
//! Linux 等でも問題なくコンパイルできます。

use anyhow::{Context, Result};
use serde::Serialize;

// ---------------------------------------------------------------------------
// 公開型
// ---------------------------------------------------------------------------

/// Bluetooth USB デバイスの情報。
#[derive(Debug, Clone, Serialize)]
pub struct BtUsbDevice {
    /// USB ベンダー ID（小文字16進数、例: "0a12"）
    pub vid: String,
    /// USB プロダクト ID（小文字16進数、例: "0001"）
    pub pid: String,
    /// デバイスの説明文字列
    pub description: String,
    /// 現在適用中のドライバ（例: "WinUSB"、"BthUsb"、"Unknown"）
    pub driver: String,
    /// 同一 VID/PID デバイスが複数ある場合のインスタンス番号（0 始まり）
    pub instance: u32,
}

// ---------------------------------------------------------------------------
// Windows 実装
// ---------------------------------------------------------------------------

#[cfg(windows)]
mod platform {
    use super::*;

    /// INF ファイルを保存するディレクトリ
    const INF_DIR: &str = r"C:\Windows\Temp";

    fn inf_name(vid: u16, pid: u16) -> String {
        format!("btstack_{vid:04x}_{pid:04x}.inf")
    }
    fn inf_path(vid: u16, pid: u16) -> String {
        format!(r"{INF_DIR}\{}", inf_name(vid, pid))
    }

    // ---- デバイス一覧 -------------------------------------------------------

    /// Bluetooth ドングルと思われる USB デバイスを PowerShell / WMI で列挙する。
    ///
    /// WinUSB に入れ替え済みのデバイスは PNPClass が "USBDevice" に変わるため、
    /// Bluetooth クラスだけでなく以下の条件でも取得する:
    ///   - PNPClass が "Bluetooth" / Description に "Bluetooth" を含む
    ///   - Service が "WinUSB"（BTStack 用に入れ替え済み）
    ///   - DeviceID が USB\VID_ で始まり、既知の BT ドングル VID/PID に一致する
    pub async fn list_bt_usb_devices() -> Result<Vec<BtUsbDevice>> {
        let output = tokio::process::Command::new("powershell")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                r#"
Get-WmiObject Win32_PnPEntity |
  Where-Object {
    ($_.DeviceID -match '^USB\\') -and (
      $_.PNPClass -eq 'Bluetooth' -or
      $_.Description -match 'Bluetooth' -or
      $_.Service -eq 'WinUSB' -or
      $_.Service -eq 'WinUsb'
    )
  } |
  Select-Object DeviceID, Description, Service, PNPClass |
  ConvertTo-Json -Compress
"#,
            ])
            .output()
            .await
            .context("PowerShell によるデバイス列挙に失敗しました")?;

        if !output.status.success() {
            let msg = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow::anyhow!("PowerShell エラー: {msg}"));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stdout = stdout.trim();
        if stdout.is_empty() || stdout == "null" {
            return Ok(vec![]);
        }

        // PowerShell は単一オブジェクトまたは配列を返す場合があるため統一する
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
            #[serde(rename = "PNPClass")]
            pnp_class: Option<String>,
        }

        let entries: Vec<PnpEntry> = serde_json::from_str(&json_str).unwrap_or_default();

        // 同一 VID/PID のデバイスにインスタンス番号を付与するためカウンターを管理
        let mut instance_counters: std::collections::HashMap<(String, String), u32> = std::collections::HashMap::new();

        let mut devices = Vec::new();
        for entry in entries {
            let device_id = entry.device_id.unwrap_or_default();
            // USB\ で始まるデバイスのみ対象
            if !device_id.starts_with("USB\\") {
                continue;
            }
            // "USB\VID_0A12&PID_0001\..." 形式から VID/PID を抽出
            let (vid, pid) = parse_vid_pid(&device_id);
            if vid == "0000" && pid == "0000" {
                continue;
            }
            let key = (vid.clone(), pid.clone());
            let instance = *instance_counters.entry(key).and_modify(|c| *c += 1).or_insert(0);
            let driver = entry.service.unwrap_or_else(|| "不明".into());
            devices.push(BtUsbDevice {
                vid,
                pid,
                description: entry.description.unwrap_or_else(|| "不明".into()),
                driver,
                instance,
            });
        }
        Ok(devices)
    }

    fn parse_vid_pid(device_id: &str) -> (String, String) {
        let upper = device_id.to_uppercase();
        let vid = extract_id_field(&upper, "VID_");
        let pid = extract_id_field(&upper, "PID_");
        (vid.to_lowercase(), pid.to_lowercase())
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

    // ---- WinUSB ドライバ導入 -----------------------------------------------

    /// 指定した VID/PID の USB デバイスに WinUSB ドライバを導入する。
    /// INF ファイルを生成し `UpdateDriverForPlugAndPlayDevicesW` (newdev.dll) で
    /// デバイスのドライバを強制更新します。
    /// pnputil は署名なし INF を拒否するため、Win32 API を直接使用します。
    /// 署名なしドライバの場合、Windows が信頼確認ダイアログを表示します。
    /// 管理者権限が必要です。
    pub async fn install_winusb(vid: u16, pid: u16) -> Result<String> {
        let inf_path = inf_path(vid, pid);

        // WinUSB 用 INF ファイルを書き出す
        write_winusb_inf(vid, pid, &inf_path)?;

        let hwid = format!("USB\\\\VID_{vid:04X}&PID_{pid:04X}");

        // PowerShell 経由で UpdateDriverForPlugAndPlayDevicesW を P/Invoke 呼び出し。
        // この API は HWID を指定して該当デバイスのドライバを直接更新する。
        // DiInstallDriverW はドライバストアへの追加のみで既存デバイスを
        // 更新しないことがあるため、こちらを使用する。
        let ps_script = format!(
            r#"
Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
using System.ComponentModel;

public class WinUsbInstaller {{
    [DllImport("newdev.dll", SetLastError = true, CharSet = CharSet.Unicode)]
    public static extern bool UpdateDriverForPlugAndPlayDevicesW(
        IntPtr hwndParent,
        string HardwareId,
        string FullInfPath,
        uint InstallFlags,
        out bool bRebootRequired
    );

    public static void Install(string hwid, string infPath) {{
        bool needReboot = false;
        // INSTALLFLAG_FORCE = 0x01 : 現在のドライバより低ランクでも強制適用
        bool result = UpdateDriverForPlugAndPlayDevicesW(
            IntPtr.Zero, hwid, infPath, 0x01, out needReboot);
        if (!result) {{
            int err = Marshal.GetLastWin32Error();
            throw new Win32Exception(err);
        }}
        if (needReboot) {{
            Console.WriteLine("REBOOT_REQUIRED");
        }}
    }}
}}
"@

[WinUsbInstaller]::Install("{hwid}", "{inf_path}")
Write-Output "OK"
"#,
            hwid = hwid,
            inf_path = inf_path.replace('\\', "\\\\"),
        );

        let output = tokio::process::Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", &ps_script])
            .output()
            .await
            .context("UpdateDriverForPlugAndPlayDevicesW の実行に失敗しました")?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        if output.status.success() && stdout.contains("OK") {
            let mut msg = format!("WinUSB ドライバを導入しました ({vid:04x}:{pid:04x})");
            if stdout.contains("REBOOT_REQUIRED") {
                msg.push_str("（再起動が必要です）");
            }
            tracing::info!("{msg}");
            Ok(msg)
        } else {
            let detail = if !stderr.trim().is_empty() {
                stderr.trim().to_string()
            } else {
                stdout.trim().to_string()
            };
            Err(anyhow::anyhow!("WinUSB ドライバの導入に失敗しました: {detail}"))
        }
    }

    /// WinUSB 用の最小限の INF ファイルを生成する。
    /// Zadig / libwdi が生成するものと同等の内容です。
    fn write_winusb_inf(vid: u16, pid: u16, path: &str) -> Result<()> {
        let inf = format!(
            r#"[Version]
Signature   = "$Windows NT$"
Class       = "USBDevice"
ClassGUID   = {{88BAE032-5A81-49f0-BC3D-A4FF138216D6}}
Provider    = %ProviderName%
DriverVer   = 06/21/2006,6.1.7600.16385

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
            .with_context(|| format!("INF ファイルの書き込みに失敗しました: {path}"))?;
        Ok(())
    }

    // ---- 元ドライバへの復元 -------------------------------------------------

    /// WinUSB ドライバを削除し、元の Bluetooth ドライバに戻す。
    ///
    /// 戦略 1: `pnputil /delete-driver /uninstall /force` で INF を削除する。
    /// 戦略 2: `devcon update` で直接 BthUsb ドライバを適用する（ベストエフォート）。
    pub async fn restore_driver(vid: u16, pid: u16) -> Result<String> {
        let inf = inf_name(vid, pid);

        // まず pnputil でドライバストアから INF を削除する
        let del = tokio::process::Command::new("pnputil")
            .args(["/delete-driver", &inf, "/uninstall", "/force"])
            .output()
            .await;

        match del {
            Ok(out) if out.status.success() => {
                let msg = format!("WinUSB ドライバを削除しました ({vid:04x}:{pid:04x})");
                tracing::info!("{msg}");
                // さらに devcon で即時ドライバ更新を試みる（ベストエフォート）
                update_via_devcon(vid, pid).await;
                return Ok(msg);
            }
            _ => {
                tracing::warn!("pnputil /delete-driver 失敗。devcon を試みます");
            }
        }

        // pnputil が失敗した場合は devcon で直接適用を試みる
        update_via_devcon(vid, pid).await;
        Ok(format!(
            "{vid:04x}:{pid:04x} のドライバ復元を試みました（デバイスマネージャーで確認してください）"
        ))
    }

    /// devcon.exe を使って Windows 標準 Bluetooth ドライバを適用する。
    /// devcon は WDK / Windows SDK に含まれているため、
    /// インストールされていない環境では警告のみを出してスキップします。
    async fn update_via_devcon(vid: u16, pid: u16) {
        let hwid = format!("USB\\VID_{:04X}&PID_{:04X}", vid, pid);
        let result = tokio::process::Command::new("devcon")
            .args(["update", r"C:\Windows\INF\bth.inf", &hwid])
            .output()
            .await;
        match result {
            Ok(out) if out.status.success() => {
                tracing::info!("devcon update 成功: {hwid}");
            }
            Ok(out) => {
                let stderr = String::from_utf8_lossy(&out.stderr);
                tracing::warn!("devcon update 失敗 ({hwid}): {stderr}");
            }
            Err(e) => {
                tracing::warn!(
                    "devcon が見つからないか実行できません ({e})。手動でドライバを復元してください"
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 非 Windows スタブ
// ---------------------------------------------------------------------------

#[cfg(not(windows))]
mod platform {
    use super::*;

    pub async fn list_bt_usb_devices() -> Result<Vec<BtUsbDevice>> {
        Ok(vec![])
    }

    pub async fn install_winusb(_vid: u16, _pid: u16) -> Result<String> {
        Err(anyhow::anyhow!("ドライバ管理は Windows のみサポートしています"))
    }

    pub async fn restore_driver(_vid: u16, _pid: u16) -> Result<String> {
        Err(anyhow::anyhow!("ドライバ管理は Windows のみサポートしています"))
    }
}

// ---------------------------------------------------------------------------
// 公開再エクスポート
// ---------------------------------------------------------------------------

pub use platform::{install_winusb, list_bt_usb_devices, restore_driver};
