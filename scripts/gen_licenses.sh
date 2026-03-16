#!/bin/bash
# gen_licenses.sh — cargo metadata からサードパーティライセンス一覧を生成する
# cargo-about が Rust 1.77 でビルドできないため、簡易スクリプトで代替。
set -euo pipefail

# BTStack ライセンス（先頭に配置）
cat csrc/BTSTACK_LICENSE.txt
echo

# Rust クレートのライセンス
echo "================================================================================"
echo "Rust Crate Licenses"
echo "================================================================================"
echo

cargo metadata --format-version=1 2>/dev/null | \
  jq -r '
    .workspace_members as $roots |
    [.packages[] | select(.id as $id | $roots | index($id) | not)] |
    sort_by(.name) | .[] |
    "\(.name) v\(.version)\n  License: \(.license // "Unknown")" +
    (if .repository then "\n  Repository: \(.repository)" else "" end) +
    "\n"
  '
