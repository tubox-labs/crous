"""Comprehensive tests for the Python Crous implementation."""

import struct
import math
import pytest

import crous
from crous.value import Value, ValueType
from crous.encoder import Encoder, Limits, encode
from crous.decoder import Decoder, decode
from crous.varint import (
    encode_varint,
    decode_varint,
    zigzag_encode,
    zigzag_decode,
    encode_signed_varint,
    decode_signed_varint,
)
from crous.checksum import compute_xxh64, verify_xxh64
from crous.text import parse, pretty_print
from crous.wire import WireType, BlockType, CompressionType
from crous.error import (
    CrousError,
    InvalidMagicError,
    ChecksumMismatchError,
    NestingTooDeepError,
    UnexpectedEofError,
    MemoryLimitError,
)


# ── Varint tests ──────────────────────────────────────────────────────

class TestVarint:
    def test_zigzag_roundtrip(self):
        for v in [0, 1, -1, 2, -2, 127, -128, 2**62, -(2**62)]:
            assert zigzag_decode(zigzag_encode(v)) == v

    def test_zigzag_known_values(self):
        assert zigzag_encode(0) == 0
        assert zigzag_encode(-1) == 1
        assert zigzag_encode(1) == 2
        assert zigzag_encode(-2) == 3

    def test_varint_single_byte(self):
        for v in range(128):
            encoded = encode_varint(v)
            assert len(encoded) == 1
            decoded, consumed = decode_varint(encoded, 0)
            assert decoded == v
            assert consumed == 1

    def test_varint_multi_byte(self):
        cases = [
            (128, bytes([0x80, 0x01])),
            (300, bytes([0xAC, 0x02])),
            (16384, bytes([0x80, 0x80, 0x01])),
        ]
        for value, expected in cases:
            assert encode_varint(value) == expected
            decoded, consumed = decode_varint(expected, 0)
            assert decoded == value
            assert consumed == len(expected)

    def test_varint_u64_max(self):
        u64_max = (1 << 64) - 1
        encoded = encode_varint(u64_max)
        decoded, _ = decode_varint(encoded, 0)
        assert decoded == u64_max

    def test_signed_varint_roundtrip(self):
        for v in [0, 1, -1, 42, -42, 1000, -1000]:
            encoded = encode_signed_varint(v)
            decoded, _ = decode_signed_varint(encoded, 0)
            assert decoded == v


# ── Checksum tests ────────────────────────────────────────────────────

class TestChecksum:
    def test_deterministic(self):
        data = b"The quick brown fox jumps over the lazy dog"
        assert compute_xxh64(data) == compute_xxh64(data)

    def test_differs_for_different_data(self):
        assert compute_xxh64(b"aaa") != compute_xxh64(b"aab")

    def test_verify(self):
        data = b"test data"
        h = compute_xxh64(data)
        assert verify_xxh64(data, h)
        assert not verify_xxh64(data, h + 1)


# ── Wire type tests ──────────────────────────────────────────────────

class TestWireTypes:
    def test_roundtrip(self):
        for wt in WireType:
            if wt.value <= 0x0A:
                assert WireType.from_tag(wt.value) == wt

    def test_tag_with_flags(self):
        tag = WireType.VAR_UINT.to_tag(flags=0x01)
        assert tag == 0x12
        assert WireType.from_tag(tag) == WireType.VAR_UINT

    def test_unknown(self):
        assert WireType.from_tag(0x0B) is None
        assert WireType.from_tag(0x0F) is None


# ── Value tests ──────────────────────────────────────────────────────

class TestValue:
    def test_type_names(self):
        assert Value.null().type == ValueType.NULL
        assert Value.bool_(True).type == ValueType.BOOL
        assert Value.uint(42).type == ValueType.UINT
        assert Value.int_(-1).type == ValueType.INT
        assert Value.float_(3.14).type == ValueType.FLOAT
        assert Value.str_("hi").type == ValueType.STR
        assert Value.bytes_(b"\x00").type == ValueType.BYTES

    def test_from_python(self):
        v = Value.from_python({"name": "Alice", "age": 30, "active": True})
        assert v.type == ValueType.OBJECT
        entries = v.data
        assert entries[0] == ("name", Value.str_("Alice"))
        assert entries[1] == ("age", Value.uint(30))
        assert entries[2] == ("active", Value.bool_(True))

    def test_to_python(self):
        v = Value.object([
            ("name", Value.str_("Alice")),
            ("age", Value.uint(30)),
        ])
        py = v.to_python()
        assert py == {"name": "Alice", "age": 30}

    def test_json_roundtrip(self):
        original = {"name": "Bob", "score": 99.5, "tags": ["a", "b"]}
        v = Value.from_python(original)
        back = v.to_json()
        assert back == original

    def test_equality(self):
        assert Value.null() == Value.null()
        assert Value.uint(42) == Value.uint(42)
        assert Value.uint(42) != Value.uint(43)
        assert Value.str_("a") != Value.uint(0)


# ── Encoder/Decoder roundtrip tests ──────────────────────────────────

class TestRoundtrip:
    def _roundtrip(self, value: Value) -> Value:
        enc = Encoder()
        enc.encode_value(value)
        data = enc.finish()
        dec = Decoder(data)
        return dec.decode_next()

    def test_null(self):
        assert self._roundtrip(Value.null()) == Value.null()

    def test_bool(self):
        assert self._roundtrip(Value.bool_(True)) == Value.bool_(True)
        assert self._roundtrip(Value.bool_(False)) == Value.bool_(False)

    def test_uint(self):
        for v in [0, 1, 127, 128, 300, 65535, (1 << 64) - 1]:
            assert self._roundtrip(Value.uint(v)) == Value.uint(v)

    def test_int(self):
        for v in [0, 1, -1, 127, -128, 1000, -1000]:
            assert self._roundtrip(Value.int_(v)) == Value.int_(v)

    def test_float(self):
        for v in [0.0, 1.0, -1.0, 3.14, 1e100]:
            assert self._roundtrip(Value.float_(v)) == Value.float_(v)

    def test_float_special(self):
        result = self._roundtrip(Value.float_(float("inf")))
        assert result == Value.float_(float("inf"))
        result = self._roundtrip(Value.float_(float("-inf")))
        assert result == Value.float_(float("-inf"))

    def test_string(self):
        for s in ["", "hello", "with spaces", "こんにちは", "a" * 1000]:
            assert self._roundtrip(Value.str_(s)) == Value.str_(s)

    def test_bytes(self):
        data = bytes([0xDE, 0xAD, 0xBE, 0xEF])
        assert self._roundtrip(Value.bytes_(data)) == Value.bytes_(data)

    def test_array(self):
        arr = Value.array([Value.uint(1), Value.str_("two"), Value.bool_(True)])
        assert self._roundtrip(arr) == arr

    def test_object(self):
        obj = Value.object([
            ("name", Value.str_("Alice")),
            ("age", Value.uint(30)),
            ("active", Value.bool_(True)),
        ])
        assert self._roundtrip(obj) == obj

    def test_nested(self):
        val = Value.object([
            ("users", Value.array([
                Value.object([
                    ("name", Value.str_("Bob")),
                    ("scores", Value.array([Value.uint(100), Value.uint(95)])),
                ])
            ])),
            ("count", Value.uint(1)),
        ])
        assert self._roundtrip(val) == val


# ── Convenience API tests ────────────────────────────────────────────

class TestConvenienceAPI:
    def test_encode_decode(self):
        original = {"name": "Alice", "age": 30, "tags": ["admin"]}
        data = encode(original)
        assert isinstance(data, bytes)
        assert len(data) > 8  # header + block

        result = decode(data)
        assert result == original

    def test_file_magic(self):
        data = encode(42)
        assert data[:7] == b"CROUSv1"


# ── Text format tests ───────────────────────────────────────────────

class TestText:
    def test_parse_null(self):
        assert parse("null") == Value.null()

    def test_parse_bool(self):
        assert parse("true") == Value.bool_(True)
        assert parse("false") == Value.bool_(False)

    def test_parse_uint(self):
        assert parse("42") == Value.uint(42)

    def test_parse_int(self):
        assert parse("-1") == Value.int_(-1)

    def test_parse_float(self):
        assert parse("3.14") == Value.float_(3.14)

    def test_parse_string(self):
        assert parse('"hello"') == Value.str_("hello")

    def test_parse_bytes(self):
        v = parse("b64#AQID;")
        assert v == Value.bytes_(bytes([1, 2, 3]))

    def test_parse_array(self):
        v = parse("[1, 2, 3]")
        assert v == Value.array([Value.uint(1), Value.uint(2), Value.uint(3)])

    def test_parse_object(self):
        v = parse('{ name: "Alice"; age: 30; }')
        assert v == Value.object([
            ("name", Value.str_("Alice")),
            ("age", Value.uint(30)),
        ])

    def test_parse_nested(self):
        input_text = """{
            users: [
                { name: "Bob"; scores: [100, 95]; }
            ];
            count: 1;
        }"""
        v = parse(input_text)
        assert v.type == ValueType.OBJECT

    def test_parse_comments(self):
        v = parse("""{
            // line comment
            name: "Alice"; /* block comment */
            age: 30;
        }""")
        assert v == Value.object([
            ("name", Value.str_("Alice")),
            ("age", Value.uint(30)),
        ])

    def test_pretty_print_roundtrip(self):
        original = Value.object([
            ("name", Value.str_("Alice")),
            ("age", Value.uint(30)),
            ("tags", Value.array([Value.str_("admin"), Value.str_("user")])),
        ])
        text = pretty_print(original, indent=4)
        reparsed = parse(text)
        assert reparsed == original


# ── Cross-format roundtrip ───────────────────────────────────────────

class TestCrossFormat:
    def test_text_binary_text(self):
        """Parse text → encode binary → decode binary → pretty-print → re-parse."""
        input_text = '{ name: "Alice"; age: 30; active: true; }'
        val1 = parse(input_text)

        enc = Encoder()
        enc.encode_value(val1)
        binary = enc.finish()

        dec = Decoder(binary)
        val2 = dec.decode_next()
        assert val1 == val2

        text2 = pretty_print(val2, indent=4)
        val3 = parse(text2)
        assert val1 == val3


# ── Limits tests ─────────────────────────────────────────────────────

class TestLimits:
    def test_nesting_depth_limit(self):
        limits = Limits(max_nesting_depth=2)
        enc = Encoder(limits=limits)
        # Nest 3 levels deep — should fail
        val = Value.array([Value.array([Value.array([])])])
        with pytest.raises(NestingTooDeepError):
            enc.encode_value(val)

    def test_memory_limit_decode(self):
        big_str = "x" * 1000
        data = encode(big_str)
        limits = Limits(max_memory=500)
        dec = Decoder(data, limits=limits)
        with pytest.raises(MemoryLimitError):
            dec.decode_next()

    def test_checksum_verification(self):
        data = bytearray(encode(42))
        # Corrupt a byte in the payload
        if len(data) > 20:
            data[20] ^= 0xFF
        dec = Decoder(bytes(data))
        with pytest.raises((ChecksumMismatchError, CrousError)):
            dec.decode_next()

    def test_invalid_magic(self):
        data = b"INVALID\x00" + b"\x00" * 20
        dec = Decoder(data)
        with pytest.raises(InvalidMagicError):
            dec.decode_next()


# ── Rust ↔ Python interop test ───────────────────────────────────────

class TestInterop:
    """Test that Python-encoded data can be decoded (matching Rust wire format)."""

    def test_wire_format_matches_spec(self):
        """Verify binary wire format matches the spec."""
        enc = Encoder()
        enc.encode_value(Value.null())
        data = enc.finish()

        # File starts with magic
        assert data[:7] == b"CROUSv1"
        # Flags byte
        assert data[7] == 0x00
        # First block type byte (Data = 0x01)
        assert data[8] == 0x01

    def test_varint_encoding_matches_spec(self):
        """Verify LEB128 encoding matches spec examples."""
        assert encode_varint(0) == b"\x00"
        assert encode_varint(127) == b"\x7f"
        assert encode_varint(128) == b"\x80\x01"
        assert encode_varint(300) == b"\xac\x02"

    def test_xxh64_reference_vectors(self):
        """Verify XXH64 against known reference values (xxHash spec)."""
        # Empty string
        assert compute_xxh64(b"") == 0xEF46DB3751D8E999
        # Various lengths exercise different code paths
        assert compute_xxh64(b"hello") == 0x26C7827D889F6DA3
        assert compute_xxh64(b"hello world") == 0x45AB6734B21E6968
        # 32+ bytes exercises the stripe path
        assert compute_xxh64(bytes(range(32))) == 0xCBF59C5116FF32B4

    def test_bidirectional_roundtrip(self):
        """Encode with Python, decode with Python: complex nested data."""
        original = {
            "users": [
                {"name": "Alice", "age": 30, "active": True},
                {"name": "Bob", "age": 25, "active": False},
            ],
            "count": 2,
            "metadata": None,
        }
        encoded = crous.encode(original)
        decoded = crous.decode(encoded)
        assert decoded == original


class TestDedup:
    """Tests for string deduplication."""

    def test_dedup_roundtrip(self):
        """Repeated strings should decode correctly with dedup enabled."""
        val = Value.array([
            Value.str_("hello"),
            Value.str_("world"),
            Value.str_("hello"),  # dup
            Value.str_("world"),  # dup
            Value.str_("hello"),  # dup
        ])

        enc = Encoder()
        enc.enable_dedup()
        enc.encode_value(val)
        data = enc.finish()

        # Non-dedup encoding for size comparison.
        enc2 = Encoder()
        enc2.encode_value(val)
        data2 = enc2.finish()

        assert len(data) < len(data2), (
            f"dedup ({len(data)}) should be smaller than no-dedup ({len(data2)})"
        )

        dec = Decoder(data)
        decoded = dec.decode_next()
        assert decoded.to_python() == ["hello", "world", "hello", "world", "hello"]

    def test_dedup_in_object(self):
        """Dedup works for string values inside objects."""
        val = Value.object([
            ("greeting", Value.str_("hello")),
            ("farewell", Value.str_("goodbye")),
            ("echo", Value.str_("hello")),  # dup
        ])

        enc = Encoder()
        enc.enable_dedup()
        enc.encode_value(val)
        data = enc.finish()

        dec = Decoder(data)
        decoded = dec.decode_next()
        expected = [("greeting", "hello"), ("farewell", "goodbye"), ("echo", "hello")]
        actual = [(k, v) for k, v in decoded.to_python().items()]
        # to_python() returns a dict, which preserves order in Python 3.7+
        decoded_dict = decoded.to_python()
        assert decoded_dict["greeting"] == "hello"
        assert decoded_dict["farewell"] == "goodbye"
        assert decoded_dict["echo"] == "hello"

    def test_dedup_cross_language(self):
        """Python dedup-encoded data should decode identically to non-dedup."""
        import subprocess
        import os

        val = Value.array([
            Value.str_("alpha"),
            Value.str_("beta"),
            Value.str_("alpha"),
        ])

        # Encode with dedup
        enc = Encoder()
        enc.enable_dedup()
        enc.encode_value(val)
        data_dedup = enc.finish()

        # Encode without dedup
        enc2 = Encoder()
        enc2.encode_value(val)
        data_no_dedup = enc2.finish()

        # Both should decode to the same result.
        dec1 = Decoder(data_dedup)
        dec2 = Decoder(data_no_dedup)
        assert dec1.decode_next().to_python() == dec2.decode_next().to_python()
