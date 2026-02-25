"""Encoder for the Crous binary format.

Encodes Python values into the canonical Crous binary representation.
"""

from __future__ import annotations

import struct
from dataclasses import dataclass, field

from crous.checksum import compute_xxh64
from crous.error import CrousError, NestingTooDeepError
from crous.value import Value, ValueType
from crous.varint import encode_varint, encode_signed_varint
from crous.wire import WireType, BlockType, CompressionType

# File magic
MAGIC = b"CROUSv1"
HEADER_SIZE = 8
FLAGS_NONE = 0x00


@dataclass
class Limits:
    """Configurable resource limits."""
    max_nesting_depth: int = 128
    max_block_size: int = 64 * 1024 * 1024  # 64 MiB
    max_items: int = 1_000_000
    max_memory: int = 256 * 1024 * 1024  # 256 MiB
    max_string_length: int = 16 * 1024 * 1024  # 16 MiB

    @staticmethod
    def strict() -> Limits:
        return Limits(
            max_nesting_depth=32,
            max_block_size=1024 * 1024,
            max_items=10_000,
            max_memory=4 * 1024 * 1024,
            max_string_length=65536,
        )

    @staticmethod
    def unlimited() -> Limits:
        import sys
        m = sys.maxsize
        return Limits(m, m, m, m, m)


class Encoder:
    """Encoder that serializes Values into Crous binary format.

    Example::

        enc = Encoder()
        enc.encode_value(Value.from_python({"name": "Alice", "age": 30}))
        data = enc.finish()
    """

    def __init__(self, limits: Limits | None = None):
        self._output = bytearray()
        self._block_buf = bytearray()
        self._depth = 0
        self._limits = limits or Limits()
        self._header_written = False
        self._flags = FLAGS_NONE
        self._compression = CompressionType.NONE
        self._string_dict: dict[str, int] = {}  # string → index for dedup
        self._dedup_strings = False

    def enable_dedup(self) -> None:
        """Enable string deduplication. Repeated strings within a block
        will be encoded as Reference wire types pointing to the dictionary."""
        self._dedup_strings = True

    def set_flags(self, flags: int) -> None:
        self._flags = flags

    def set_compression(self, comp: CompressionType) -> None:
        self._compression = comp

    def _ensure_header(self) -> None:
        if not self._header_written:
            self._output.extend(MAGIC)
            self._output.append(self._flags)
            self._header_written = True

    def encode_value(self, value: Value) -> None:
        """Encode a Value into the current block buffer."""
        self._encode_inner(value)

    def _encode_inner(self, value: Value) -> None:
        vt = value.type
        buf = self._block_buf

        if vt == ValueType.NULL:
            buf.append(WireType.NULL)

        elif vt == ValueType.BOOL:
            buf.append(WireType.BOOL)
            buf.append(0x01 if value.data else 0x00)

        elif vt == ValueType.UINT:
            buf.append(WireType.VAR_UINT)
            buf.extend(encode_varint(value.data))

        elif vt == ValueType.INT:
            buf.append(WireType.VAR_INT)
            buf.extend(encode_signed_varint(value.data))

        elif vt == ValueType.FLOAT:
            buf.append(WireType.FIXED64)
            buf.extend(struct.pack("<d", value.data))

        elif vt == ValueType.STR:
            s = value.data
            if self._dedup_strings:
                if s in self._string_dict:
                    # Emit a Reference to the dictionary entry.
                    buf.append(WireType.REFERENCE)
                    buf.extend(encode_varint(self._string_dict[s]))
                    return
                # First occurrence: record in dictionary.
                self._string_dict[s] = len(self._string_dict)
            buf.append(WireType.LEN_DELIMITED)
            buf.append(0x00)  # sub-type: UTF-8 string
            encoded = s.encode("utf-8")
            buf.extend(encode_varint(len(encoded)))
            buf.extend(encoded)

        elif vt == ValueType.BYTES:
            buf.append(WireType.LEN_DELIMITED)
            buf.append(0x01)  # sub-type: raw binary
            buf.extend(encode_varint(len(value.data)))
            buf.extend(value.data)

        elif vt == ValueType.ARRAY:
            items: list[Value] = value.data
            if self._depth >= self._limits.max_nesting_depth:
                raise NestingTooDeepError(self._depth, self._limits.max_nesting_depth)
            if len(items) > self._limits.max_items:
                raise CrousError(
                    f"Too many items: {len(items)} exceeds max {self._limits.max_items}"
                )
            buf.append(WireType.START_ARRAY)
            buf.extend(encode_varint(len(items)))
            self._depth += 1
            for item in items:
                self._encode_inner(item)
            self._depth -= 1
            buf.append(WireType.END_ARRAY)

        elif vt == ValueType.OBJECT:
            entries: list[tuple[str, Value]] = value.data
            if self._depth >= self._limits.max_nesting_depth:
                raise NestingTooDeepError(self._depth, self._limits.max_nesting_depth)
            if len(entries) > self._limits.max_items:
                raise CrousError(
                    f"Too many items: {len(entries)} exceeds max {self._limits.max_items}"
                )
            buf.append(WireType.START_OBJECT)
            buf.extend(encode_varint(len(entries)))
            self._depth += 1
            for key, val in entries:
                key_bytes = key.encode("utf-8")
                buf.extend(encode_varint(len(key_bytes)))
                buf.extend(key_bytes)
                self._encode_inner(val)
            self._depth -= 1
            buf.append(WireType.END_OBJECT)

        else:
            raise CrousError(f"Unknown value type: {vt}")

    def flush_block(self) -> int:
        """Flush the current block buffer into a framed block."""
        if not self._block_buf:
            return 0

        self._ensure_header()

        payload = bytes(self._block_buf)
        checksum = compute_xxh64(payload)

        self._output.append(BlockType.DATA)
        self._output.extend(encode_varint(len(payload)))
        self._output.append(self._compression)
        self._output.extend(struct.pack("<Q", checksum))
        self._output.extend(payload)

        size = 1 + 1 + 8 + len(payload)
        self._block_buf.clear()
        self._string_dict.clear()  # Reset per-block dictionary
        return size

    def finish(self) -> bytes:
        """Finish encoding and return the complete binary output.

        Flushes remaining data and appends a file trailer.
        """
        self.flush_block()

        # File trailer: XXH64 of everything so far.
        overall_checksum = compute_xxh64(bytes(self._output))

        self._output.append(BlockType.TRAILER)
        self._output.extend(encode_varint(8))
        self._output.append(CompressionType.NONE)
        trailer_checksum = compute_xxh64(struct.pack("<Q", overall_checksum))
        self._output.extend(struct.pack("<Q", trailer_checksum))
        self._output.extend(struct.pack("<Q", overall_checksum))

        return bytes(self._output)

    def current_size(self) -> int:
        """Current size of output including unflushed block data."""
        return len(self._output) + len(self._block_buf)


def encode(obj: object) -> bytes:
    """Convenience: encode a Python object to Crous binary.

    Accepts dicts, lists, strings, ints, floats, bools, None, and bytes.

    >>> import crous
    >>> data = crous.encode({"name": "Alice", "age": 30})
    >>> len(data) > 8
    True
    """
    enc = Encoder()
    enc.encode_value(Value.from_python(obj))
    return enc.finish()
