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

# ---------------------------------------------------------------------------
# パッチ 3: btkeyLib.c に reconnect_gamepad() を追加
# ---------------------------------------------------------------------------
echo "[patch] btkeyLib.c にパッチを適用中 ..."

BTKEYLIB_C="${BTSTACK_ROOT}/example/btkeyLib.c"

if [ ! -f "$BTKEYLIB_C" ]; then
    echo "[patch] エラー: ファイルが見つかりません: $BTKEYLIB_C" >&2
    exit 1
fi

backup "$BTKEYLIB_C"

# reconnect_gamepad() / sync_gamepad() が未追加の場合のみ追加
# 重要: BTStack API はスレッドセーフではないため、hci_power_control() 等は
# BTStack の run loop スレッドで実行する必要がある。
# btstack_run_loop_execute_on_main_thread() を使って安全にキューイングする。
if ! grep -q 'reconnect_gamepad' "$BTKEYLIB_C"; then
    # gamepad_paired() 関数の直前に reconnect_gamepad() と sync_gamepad() を挿入
    awk '
    /^bool.*gamepad_paired/ && !patched_reconnect {
        print "#include \"btstack_run_loop.h\""
        print ""
        print "/* --- switch-bt-ws patch: thread-safe reconnect/sync via run loop --- */"
        print "static btstack_context_callback_registration_t reconnect_callback_reg;"
        print "static btstack_context_callback_registration_t sync_callback_reg;"
        print ""
        print "static void do_reconnect_on_main(void * context) {"
        print "    (void)context;"
        print "    fprintf(stderr, \"[btkeyLib] do_reconnect: HCI OFF->ON (on main thread)\\n\");"
        print "    paired = false;"
        print "    hid_cid = 0;"
        print "    pairing_state = 0;"
        print "    hci_power_control(HCI_POWER_OFF);"
        print "    hci_power_control(HCI_POWER_ON);"
        print "}"
        print ""
        print "static void do_sync_on_main(void * context) {"
        print "    (void)context;"
        print "    fprintf(stderr, \"[btkeyLib] do_sync: delete link keys + HCI OFF->ON (on main thread)\\n\");"
        print "    paired = false;"
        print "    hid_cid = 0;"
        print "    pairing_state = 0;"
        print "    gap_delete_all_link_keys();"
        print "    hci_power_control(HCI_POWER_OFF);"
        print "    hci_power_control(HCI_POWER_ON);"
        print "}"
        print ""
        print "//----------------------------------------------------------"
        print "// DLL関数"
        print "// 再接続シグナルを送信（HCI OFF→ON で discoverable 状態をリセット）"
        print "// 任意のスレッドから安全に呼び出し可能"
        print "//----------------------------------------------------------"
        print "void EXPORT_API reconnect_gamepad()"
        print "{"
        print "    fprintf(stderr, \"[btkeyLib] reconnect_gamepad: queuing on main thread\\n\");"
        print "    reconnect_callback_reg.callback = &do_reconnect_on_main;"
        print "    btstack_run_loop_execute_on_main_thread(&reconnect_callback_reg);"
        print "}"
        print ""
        print "//----------------------------------------------------------"
        print "// DLL関数"
        print "// シンクロボタン長押し相当: リンクキー全削除 + HCI リセット"
        print "// 任意のスレッドから安全に呼び出し可能"
        print "//----------------------------------------------------------"
        print "void EXPORT_API sync_gamepad()"
        print "{"
        print "    fprintf(stderr, \"[btkeyLib] sync_gamepad: queuing on main thread\\n\");"
        print "    sync_callback_reg.callback = &do_sync_on_main;"
        print "    btstack_run_loop_execute_on_main_thread(&sync_callback_reg);"
        print "}"
        print "/* --- end switch-bt-ws patch --- */"
        print ""
        patched_reconnect = 1
    }
    { print }
    ' "$BTKEYLIB_C" > "${BTKEYLIB_C}.tmp"
    mv "${BTKEYLIB_C}.tmp" "$BTKEYLIB_C"
    echo "[patch] btkeyLib.c: reconnect_gamepad() / sync_gamepad() を追加 (thread-safe)"
else
    echo "[patch] btkeyLib.c: reconnect_gamepad() は既に存在（スキップ）"
fi

# ---------------------------------------------------------------------------
# パッチ 4: HCI_STATE_WORKING で gap_discoverable_control(1) を再呼び出し
# ---------------------------------------------------------------------------
# HCI OFF→ON 後に discoverable モードが無効になる問題を修正。
# nintendo_packet_handler の BTSTACK_EVENT_STATE ハンドラに
# gap_discoverable_control(1) を追加する。
if ! grep -q 'gap_discoverable_control.*nintendo_packet_handler\|re-enable discoverable' "$BTKEYLIB_C"; then
    awk '
    /btstack_event_state_get_state.*HCI_STATE_WORKING.*return/ && !patched_discoverable {
        print $0
        print "                /* switch-bt-ws patch: HCI 再起動後も discoverable + connectable に */"
        print "                gap_discoverable_control(1);"
        print "                gap_connectable_control(1);"
        patched_discoverable = 1
        next
    }
    { print }
    ' "$BTKEYLIB_C" > "${BTKEYLIB_C}.tmp"
    mv "${BTKEYLIB_C}.tmp" "$BTKEYLIB_C"
    echo "[patch] btkeyLib.c: HCI_STATE_WORKING で gap_discoverable_control(1) + gap_connectable_control(1) を追加"
else
    echo "[patch] btkeyLib.c: gap_discoverable_control パッチは既に存在（スキップ）"
fi

# ---------------------------------------------------------------------------
# パッチ 5: nintendo_packet_handler にデバッグログを追加
# ---------------------------------------------------------------------------
echo "[patch] btkeyLib.c にデバッグログを追加中 ..."

# 5a: HCI_STATE_WORKING の gap_connectable_control(1) の直後にログ追加
if ! grep -q 'fprintf.*stderr.*discoverable' "$BTKEYLIB_C"; then
    sed -i '/gap_connectable_control(1);/{
        a\                fprintf(stderr, "[btkeyLib] HCI_STATE_WORKING: discoverable(1) + connectable(1) called\\n");
    }' "$BTKEYLIB_C"
    echo "[patch] btkeyLib.c: HCI_STATE_WORKING デバッグログを追加"
fi

# 5b-5f: HID イベントと btstack_main のデバッグログ追加（awk で安全に挿入）
if ! grep -q 'fprintf.*stderr.*HID_CONNECTION_OPENED' "$BTKEYLIB_C"; then
    awk '
    # HID_SUBEVENT_CONNECTION_OPENED: hid_cid 取得行の直後にログ
    /hid_cid = hid_subevent_connection_opened_get_hid_cid/ {
        print $0
        print "                        fprintf(stderr, \"[btkeyLib] HID_CONNECTION_OPENED: hid_cid=%d\\n\", hid_cid);"
        next
    }
    # HID_SUBEVENT_CONNECTION_OPENED: status チェック失敗時のログ
    # "if (status)" の次の行の "{" の中にログを挿入
    /hid_subevent_connection_opened_get_status/ { found_status = 1 }
    found_status && /if \(status\)/ { found_if_status = 1 }
    found_if_status && /\{/ && !added_fail_log {
        print $0
        print "                            fprintf(stderr, \"[btkeyLib] HID_CONNECTION_OPENED_FAILED: status=%d\\n\", status);"
        added_fail_log = 1
        found_status = 0
        found_if_status = 0
        next
    }
    # HID_SUBEVENT_CONNECTION_CLOSED: ブロック開始 { の中にログ
    /case HID_SUBEVENT_CONNECTION_CLOSED:/ { found_closed = 1 }
    found_closed && /\{/ && !added_closed_log {
        print $0
        print "                        fprintf(stderr, \"[btkeyLib] HID_CONNECTION_CLOSED\\n\");"
        added_closed_log = 1
        found_closed = 0
        next
    }
    # paired = true の直後にログ
    /paired = true;/ {
        print $0
        print "                                fprintf(stderr, \"[btkeyLib] >>> PAIRED! pairing_state=%d hid_cid=%d\\n\", pairing_state, hid_cid);"
        next
    }
    # HCI_EVENT_HID_META: subevent ログは --debug 時のみ有用なので削除
    # btstack_main の HCI_POWER_ON ログは不要（HCI_STATE_WORKING ハンドラでログ済み）
    { print }
    ' "$BTKEYLIB_C" > "${BTKEYLIB_C}.tmp"
    mv "${BTKEYLIB_C}.tmp" "$BTKEYLIB_C"
    echo "[patch] btkeyLib.c: HID イベント + btstack_main デバッグログを追加"
fi

# ---------------------------------------------------------------------------
# パッチ 6: btstack_main() に gap_connectable_control(1) を追加
# ---------------------------------------------------------------------------
# btstack_main() の gap_discoverable_control(1) 直後に connectable も有効化する。
if ! grep -q 'gap_connectable_control' "$BTKEYLIB_C" | grep -q 'btstack_main'; then
    # btstack_main 内の gap_discoverable_control(1) の直後（gap_set_class_of_device の前）に挿入
    sed -i '/^    gap_discoverable_control(1);$/{
        a\    gap_connectable_control(1);  /* switch-bt-ws patch: page scan 有効化 */
    }' "$BTKEYLIB_C"
    echo "[patch] btkeyLib.c: btstack_main に gap_connectable_control(1) を追加"
fi

# ---------------------------------------------------------------------------
# パッチ 7: hid_normally_connectable を 1 に変更
# ---------------------------------------------------------------------------
# joycontrol は HIDNormallyConnectable=true に設定している。
# 元コードは 0 だが、Switch がデバイスを通常接続可能と認識するために 1 が必要。
if grep -q 'hid_normally_connectable = 0' "$BTKEYLIB_C"; then
    sed -i 's/uint8_t hid_normally_connectable = 0;/uint8_t hid_normally_connectable = 1;  \/* switch-bt-ws patch: joycontrol と同じ *\//' "$BTKEYLIB_C"
    echo "[patch] btkeyLib.c: hid_normally_connectable を 1 に変更"
fi

# ---------------------------------------------------------------------------
# パッチ 8: SSP 無効化 + bondable モード設定
# ---------------------------------------------------------------------------
# joycontrol は認証を完全に無効化している (RequireAuthentication=False)。
# BTStack のデフォルトでは SSP が有効で、Switch との接続ネゴシエーションに
# 失敗する可能性がある。
# gap_ssp_set_enable(0) で SSP を無効化し、Switch 側から PIN なしで接続できるようにする。
# gap_ssp_set_io_capability(SSP_IO_CAPABILITY_NO_INPUT_NO_OUTPUT) で "Just Works" ペアリング。
# gap_set_bondable_mode(1) でリンクキーの保存を許可する。
if ! grep -q 'gap_ssp_set_io_capability' "$BTKEYLIB_C"; then
    sed -i '/gap_set_allow_role_switch(true);/a\    /* switch-bt-ws patch: SSP を "Just Works" に設定（joycontrol 互換） */\n    gap_ssp_set_io_capability(SSP_IO_CAPABILITY_NO_INPUT_NO_OUTPUT);\n    gap_ssp_set_authentication_requirement(0);  /* no MITM */\n    gap_set_bondable_mode(1);' "$BTKEYLIB_C"
    echo "[patch] btkeyLib.c: SSP Just Works + bondable モードを設定"
fi

# パッチ 9 は削除済み（HCI パケットログは冗長すぎるため）
# --debug フラグ使用時のみ詳細ログが必要な場合は、別途有効化すること。

# ---------------------------------------------------------------------------
# パッチ 10: do_sync_on_main で gap_discoverable + gap_connectable を明示的に呼ぶ
# ---------------------------------------------------------------------------
# HCI OFF→ON 後、HCI_STATE_WORKING イベントで discoverable/connectable を
# 再有効化するが、念のため sync 関数内でも明示的にフラグをリセットする。
# また、SSP 設定も再適用する。
if ! grep -q 'gap_discoverable_control.*do_sync' "$BTKEYLIB_C"; then
    sed -i '/do_sync_on_main.*context.*{/,/^}/ {
        /hci_power_control(HCI_POWER_ON);/a\    /* switch-bt-ws patch: sync 後に明示的に discoverable + connectable 再設定 */\n    fprintf(stderr, "[btkeyLib] do_sync: HCI OFF->ON queued, waiting for HCI_STATE_WORKING...\\n");
    }' "$BTKEYLIB_C"
    echo "[patch] btkeyLib.c: do_sync_on_main にログを追加"
fi

# ---------------------------------------------------------------------------
# パッチ 11: hid_report_data_callback にデバッグログを追加
# ---------------------------------------------------------------------------
# Switch からの HID レポートが到着しているかを確認するためのログ。
if ! grep -q 'fprintf.*stderr.*hid_report_data_callback.*report_id' "$BTKEYLIB_C"; then
    sed -i '/^static void hid_report_data_callback.*report_size.*report)/{
        N
        s/$/\n    fprintf(stderr, "[btkeyLib] hid_report: id=%d size=%d r9=0x%02x r10=0x%02x ps=%d\\n", report_id, report_size, report_size > 9 ? report[9] : 0, report_size > 10 ? report[10] : 0, pairing_state);/
    }' "$BTKEYLIB_C"
    echo "[patch] btkeyLib.c: hid_report_data_callback にデバッグログを追加"
fi

# ---------------------------------------------------------------------------
# パッチ 12: CAN_SEND_NOW にデバッグログ（初回のみ）
# ---------------------------------------------------------------------------
if ! grep -q 'fprintf.*stderr.*CAN_SEND_NOW.*pairing_state' "$BTKEYLIB_C"; then
    awk '
    /case HID_SUBEVENT_CAN_SEND_NOW:/ && !patched_csn {
        print $0
        print "                    {"
        print "                        static int csn_log_count = 0;"
        print "                        if (csn_log_count < 5) {"
        print "                            fprintf(stderr, \"[btkeyLib] CAN_SEND_NOW: pairing_state=%d hid_cid=%d\\n\", pairing_state, hid_cid);"
        print "                            csn_log_count++;"
        print "                        }"
        print "                    }"
        patched_csn = 1
        next
    }
    { print }
    ' "$BTKEYLIB_C" > "${BTKEYLIB_C}.tmp"
    mv "${BTKEYLIB_C}.tmp" "$BTKEYLIB_C"
    echo "[patch] btkeyLib.c: CAN_SEND_NOW にデバッグログを追加"
fi

# ---------------------------------------------------------------------------
# パッチ 13: pairing_state==15 でも paired=true にする
# ---------------------------------------------------------------------------
# Switch のレポート到着順序により、pairing_state が 14 を経由せず 15 に到達する
# 場合がある。state 15 は state 14 以降の後処理状態であり、ここに到達した時点で
# ペアリングハンドシェイクは実質完了している。
# CAN_SEND_NOW 内の else if (paired && (pairing_state == 13 || pairing_state == 15))
# を修正して、paired でなくても state 15 で paired = true にする。
if ! grep -q 'switch-bt-ws patch: state 15 also sets paired' "$BTKEYLIB_C"; then
    awk '
    /else if \(paired && \(pairing_state == 13 \|\| pairing_state == 15\)\)/ && !patched_s15 {
        print "                            /* switch-bt-ws patch: state 15 also sets paired */"
        print "                            else if (pairing_state == 15)"
        print "                            {"
        print "                                pairing_state = 0;"
        print "                                if (!paired) {"
        print "                                    joy.timer = tim+1;"
        print "                                    paired = true;"
        print "                                    fprintf(stderr, \"[btkeyLib] >>> PAIRED (via state 15)! hid_cid=%d\\n\", hid_cid);"
        print "                                }"
        print "                            }"
        print "                            else if (paired && pairing_state == 13)"
        print "                            {"
        print "                                pairing_state = 0;"
        print "                            }"
        patched_s15 = 1
        next
    }
    { print }
    ' "$BTKEYLIB_C" > "${BTKEYLIB_C}.tmp"
    mv "${BTKEYLIB_C}.tmp" "$BTKEYLIB_C"
    echo "[patch] btkeyLib.c: pairing_state==15 でも paired=true を設定"
fi

# ---------------------------------------------------------------------------
# パッチ 14: Switch が割り当てた player LED を取得する関数を追加
# ---------------------------------------------------------------------------
# Switch はサブコマンド 0x30 でプレイヤー LED を設定する。
# report[9]==0x30 の時、report[10] が LED ビットパターン:
#   P1=0x01, P2=0x03, P3=0x07, P4=0x0F（累積パターン）
# この値を保持し、get_player_leds() で取得できるようにする。
if ! grep -q 'player_leds' "$BTKEYLIB_C"; then
    # 1. グローバル変数を paired の直後に追加
    sed -i '/^bool paired = false;/a\uint8_t player_leds = 0;  /* switch-bt-ws patch: Switch assigned player LED pattern */' "$BTKEYLIB_C"

    # 2. hid_report_data_callback 内で report[9]==0x30 の時に player_leds をキャプチャ
    #    pairing_state = 14 の条件（report_id==1 && report[9]==48 && report[10]==1）の直前に挿入
    sed -i '/report_id == 1 && report\[9\] == 48 && report\[10\] == 1/{
        i\    if(report[9] == 48) { player_leds = report[10]; }  /* switch-bt-ws patch: capture player LED */
    }' "$BTKEYLIB_C"

    # 3. gamepad_paired() の直後に get_player_leds() を追加
    sed -i '/^bool EXPORT_API gamepad_paired()/{
        N
        N
        a\uint8_t EXPORT_API get_player_leds()\n{\n    return player_leds;\n}
    }' "$BTKEYLIB_C"

    echo "[patch] btkeyLib.c: player_leds グローバル変数と get_player_leds() を追加"
fi

echo "[patch] 全パッチの適用が完了しました。"
