"""Value types for schema-less Crous data."""

from __future__ import annotations

import base64
import json
from enum import Enum, auto
from typing import Any


class ValueType(Enum):
    """Type discriminant for Crous values."""
    NULL = auto()
    BOOL = auto()
    UINT = auto()
    INT = auto()
    FLOAT = auto()
    STR = auto()
    BYTES = auto()
    ARRAY = auto()
    OBJECT = auto()


class Value:
    """Owned, schema-less Crous value (analogous to serde_json.Value).

    Wraps Python primitives with explicit type discrimination matching the
    Crous wire format. This ensures lossless round-trips (e.g., distinguishing
    unsigned vs signed integers, strings vs bytes).

    For convenience, use Value.from_python() to convert Python dicts/lists.
    """

    __slots__ = ("type", "data")

    def __init__(self, value_type: ValueType, data: Any = None):
        self.type = value_type
        self.data = data

    # -- Constructors --

    @staticmethod
    def null() -> Value:
        return Value(ValueType.NULL)

    @staticmethod
    def bool_(v: bool) -> Value:
        return Value(ValueType.BOOL, v)

    @staticmethod
    def uint(v: int) -> Value:
        if v < 0:
            raise ValueError("uint requires non-negative value")
        return Value(ValueType.UINT, v)

    @staticmethod
    def int_(v: int) -> Value:
        return Value(ValueType.INT, v)

    @staticmethod
    def float_(v: float) -> Value:
        return Value(ValueType.FLOAT, v)

    @staticmethod
    def str_(v: str) -> Value:
        return Value(ValueType.STR, v)

    @staticmethod
    def bytes_(v: bytes) -> Value:
        return Value(ValueType.BYTES, v)

    @staticmethod
    def array(items: list[Value]) -> Value:
        return Value(ValueType.ARRAY, items)

    @staticmethod
    def object(entries: list[tuple[str, Value]]) -> Value:
        return Value(ValueType.OBJECT, entries)

    # -- Conversion from Python natives --

    @staticmethod
    def from_python(obj: Any) -> Value:
        """Convert a Python object to a Crous Value.

        - None → Null
        - bool → Bool
        - int → UInt (if >= 0) or Int (if < 0)
        - float → Float
        - str → Str
        - bytes → Bytes
        - list/tuple → Array
        - dict → Object (preserving insertion order)
        """
        if obj is None:
            return Value.null()
        if isinstance(obj, bool):
            return Value.bool_(obj)
        if isinstance(obj, int):
            return Value.uint(obj) if obj >= 0 else Value.int_(obj)
        if isinstance(obj, float):
            return Value.float_(obj)
        if isinstance(obj, str):
            return Value.str_(obj)
        if isinstance(obj, (bytes, bytearray)):
            return Value.bytes_(bytes(obj))
        if isinstance(obj, (list, tuple)):
            return Value.array([Value.from_python(item) for item in obj])
        if isinstance(obj, dict):
            return Value.object(
                [(str(k), Value.from_python(v)) for k, v in obj.items()]
            )
        raise TypeError(f"Cannot convert {type(obj).__name__} to Crous Value")

    # -- Conversion to Python natives --

    def to_python(self) -> Any:
        """Convert this Crous Value back to a Python native object."""
        if self.type == ValueType.NULL:
            return None
        if self.type == ValueType.BOOL:
            return self.data
        if self.type in (ValueType.UINT, ValueType.INT, ValueType.FLOAT):
            return self.data
        if self.type == ValueType.STR:
            return self.data
        if self.type == ValueType.BYTES:
            return self.data
        if self.type == ValueType.ARRAY:
            return [item.to_python() for item in self.data]
        if self.type == ValueType.OBJECT:
            return {k: v.to_python() for k, v in self.data}
        raise ValueError(f"Unknown value type: {self.type}")

    # -- JSON interop --

    def to_json(self) -> Any:
        """Convert to a JSON-compatible Python object."""
        if self.type == ValueType.NULL:
            return None
        if self.type == ValueType.BOOL:
            return self.data
        if self.type in (ValueType.UINT, ValueType.INT):
            return self.data
        if self.type == ValueType.FLOAT:
            return self.data
        if self.type == ValueType.STR:
            return self.data
        if self.type == ValueType.BYTES:
            return base64.b64encode(self.data).decode("ascii")
        if self.type == ValueType.ARRAY:
            return [item.to_json() for item in self.data]
        if self.type == ValueType.OBJECT:
            return {k: v.to_json() for k, v in self.data}
        raise ValueError(f"Unknown value type: {self.type}")

    @staticmethod
    def from_json(obj: Any) -> Value:
        """Convert a parsed JSON object to a Crous Value.

        Same as from_python but named for clarity when working with JSON data.
        """
        return Value.from_python(obj)

    def to_json_string(self, pretty: bool = False) -> str:
        """Serialize to a JSON string."""
        if pretty:
            return json.dumps(self.to_json(), indent=2, ensure_ascii=False)
        return json.dumps(self.to_json(), ensure_ascii=False)

    # -- Display --

    def __repr__(self) -> str:
        if self.type == ValueType.NULL:
            return "Value.null()"
        if self.type == ValueType.BOOL:
            return f"Value.bool_({self.data!r})"
        if self.type == ValueType.UINT:
            return f"Value.uint({self.data})"
        if self.type == ValueType.INT:
            return f"Value.int_({self.data})"
        if self.type == ValueType.FLOAT:
            return f"Value.float_({self.data!r})"
        if self.type == ValueType.STR:
            return f"Value.str_({self.data!r})"
        if self.type == ValueType.BYTES:
            return f"Value.bytes_({self.data!r})"
        if self.type == ValueType.ARRAY:
            return f"Value.array({self.data!r})"
        if self.type == ValueType.OBJECT:
            return f"Value.object({self.data!r})"
        return f"Value({self.type}, {self.data!r})"

    def __eq__(self, other: object) -> bool:
        if not isinstance(other, Value):
            return NotImplemented
        return self.type == other.type and self.data == other.data

    def __hash__(self) -> int:
        if self.type in (ValueType.ARRAY, ValueType.OBJECT, ValueType.BYTES):
            return hash((self.type, id(self.data)))
        return hash((self.type, self.data))
