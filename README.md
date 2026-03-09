# switch-bt-ws

Nintendo Switch の Pro Controller を Bluetooth 経由でエミュレートし、
WebSocket でブラウザから操作するシングルバイナリの Rust プログラムです。

---

## 概要

```
ブラウザ（Web Gamepad API）
       │  WebSocket JSON
       ▼
switch-bt-ws.exe  （Rust / サーバーモード）
  ├── Tokio 非同期ランタイム
  │   ├── axum HTTP サーバー  :8765（SWITCH_BT_WS_PORT で変更可）
  │   │   ├── GET  /ws/:id                 WebSocket（コントローラー指定）
  │   │   ├── GET  /api/controllers        コントローラー一覧
  │   │   ├── POST /api/controllers        コントローラー追加
  │   │   ├── DELETE /api/controllers/:id  コントローラー削除
  │   │   ├── GET  /api/driver/list        BT USB デバイス一覧 + バージョン
  │   │   ├── POST /api/driver/install     WinUSB ドライバ導入
  │   │   ├── POST /api/driver/restore     元の BT ドライバに戻す
  │   │   ├── GET  /api/tlv               リンクキー TLV ファイル一覧
  │   │   ├── GET  /api/tlv/:filename     TLV ダウンロード
  │   │   └── POST /api/tlv/:filename     TLV アップロード
  │   └── WebSocket ハンドラ
  │       ├── 受信: gamepad_state → IPC でワーカーに転送
  │       └── 送信: ステータスブロードキャスト
  └── ワーカーサブプロセス管理
      └── ドングルごとに switch-bt-ws.exe --worker を起動

switch-bt-ws.exe --worker <id> <vid> <pid> <instance>  （ワーカーモード）
  ├── BTStack スレッド（ブロッキング C コード）
  │   ├── btstack_platform_run()
  │   └── btkeyLib: Pro Controller HID エミュレーション
  │           │ WinUSB / BTStack HCI
  │           ▼
  │     Bluetooth ドングル → Nintendo Switch
  └── stdin/stdout NDJSON IPC
```

---

## アーキテクチャ

### マルチコントローラー

ドングルごとに独立したワーカープロセスを起動します。
BTStack はグローバルな C 状態を持つため、プロセス分離で安全にマルチドングルを実現しています。

```
サーバー ─┬─ ワーカー 0 (vid=0411 pid=0374 inst=0) ─── ドングル A ─── Switch A
          └─ ワーカー 1 (vid=0a12 pid=0001 inst=0) ─── ドングル B ─── Switch B
```

### IPC プロトコル（stdin/stdout NDJSON）

サーバー → ワーカー: `WorkerCommand`（ボタン入力、スティック、ジャイロ、シャットダウン等）
ワーカー → サーバー: `WorkerEvent`（ステータス、接続状態）

---

## 必要なもの

### ハードウェア
- USB Bluetooth 4.0 以上のドングル（CSR・BCM・RTL チップセット推奨）
- Nintendo Switch

### ソフトウェア
- Windows 10/11（64bit）

---

## ビルド

Docker を使ったクロスコンパイルが推奨です。

```bash
cd switch-bt-ws

# イメージのビルド（ソース変更後は --no-cache を付ける）
docker compose build --no-cache build

# ビルド実行 → dist/switch-bt-ws.exe に出力
docker compose up build
```

ローカルで直接ビルドする場合：

```powershell
# 前提:
#   repos/btstack/windows/   ← mizuyoukanao/btstack クローン
#   repos/btstack/switch-bt-ws/  ← このプロジェクト

cd repos\btstack\switch-bt-ws
cargo build --release --target x86_64-pc-windows-gnu
```

`build.rs` が `cc` クレートを使って BTStack の全 C ソースを自動コンパイルします。

---

## 実行

exe には UAC マニフェストが埋め込まれており、起動時に自動で管理者権限を要求します。

```powershell
.\dist\switch-bt-ws.exe
```

### 環境変数

| 変数名 | デフォルト | 説明 |
|--------|-----------|------|
| `SWITCH_BT_WS_PORT` | `8765` | HTTP/WS サーバーのリスンポート |
| `RUST_LOG` | `info` | ログレベル（`debug`, `trace` 等） |

```powershell
$env:SWITCH_BT_WS_PORT = "9000"
$env:RUST_LOG = "debug"
.\dist\switch-bt-ws.exe
```

---

## ドライバ設定（ドングルごとに 1 回だけ）

BTStack は OS の `BthUsb.sys` の代わりに WinUSB ドライバが必要です。

### 方法 A — REST API（ブラウザ UI から操作可能）

ブラウザの管理画面から「BTStack 用に切替」ボタンを押すだけで自動切替されます。
自己署名証明書を使った署名付きドライバパッケージを生成・インストールします。

```sh
# CLI で操作する場合
curl -X POST http://localhost:8765/api/driver/install \
  -H "Content-Type: application/json" \
  -d '{"vid": 2652, "pid": 1}'

# 元の Bluetooth ドライバに戻す
curl -X POST http://localhost:8765/api/driver/restore \
  -H "Content-Type: application/json" \
  -d '{"vid": 2652, "pid": 1}'
```

### 方法 B — Zadig（GUI）

1. [Zadig](https://zadig.akeo.ie) をダウンロード
2. Options → "List all devices" を有効化
3. プルダウンから Bluetooth ドングルを選択
4. 右側のドライバ選択を **WinUSB** にする
5. **Replace Driver** をクリック

---

## WebSocket プロトコル

`ws://localhost:8765/ws/<controller_id>` に接続してください。
`controller_id` は `POST /api/controllers` で返される ID です。

### クライアント → サーバー

```jsonc
// ゲームパッド入力（毎アニメーションフレーム送信）
// buttons は boolean[] または number[] (0.0〜1.0) のどちらでも可
// axes は -1.0〜1.0 または 0〜4095（マッピング済み）のどちらでも可
{
  "type": "gamepad_state",
  "buttons": [false, true, false, ...],
  "axes":    [2048, 2048, 2048, 2048]
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

// 振動時に押下するボタンマスクを登録
{ "type": "rumble_register", "key": 4 }
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
| GET  | `/api/controllers` | コントローラー情報一覧 |
| POST | `/api/controllers` | コントローラー追加 `{ vid, pid, instance }` |
| DELETE | `/api/controllers/:id` | コントローラー削除 |
| POST | `/api/controllers/:id/reconnect` | 再接続シグナル送信 |
| POST | `/api/controllers/:id/sync` | シンクロ（リンクキー削除 + 新規ペアリング） |
| POST | `/api/controllers/:id/sync-start` | ペアリングループ開始（接続されるまで自動リトライ） |
| POST | `/api/controllers/:id/sync-stop` | ペアリングループ停止 |
| GET  | `/api/driver/list` | BT USB デバイス一覧 + サーバーバージョン |
| POST | `/api/driver/install` | WinUSB ドライバを導入 `{ vid, pid }` |
| POST | `/api/driver/restore` | 元の BT ドライバに戻す `{ vid, pid }` |
| GET  | `/api/tlv` | リンクキー TLV ファイル一覧 |
| GET  | `/api/tlv/:filename` | TLV ファイルダウンロード（バイナリ） |
| POST | `/api/tlv/:filename` | TLV ファイルアップロード（バイナリ） |

### レスポンス例

```jsonc
// GET /api/driver/list
{
  "version": "0.1.0",
  "devices": [
    { "vid": "0411", "pid": "0374", "description": "Bluetooth Dongle", "driver": "WinUSB", "instance": 0 }
  ]
}

// GET /api/controllers
[
  { "id": 0, "vid": "0411", "pid": "0374", "instance": 0, "paired": true, "rumble": false, "syncing": false }
]

// GET /api/tlv
[
  { "filename": "btstack_00-1A-7D-DA-71-13.tlv", "size": 256 }
]
```

---

## Switch とのペアリング手順

1. サーバーを起動（ドングルに WinUSB ドライバが適用済みであること）
2. ブラウザ UI でドングルを「接続」するか、`POST /api/controllers` で追加
3. Switch で **コントローラー → コントローラーの持ち方/順番を変える** を開く
4. 数秒待つ → 「Pro Controller」として自動ペアリング

再接続時は Switch 側の再ペアリング操作不要です。
リンクキーは実行ファイルと同じディレクトリに `btstack_<BD_ADDR>.tlv` として保存されます。
ブラウザ UI からリンクキーのダウンロード・アップロードが可能です。

---

## プロジェクト構成

```
switch-bt-ws/
├── Cargo.toml
├── Dockerfile                Docker クロスコンパイル環境
├── docker-compose.yml        ビルド用 compose 定義
├── build.rs                  BTStack C ソースを libbtstack_gamepad にコンパイル
├── patches/
│   └── apply_patches.sh      BTStack ソースへのパッチ適用スクリプト
├── csrc/
│   ├── btstack_platform.c    Windows プラットフォーム初期化
│   │                         （port/windows-winusb/main.c の改変版）
│   ├── btstack_stub.c        非 Windows ビルド用のスタブ
│   ├── app.manifest          UAC マニフェスト（管理者権限要求）
│   └── app.rc                Windows リソース定義
└── src/
    ├── main.rs               エントリーポイント（サーバー / ワーカー分岐）
    ├── btstack.rs            C FFI バインディング + 安全な Wrapper
    ├── controller.rs         マルチコントローラー管理 + ワーカープロセス起動
    ├── worker.rs             ワーカーモードのエントリーポイント
    ├── ipc.rs                サーバー⇔ワーカー間の IPC メッセージ型
    ├── protocol.rs           WebSocket メッセージ型（serde JSON）
    ├── gamepad.rs            Web Gamepad → Switch マッピング
    ├── ws_server.rs          WebSocket ハンドラ
    ├── api.rs                HTTP REST API ルーター
    └── driver.rs             Windows ドライバ管理（自己署名 + WinUSB インストール）
```

---

## ライセンス

このソフトウェアは [BTStack](https://github.com/bluekitchen/btstack) を使用しています。

> Copyright (C) 2009 BlueKitchen GmbH. All rights reserved.
>
> Redistribution and use in source and binary forms, with or without modification,
> are permitted provided that the following conditions are met:
>
> 1. Redistributions of source code must retain the above copyright notice,
>    this list of conditions and the following disclaimer.
> 2. Redistributions in binary form must reproduce the above copyright notice,
>    this list of conditions and the following disclaimer in the documentation
>    and/or other materials provided with the distribution.
> 3. Neither the name of the copyright holders nor the names of contributors
>    may be used to endorse or promote products derived from this software
>    without specific prior written permission.
> 4. Any redistribution, use, or modification is done solely for personal benefit
>    and not for any commercial purpose or for monetary gain.
>
> See the full license text:
> - [bluekitchen/btstack LICENSE](https://github.com/bluekitchen/btstack/blob/master/LICENSE)
> - [mizuyoukanao/btstack LICENSE](https://github.com/mizuyoukanao/btstack?tab=License-1-ov-file)
