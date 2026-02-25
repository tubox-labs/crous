"""Decoder for the Crous binary format.

Decodes Crous binary data into Value objects.
"""

from __future__ import annotations

import struct

from crous.checksum import compute_xxh64
from crous.encoder import MAGIC, HEADER_SIZE, Limits
from crous.error import (
    CrousError,
    InvalidMagicError,
    ChecksumMismatchError,
    NestingTooDeepError,
    UnexpectedEofError,
    MemoryLimitError,
)
from crous.value import Value, ValueType
from crous.varint import decode_varint, decode_signed_varint
from crous.wire import WireType, BlockType, CompressionType


class Decoder:
    """Decoder that reads Crous binary data and produces Values.

    Example::

        dec = Decoder(data)
        value = dec.decode_next()
        print(value.to_python())
    """

    def __init__(self, data: bytes | bytearray | memoryview, limits: Limits | None = None):
        self._data = memoryview(data) if not isinstance(data, memoryview) else data
        self._pos = 0
        self._header = None
        self._limits = limits or Limits()
        self._depth = 0
        self._current_block: tuple[int, int] | None = None  # (start, end)
        self._block_pos = 0
        self._memory_used = 0
        self._string_dict: list[str] = []  # Per-block string dictionary for Reference resolution

    def _track_alloc(self, size: int) -> None:
        self._memory_used += size
        if self._memory_used > self._limits.max_memory:
            raise MemoryLimitError(self._memory_used, self._limits.max_memory)

    def _ensure_header(self) -> None:
        if self._header is not None:
            return
        if len(self._data) < HEADER_SIZE:
            raise UnexpectedEofError(len(self._data))
        if bytes(self._data[:7]) != MAGIC:
            raise InvalidMagicError("Invalid magic bytes: expected CROUSv1")
        self._header = {"flags": self._data[7]}
        self._pos = HEADER_SIZE

    def header(self) -> dict:
        """Parse and return the file header."""
        self._ensure_header()
        return self._header  # type: ignore

    def _read_next_block(self) -> tuple[BlockType, int, int] | None:
        """Read the next block. Returns (block_type, payload_start, payload_end) or None."""
        self._ensure_header()

        if self._pos >= len(self._data):
            return None

        block_type_byte = self._data[self._pos]
        self._pos += 1

        bt = BlockType.from_byte(block_type_byte)
        if bt is None:
            raise CrousError(f"Invalid block type: 0x{block_type_byte:02x}")
        if bt == BlockType.TRAILER:
            return None

        block_len, varint_bytes = decode_varint(self._data, self._pos)
        self._pos += varint_bytes

        if block_len > self._limits.max_block_size:
            raise CrousError(
                f"Block size {block_len} exceeds maximum {self._limits.max_block_size}"
            )

        comp_byte = self._data[self._pos]
        self._pos += 1
        ct = CompressionType.from_byte(comp_byte)
        if ct is None:
            raise CrousError(f"Unknown compression type: 0x{comp_byte:02x}")

        if self._pos + 8 > len(self._data):
            raise UnexpectedEofError(self._pos)

        expected_checksum = struct.unpack_from("<Q", self._data, self._pos)[0]
        self._pos += 8

        payload_start = self._pos
        payload_end = self._pos + block_len
        if payload_end > len(self._data):
            raise UnexpectedEofError(payload_end)

        actual_checksum = compute_xxh64(bytes(self._data[payload_start:payload_end]))
        if actual_checksum != expected_checksum:
            raise ChecksumMismatchError(expected_checksum, actual_checksum)

        self._pos = payload_end
        return bt, payload_start, payload_end

    def decode_next(self) -> Value:
        """Decode the next value from the input.

        Automatically reads blocks as needed.
        """
        if self._current_block is None:
            result = self._read_next_block()
            if result is None:
                raise UnexpectedEofError(self._pos)
            bt, start, end = result
            if bt == BlockType.DATA:
                self._current_block = (start, end)
                self._block_pos = start
                self._string_dict.clear()  # Reset per-block dictionary
            else:
                # Skip non-data blocks, try again.
                return self.decode_next()

        block_start, block_end = self._current_block

        if self._block_pos >= block_end:
            self._current_block = None
            return self.decode_next()

        return self._decode_value_at(block_end)

    def _decode_value_at(self, block_end: int) -> Value:
        """Decode a value starting at self._block_pos."""
        if self._block_pos >= block_end:
            raise UnexpectedEofError(self._block_pos)

        tag = self._data[self._block_pos]
        self._block_pos += 1

        wt = WireType.from_tag(tag)
        if wt is None:
            raise CrousError(f"Invalid wire type tag: 0x{tag:02x}")

        if wt == WireType.NULL:
            return Value.null()

        elif wt == WireType.BOOL:
            if self._block_pos >= block_end:
                raise UnexpectedEofError(self._block_pos)
            b = self._data[self._block_pos] != 0
            self._block_pos += 1
            return Value.bool_(b)

        elif wt == WireType.VAR_UINT:
            val, consumed = decode_varint(self._data, self._block_pos)
            self._block_pos += consumed
            return Value.uint(val)

        elif wt == WireType.VAR_INT:
            val, consumed = decode_signed_varint(self._data, self._block_pos)
            self._block_pos += consumed
            return Value.int_(val)

        elif wt == WireType.FIXED64:
            if self._block_pos + 8 > block_end:
                raise UnexpectedEofError(self._block_pos)
            f = struct.unpack_from("<d", self._data, self._block_pos)[0]
            self._block_pos += 8
            return Value.float_(f)

        elif wt == WireType.LEN_DELIMITED:
            if self._block_pos >= block_end:
                raise UnexpectedEofError(self._block_pos)
            sub_type = self._data[self._block_pos]
            self._block_pos += 1

            length, consumed = decode_varint(self._data, self._block_pos)
            self._block_pos += consumed

            if length > self._limits.max_string_length:
                raise MemoryLimitError(length, self._limits.max_string_length)
            self._track_alloc(length)

            if self._block_pos + length > block_end:
                raise UnexpectedEofError(self._block_pos + length)

            payload = bytes(self._data[self._block_pos : self._block_pos + length])
            self._block_pos += length

            if sub_type == 0x00:
                s = payload.decode("utf-8")
                self._string_dict.append(s)  # Record for Reference resolution
                return Value.str_(s)
            else:
                return Value.bytes_(payload)

        elif wt == WireType.START_ARRAY:
            if self._depth >= self._limits.max_nesting_depth:
                raise NestingTooDeepError(self._depth, self._limits.max_nesting_depth)

            count, consumed = decode_varint(self._data, self._block_pos)
            self._block_pos += consumed

            if count > self._limits.max_items:
                raise CrousError(
                    f"Item count {count} exceeds maximum {self._limits.max_items}"
                )
            self._track_alloc(count * 64)  # Approximate per-item overhead

            self._depth += 1
            items = []
            for _ in range(count):
                items.append(self._decode_value_at(block_end))
            self._depth -= 1

            # Consume EndArray tag
            if (
                self._block_pos < block_end
                and self._data[self._block_pos] == WireType.END_ARRAY
            ):
                self._block_pos += 1

            return Value.array(items)

        elif wt == WireType.START_OBJECT:
            if self._depth >= self._limits.max_nesting_depth:
                raise NestingTooDeepError(self._depth, self._limits.max_nesting_depth)

            count, consumed = decode_varint(self._data, self._block_pos)
            self._block_pos += consumed

            if count > self._limits.max_items:
                raise CrousError(
                    f"Item count {count} exceeds maximum {self._limits.max_items}"
                )
            self._track_alloc(count * 128)

            self._depth += 1
            entries: list[tuple[str, Value]] = []
            for _ in range(count):
                key_len, kc = decode_varint(self._data, self._block_pos)
                self._block_pos += kc

                if self._block_pos + key_len > block_end:
                    raise UnexpectedEofError(self._block_pos + key_len)

                key = bytes(
                    self._data[self._block_pos : self._block_pos + key_len]
                ).decode("utf-8")
                self._block_pos += key_len

                val = self._decode_value_at(block_end)
                entries.append((key, val))
            self._depth -= 1

            # Consume EndObject tag
            if (
                self._block_pos < block_end
                and self._data[self._block_pos] == WireType.END_OBJECT
            ):
                self._block_pos += 1

            return Value.object(entries)

        elif wt in (WireType.END_OBJECT, WireType.END_ARRAY):
            raise CrousError(f"Unexpected end marker: 0x{tag:02x}")

        elif wt == WireType.REFERENCE:
            ref_id, consumed = decode_varint(self._data, self._block_pos)
            self._block_pos += consumed
            # Resolve from per-block string dictionary.
            if ref_id < len(self._string_dict):
                return Value.str_(self._string_dict[ref_id])
            # Unknown reference — return as uint for forward compatibility.
            return Value.uint(ref_id)

        else:
            raise CrousError(f"Unhandled wire type: {wt}")

    def decode_all(self) -> list[Value]:
        """Decode all remaining values."""
        values = []
        while True:
            try:
                values.append(self.decode_next())
            except UnexpectedEofError:
                break
        return values

    @property
    def memory_used(self) -> int:
        return self._memory_used

    @property
    def position(self) -> int:
        return self._pos


def decode(data: bytes | bytearray | memoryview) -> object:
    """Convenience: decode Crous binary to a Python object.

    Returns the first decoded value as a native Python object.

    >>> import crous
    >>> data = crous.encode({"name": "Alice"})
    >>> crous.decode(data)
    {'name': 'Alice'}
    """
    dec = Decoder(data)
    values = dec.decode_all()
    if not values:
        return None
    if len(values) == 1:
        return values[0].to_python()
    return [v.to_python() for v in values]
