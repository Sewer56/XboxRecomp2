/**
 * @file xbox_recomp2.h
 *
 * Handwritten C ABI for xbox-recomp2-xbox mirroring
 * src/lib.rs. Keep in sync so APIs do not drift.
 */
#ifndef XBOX_RECOMP2_XBOX_RECOMP2_H
#define XBOX_RECOMP2_XBOX_RECOMP2_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

// Keep in sync with XB_ERR_* in lib.rs
enum {
    XB_OK = 0,
    XB_ERR_LOAD = 1,
    XB_ERR_NO_DERIVABLE_FUNCTIONS = 2,
    XB_ERR_MISSING_ENTRY_POINT = 3,
    XB_ERR_OUTPUT_PATH_IS_SYMLINK = 4,
    XB_ERR_IO = 5,
    XB_ERR_INVALID_ARGUMENT = 6,
    XB_ERR_PANIC = 7
};

// Receives diagnostic text (not NUL-terminated).
typedef void (*XbDiagFn)(const uint8_t *data, size_t len, void *user);

// Filled by xb_run on failure. Release with xb_error_free.
typedef struct XbRunError {
    int code;
    bool has_address;
    uint64_t address;
    char *path;    // may be NULL
    char *message; // may be NULL
} XbRunError;

// Returns XB_OK or an XB_ERR_* code.
int xb_run(const char *input, XbDiagFn diag, void *user, XbRunError *out_error);
void xb_error_free(XbRunError *err);

typedef struct XbExecutable XbExecutable;

typedef struct XbSection {
    const char *name; // NOT NUL-terminated. Valid while the executable lives
    size_t name_len;
    uint64_t base;
    uint32_t size;
    uint32_t raw_address;
    uint32_t raw_size;
} XbSection;

// A failed parse gives an empty executable. Check with xb_executable_is_empty.
XbExecutable *xb_parse_executable(const uint8_t *data, size_t len);
void xb_executable_free(XbExecutable *exe);
bool xb_executable_is_empty(const XbExecutable *exe);
uint64_t xb_executable_base(const XbExecutable *exe);
uint64_t xb_executable_entry_point(const XbExecutable *exe);
uint32_t xb_executable_size(const XbExecutable *exe);
uint64_t xb_executable_size_of_image(const XbExecutable *exe);
size_t xb_executable_section_count(const XbExecutable *exe);
bool xb_executable_section(const XbExecutable *exe, size_t index, XbSection *out);

#ifdef __cplusplus
}
#endif

#endif
