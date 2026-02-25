//! # crous-ffi
//!
//! C-compatible FFI bindings for the Crous encoder/decoder.
//!
//! Memory ownership:
//! - `crous_encode_buffer`: caller provides input, library allocates output.
//!   Caller must free output with `crous_free`.
//! - `crous_decode_buffer`: caller provides Crous binary input, library allocates
//!   JSON string output. Caller must free with `crous_free`.
//! - `crous_free`: frees memory allocated by this library.

use std::slice;

/// Encode a JSON string to Crous binary format.
///
/// # Safety
/// - `in_ptr` must point to `in_len` valid bytes of JSON text.
/// - `out_ptr` and `out_len` must be valid, non-null pointers.
/// - Caller must free `*out_ptr` with `crous_free`.
///
/// Returns 0 on success, -1 on error.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crous_encode_buffer(
    in_ptr: *const u8,
    in_len: usize,
    out_ptr: *mut *mut u8,
    out_len: *mut usize,
) -> i32 {
    if in_ptr.is_null() || out_ptr.is_null() || out_len.is_null() {
        return -1;
    }

    let input = unsafe { slice::from_raw_parts(in_ptr, in_len) };

    // Parse JSON input.
    let json_str = match std::str::from_utf8(input) {
        Ok(s) => s,
        Err(_) => return -1,
    };

    let json_value: serde_json::Value = match serde_json::from_str(json_str) {
        Ok(v) => v,
        Err(_) => return -1,
    };

    let crous_value = crous_core::Value::from(&json_value);

    let mut encoder = crous_core::Encoder::new();
    if encoder.encode_value(&crous_value).is_err() {
        return -1;
    }

    let bytes = match encoder.finish() {
        Ok(b) => b,
        Err(_) => return -1,
    };

    // Allocate output buffer.
    let boxed = bytes.into_boxed_slice();
    let len = boxed.len();
    let raw = Box::into_raw(boxed) as *mut u8;

    unsafe {
        *out_ptr = raw;
        *out_len = len;
    }

    0
}

/// Decode a Crous binary buffer to a JSON string.
///
/// # Safety
/// - `in_ptr` must point to `in_len` valid bytes of Crous binary data.
/// - `json_out` and `json_len` must be valid, non-null pointers.
/// - Caller must free `*json_out` with `crous_free`.
///
/// Returns 0 on success, -1 on error.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crous_decode_buffer(
    in_ptr: *const u8,
    in_len: usize,
    json_out: *mut *mut u8,
    json_len: *mut usize,
) -> i32 {
    if in_ptr.is_null() || json_out.is_null() || json_len.is_null() {
        return -1;
    }

    let input = unsafe { slice::from_raw_parts(in_ptr, in_len) };

    let mut decoder = crous_core::Decoder::new(input);
    let values = match decoder.decode_all_owned() {
        Ok(v) => v,
        Err(_) => return -1,
    };

    // Convert to JSON.
    let json_values: Vec<serde_json::Value> = values.iter().map(serde_json::Value::from).collect();
    let json_string = if json_values.len() == 1 {
        serde_json::to_string_pretty(&json_values[0]).unwrap_or_default()
    } else {
        serde_json::to_string_pretty(&json_values).unwrap_or_default()
    };

    let bytes = json_string.into_bytes().into_boxed_slice();
    let len = bytes.len();
    let raw = Box::into_raw(bytes) as *mut u8;

    unsafe {
        *json_out = raw;
        *json_len = len;
    }

    0
}

/// Free memory allocated by `crous_encode_buffer` or `crous_decode_buffer`.
///
/// # Safety
/// - `ptr` must have been allocated by this library, or be null (no-op).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crous_free(ptr: *mut u8, len: usize) {
    if !ptr.is_null() && len > 0 {
        unsafe {
            drop(Box::from_raw(std::ptr::slice_from_raw_parts_mut(ptr, len)));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ptr;

    #[test]
    fn ffi_encode_decode_roundtrip() {
        let json_input = br#"{"name":"Alice","age":30}"#;

        let mut out_ptr: *mut u8 = ptr::null_mut();
        let mut out_len: usize = 0;

        let rc = unsafe {
            crous_encode_buffer(
                json_input.as_ptr(),
                json_input.len(),
                &mut out_ptr,
                &mut out_len,
            )
        };
        assert_eq!(rc, 0);
        assert!(!out_ptr.is_null());
        assert!(out_len > 0);

        // Decode back.
        let mut json_out: *mut u8 = ptr::null_mut();
        let mut json_len: usize = 0;

        let rc2 = unsafe { crous_decode_buffer(out_ptr, out_len, &mut json_out, &mut json_len) };
        assert_eq!(rc2, 0);

        let json_str =
            unsafe { std::str::from_utf8(slice::from_raw_parts(json_out, json_len)).unwrap() };
        assert!(json_str.contains("Alice"));
        assert!(json_str.contains("30"));

        // Free.
        unsafe {
            crous_free(out_ptr, out_len);
            crous_free(json_out, json_len);
        }
    }
}
