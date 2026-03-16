#!/bin/bash
set -e

cd /app

# BTStack パッチの適用（既に適用済みならスキップ）
if [ ! -f /btstack/.patched ]; then
    bash patches/apply_patches.sh /btstack/windows
    touch /btstack/.patched
fi

# サードパーティライセンス生成
bash scripts/gen_licenses.sh > csrc/THIRD_PARTY_LICENSES.txt

# ビルド
cargo build --release --target x86_64-pc-windows-gnu

# バージョン付きファイル名でコピー
VERSION=$(grep '^version' Cargo.toml | head -1 | sed 's/.*"\(.*\)".*/\1/')
cp target/x86_64-pc-windows-gnu/release/switch-bt-ws.exe \
   /out/switch-bt-ws-v${VERSION}.exe

echo "==> /out/switch-bt-ws-v${VERSION}.exe"
