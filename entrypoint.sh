#!/bin/bash
set -e

cd /app

# BTStack コアへのパッチ適用（hci_transport のみ）
# btkeyLib.c は csrc/ にパッチ適用済みの状態で管理しているため対象外
cd /btstack/windows && git checkout -- . && cd /app
bash patches/apply_patches.sh /btstack/windows

# サードパーティライセンス生成
bash scripts/gen_licenses.sh > csrc/THIRD_PARTY_LICENSES.txt

# ビルド
cargo build --release --target x86_64-pc-windows-gnu

# バージョン付きファイル名でコピー
VERSION=$(grep '^version' Cargo.toml | head -1 | sed 's/.*"\(.*\)".*/\1/')
cp target/x86_64-pc-windows-gnu/release/switch-bt-ws.exe \
   /out/switch-bt-ws-v${VERSION}.exe

echo "==> /out/switch-bt-ws-v${VERSION}.exe"
