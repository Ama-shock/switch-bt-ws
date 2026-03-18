#!/usr/bin/env bash
# apply_patches.sh
#
# bluekitchen/btstack の WinUSB HCI トランスポートに VID/PID+インスタンス指定機能を追加する。
#
# btkeyLib.c のパッチは csrc/btkeyLib.c に直接適用済みの状態で管理しているため、
# このスクリプトでは BTStack コアのみをパッチする。
#
# 変更内容:
#   1. hci_transport_h2_winusb.c
#      - グローバル変数 usb_target_vid / usb_target_pid / usb_target_instance を追加
#      - hci_transport_usb_set_target(vid, pid, instance) 関数を追加
#      - usb_open() のデバイス選択ロジックを VID/PID+インスタンス対応に変更
#        (Unknown デバイスはプローブでスキップ)
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

# ---------------------------------------------------------------------------
# パッチ 1: hci_transport_h2_winusb.c に VID/PID+インスタンス指定を追加
# ---------------------------------------------------------------------------
echo "[patch] hci_transport_h2_winusb.c にパッチを適用中 ..."

awk '
# --- パッチ 1a: インクルード後にグローバル変数と set_target() を挿入 ---
/^#include "hci_transport_usb\.h"$/ && !patched_globals {
    print $0
    print ""
    print "/* --- switch-bt-ws patch: VID/PID+インスタンス指定 --- */"
    print "static uint16_t usb_target_vid      = 0;"
    print "static uint16_t usb_target_pid      = 0;"
    print "static int      usb_target_instance = 0;"
    print ""
    print "void hci_transport_usb_set_target(uint16_t vid, uint16_t pid, int instance) {"
    print "    usb_target_vid      = vid;"
    print "    usb_target_pid      = pid;"
    print "    usb_target_instance = instance;"
    print "}"
    print "/* --- end switch-bt-ws patch --- */"
    patched_globals = 1
    next
}

# --- パッチ 1b-pre: while ループ直前にマッチカウンター宣言を挿入 ---
# (HCI OFF→ON で usb_open() が再呼び出しされた時にカウンターが 0 から始まるようにする)
/while\(GetLastError\(\) != ERROR_NO_MORE_ITEMS\)/ && !patched_counter {
    print "\tint usb_vid_pid_match_count = 0; /* switch-bt-ws patch: per-call match counter */"
    print $0
    patched_counter = 1
    next
}

# --- パッチ 1b: "// try all devices" から HeapFree の直前までを置換 ---
/\/\/ try all devices/ && !patched_open {
    skip_open = 1
    next
}

skip_open && /HeapFree/ {
    print "            /* switch-bt-ws patch: VID/PID+インスタンス指定フィルタリング */"
    print "            int do_try = 0;"
    print "            if (usb_target_vid == 0 && usb_target_pid == 0) {"
    print "                /* ターゲット未指定: 最初に開けたデバイスを使用 */"
    print "                do_try = 1;"
    print "            } else {"
    print "                /* VID/PID でフィルタリングし、インスタンス番号で選択 */"
    print "                char vid_pid_match[40];"
    print "                sprintf(vid_pid_match, \"\\\\\\\\?\\\\usb#vid_%04x&pid_%04x\","
    print "                        (unsigned)usb_target_vid, (unsigned)usb_target_pid);"
    print "                if (strncmp(DevIntfDetailData->DevicePath, vid_pid_match, strlen(vid_pid_match)) == 0) {"
    print "                    /* デバイスが実際にオープンできるかプローブ */"
    print "                    HANDLE hProbe = CreateFileA(DevIntfDetailData->DevicePath,"
    print "                        GENERIC_READ | GENERIC_WRITE, FILE_SHARE_READ | FILE_SHARE_WRITE,"
    print "                        NULL, OPEN_EXISTING, FILE_FLAG_OVERLAPPED, NULL);"
    print "                    fprintf(stderr, \"[usb_open] VID/PID match #%d: %s\\n\","
    print "                            usb_vid_pid_match_count, DevIntfDetailData->DevicePath);"
    print "                    if (hProbe != INVALID_HANDLE_VALUE) {"
    print "                        CloseHandle(hProbe);"
    print "                        if (usb_vid_pid_match_count == usb_target_instance) {"
    print "                            do_try = 1;"
    print "                        }"
    print "                    } else {"
    print "                        fprintf(stderr, \"[usb_open] VID/PID match #%d probe failed (err=%lu), skipping\\n\","
    print "                                usb_vid_pid_match_count, GetLastError());"
    print "                    }"
    print "                    usb_vid_pid_match_count++; /* プローブ結果に関わらずカウント */"
    print "                }"
    print "            }"
    print "            BOOL result = FALSE;"
    print "            if (do_try) {"
    print "                result = usb_try_open_device(DevIntfDetailData->DevicePath);"
    print "                if (result) {"
    print "                    fprintf(stderr, \"[usb_open] Device opened (vid=%04x pid=%04x inst=%d)\\n\","
    print "                             usb_target_vid, usb_target_pid, usb_target_instance);"
    print "                    r = 0;"
    print "                } else {"
    print "                    fprintf(stderr, \"[usb_open] Device open FAILED (vid=%04x pid=%04x inst=%d)\\n\","
    print "                             usb_target_vid, usb_target_pid, usb_target_instance);"
    print "                }"
    print "            }"
    print "        }"
    print $0
    patched_open = 1
    skip_open = 0
    next
}

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
    print " * @brief VID/PID+インスタンス番号でターゲット USB デバイスを指定する。"
    print " *        switch-bt-ws パッチにより追加。"
    print " */"
    print "void hci_transport_usb_set_target(uint16_t vid, uint16_t pid, int instance);"
    print ""
    patched_header = 1
}
{ print }
' "$USB_H" > "${USB_H}.tmp"

mv "${USB_H}.tmp" "$USB_H"
echo "[patch] hci_transport_usb.h: 完了"

# ---------------------------------------------------------------------------
# パッチ 3: hid_device.c の L2CAP MTU を 48 → l2cap_max_mtu() に変更
# ---------------------------------------------------------------------------
HID_DEVICE_C="${BTSTACK_ROOT}/src/classic/hid_device.c"
if [ -f "$HID_DEVICE_C" ]; then
    backup "$HID_DEVICE_C"
    echo "[patch] hid_device.c にパッチを適用中 ..."
    sed -i 's/l2cap_create_channel(packet_handler, device->bd_addr, PSM_HID_INTERRUPT, 48,/l2cap_create_channel(packet_handler, device->bd_addr, PSM_HID_INTERRUPT, l2cap_max_mtu(),/' "$HID_DEVICE_C"
    sed -i 's/l2cap_create_channel(packet_handler, hid_device->bd_addr, PSM_HID_CONTROL, 48,/l2cap_create_channel(packet_handler, hid_device->bd_addr, PSM_HID_CONTROL, l2cap_max_mtu(),/' "$HID_DEVICE_C"
    echo "[patch] hid_device.c: 完了"
fi

echo "[patch] 全パッチの適用が完了しました。"
