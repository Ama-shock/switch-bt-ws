#!/usr/bin/env bash
# apply_patches.sh
#
# BTStack の WinUSB HCI トランスポートに VID/PID+インスタンス指定機能を追加する。
#
# 変更内容:
#   1. hci_transport_h2_winusb.c
#      - グローバル変数 usb_target_vid / usb_target_pid / usb_target_instance を追加
#      - hci_transport_usb_set_target(vid, pid, instance) 関数を追加
#      - usb_open() のデバイス選択ロジックを VID/PID+インスタンス対応に変更
#   2. hci_transport_usb.h
#      - hci_transport_usb_set_target() の宣言を追加
#
# 使用方法:
#   ./apply_patches.sh <BTSTACK_ROOT>

set -euo pipefail

BTSTACK_ROOT="${1:?使用方法: $0 <BTSTACK_ROOT>}"

WINUSB_C="${BTSTACK_ROOT}/platform/windows/hci_transport_h2_winusb.c"
USB_H="${BTSTACK_ROOT}/src/hci_transport_usb.h"

for f in "$WINUSB_C" "$USB_H"; do
    if [ ! -f "$f" ]; then
        echo "[patch] エラー: ファイルが見つかりません: $f" >&2
        exit 1
    fi
done

# バックアップ（既に存在する場合はスキップ）
backup() {
    local f="$1"
    if [ ! -f "${f}.orig" ]; then
        cp "$f" "${f}.orig"
        echo "[patch] バックアップ作成: ${f}.orig"
    fi
}

backup "$WINUSB_C"
backup "$USB_H"

echo "[patch] hci_transport_h2_winusb.c にパッチを適用中 ..."

# awk で処理する:
#   1. '#include "hci_transport_usb.h"' の直後にグローバル変数と関数を挿入
#   2. "// try all devices" から "HeapFree" の直前までを置換コードに置き換え
awk '
# --- パッチ 1: インクルード後にグローバル変数と set_target() を挿入 ---
/^#include "hci_transport_usb\.h"$/ && !patched_globals {
    print $0
    print ""
    print "/* --- switch-bt-ws patch: VID/PID+\u30a4\u30f3\u30b9\u30bf\u30f3\u30b9\u6307\u5b9a ----------------------------------------"
    print " * hci_transport_usb_set_target(vid, pid, instance) \u3067\u30bf\u30fc\u30b2\u30c3\u30c8\u30c7\u30d0\u30a4\u30b9\u3092\u6307\u5b9a\u3059\u308b\u3002"
    print " * vid=0 / pid=0 \u306e\u5834\u5408\u306f\u6700\u521d\u306b\u958b\u3051\u305f\u30c7\u30d0\u30a4\u30b9\u3092\u4f7f\u7528\u3059\u308b\uff08\u30c7\u30d5\u30a9\u30eb\u30c8\u52d5\u4f5c\uff09\u3002"
    print " * instance \u306f\u540c\u4e00 VID/PID \u3092\u6301\u3064\u30c7\u30d0\u30a4\u30b9\u304c\u8907\u6570\u3042\u308b\u5834\u5408\u306e 0 \u59cb\u307e\u308a\u306e\u30a4\u30f3\u30c7\u30c3\u30af\u30b9\u3002"
    print " */"
    print "static uint16_t usb_target_vid      = 0;"
    print "static uint16_t usb_target_pid      = 0;"
    print "static int      usb_target_instance = 0;"
    print ""
    print "void hci_transport_usb_set_target(uint16_t vid, uint16_t pid, int instance) {"
    print "    usb_target_vid      = vid;"
    print "    usb_target_pid      = pid;"
    print "    usb_target_instance = instance;"
    print "}"
    print "/* --- end switch-bt-ws patch ----------------------------------------------- */"
    patched_globals = 1
    next
}

# --- パッチ 2: "// try all devices" から HeapFree の直前までを置換 ---
# "// try all devices" を検出したらスキップモード開始
/\/\/ try all devices/ && !patched_open {
    skip_open = 1
    next
}

# スキップ中に HeapFree が来たら置換コードを出力してスキップ終了
skip_open && /HeapFree/ {
    print "            /* switch-bt-ws patch: VID/PID+\u30a4\u30f3\u30b9\u30bf\u30f3\u30b9\u6307\u5b9a\u30d5\u30a3\u30eb\u30bf\u30ea\u30f3\u30b0 */"
    print "            int do_try = 0;"
    print "            if (usb_target_vid == 0 && usb_target_pid == 0) {"
    print "                /* \u30bf\u30fc\u30b2\u30c3\u30c8\u672a\u6307\u5b9a: \u6700\u521d\u306b\u958b\u3051\u305f\u30c7\u30d0\u30a4\u30b9\u3092\u4f7f\u7528 */"
    print "                do_try = 1;"
    print "            } else {"
    print "                /* VID/PID \u3067\u30d5\u30a3\u30eb\u30bf\u30ea\u30f3\u30b0\u3057\u3001\u30a4\u30f3\u30b9\u30bf\u30f3\u30b9\u756a\u53f7\u3067\u9078\u629e */"
    print "                char vid_pid_match[40];"
    print "                sprintf(vid_pid_match, \"\\\\\\\\?\\\\usb#vid_%04x&pid_%04x\","
    print "                        (unsigned)usb_target_vid, (unsigned)usb_target_pid);"
    print "                if (strncmp(DevIntfDetailData->DevicePath, vid_pid_match, strlen(vid_pid_match)) == 0) {"
    print "                    static int match_count = 0;"
    print "                    if (match_count == usb_target_instance) {"
    print "                        do_try = 1;"
    print "                    }"
    print "                    match_count++;"
    print "                }"
    print "            }"
    print "            BOOL result = FALSE;"
    print "            if (do_try) {"
    print "                result = usb_try_open_device(DevIntfDetailData->DevicePath);"
    print "                if (result) {"
    print "                    log_info(\"usb_open: Device opened (vid=%04x pid=%04x inst=%d), stop scanning\","
    print "                             usb_target_vid, usb_target_pid, usb_target_instance);"
    print "                    r = 0;"
    print "                } else {"
    print "                    log_error(\"usb_open: Device open failed\");"
    print "                }"
    print "            }"
    print "        }"  # SetupDiGetDeviceInterfaceDetail ブロック閉じ
    print $0           # HeapFree 行を出力
    patched_open = 1
    skip_open = 0
    next
}

# スキップ中の行は出力しない
skip_open { next }

{ print }
' "$WINUSB_C" > "${WINUSB_C}.tmp"

mv "${WINUSB_C}.tmp" "$WINUSB_C"
echo "[patch] hci_transport_h2_winusb.c: 完了"

# ---------------------------------------------------------------------------
# パッチ 2: hci_transport_usb.h に set_target() 宣言を追加
# ---------------------------------------------------------------------------
echo "[patch] hci_transport_usb.h にパッチを適用中 ..."

awk '
/^\/\* API_END \*\/$/ && !patched_header {
    print "/**"
    print " * @brief VID/PID+\u30a4\u30f3\u30b9\u30bf\u30f3\u30b9\u756a\u53f7\u3067\u30bf\u30fc\u30b2\u30c3\u30c8 USB \u30c7\u30d0\u30a4\u30b9\u3092\u6307\u5b9a\u3059\u308b\u3002"
    print " *        vid=0 \u304b\u3064 pid=0 \u306e\u5834\u5408\u306f\u6700\u521d\u306b\u898b\u3064\u304b\u3063\u305f\u30c7\u30d0\u30a4\u30b9\u3092\u4f7f\u7528\u3059\u308b\u3002"
    print " *        instance \u306f\u540c\u4e00 VID/PID \u306e\u30c7\u30d0\u30a4\u30b9\u304c\u8907\u6570\u3042\u308b\u5834\u5408\u306e 0 \u59cb\u307e\u308a\u306e\u30a4\u30f3\u30c7\u30c3\u30af\u30b9\u3002"
    print " *        switch-bt-ws \u30d1\u30c3\u30c1\u306b\u3088\u308a\u8ffd\u52a0\u3002"
    print " */"
    print "void hci_transport_usb_set_target(uint16_t vid, uint16_t pid, int instance);"
    print ""
    patched_header = 1
}
{ print }
' "$USB_H" > "${USB_H}.tmp"

mv "${USB_H}.tmp" "$USB_H"
echo "[patch] hci_transport_usb.h: 完了"

echo "[patch] 全パッチの適用が完了しました。"
