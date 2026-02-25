/*
 * test_crous.c — Simple test program for the Crous C FFI.
 *
 * Build:
 *   cargo build --release -p crous-ffi
 *   cc -o test_crous test_crous.c -L target/release -lcrous_ffi
 *
 * Run:
 *   LD_LIBRARY_PATH=target/release ./test_crous
 *   # or on macOS: DYLD_LIBRARY_PATH=target/release ./test_crous
 */

#include <stdio.h>
#include <string.h>
#include "crous.h"

int main(void) {
    const char *json = "{\"name\":\"Alice\",\"age\":30}";
    uint8_t *crous_buf = NULL;
    size_t crous_len = 0;

    printf("Input JSON: %s\n", json);

    /* Encode JSON → Crous */
    int rc = crous_encode_buffer(
        (const uint8_t *)json, strlen(json),
        &crous_buf, &crous_len
    );
    if (rc != 0) {
        fprintf(stderr, "Encode failed!\n");
        return 1;
    }
    printf("Encoded to %zu bytes of Crous binary\n", crous_len);

    /* Decode Crous → JSON */
    uint8_t *json_out = NULL;
    size_t json_len = 0;
    rc = crous_decode_buffer(crous_buf, crous_len, &json_out, &json_len);
    if (rc != 0) {
        fprintf(stderr, "Decode failed!\n");
        crous_free(crous_buf, crous_len);
        return 1;
    }

    printf("Decoded JSON (%zu bytes):\n", json_len);
    fwrite(json_out, 1, json_len, stdout);
    printf("\n");

    /* Cleanup */
    crous_free(crous_buf, crous_len);
    crous_free(json_out, json_len);

    printf("SUCCESS: FFI roundtrip complete.\n");
    return 0;
}
