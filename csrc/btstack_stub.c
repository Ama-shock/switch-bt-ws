/*
 * btstack_stub.c
 *
 * btkeyLib.c / btstack_platform.c がエクスポートする全シンボルの空スタブ。
 * Windows 以外のプラットフォーム（Linux、macOS 等）でビルドする際に使用します。
 * これにより開発環境や CI でも Rust クレートがコンパイルできます。
 */

#include <stdbool.h>

void  start_gamepad(void)                                              {}
void  shutdown_gamepad(void)                                           {}
bool  gamepad_paired(void)                                             { return false; }
void  send_button(unsigned int button_status, unsigned int press_time) { (void)button_status; (void)press_time; }
void  send_stick_r(unsigned int h, unsigned int v, unsigned int t)     { (void)h; (void)v; (void)t; }
void  send_stick_l(unsigned int h, unsigned int v, unsigned int t)     { (void)h; (void)v; (void)t; }
void  send_gyro(short g1, short g2, short g3)                         { (void)g1; (void)g2; (void)g3; }
void  send_accel(short x, short y, short z)                            { (void)x; (void)y; (void)z; }
void  send_padcolor(unsigned int a, unsigned int b,
                    unsigned int c, unsigned int d)                    { (void)a; (void)b; (void)c; (void)d; }
bool  get_rumble(void)                                                  { return false; }
void  rumble_register(unsigned int key)                                { (void)key; }
void  send_amiibo(const char *path)                                    { (void)path; }
void  reconnect_gamepad(void)                                          {}
void  sync_gamepad(void)                                               {}
void  disconnect_gamepad(void)                                         {}
void  hci_transport_usb_set_target(unsigned short vid, unsigned short pid, int instance) { (void)vid; (void)pid; (void)instance; }
int   export_link_keys(unsigned char *buf, int buf_size)              { (void)buf; (void)buf_size; return 0; }
void  import_link_keys(const unsigned char *buf, int len)             { (void)buf; (void)len; }
unsigned char get_player_leds(void)                                    { return 0; }
