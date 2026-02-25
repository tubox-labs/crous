/*
 * crous.h — C FFI header for the Crous binary format library.
 *
 * Memory ownership:
 *   - crous_encode_buffer: allocates *out; caller must free with crous_free(*out, *out_len).
 *   - crous_decode_buffer: allocates *json_out; caller must free with crous_free(*json_out, *json_len).
 *   - crous_free: frees memory allocated by this library. Safe to call with NULL.
 *
 * All functions return 0 on success, -1 on error.
 *
 * License: MIT OR Apache-2.0
 */

#ifndef CROUS_H
#define CROUS_H

#include <stdint.h>
#include <stddef.h>

#ifdef __cplusplus
extern "C" {
#endif

/**
 * Encode a JSON string to Crous binary format.
 *
 * @param in_ptr    Pointer to JSON text (UTF-8).
 * @param in_len    Length of JSON text in bytes.
 * @param out_ptr   [out] Pointer to allocated Crous binary output.
 * @param out_len   [out] Length of output in bytes.
 * @return          0 on success, -1 on error.
 */
int crous_encode_buffer(
    const uint8_t *in_ptr,
    size_t in_len,
    uint8_t **out_ptr,
    size_t *out_len
);

/**
 * Decode Crous binary data to a JSON string.
 *
 * @param in_ptr    Pointer to Crous binary data.
 * @param in_len    Length of input in bytes.
 * @param json_out  [out] Pointer to allocated JSON string (UTF-8, not null-terminated).
 * @param json_len  [out] Length of JSON string in bytes.
 * @return          0 on success, -1 on error.
 */
int crous_decode_buffer(
    const uint8_t *in_ptr,
    size_t in_len,
    uint8_t **json_out,  /* actually char** but using uint8_t for ABI */
    size_t *json_len
);

/**
 * Free memory allocated by crous_encode_buffer or crous_decode_buffer.
 *
 * @param ptr   Pointer to free (may be NULL — no-op).
 * @param len   Length of the allocation.
 */
void crous_free(uint8_t *ptr, size_t len);

#ifdef __cplusplus
}
#endif

#endif /* CROUS_H */
