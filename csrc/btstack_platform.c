/*
 * btstack_platform.c
 *
 * BTStack Windows/WinUSB platform initialisation for Rust integration.
 *
 * This is a lightly-modified copy of port/windows-winusb/main.c.
 * The only structural change is that the C entry-point "main()" has been
 * renamed to "btstack_platform_run()" so it does not conflict with Rust's
 * own main() when linking the static library.
 *
 * start_gamepad()    — called by Rust on a dedicated OS thread; blocks until
 *                      shutdown is requested.
 * shutdown_gamepad() — called by Rust to initiate a clean shutdown.
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

/* Forward declaration — implemented in btkeyLib.c */
int btstack_main(int argc, const char * argv[]);

static btstack_packet_callback_registration_t hci_event_callback_registration;

#define TLV_DB_PATH_PREFIX  "btstack_"
#define TLV_DB_PATH_POSTFIX ".tlv"

static char                  tlv_db_path[100];
static const btstack_tlv_t * tlv_impl;
static btstack_tlv_windows_t tlv_context;
static bd_addr_t             local_addr;
static bool                  shutdown_triggered;

/* ---------------------------------------------------------------------- */
/* Internal helpers                                                         */
/* ---------------------------------------------------------------------- */

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
            printf("[btstack] Running on %s\n", bd_addr_to_str(local_addr));

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
            log_info("BTstack shut down cleanly.");
            /* The run loop will return after this, ending btstack_platform_run(). */
            break;

        default:
            break;
    }
}

static void trigger_shutdown(void)
{
    printf("[btstack] Shutdown requested.\n");
    log_info("trigger_shutdown: powering off HCI");
    shutdown_triggered = true;
    hci_power_control(HCI_POWER_OFF);
}

/* hal_led_toggle() is required by some BTStack platform code */
static int led_state = 0;
void hal_led_toggle(void)
{
    led_state = 1 - led_state;
}

/* ---------------------------------------------------------------------- */
/* Platform run — replaces main() from the original port                   */
/* ---------------------------------------------------------------------- */

static void btstack_platform_run(void)
{
    /* Unbuffered stdout so log lines appear immediately */
    setvbuf(stdout, NULL, _IONBF, 0);

    printf("[btstack] switch-bt-ws platform starting\n");

    /* Core init */
    btstack_memory_init();
    btstack_run_loop_init(btstack_run_loop_windows_get_instance());

    /* USB HCI transport (WinUSB dongle) */
    hci_init(hci_transport_usb_instance(), NULL);

    /* State notifications (for TLV setup and shutdown) */
    hci_event_callback_registration.callback = &packet_handler;
    hci_add_event_handler(&hci_event_callback_registration);

    /* Register CTRL-C handler so a console kill triggers a clean shutdown */
    btstack_stdin_windows_init();
    btstack_stdin_window_register_ctrl_c_callback(&trigger_shutdown);

    /* Initialise the Pro Controller emulator (btkeyLib.c) */
    btstack_main(0, NULL);

    /* Block here until shutdown */
    btstack_run_loop_execute();
}

/* ---------------------------------------------------------------------- */
/* Public API called from Rust                                              */
/* ---------------------------------------------------------------------- */

void start_gamepad(void)
{
    btstack_platform_run();
}

void shutdown_gamepad(void)
{
    trigger_shutdown();
}
