/*
 * btstack_platform.c
 *
 * Rust 統合のための BTStack Windows/WinUSB プラットフォーム初期化。
 *
 * port/windows-winusb/main.c を軽微に改変したファイルです。
 * 唯一の構造的な変更点は、C のエントリーポイント "main()" を
 * "btstack_platform_run()" にリネームしたことです。
 * これにより、静的ライブラリをリンクした際に Rust 自身の main() と
 * シンボルが衝突しなくなります。
 *
 * start_gamepad()    — Rust から専用 OS スレッドで呼び出す。シャットダウンまでブロック。
 * shutdown_gamepad() — Rust からクリーンシャットダウンを開始するために呼び出す。
 */

#define BTSTACK_FILE__ "btstack_platform.c"

#include <Windows.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <signal.h>

#include "btstack_config.h"

#include "btstack_audio.h"
#include "btstack_debug.h"
#include "btstack_event.h"
#include "btstack_memory.h"
#include "btstack_run_loop.h"
#include "btstack_run_loop_windows.h"
#include "btstack_stdin.h"
#include "btstack_stdin_windows.h"
#include "classic/btstack_link_key_db_memory.h"
#include "btstack_tlv.h"
#include "btstack_tlv_windows.h"
#include "ble/le_device_db_tlv.h"
#include "hal_led.h"
#include "hci.h"
#include "hci_dump.h"
#include "hci_dump_windows_fs.h"
#include "hci_transport.h"
#include "hci_transport_usb.h"

/* btkeyLib.c で実装されている */
int btstack_main(int argc, const char * argv[]);

static btstack_packet_callback_registration_t hci_event_callback_registration;

static bd_addr_t             local_addr;
static bool                  shutdown_triggered;

/* 診断: HCI 状態遷移タイミング */
static LARGE_INTEGER          perf_freq;
static LARGE_INTEGER          hci_off_time;
static int                    hci_cycle_count = 0;

/* TLV インスタンス（LE device DB 用） */
static const btstack_tlv_t             *btstack_tlv_impl;
static btstack_tlv_windows_t            btstack_tlv_context;

/* ---------------------------------------------------------------------- */
/* 内部ヘルパー                                                             */
/* ---------------------------------------------------------------------- */

/* BTStack の状態変化イベントを処理する。
 * HCI_STATE_WORKING 時にメモリ内リンクキー DB を設定し、
 * HCI_STATE_OFF 時にクリーンアップを行う。 */
static void packet_handler(uint8_t packet_type, uint16_t channel,
                            uint8_t *packet, uint16_t size)
{
    (void)channel;
    (void)size;

    if (packet_type != HCI_EVENT_PACKET) return;

    /* 診断: HCI コマンド完了イベントを監視 */
    if (hci_event_packet_get_type(packet) == HCI_EVENT_COMMAND_COMPLETE) {
        uint16_t opcode = hci_event_command_complete_get_command_opcode(packet);
        uint8_t status = packet[5];
        /* Write Scan Enable = 0x0C1A */
        if (opcode == 0x0C1A) {
            fprintf(stderr, "[btstack] HCI Write_Scan_Enable complete: status=0x%02x\n", status);
        }
        /* Write Class of Device = 0x0C24 */
        if (opcode == 0x0C24) {
            fprintf(stderr, "[btstack] HCI Write_Class_Of_Device complete: status=0x%02x\n", status);
        }
        /* Write Local Name = 0x0C13 */
        if (opcode == 0x0C13) {
            fprintf(stderr, "[btstack] HCI Write_Local_Name complete: status=0x%02x\n", status);
        }
    }

    if (hci_event_packet_get_type(packet) != BTSTACK_EVENT_STATE) return;

    switch (btstack_event_state_get_state(packet)) {
        case HCI_STATE_WORKING:
        {
            hci_cycle_count++;
            gap_local_bd_addr(local_addr);
            LARGE_INTEGER now;
            QueryPerformanceCounter(&now);
            double off_to_on_ms = 0;
            if (hci_off_time.QuadPart > 0 && perf_freq.QuadPart > 0) {
                off_to_on_ms = (double)(now.QuadPart - hci_off_time.QuadPart) * 1000.0 / perf_freq.QuadPart;
            }
            fprintf(stderr, "[btstack] HCI working (#%d): BD_ADDR=%s off->on=%.1fms\n",
                    hci_cycle_count, bd_addr_to_str(local_addr), off_to_on_ms);
            /* 診断: HCI コントローラーの機能を確認 */
#ifdef ENABLE_CLASSIC
            /* hci_classic_supported() は v1.6+ のみ。v1.5.x では ENABLE_CLASSIC マクロで判定 */
            fprintf(stderr, "[btstack] BR/EDR: enabled (ENABLE_CLASSIC)\n");
#endif

            /* TLV ストレージを初期化（LE device DB が内部で使用）
             * ファイルパスに NUL を指定してディスク書き出しを無効化 */
            btstack_tlv_impl = btstack_tlv_windows_init_instance(
                &btstack_tlv_context, "NUL");
            btstack_tlv_set_instance(btstack_tlv_impl, &btstack_tlv_context);
#ifdef ENABLE_CLASSIC
            /* クラシック BT リンクキーはメモリ上のみで管理（ファイル不要） */
            hci_set_link_key_db(btstack_link_key_db_memory_instance());
#endif
            le_device_db_tlv_configure(btstack_tlv_impl, &btstack_tlv_context);
            break;
        }

        case HCI_STATE_OFF:
            QueryPerformanceCounter(&hci_off_time);
            fprintf(stderr, "[btstack] HCI OFF (cycle #%d)\n", hci_cycle_count);
            if (!shutdown_triggered) break;
            btstack_stdin_reset();
            log_info("BTStack shutdown complete");
            break;

        default:
            break;
    }
}

/* シャットダウンを開始する。CTRL-C ハンドラおよび shutdown_gamepad() から呼ばれる。 */
static void trigger_shutdown(void)
{
    fprintf(stderr, "[btstack] shutdown requested\n");
    shutdown_triggered = true;
    hci_power_control(HCI_POWER_OFF);
}

/* 一部の BTStack プラットフォームコードから必要とされる LED トグル関数 */
static int led_state = 0;
void hal_led_toggle(void)
{
    led_state = 1 - led_state;
}

/* ---------------------------------------------------------------------- */
/* プラットフォーム実行関数 — 元の main() の置き換え                        */
/* ---------------------------------------------------------------------- */

static void btstack_platform_run(void)
{
    /* ログ行がすぐに表示されるよう標準出力/エラーのバッファリングを無効化 */
    setvbuf(stdout, NULL, _IONBF, 0);
    setvbuf(stderr, NULL, _IONBF, 0);

    fprintf(stderr, "[btstack] platform starting\n");
    QueryPerformanceFrequency(&perf_freq);

    /* コア初期化 */
    btstack_memory_init();
    btstack_run_loop_init(btstack_run_loop_windows_get_instance());

    /* USB HCI トランスポート（WinUSB ドングル） */
    hci_init(hci_transport_usb_instance(), NULL);

    /* 状態通知ハンドラを登録 */
    hci_event_callback_registration.callback = &packet_handler;
    hci_add_event_handler(&hci_event_callback_registration);

    /* コンソールで CTRL-C が押されたときクリーンシャットダウンを実行 */
    btstack_stdin_windows_init();
    btstack_stdin_window_register_ctrl_c_callback(&trigger_shutdown);

    /* Pro Controller エミュレーターを初期化（btkeyLib.c） */
    btstack_main(0, NULL);

    /* シャットダウンされるまでここでブロック */
    btstack_run_loop_execute();
}

/* ---------------------------------------------------------------------- */
/* Rust から呼び出す公開 API                                                */
/* ---------------------------------------------------------------------- */

/* BTStack を初期化してランループを開始する。シャットダウンまでブロック。 */
void start_gamepad(void)
{
    btstack_platform_run();
}

/* クリーンシャットダウンを要求する。 */
void shutdown_gamepad(void)
{
    trigger_shutdown();
}

/*
 * リンクキーのエクスポート。
 * 各エントリ: BD_ADDR(6) + link_key(16) + key_type(1) = 23 バイト。
 * buf にエントリを連結して書き込み、書き込んだバイト数を返す。
 */
int export_link_keys(uint8_t *buf, int buf_size)
{
    const btstack_link_key_db_t *db = btstack_link_key_db_memory_instance();
    btstack_link_key_iterator_t it;
    int offset = 0;

    if (!db->iterator_init(&it)) return 0;

    bd_addr_t addr;
    link_key_t key;
    link_key_type_t type;
    while (db->iterator_get_next(&it, addr, key, &type)) {
        if (offset + 23 > buf_size) break;
        memcpy(buf + offset, addr, 6);
        memcpy(buf + offset + 6, key, 16);
        buf[offset + 22] = (uint8_t)type;
        offset += 23;
    }
    db->iterator_done(&it);
    fprintf(stderr, "[btstack] export_link_keys: %d bytes (%d entries)\n", offset, offset / 23);
    return offset;
}

/*
 * リンクキーのインポート。
 * export_link_keys と同じフォーマット（23 バイト/エントリ）を受け取る。
 */
void import_link_keys(const uint8_t *buf, int len)
{
    const btstack_link_key_db_t *db = btstack_link_key_db_memory_instance();
    int count = 0;
    for (int i = 0; i + 23 <= len; i += 23) {
        bd_addr_t addr;
        link_key_t key;
        memcpy(addr, buf + i, 6);
        memcpy(key, buf + i + 6, 16);
        link_key_type_t type = (link_key_type_t)buf[i + 22];
        db->put_link_key(addr, key, type);
        fprintf(stderr, "[btstack] import_link_key: addr=%02x:%02x:%02x:%02x:%02x:%02x type=%d\n",
                addr[0], addr[1], addr[2], addr[3], addr[4], addr[5], type);
        count++;
    }
    fprintf(stderr, "[btstack] imported %d link key(s)\n", count);
}
