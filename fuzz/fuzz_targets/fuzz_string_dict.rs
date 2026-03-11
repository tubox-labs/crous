//! Fuzz target: StringDict block corruption.
//!
//! Feeds arbitrary bytes as a StringDict block payload to test
//! decoder resilience against malformed dictionary entries.
//!
//! Run with: cargo +nightly fuzz run fuzz_string_dict

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // Construct a minimal valid Crous file with a StringDict block
    // whose payload is the fuzz input, followed by a Data block.
    let mut file = b"CROUSv1\x00".to_vec();

    // StringDict block: type(0x04) | len(varint) | comp(0x00) | checksum(8) | payload
    if data.len() > 65536 {
        return; // Skip unreasonably large inputs
    }

    let checksum = xxhash_rust::xxh64::xxh64(data, 0);
    file.push(0x04); // BlockType::StringDict
    // Encode length as varint
    let mut len_buf = [0u8; 10];
    let len_bytes = leb128_encode(data.len() as u64, &mut len_buf);
    file.extend_from_slice(&len_buf[..len_bytes]);
    file.push(0x00); // CompressionType::None
    file.extend_from_slice(&checksum.to_le_bytes());
    file.extend_from_slice(data);

    // Minimal Data block with a single Null value
    let data_payload = [0x00u8]; // WireType::Null
    let data_checksum = xxhash_rust::xxh64::xxh64(&data_payload, 0);
    file.push(0x01); // BlockType::Data
    file.push(0x01); // length = 1
    file.push(0x00); // CompressionType::None
    file.extend_from_slice(&data_checksum.to_le_bytes());
    file.extend_from_slice(&data_payload);

    // Try decoding — must not panic
    let mut dec = crous_core::Decoder::new(&file);
    let _ = dec.decode_all_owned();
});

fn leb128_encode(mut value: u64, buf: &mut [u8; 10]) -> usize {
    let mut i = 0;
    loop {
        let mut byte = (value & 0x7F) as u8;
        value >>= 7;
        if value != 0 {
            byte |= 0x80;
        }
        buf[i] = byte;
        i += 1;
        if value == 0 {
            break;
        }
    }
    i
}
