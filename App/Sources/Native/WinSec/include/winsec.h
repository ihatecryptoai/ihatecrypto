// The Windows calls of App/Windows/native (winsec.dll, Rust): Windows Hello, DPAPI, the signer's process hardening
// and its start with pipes. Off Windows, winsec_stub.c stands in for them so the Windows signer and the engine build
// and run their tests on a Mac or Linux; it guards nothing.
#ifndef WINSEC_H
#define WINSEC_H

#include <stddef.h>
#include <stdint.h>

#define WINSEC_OK 0
#define WINSEC_CANCELLED 1
#define WINSEC_NOT_FOUND 2
#define WINSEC_UNAVAILABLE 3
#define WINSEC_LOCKED 4
#define WINSEC_WRONG_PARENT 5
#define WINSEC_FAILED (-1)

/// 1 in the stand-in that builds off Windows, 0 in winsec.dll.
int32_t winsec_is_stub(void);

int32_t winsec_hello_available(void);
/// Signs `challenge` with the Windows Hello key `name` (made first when `create`), showing `reason` meanwhile. The
/// signature goes to `out`, freed with winsec_free.
int32_t winsec_hello_sign(const uint8_t *name, size_t name_len, const uint8_t *challenge, size_t challenge_len,
                          const uint8_t *reason, size_t reason_len, int32_t create, uint8_t **out, size_t *out_len);
int32_t winsec_hello_in_tpm(const uint8_t *name, size_t name_len);

int32_t winsec_protect(const uint8_t *data, size_t len, uint8_t **out, size_t *out_len);
int32_t winsec_unprotect(const uint8_t *data, size_t len, uint8_t **out, size_t *out_len);
void winsec_free(uint8_t *ptr, size_t len);
void winsec_wipe(uint8_t *ptr, size_t len);

/// 0, or a bit per step that failed (1 process rule, 2 threads, 4 threads to come, 8 mitigations).
int32_t winsec_harden(void);
int32_t winsec_parent_check(const uint8_t *expected, size_t expected_len);
int32_t winsec_folder_protected(void);

int32_t winsec_spawn_signer(const uint8_t *exe, size_t exe_len, uint32_t *pid, intptr_t *process, intptr_t *to_child,
                            intptr_t *from_child);
int32_t winsec_stdio(intptr_t *input, intptr_t *output);
int32_t winsec_read(intptr_t handle, uint8_t *buf, size_t len);
int32_t winsec_write(intptr_t handle, const uint8_t *buf, size_t len);
void winsec_close(intptr_t handle);
int32_t winsec_allow_foreground(uint32_t pid);

/// Milliseconds since the last keyboard or mouse input anywhere in this Windows session (the app's lock after time away).
uint32_t winsec_idle_ms(void);
/// 1 while this Windows session is locked (Win+L, the lock screen after sleep), 0 otherwise.
int32_t winsec_session_locked(void);

/// A Ledger over USB HID (vendor 0x2c97, usage page 0xffa0), for one exchange: an opaque handle, NULL when none is
/// plugged in or another program holds it. The packets are Ledger's 64-byte HID frames (LedgerFraming).
void *winsec_ledger_open(void);
/// The model of the first Ledger plugged in («Nano S Plus»…) as UTF-8: its length, 0 when none is.
int32_t winsec_ledger_product(uint8_t *out, int32_t cap);
/// 0 when the packet was written.
int32_t winsec_ledger_write(void *device, const uint8_t *packet, int32_t len);
/// One packet into `out`, waiting at most `timeout_ms`: its length, 0 when none came, -1 on an error.
int32_t winsec_ledger_read(void *device, uint8_t *out, int32_t cap, int32_t timeout_ms);
void winsec_ledger_close(void *device);

/// A Ledger over Bluetooth LE (Nano X, Stax, Flex, Apex), linked within `timeout_ms`: an opaque handle, or NULL with
/// the reason in `why` (UTF-8, `why_len` bytes). Frames are Ledger's without the HID channel, at most `mtu` bytes.
void *winsec_ble_open(int32_t timeout_ms, uint8_t *why, int32_t why_cap, int32_t *why_len);
/// The model's name into `model` (its length) and the most one write carries into `mtu`.
int32_t winsec_ble_info(void *link, uint8_t *model, int32_t cap, int32_t *mtu);
int32_t winsec_ble_write(void *link, const uint8_t *frame, int32_t len);
/// One notification into `out`, waiting at most `timeout_ms`: its length, 0 when none came, -1 once the link is gone.
int32_t winsec_ble_read(void *link, uint8_t *out, int32_t cap, int32_t timeout_ms);
void winsec_ble_drain(void *link);
void winsec_ble_close(void *link);

#endif
