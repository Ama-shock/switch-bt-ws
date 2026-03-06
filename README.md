# switch-bt-ws

Nintendo Switch の Pro Controller を Bluetooth 経由でエミュレートし、
WebSocket でブラウザから操作するシングルバイナリの Rust プログラムです。

---

## 概要

```
ブラウザ（Web Gamepad API）
       │  WebSocket JSON
       ▼
switch-bt-ws.exe  （Rust）
  ├── Tokio 非同期ランタイム
  │   ├── axum HTTP サーバー  :8765
  │   │   ├── GET  /ws                  WebSocket エンドポイント
  │   │   ├── GET  /api/status          接続状態・振動状態
  │   │   ├── GET  /api/driver/list     BT USB デバイス一覧
  │   │   ├── POST /api/driver/install  WinUSB ドライバ導入
  │   │   └── POST /api/driver/restore  元の BT ドライバに戻す
  │   └── WebSocket ハンドラ
  │       ├── 受信: gamepad_state → FFI 呼び出し
  │       └── 送信: ステータスブロードキャスト（100ms 間隔）
  └── BTStack スレッド（ブロッキング C コード）
      ├── btstack_platform_run()
      └── btkeyLib: Pro Controller HID エミュレーション
              │ WinUSB / BTStack HCI
              ▼
        Bluetooth ドングル → Nintendo Switch
```

---

## スレッドモデル

| スレッド | 役割 |
|---------|------|
| `btstack`（OS スレッド） | `btstack_run_loop_execute()` を実行。シャットダウンまでブロック |
| Tokio ワーカー | HTTP サーバー・WebSocket・ステータスブロードキャスト |

BTStack の C グローバル変数（`button_flg` 等）は Tokio 側が書き、BTStack 側が約 60Hz で読みます。
x86-64 ではアラインメントされた 32bit のストアはハードウェアレベルでアトミックになるため実用上問題ありません。
将来的には `_Atomic` 修飾を btkeyLib.c に追加することで正式に安全にできます。

---

## 必要なもの

### ハードウェア
- USB Bluetooth 4.0 以上のドングル（CSR・BCM・RTL チップセット推奨）
- Nintendo Switch

### ソフトウェア
- Windows 10/11（64bit）
- Rust stable ツールチェーン（`x86_64-pc-windows-msvc` または `x86_64-pc-windows-gnu`）
- Visual Studio Build Tools **または** mingw-w64（C コンパイル用）
- Windows SDK（WinUSB ヘッダー。VS Build Tools に同梱）

---

## ドライバ設定（ドングルごとに 1 回だけ）

BTStack は OS の `BthUsb.sys` の代わりに WinUSB ドライバが必要です。

### 方法 A — Zadig（GUI、推奨）

1. [Zadig](https://zadig.akeo.ie) をダウンロード
2. Options → "List all devices" を有効化
3. プルダウンから Bluetooth ドングルを選択
4. 右側のドライバ選択を **WinUSB** にする
5. **Replace Driver** をクリック

### 方法 B — REST API（プログラム的）

まずデバイスマネージャーでドングルの VID/PID を調べてから：

```sh
# WinUSB ドライバを導入
curl -X POST http://localhost:8765/api/driver/install \
  -H "Content-Type: application/json" \
  -d '{"vid": 2652, "pid": 1}'

# 元の Bluetooth ドライバに戻す
curl -X POST http://localhost:8765/api/driver/restore \
  -H "Content-Type: application/json" \
  -d '{"vid": 2652, "pid": 1}'
```

> **管理者権限が必要です。** サーバーは「管理者として実行」してください。

---

## ビルド

```powershell
# ディレクトリ構成の前提
#   repos/btstack/windows/        ← mizuyoukanao/btstack クローン
#   repos/btstack/switch-bt-ws/   ← このプロジェクト

cd repos\btstack\switch-bt-ws
cargo build --release
```

`build.rs` が `cc` クレートを使って BTStack の全 C ソースを自動コンパイルします。

---

## 実行

```powershell
# 管理者として実行（WinUSB / BTStack HCI アクセスに必要）
.\target\release\switch-bt-ws.exe
```

詳細ログを出す場合：

```powershell
$env:RUST_LOG = "debug"
.\target\release\switch-bt-ws.exe
```

---

## WebSocket プロトコル

`ws://localhost:8765/ws` に接続してください。

### クライアント → サーバー

```jsonc
// ゲームパッド入力（毎アニメーションフレーム送信）
{
  "type": "gamepad_state",
  "buttons": [0.0, 1.0, 0.0, ...],  // Web Gamepad ボタン値（0.0〜1.0）
  "axes":    [-0.5, 0.3, 0.0, 1.0]  // [左X, 左Y, 右X, 右Y]（-1.0〜1.0）
}

// モーションセンサー（DeviceMotion API、任意）
{ "type": "motion", "gyro": [0, 0, 0], "accel": [100, 100, 100] }

// コントローラーカラー変更（0x00RRGGBB）
{
  "type": "set_color",
  "pad_color": 16711680,
  "button_color": 0,
  "left_grip_color": 0,
  "right_grip_color": 0
}

// Amiibo 送信（サーバー側のファイルパス、540バイト .bin）
{ "type": "send_amiibo", "path": "C:\\amiibo\\pikachu.bin" }
```

### サーバー → クライアント

```jsonc
// 約 100ms ごとにプッシュ
{ "type": "status", "paired": true, "rumble": false }

// 不正なメッセージへのエラー応答
{ "type": "error", "message": "Invalid message: ..." }
```

---

## ボタンマッピング

| Web Gamepad インデックス | Web ボタン         | Switch ボタン |
|:----------------------:|-------------------|--------------|
| 0                      | A（下面ボタン）    | B            |
| 1                      | B（右面ボタン）    | A            |
| 2                      | X（左面ボタン）    | Y            |
| 3                      | Y（上面ボタン）    | X            |
| 4                      | LB / L1           | L            |
| 5                      | RB / R1           | R            |
| 6                      | LT / L2           | ZL           |
| 7                      | RT / R2           | ZR           |
| 8                      | Back / Select     | −            |
| 9                      | Start             | +            |
| 10                     | L3                | LS           |
| 11                     | R3                | RS           |
| 12                     | 十字キー 上        | 十字キー 上  |
| 13                     | 十字キー 下        | 十字キー 下  |
| 14                     | 十字キー 左        | 十字キー 左  |
| 15                     | 十字キー 右        | 十字キー 右  |
| 16                     | Home / Guide      | HOME         |
| 17                     | Screenshot        | キャプチャ   |

スティック軸は線形マッピング：Web −1.0 → Switch 0x000、Web 0.0 → Switch 0x800、Web +1.0 → Switch 0xFFF

---

## REST API

| メソッド | パス | 説明 |
|---------|------|------|
| GET  | `/api/status` | `{ paired, rumble }` を返す |
| GET  | `/api/driver/list` | BT USB デバイス一覧 |
| POST | `/api/driver/install` | WinUSB ドライバを導入 `{ vid, pid }` |
| POST | `/api/driver/restore` | 元の BT ドライバに戻す `{ vid, pid }` |

---

## Switch とのペアリング手順

1. サーバーを起動（ドングルに WinUSB ドライバが適用済みであること）
2. Switch で **コントローラー → コントローラーの持ち方/順番を変える** を開く
3. 数秒待つ → 「Pro Controller」として自動ペアリング
4. `/api/status` が `"paired": true` になれば完了

再接続時は Switch 側の再ペアリング操作不要です。
リンクキーが実行ファイルと同じディレクトリに `btstack_<BD_ADDR>.tlv` として保存されます。

---

## プロジェクト構成

```
switch-bt-ws/
├── Cargo.toml
├── build.rs                   BTStack C ソースを libbtstack_gamepad にコンパイル
├── csrc/
│   ├── btstack_platform.c     Windows プラットフォーム初期化
│   │                          （port/windows-winusb/main.c の改変版。
│   │                            main() を btstack_platform_run() にリネーム）
│   └── btstack_stub.c         非 Windows ビルド用のスタブ
└── src/
    ├── main.rs                エントリーポイント
    ├── btstack.rs             C FFI バインディング + 安全な Wrapper
    ├── protocol.rs            WebSocket メッセージ型（serde JSON）
    ├── gamepad.rs             Web Gamepad → Switch マッピング
    ├── ws_server.rs           WebSocket ハンドラ
    ├── api.rs                 HTTP REST API ルーター
    └── driver.rs              Windows ドライバ管理（pnputil / devcon）
```
