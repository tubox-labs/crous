"""Tests for the crous-python native extension (_crous_native)."""

import pytest
import _crous_native as cn


class TestEncodeDecode:
    """Test module-level encode/decode functions."""

    def test_roundtrip_dict(self):
        obj = {"name": "Alice", "age": 30}
        assert cn.decode(cn.encode(obj)) == obj

    def test_roundtrip_list(self):
        obj = [1, 2, 3, "hello"]
        assert cn.decode(cn.encode(obj)) == obj

    def test_roundtrip_string(self):
        assert cn.decode(cn.encode("hello world")) == "hello world"

    def test_roundtrip_int(self):
        assert cn.decode(cn.encode(42)) == 42

    def test_roundtrip_negative_int(self):
        assert cn.decode(cn.encode(-42)) == -42

    def test_roundtrip_float(self):
        result = cn.decode(cn.encode(3.15))
        assert abs(result - 3.15) < 1e-10

    def test_roundtrip_bool(self):
        assert cn.decode(cn.encode(True)) is True
        assert cn.decode(cn.encode(False)) is False

    def test_roundtrip_none(self):
        assert cn.decode(cn.encode(None)) is None

    def test_roundtrip_bytes(self):
        data = b"\x00\x01\x02\xff"
        assert cn.decode(cn.encode(data)) == data

    def test_roundtrip_nested(self):
        obj = {
            "users": [
                {"name": "Alice", "active": True},
                {"name": "Bob", "active": False},
            ],
            "count": 2,
        }
        assert cn.decode(cn.encode(obj)) == obj

    def test_roundtrip_mixed_array(self):
        obj = [None, True, 42, -10, 3.15, "text", b"\xab"]
        result = cn.decode(cn.encode(obj))
        assert result[0] is None
        assert result[1] is True
        assert result[2] == 42
        assert result[3] == -10
        assert abs(result[4] - 3.15) < 1e-10
        assert result[5] == "text"
        assert result[6] == b"\xab"

    def test_empty_dict(self):
        assert cn.decode(cn.encode({})) == {}

    def test_empty_list(self):
        assert cn.decode(cn.encode([])) == []

    def test_empty_string(self):
        assert cn.decode(cn.encode("")) == ""

    def test_encode_returns_bytes(self):
        result = cn.encode(42)
        assert isinstance(result, bytes)

    def test_unsupported_type_raises(self):
        with pytest.raises(TypeError):
            cn.encode(object())


class TestEncoderClass:
    """Test the Encoder class."""

    def test_basic_roundtrip(self):
        enc = cn.Encoder()
        enc.encode({"key": "value"})
        data = enc.finish()
        assert cn.decode(data) == {"key": "value"}

    def test_dedup(self):
        enc = cn.Encoder()
        enc.enable_dedup()
        enc.encode(["hello", "world", "hello"])
        data = enc.finish()
        assert cn.decode(data) == ["hello", "world", "hello"]

    def test_finish_twice_raises(self):
        enc = cn.Encoder()
        enc.encode(42)
        enc.finish()
        with pytest.raises(RuntimeError):
            enc.finish()

    def test_encode_after_finish_raises(self):
        enc = cn.Encoder()
        enc.encode(42)
        enc.finish()
        with pytest.raises(RuntimeError):
            enc.encode(99)

    def test_set_compression_invalid(self):
        enc = cn.Encoder()
        with pytest.raises(ValueError):
            enc.set_compression("invalid")


class TestDecoderClass:
    """Test the CrousDecoder class."""

    def test_decode_all(self):
        data = cn.encode({"a": 1})
        dec = cn.CrousDecoder(data)
        result = dec.decode_all()
        assert isinstance(result, list)
        assert len(result) == 1
        assert result[0] == {"a": 1}

    def test_decode_all_twice_raises(self):
        data = cn.encode(42)
        dec = cn.CrousDecoder(data)
        dec.decode_all()
        with pytest.raises(RuntimeError):
            dec.decode_all()


class TestInterop:
    """Test interop between native extension and pure Python."""

    def test_native_encode_matches_pure_python(self):
        """Data encoded by native extension should be decodable by pure Python."""
        try:
            import sys
            import os

            sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", "..", "python"))
            import crous

            obj = {"name": "Alice", "age": 30}
            native_bytes = cn.encode(obj)
            pure_result = crous.decode(native_bytes)

            # The pure Python decoder returns native Python types via .to_python().
            if isinstance(pure_result, dict):
                assert pure_result["name"] == "Alice"
            else:
                # Value wrapper — call to_python() if available.
                py_val = pure_result.to_python()
                assert py_val["name"] == "Alice"
        except ImportError:
            pytest.skip("pure Python crous package not available")
