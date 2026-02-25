"""LEB128 varint and ZigZag encoding/decoding."""

from crous.error import VarintOverflowError, UnexpectedEofError


def zigzag_encode(n: int) -> int:
    """Encode a signed integer using ZigZag encoding.

    Maps: 0→0, -1→1, 1→2, -2→3, 2→4, ...

    >>> zigzag_encode(0)
    0
    >>> zigzag_encode(-1)
    1
    >>> zigzag_encode(1)
    2
    """
    return ((n << 1) ^ (n >> 63)) & 0xFFFFFFFFFFFFFFFF


def zigzag_decode(n: int) -> int:
    """Decode a ZigZag-encoded unsigned integer back to signed.

    >>> zigzag_decode(0)
    0
    >>> zigzag_decode(1)
    -1
    >>> zigzag_decode(2)
    1
    """
    result = (n >> 1) ^ -(n & 1)
    # Convert to signed 64-bit
    if result >= (1 << 63):
        result -= 1 << 64
    return result


def encode_varint(value: int) -> bytes:
    """Encode an unsigned 64-bit integer as LEB128.

    >>> encode_varint(0)
    b'\\x00'
    >>> encode_varint(127)
    b'\\x7f'
    >>> encode_varint(128)
    b'\\x80\\x01'
    >>> encode_varint(300)
    b'\\xac\\x02'
    """
    if value < 0:
        raise ValueError("encode_varint requires non-negative value")

    buf = bytearray()
    while True:
        byte = value & 0x7F
        value >>= 7
        if value:
            byte |= 0x80
        buf.append(byte)
        if not value:
            break
    return bytes(buf)


def decode_varint(data: bytes | bytearray | memoryview, offset: int = 0) -> tuple[int, int]:
    """Decode an unsigned LEB128 varint from data at offset.

    Returns (value, bytes_consumed).

    >>> decode_varint(b'\\xac\\x02', 0)
    (300, 2)
    """
    result = 0
    shift = 0
    i = 0

    while True:
        if offset + i >= len(data):
            raise UnexpectedEofError(offset + i)

        byte = data[offset + i]
        i += 1

        if i > 10:
            raise VarintOverflowError("Varint exceeds 10 bytes (64-bit limit)")
        if i == 10 and byte > 0x01:
            raise VarintOverflowError("Varint overflow: 10th byte > 0x01")

        result |= (byte & 0x7F) << shift
        if not (byte & 0x80):
            return result, i
        shift += 7


def encode_signed_varint(value: int) -> bytes:
    """Encode a signed integer as ZigZag + LEB128."""
    return encode_varint(zigzag_encode(value))


def decode_signed_varint(data: bytes | bytearray | memoryview, offset: int = 0) -> tuple[int, int]:
    """Decode a ZigZag + LEB128 signed integer.

    Returns (value, bytes_consumed).
    """
    raw, consumed = decode_varint(data, offset)
    return zigzag_decode(raw), consumed
