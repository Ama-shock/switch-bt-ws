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

    /// ドライバパッケージを保存するベースディレクトリ
    const DRIVER_BASE_DIR: &str = r"C:\Windows\Temp\btstack_drivers";

    fn inf_name(vid: u16, pid: u16) -> String {
        format!("btstack_{vid:04x}_{pid:04x}.inf")
    }
    fn cat_name(vid: u16, pid: u16) -> String {
        format!("btstack_{vid:04x}_{pid:04x}.cat")
    }
    /// INF + .cat を格納する VID/PID 別ディレクトリ
    fn driver_package_dir(vid: u16, pid: u16) -> String {
        format!(r"{DRIVER_BASE_DIR}\{vid:04x}_{pid:04x}")
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
    ///
    /// 手順（Zadig / libwdi と同等）:
    /// 1. INF + CatalogFile を書き出す
    /// 2. 自己署名コード署名証明書を作成（既存なら再利用）
    /// 3. 証明書を Root + TrustedPublisher ストアに追加
    /// 4. `New-FileCatalog` で .cat ファイルを生成
    /// 5. `Set-AuthenticodeSignature` で .cat に署名
    /// 6. `UpdateDriverForPlugAndPlayDevicesW` でデバイスのドライバを更新
    ///
    /// 管理者権限が必要です。
    pub async fn install_winusb(vid: u16, pid: u16) -> Result<String> {
        let dir = driver_package_dir(vid, pid);
        let inf = inf_name(vid, pid);
        let cat = cat_name(vid, pid);

        // ドライバパッケージディレクトリを作成し INF を書き出す
        std::fs::create_dir_all(&dir)
            .with_context(|| format!("ディレクトリの作成に失敗しました: {dir}"))?;
        let inf_full = format!(r"{dir}\{inf}");
        write_winusb_inf(vid, pid, &inf_full)?;

        let hwid = format!(r"USB\VID_{vid:04X}&PID_{pid:04X}");

        // PowerShell スクリプト:
        //   自己署名証明書 → カタログ作成・署名 → UpdateDriverForPlugAndPlayDevices
        let ps_script = format!(
            r#"
$ErrorActionPreference = 'Stop'
$certSubject = 'CN=switch-bt-ws WinUSB Driver'
$dir = '{dir}'
$infFile = '{inf}'
$catFile = '{cat}'
$hwid = '{hwid}'

# --- 1. 自己署名コード署名証明書 (既存なら再利用) ---
$cert = Get-ChildItem Cert:\LocalMachine\My -CodeSigningCert |
    Where-Object {{ $_.Subject -eq $certSubject -and $_.NotAfter -gt (Get-Date) }} |
    Select-Object -First 1
if (-not $cert) {{
    $cert = New-SelfSignedCertificate `
        -Subject $certSubject `
        -Type CodeSigningCert `
        -CertStoreLocation Cert:\LocalMachine\My `
        -NotAfter (Get-Date).AddYears(10)
}}

# --- 2. Root + TrustedPublisher に追加（既にあればスキップ） ---
$thumb = $cert.Thumbprint
foreach ($store in @('Root', 'TrustedPublisher')) {{
    $existing = Get-ChildItem "Cert:\LocalMachine\$store" |
        Where-Object {{ $_.Thumbprint -eq $thumb }}
    if (-not $existing) {{
        $tmpCer = [System.IO.Path]::GetTempFileName() + '.cer'
        Export-Certificate -Cert $cert -FilePath $tmpCer -Type CERT | Out-Null
        Import-Certificate -FilePath $tmpCer -CertStoreLocation "Cert:\LocalMachine\$store" | Out-Null
        Remove-Item $tmpCer -ErrorAction SilentlyContinue
    }}
}}

# --- 3. カタログファイル (.cat) を作成 ---
$catPath = Join-Path $dir $catFile
New-FileCatalog -Path $dir -CatalogFilePath $catPath -CatalogVersion 2.0 | Out-Null

# --- 4. カタログに署名 ---
Set-AuthenticodeSignature -FilePath $catPath -Certificate $cert -HashAlgorithm SHA256 | Out-Null

# --- 5. UpdateDriverForPlugAndPlayDevicesW でドライバ適用 ---
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
}}
"@

$infPath = Join-Path $dir $infFile
$reboot = $false
$result = [WinUsbInstaller]::UpdateDriverForPlugAndPlayDevicesW(
    [IntPtr]::Zero, $hwid, $infPath, 0x01, [ref]$reboot)
if (-not $result) {{
    $err = [System.Runtime.InteropServices.Marshal]::GetLastWin32Error()
    $ex = New-Object System.ComponentModel.Win32Exception($err)
    throw "UpdateDriver failed ($err): $($ex.Message)"
}}
if ($reboot) {{ Write-Output 'REBOOT_REQUIRED' }}
Write-Output 'OK'
"#,
            dir = dir.replace('\'', "''"),
            inf = inf,
            cat = cat,
            hwid = hwid,
        );

        let output = tokio::process::Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", &ps_script])
            .output()
            .await
            .context("WinUSB ドライバ導入スクリプトの実行に失敗しました")?;

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

    /// WinUSB 用の INF ファイルを生成する（Zadig / libwdi 互換）。
    /// CatalogFile を含めることで署名済みカタログと紐づく。
    fn write_winusb_inf(vid: u16, pid: u16, path: &str) -> Result<()> {
        let cat = cat_name(vid, pid);
        let inf = format!(
            r#"[Version]
Signature   = "$Windows NT$"
Class       = USBDevice
ClassGUID   = {{88BAE032-5A81-49f0-BC3D-A4FF138216D6}}
Provider    = %ProviderName%
CatalogFile = {cat}
DriverVer   = 01/01/2024,1.0.0.0

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
            cat = cat,
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
    /// PowerShell で以下を実行:
    /// 1. ドライバストアから switch-bt-ws の OEM INF を検索
    /// 2. `pnputil /delete-driver /uninstall /force` で削除
    /// 3. デバイスを無効→有効にして Windows に標準ドライバを再適用させる
    /// 4. パッケージディレクトリを削除
    ///
    /// 管理者権限が必要です。
    pub async fn restore_driver(vid: u16, pid: u16) -> Result<String> {
        let hwid = format!(r"USB\VID_{vid:04X}&PID_{pid:04X}");
        let dir = driver_package_dir(vid, pid);
        let inf_original = inf_name(vid, pid);
        let vid_hex = format!("{vid:04X}");
        let pid_hex = format!("{pid:04X}");

        let ps_script = format!(
            r#"
$ErrorActionPreference = 'Stop'
$hwid = '{hwid}'
$infOriginal = '{inf_original}'
$vidPidPattern = 'VID_{vid_hex}&PID_{pid_hex}'

# --- 1. ドライバストアから該当 OEM INF を探す ---
# pnputil /enum-drivers は JSON 未対応なので WMI で代替
$oemInf = $null
$drivers = Get-WmiObject Win32_PnPSignedDriver |
    Where-Object {{ $_.InfName -match '^oem\d+\.inf$' -and $_.DeviceID -match $vidPidPattern }}
if ($drivers) {{
    $first = @($drivers)[0]
    $oemInf = $first.InfName
    Write-Host "Found OEM INF: $oemInf"
}}

# WMI で見つからない場合は pnputil テキスト解析にフォールバック
if (-not $oemInf) {{
    $output = pnputil /enum-drivers 2>&1 | Out-String
    $blocks = $output -split '(?=Published Name)' | Where-Object {{ $_.Trim() }}
    foreach ($block in $blocks) {{
        if ($block -match 'switch-bt-ws' -or $block -match $infOriginal) {{
            if ($block -match 'Published Name\s*:\s*(oem\d+\.inf)') {{
                $oemInf = $Matches[1]
                Write-Host "Found OEM INF (pnputil): $oemInf"
                break
            }}
        }}
    }}
}}

# --- 2. OEM INF を削除 ---
if ($oemInf) {{
    $delResult = pnputil /delete-driver $oemInf /uninstall /force 2>&1 | Out-String
    Write-Host "pnputil delete: $delResult"
}} else {{
    Write-Host "OEM INF not found in driver store, skipping delete"
}}

# --- 3. デバイスを無効→有効にして OS に標準ドライバを再割り当てさせる ---
$dev = Get-PnpDevice | Where-Object {{ $_.InstanceId -match $vidPidPattern }}
if ($dev) {{
    $instId = $dev.InstanceId
    Write-Host "Restarting device: $instId"
    Disable-PnpDevice -InstanceId $instId -Confirm:$false -ErrorAction SilentlyContinue
    Start-Sleep -Seconds 2
    Enable-PnpDevice -InstanceId $instId -Confirm:$false -ErrorAction SilentlyContinue
    Start-Sleep -Seconds 2
    # 結果確認
    $updated = Get-PnpDevice -InstanceId $instId -ErrorAction SilentlyContinue
    $svc = (Get-PnpDeviceProperty -InstanceId $instId -KeyName DEVPKEY_Device_Service -ErrorAction SilentlyContinue).Data
    Write-Host "Device service after restore: $svc"
}} else {{
    # デバイスが見つからない場合はハードウェアスキャンを実行
    pnputil /scan-devices 2>&1 | Out-String | Write-Host
}}

# --- 4. パッケージディレクトリを削除 ---
$dir = '{dir}'
if (Test-Path $dir) {{
    Remove-Item $dir -Recurse -Force -ErrorAction SilentlyContinue
}}

Write-Output 'OK'
"#,
            hwid = hwid,
            inf_original = inf_original,
            vid_hex = vid_hex,
            pid_hex = pid_hex,
            dir = dir.replace('\'', "''"),
        );

        let output = tokio::process::Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", &ps_script])
            .output()
            .await
            .context("ドライバ復元スクリプトの実行に失敗しました")?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        tracing::info!("restore stdout: {stdout}");
        if !stderr.trim().is_empty() {
            tracing::warn!("restore stderr: {stderr}");
        }

        if output.status.success() && stdout.contains("OK") {
            let msg = format!("ドライバを復元しました ({vid:04x}:{pid:04x})");
            tracing::info!("{msg}");
            Ok(msg)
        } else {
            let detail = if !stderr.trim().is_empty() {
                stderr.trim().to_string()
            } else {
                stdout.trim().to_string()
            };
            Err(anyhow::anyhow!("ドライバの復元に失敗しました: {detail}"))
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
