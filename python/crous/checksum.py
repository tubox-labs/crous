"""Checksum utilities using xxh64 (pure Python fallback)."""

import struct

# Pure-Python XXH64 implementation (no external dependencies).
# Constants from the xxHash specification.

_PRIME64_1 = 0x9E3779B185EBCA87
_PRIME64_2 = 0xC2B2AE3D27D4EB4F
_PRIME64_3 = 0x165667B19E3779F9
_PRIME64_4 = 0x85EBCA77C2B2AE63
_PRIME64_5 = 0x27D4EB2F165667C5

_MASK64 = 0xFFFFFFFFFFFFFFFF


def _rotl64(x: int, r: int) -> int:
    return ((x << r) | (x >> (64 - r))) & _MASK64


def _round64(acc: int, input_val: int) -> int:
    acc = (acc + input_val * _PRIME64_2) & _MASK64
    acc = _rotl64(acc, 31)
    acc = (acc * _PRIME64_1) & _MASK64
    return acc


def _merge_round64(acc: int, val: int) -> int:
    val = _round64(0, val)
    acc = (acc ^ val) & _MASK64
    acc = (acc * _PRIME64_1 + _PRIME64_4) & _MASK64
    return acc


def xxh64(data: bytes | bytearray | memoryview, seed: int = 0) -> int:
    """Compute XXH64 hash of data with given seed.

    Returns a 64-bit unsigned integer matching the C reference implementation.
    """
    length = len(data)
    p = 0

    if length >= 32:
        v1 = (seed + _PRIME64_1 + _PRIME64_2) & _MASK64
        v2 = (seed + _PRIME64_2) & _MASK64
        v3 = seed & _MASK64
        v4 = (seed - _PRIME64_1) & _MASK64

        limit = length - 32
        while p <= limit:
            v1 = _round64(v1, struct.unpack_from("<Q", data, p)[0])
            p += 8
            v2 = _round64(v2, struct.unpack_from("<Q", data, p)[0])
            p += 8
            v3 = _round64(v3, struct.unpack_from("<Q", data, p)[0])
            p += 8
            v4 = _round64(v4, struct.unpack_from("<Q", data, p)[0])
            p += 8

        h64 = _rotl64(v1, 1) + _rotl64(v2, 7) + _rotl64(v3, 12) + _rotl64(v4, 18)
        h64 &= _MASK64

        h64 = _merge_round64(h64, v1)
        h64 = _merge_round64(h64, v2)
        h64 = _merge_round64(h64, v3)
        h64 = _merge_round64(h64, v4)
    else:
        h64 = (seed + _PRIME64_5) & _MASK64

    h64 = (h64 + length) & _MASK64

    # Process remaining 8-byte chunks
    limit = length - 8
    while p <= limit:
        k1 = struct.unpack_from("<Q", data, p)[0]
        k1 = (k1 * _PRIME64_2) & _MASK64
        k1 = _rotl64(k1, 31)
        k1 = (k1 * _PRIME64_1) & _MASK64
        h64 = (h64 ^ k1) & _MASK64
        h64 = (_rotl64(h64, 27) * _PRIME64_1 + _PRIME64_4) & _MASK64
        p += 8

    # Process remaining 4-byte chunk
    if p + 4 <= length:
        k1 = struct.unpack_from("<I", data, p)[0]
        h64 = (h64 ^ (k1 * _PRIME64_1)) & _MASK64
        h64 = (_rotl64(h64, 23) * _PRIME64_2 + _PRIME64_3) & _MASK64
        p += 4

    # Process remaining bytes
    while p < length:
        h64 = (h64 ^ (data[p] * _PRIME64_5)) & _MASK64
        h64 = (_rotl64(h64, 11) * _PRIME64_1) & _MASK64
        p += 1

    # Avalanche
    h64 = (h64 ^ (h64 >> 33)) & _MASK64
    h64 = (h64 * _PRIME64_2) & _MASK64
    h64 = (h64 ^ (h64 >> 29)) & _MASK64
    h64 = (h64 * _PRIME64_3) & _MASK64
    h64 = (h64 ^ (h64 >> 32)) & _MASK64

    return h64


def compute_xxh64(data: bytes | bytearray | memoryview) -> int:
    """Compute XXH64 checksum with seed 0."""
    return xxh64(data, 0)


def verify_xxh64(data: bytes | bytearray | memoryview, expected: int) -> bool:
    """Verify data's XXH64 checksum matches expected."""
    return compute_xxh64(data) == expected
