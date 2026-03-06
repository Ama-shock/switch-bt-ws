/*
 * btstack_stub.c
 *
 * No-op stubs for all symbols exported by btkeyLib.c / btstack_platform.c.
 * Used when building on non-Windows platforms (Linux, macOS) so that the
 * Rust crate compiles cleanly for development and CI purposes.
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
