// Off Windows only: a stand-in for winsec.dll so the Windows signer and the engine build, run and test on a Mac or in
// Linux. Windows Hello is faked only when WINSEC_FAKE_HELLO=1 (a signature made from the challenge alone), and
// «DPAPI» is a fixed XOR: nothing here protects a real wallet, and no build of it ships.
#ifndef _WIN32
#include "winsec.h"

#include <spawn.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

extern char **environ;

int32_t winsec_is_stub(void) { return 1; }

static int fake_hello(void) {
    const char *v = getenv("WINSEC_FAKE_HELLO");
    return v && strcmp(v, "1") == 0;
}

int32_t winsec_hello_available(void) { return fake_hello() ? WINSEC_OK : WINSEC_UNAVAILABLE; }

static uint8_t *copy(const uint8_t *data, size_t len) {
    uint8_t *out = malloc(len ? len : 1);
    if (out && len) memcpy(out, data, len);
    return out;
}

int32_t winsec_hello_sign(const uint8_t *name, size_t name_len, const uint8_t *challenge, size_t challenge_len,
                          const uint8_t *reason, size_t reason_len, int32_t create, uint8_t **out, size_t *out_len) {
    (void)reason; (void)reason_len; (void)create;
    if (!fake_hello()) return WINSEC_UNAVAILABLE;
    const char *cancel = getenv("WINSEC_FAKE_CANCEL");
    if (cancel && strcmp(cancel, "1") == 0) return WINSEC_CANCELLED;
    // 256 bytes, the same for the same key name and challenge: what an RSA-2048 PKCS#1 v1.5 signature gives.
    uint8_t *sig = malloc(256);
    if (!sig) return WINSEC_FAILED;
    uint64_t h = 1469598103934665603ULL;
    for (size_t i = 0; i < name_len; i++) h = (h ^ name[i]) * 1099511628211ULL;
    for (size_t i = 0; i < challenge_len; i++) h = (h ^ challenge[i]) * 1099511628211ULL;
    for (size_t i = 0; i < 256; i++) { h = (h ^ i) * 1099511628211ULL; sig[i] = (uint8_t)(h >> 56); }
    *out = sig;
    *out_len = 256;
    return WINSEC_OK;
}

int32_t winsec_hello_in_tpm(const uint8_t *name, size_t name_len) { (void)name; (void)name_len; return WINSEC_UNAVAILABLE; }

static int32_t xor_copy(const uint8_t *data, size_t len, uint8_t **out, size_t *out_len) {
    uint8_t *o = copy(data, len);
    if (!o) return WINSEC_FAILED;
    for (size_t i = 0; i < len; i++) o[i] ^= 0x5A;
    *out = o;
    *out_len = len;
    return WINSEC_OK;
}

int32_t winsec_protect(const uint8_t *data, size_t len, uint8_t **out, size_t *out_len) { return xor_copy(data, len, out, out_len); }
int32_t winsec_unprotect(const uint8_t *data, size_t len, uint8_t **out, size_t *out_len) { return xor_copy(data, len, out, out_len); }

void winsec_wipe(uint8_t *ptr, size_t len) {
    volatile uint8_t *p = ptr;
    while (len--) *p++ = 0;
}

void winsec_free(uint8_t *ptr, size_t len) {
    if (!ptr) return;
    winsec_wipe(ptr, len);
    free(ptr);
}

int32_t winsec_harden(void) { return 0; }
int32_t winsec_parent_check(const uint8_t *expected, size_t expected_len) { (void)expected; (void)expected_len; return WINSEC_OK; }
int32_t winsec_folder_protected(void) { return WINSEC_UNAVAILABLE; }

int32_t winsec_spawn_signer(const uint8_t *exe, size_t exe_len, uint32_t *pid, intptr_t *process, intptr_t *to_child,
                            intptr_t *from_child) {
    int in[2], out[2];
    if (pipe(in) != 0) return WINSEC_FAILED;
    if (pipe(out) != 0) { close(in[0]); close(in[1]); return WINSEC_FAILED; }
    char *path = malloc(exe_len + 1);
    if (!path) return WINSEC_FAILED;
    memcpy(path, exe, exe_len);
    path[exe_len] = 0;
    posix_spawn_file_actions_t actions;
    posix_spawn_file_actions_init(&actions);
    posix_spawn_file_actions_adddup2(&actions, in[0], 0);
    posix_spawn_file_actions_adddup2(&actions, out[1], 1);
    posix_spawn_file_actions_addclose(&actions, in[1]);
    posix_spawn_file_actions_addclose(&actions, out[0]);
    char *argv[] = {path, NULL};
    pid_t child;
    int failed = posix_spawn(&child, path, &actions, NULL, argv, environ);
    posix_spawn_file_actions_destroy(&actions);
    free(path);
    close(in[0]);
    close(out[1]);
    if (failed) { close(in[1]); close(out[0]); return WINSEC_FAILED; }
    *pid = (uint32_t)child;
    *process = child;
    *to_child = in[1];
    *from_child = out[0];
    return WINSEC_OK;
}

int32_t winsec_stdio(intptr_t *input, intptr_t *output) { *input = 0; *output = 1; return WINSEC_OK; }

int32_t winsec_read(intptr_t handle, uint8_t *buf, size_t len) {
    size_t done = 0;
    while (done < len) {
        ssize_t got = read((int)handle, buf + done, len - done);
        if (got <= 0) return WINSEC_FAILED;
        done += (size_t)got;
    }
    return WINSEC_OK;
}

int32_t winsec_write(intptr_t handle, const uint8_t *buf, size_t len) {
    size_t done = 0;
    while (done < len) {
        ssize_t put = write((int)handle, buf + done, len - done);
        if (put <= 0) return WINSEC_FAILED;
        done += (size_t)put;
    }
    return WINSEC_OK;
}

void winsec_close(intptr_t handle) { close((int)handle); }
int32_t winsec_allow_foreground(uint32_t pid) { (void)pid; return WINSEC_OK; }

// No session to watch off Windows: never away, never locked.
uint32_t winsec_idle_ms(void) { return 0; }
int32_t winsec_session_locked(void) { return 0; }

// No Ledger off Windows: the Mac's app has its own USB transport (IOKit).
void *winsec_ledger_open(void) { return NULL; }
int32_t winsec_ledger_product(uint8_t *out, int32_t cap) { (void)out; (void)cap; return 0; }
int32_t winsec_ledger_write(void *device, const uint8_t *packet, int32_t len) { (void)device; (void)packet; (void)len; return -1; }
int32_t winsec_ledger_read(void *device, uint8_t *out, int32_t cap, int32_t timeout_ms) { (void)device; (void)out; (void)cap; (void)timeout_ms; return -1; }
void winsec_ledger_close(void *device) { (void)device; }

void *winsec_ble_open(int32_t timeout_ms, uint8_t *why, int32_t why_cap, int32_t *why_len) {
    (void)timeout_ms;
    static const char text[] = "Bluetooth to a Ledger works on Windows.";
    int32_t n = (int32_t)sizeof(text) - 1 < why_cap ? (int32_t)sizeof(text) - 1 : why_cap;
    memcpy(why, text, (size_t)n);
    *why_len = n;
    return NULL;
}
int32_t winsec_ble_info(void *link, uint8_t *model, int32_t cap, int32_t *mtu) { (void)link; (void)model; (void)cap; (void)mtu; return -1; }
int32_t winsec_ble_write(void *link, const uint8_t *frame, int32_t len) { (void)link; (void)frame; (void)len; return -1; }
int32_t winsec_ble_read(void *link, uint8_t *out, int32_t cap, int32_t timeout_ms) { (void)link; (void)out; (void)cap; (void)timeout_ms; return -1; }
void winsec_ble_drain(void *link) { (void)link; }
void winsec_ble_close(void *link) { (void)link; }
#endif
