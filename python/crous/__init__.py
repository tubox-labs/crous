"""
crous — Pure-Python encoder/decoder for the Crous binary serialization format.

A compact, canonical binary serializer and human-readable alternative to JSON.

Quick start:

    >>> import crous
    >>> data = crous.encode({"name": "Alice", "age": 30})
    >>> crous.decode(data)
    {'name': 'Alice', 'age': 30}
"""

from crous.value import Value, ValueType
from crous.encoder import Encoder, encode
from crous.decoder import Decoder, decode
from crous.text import parse as parse_text, pretty_print
from crous.wire import WireType, BlockType, CompressionType
from crous.varint import encode_varint, decode_varint, zigzag_encode, zigzag_decode
from crous.error import CrousError

__version__ = "0.1.0"
__all__ = [
    "Value",
    "ValueType",
    "Encoder",
    "Decoder",
    "encode",
    "decode",
    "parse_text",
    "pretty_print",
    "WireType",
    "BlockType",
    "CompressionType",
    "encode_varint",
    "decode_varint",
    "zigzag_encode",
    "zigzag_decode",
    "CrousError",
]
