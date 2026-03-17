# switch-bt-ws

Nintendo Switch の Pro Controller を Bluetooth 経由でエミュレートし、
WebSocket でブラウザから操作するシングルバイナリの Rust プログラムです。

複数の USB Bluetooth ドングルを同時に使用して、最大 4 台のコントローラーをエミュレートできます。

---

## 概要

```
ブラウザ（video-html フロントエンド）
       │  WebSocket JSON
       ▼
switch-bt-ws.exe  （Rust / サーバーモード）
  ├── Tokio 非同期ランタイム
  │   ├── axum HTTP サーバー  :8765
  │   │   ├── GET  /ws                        グローバル WebSocket（デバイス一覧）
  │   │   ├── GET  /ws/:id                    コントローラー WebSocket
  │   │   ├── GET  /api/controllers           コントローラー一覧
  │   │   ├── POST /api/controllers           コントローラー追加
  │   │   ├── DELETE /api/controllers/:id     コントローラー削除
  │   │   ├── GET  /api/driver/list           BT USB デバイス一覧 + バージョン
  │   │   ├── POST /api/driver/install        WinUSB ドライバ導入（UAC 昇格）
  │   │   └── POST /api/driver/restore        元の BT ドライバに戻す（UAC 昇格）
  │   └── WebSocket ハンドラ
  │       ├── 受信: gamepad_state / reconnect / sync 等
  │       └── 送信: status / link_keys
  └── ワーカーサブプロセス管理
      └── ドングルごとに switch-bt-ws.exe --worker を起動

switch-bt-ws.exe --worker <id> <vid> <pid> <instance> [--link-keys <base64>]
  ├── BTStack スレッド（ブロッキング C コード）
  │   ├── btstack_platform_run()
  │   └── btkeyLib: Pro Controller HID エミュレーション
  │       ├── ペアリング（discoverable + SSP Just Works）
  │       ├── 能動的再接続（hid_device_connect）
  │       └── ボタン / スティック / ジャイロ / Amiibo
  │               │ WinUSB / BTStack HCI
  │               ▼
  │         Bluetooth ドングル → Nintendo Switch
  └── stdin/stdout NDJSON IPC
```

---

## アーキテクチャ

### マルチコントローラー

ドングルごとに独立したワーカープロセスを起動します。
BTStack はグローバルな C 状態を持つため、プロセス分離で安全にマルチドングルを実現しています。

```
サーバー ─┬─ ワーカー 0 (vid=0411 pid=0374 inst=0) ─── ドングル A ─── Switch
          ├─ ワーカー 1 (vid=0a12 pid=0001 inst=0) ─── ドングル B ─── Switch
          └─ ワーカー 2 (vid=0a12 pid=0001 inst=1) ─── ドングル C ─── Switch
```

同一 VID:PID のドングルが複数ある場合はインスタンス番号で区別します。
USB デバイス列挙時に実際にオープン可能なデバイスのみカウントし、Unknown デバイスをスキップします。

### 接続フロー

**ペアリング（初回）:**
1. `sync_gamepad()` でリンクキー削除 + HCI リセット
2. Discoverable + Connectable モードで Switch からの接続を待機
3. Switch が接続 → SSP Just Works でリンクキー交換
4. HID チャネル確立 → pairing_state ハンドシェイク完了

**再接続（リンクキー有り）:**
1. リンクキー DB からSwitchのBD_ADDRを取得
2. `hid_device_connect()` でコントローラー側から能動的にL2CAP接続を開始
3. Switch 側の画面操作不要で再接続

**自動リカバリ:**
- HID接続失敗 (0x6A baseband disconnect / 0x66 security refused) 時にリンクキーを削除して自動再ペアリング

### IPC プロトコル（stdin/stdout NDJSON）

サーバー → ワーカー: `WorkerCommand`（ボタン入力、スティック、ジャイロ、reconnect、sync、disconnect 等）
ワーカー → サーバー: `WorkerEvent`（ステータス、リンクキー、シャットダウン）

---

## 必要なもの

### ハードウェア
- USB Bluetooth 4.0+ ドングル（CSR `0a12:0001` / BCM / Realtek 推奨）
  - BT 5.4 ドングルは一部チップセットで非互換
- Nintendo Switch

### ソフトウェア
- Windows 10/11（64bit）

---

## ビルド

Docker を使ったクロスコンパイルが推奨です。

```bash
cd switch-bt-ws

# イメージのビルド（初回 or Dockerfile 変更時）
docker compose build build

# ビルド実行 → dist/switch-bt-ws-v<version>.exe に出力
docker compose run --rm build
```

BTStack は bluekitchen/btstack v1.5.3 を Docker イメージ内でクローンします。
btkeyLib.c（Pro Controller エミュレーション）は `csrc/btkeyLib.c` にパッチ適用済みで管理しています。

### GitHub Actions リリース

`v*` タグをプッシュすると、タグ名と `Cargo.toml` のバージョンが一致する場合に
自動ビルド + GitHub Release が作成されます。

```bash
# Cargo.toml の version を更新後
git tag v0.2.0
git push origin master --tags
```

---

## 実行

```powershell
.\switch-bt-ws-v0.1.0.exe
```

起動時にバージョンとビルド ID が表示されます。
ドライバ操作（install/restore）時のみ UAC 昇格ダイアログが表示されます。

### コマンドラインオプション

| オプション | 説明 |
|-----------|------|
| `--debug` | デバッグログを有効化 |
| `--licenses` | サードパーティライセンスを表示して終了 |

### 環境変数

| 変数名 | デフォルト | 説明 |
|--------|-----------|------|
| `SWITCH_BT_WS_PORT` | `8765` | HTTP/WS サーバーのリスンポート |
| `RUST_LOG` | `info` | ログレベル |

---

## ドライバ設定（ドングルごとに 1 回だけ）

BTStack は OS の `BthUsb.sys` の代わりに WinUSB ドライバが必要です。

### 方法 A — ブラウザ UI（推奨）

ブラウザの管理画面から「WinUSB 導入」ボタンを押すだけで自動切替されます。
自己署名証明書を使った署名付きドライバパッケージを生成・インストールします。
UAC 昇格ダイアログが表示されます。

### 方法 B — Zadig（GUI）

1. [Zadig](https://zadig.akeo.ie) をダウンロード
2. Options → "List all devices" を有効化
3. プルダウンから Bluetooth ドングルを選択
4. 右側のドライバ選択を **WinUSB** にする
5. **Replace Driver** をクリック

---

## WebSocket プロトコル

### グローバル WS (`/ws`)

デバイス一覧・コントローラー一覧のスナップショットをリアルタイムで配信。

### コントローラー WS (`/ws/<controller_id>`)

#### クライアント → サーバー

```jsonc
// ゲームパッド入力
{
  "type": "gamepad_state",
  "buttons": [false, true, false, ...],
  "axes": [2048, 2048, 2048, 2048]
}

// 再接続（リンクキーは起動時にインポート済み）
{ "type": "reconnect", "link_keys": null }

// ペアリング開始/停止
{ "type": "sync_start" }
{ "type": "sync_stop" }

// 切断
{ "type": "disconnect" }
```

#### サーバー → クライアント

```jsonc
// 約 200ms ごとにプッシュ
{ "type": "status", "paired": true, "rumble": false, "syncing": false, "player": 1 }

// リンクキー（ペアリング成功時）
{ "type": "link_keys", "data": "<base64>" }
```

---

## ボタンマッピング

| Web Gamepad | Switch |
|:-----------:|--------|
| 0 (A)       | B      |
| 1 (B)       | A      |
| 2 (X)       | Y      |
| 3 (Y)       | X      |
| 4 (LB)      | L      |
| 5 (RB)      | R      |
| 6 (LT)      | ZL     |
| 7 (RT)      | ZR     |
| 8 (Back)    | −      |
| 9 (Start)   | +      |
| 10 (L3)     | LS     |
| 11 (R3)     | RS     |
| 12-15       | D-pad  |
| 16 (Home)   | HOME   |
| 17          | Capture|

スティック軸: Web `[-1, 1]` → Switch `[0, 4095]`（中央 = 2048）

---

## プロジェクト構成

```
switch-bt-ws/
├── Cargo.toml
├── Dockerfile                Docker クロスコンパイル環境 (bluekitchen/btstack v1.5.3)
├── docker-compose.yml        ビルド用 compose 定義
├── entrypoint.sh             Docker ビルドエントリーポイント
├── build.rs                  BTStack C ソースを libbtstack_gamepad にコンパイル
├── .github/
│   └── workflows/
│       └── release.yml       タグプッシュ時の自動リリース
├── patches/
│   └── apply_patches.sh      BTStack コアへのパッチ（hci_transport VID/PID 指定）
├── scripts/
│   └── gen_licenses.sh       Cargo クレートライセンス自動生成
├── csrc/
│   ├── btkeyLib.c            Pro Controller HID エミュレーション
│   │                         (Originally from mizuyoukanao/btstack, modified)
│   ├── btstack_platform.c    Windows プラットフォーム初期化
│   ├── btstack_stub.c        非 Windows ビルド用スタブ
│   ├── BTSTACK_LICENSE.txt    BTStack ライセンス
│   ├── THIRD_PARTY_LICENSES.txt  生成されるクレートライセンス
│   ├── app.manifest          UAC マニフェスト (asInvoker)
│   └── app.rc                Windows リソース定義
└── src/
    ├── main.rs               エントリーポイント（サーバー / ワーカー分岐）
    ├── btstack.rs            C FFI バインディング + 安全なラッパー
    ├── controller.rs         マルチコントローラー管理 + ワーカープロセス起動
    ├── worker.rs             ワーカーモードのエントリーポイント
    ├── ipc.rs                サーバー⇔ワーカー間 IPC メッセージ型
    ├── protocol.rs           WebSocket メッセージ型
    ├── gamepad.rs            Web Gamepad → Switch マッピング
    ├── ws_server.rs          WebSocket ハンドラ
    ├── api.rs                HTTP REST API
    ├── global_ws.rs          グローバル WebSocket（デバイス一覧配信）
    └── driver.rs             Windows ドライバ管理（UAC 昇格分離）
```

---

## ライセンス

このソフトウェアは以下のオープンソースプロジェクトを使用しています:

### BTStack
> Copyright (C) 2009 BlueKitchen GmbH. All rights reserved.
>
> BSD-3-Clause + Non-Commercial restriction.
> See [bluekitchen/btstack LICENSE](https://github.com/bluekitchen/btstack/blob/master/LICENSE)

### btkeyLib.c
> Originally from [mizuyoukanao/btstack](https://github.com/mizuyoukanao/btstack)
> Modified by the switch-bt-ws project.

### Rust クレート依存
`--licenses` フラグで全サードパーティライセンスを表示できます。
