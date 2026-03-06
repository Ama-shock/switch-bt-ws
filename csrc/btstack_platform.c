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

#include "ble/le_device_db_tlv.h"
#include "btstack_audio.h"
#include "btstack_debug.h"
#include "btstack_event.h"
#include "btstack_memory.h"
#include "btstack_run_loop.h"
#include "btstack_run_loop_windows.h"
#include "btstack_stdin.h"
#include "btstack_stdin_windows.h"
#include "btstack_tlv_windows.h"
#include "classic/btstack_link_key_db_tlv.h"
#include "hal_led.h"
#include "hci.h"
#include "hci_dump.h"
#include "hci_dump_windows_fs.h"
#include "hci_transport.h"
#include "hci_transport_usb.h"

/* btkeyLib.c で実装されている */
int btstack_main(int argc, const char * argv[]);

static btstack_packet_callback_registration_t hci_event_callback_registration;

/* TLV リンクキーデータベースのファイル名プレフィックス / サフィックス */
#define TLV_DB_PATH_PREFIX  "btstack_"
#define TLV_DB_PATH_POSTFIX ".tlv"

static char                  tlv_db_path[100];
static const btstack_tlv_t * tlv_impl;
static btstack_tlv_windows_t tlv_context;
static bd_addr_t             local_addr;
static bool                  shutdown_triggered;

/* ---------------------------------------------------------------------- */
/* 内部ヘルパー                                                             */
/* ---------------------------------------------------------------------- */

/* BTStack の状態変化イベントを処理する。
 * HCI_STATE_WORKING 時に TLV データベースを初期化し、
 * HCI_STATE_OFF 時にクリーンアップを行う。 */
static void packet_handler(uint8_t packet_type, uint16_t channel,
                            uint8_t *packet, uint16_t size)
{
    (void)channel;
    (void)size;

    if (packet_type != HCI_EVENT_PACKET) return;
    if (hci_event_packet_get_type(packet) != BTSTACK_EVENT_STATE) return;

    switch (btstack_event_state_get_state(packet)) {
        case HCI_STATE_WORKING:
            gap_local_bd_addr(local_addr);
            printf("[btstack] 動作中: %s\n", bd_addr_to_str(local_addr));

            /* ローカル BD アドレスを使った TLV パスを構築 */
            btstack_strcpy(tlv_db_path, sizeof(tlv_db_path), TLV_DB_PATH_PREFIX);
            btstack_strcat(tlv_db_path, sizeof(tlv_db_path),
                           bd_addr_to_str_with_delimiter(local_addr, '-'));
            btstack_strcat(tlv_db_path, sizeof(tlv_db_path), TLV_DB_PATH_POSTFIX);

            tlv_impl = btstack_tlv_windows_init_instance(&tlv_context, tlv_db_path);
            btstack_tlv_set_instance(tlv_impl, &tlv_context);
#ifdef ENABLE_CLASSIC
            hci_set_link_key_db(
                btstack_link_key_db_tlv_get_instance(tlv_impl, &tlv_context));
#endif
#ifdef ENABLE_BLE
            le_device_db_tlv_configure(tlv_impl, &tlv_context);
#endif
            break;

        case HCI_STATE_OFF:
            btstack_tlv_windows_deinit(&tlv_context);
            if (!shutdown_triggered) break;
            btstack_stdin_reset();
            log_info("BTStack が正常にシャットダウンしました。");
            /* この後ランループが返り、btstack_platform_run() が終了する */
            break;

        default:
            break;
    }
}

/* シャットダウンを開始する。CTRL-C ハンドラおよび shutdown_gamepad() から呼ばれる。 */
static void trigger_shutdown(void)
{
    printf("[btstack] シャットダウン要求を受け付けました。\n");
    log_info("trigger_shutdown: HCI 電源オフを要求");
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
    /* ログ行がすぐに表示されるよう標準出力のバッファリングを無効化 */
    setvbuf(stdout, NULL, _IONBF, 0);

    printf("[btstack] switch-bt-ws プラットフォーム起動\n");

    /* コア初期化 */
    btstack_memory_init();
    btstack_run_loop_init(btstack_run_loop_windows_get_instance());

    /* USB HCI トランスポート（WinUSB ドングル） */
    hci_init(hci_transport_usb_instance(), NULL);

    /* 状態通知ハンドラを登録（TLV 初期化・シャットダウン処理用） */
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
