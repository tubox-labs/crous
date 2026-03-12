"""
Type stubs for the ``crous-native`` extension module.

This file provides IDE support (auto-complete, type-checking, inline docs)
for the Rust-backed ``_crous_native`` module built with PyO3.
"""

from __future__ import annotations

from os import PathLike
from typing import Any, Union

__version__: str
"""Package version string (e.g. ``'1.1.1'``)."""

# ---------------------------------------------------------------------------
# Module-level functions
# ---------------------------------------------------------------------------

def encode(obj: Any) -> bytes:
    """Encode a Python object into Crous binary format.

    Supported types: ``dict``, ``list``, ``str``, ``int``, ``float``,
    ``bytes``, ``bool``, and ``None``.

    Args:
        obj: The Python object to encode.

    Returns:
        The encoded binary data as ``bytes``.

    Raises:
        TypeError: If *obj* contains a type that cannot be converted.
        RuntimeError: If the encoder encounters an internal error.

    Example::

        >>> import _crous_native as cn
        >>> data = cn.encode({"name": "Alice", "age": 30})
        >>> isinstance(data, bytes)
        True
    """
    ...

def decode(data: bytes) -> Any:
    """Decode Crous binary ``bytes`` into Python objects.

    Returns a single value if the data contains exactly one top-level
    value, or a ``list`` of values otherwise.

    Args:
        data: Raw Crous binary data.

    Returns:
        The decoded Python object (``dict``, ``list``, ``str``, ``int``,
        ``float``, ``bytes``, ``bool``, or ``None``).

    Raises:
        RuntimeError: If decoding fails (corrupt data, unsupported wire
            type, checksum mismatch, etc.).

    Example::

        >>> import _crous_native as cn
        >>> cn.decode(cn.encode({"key": "value"}))
        {'key': 'value'}
    """
    ...

def encode_to_file(obj: Any, path: Union[str, PathLike[str]]) -> None:
    """Encode a Python object and write the binary output to a file.

    This is a convenience wrapper equivalent to::

        with open(path, "wb") as f:
            f.write(encode(obj))

    Args:
        obj: The Python object to encode.
        path: Destination file path.

    Raises:
        TypeError: If *obj* contains an unsupported type.
        RuntimeError: If encoding fails.
        OSError: If writing to *path* fails.

    Example::

        >>> import _crous_native as cn
        >>> cn.encode_to_file({"name": "Alice"}, "data.crous")
    """
    ...

def decode_from_file(path: Union[str, PathLike[str]]) -> Any:
    """Read a Crous binary file and decode it into Python objects.

    This is a convenience wrapper equivalent to::

        with open(path, "rb") as f:
            return decode(f.read())

    Args:
        path: Source file path to read.

    Returns:
        The decoded Python object.

    Raises:
        RuntimeError: If decoding fails.
        OSError: If reading *path* fails.

    Example::

        >>> import _crous_native as cn
        >>> cn.encode_to_file([1, 2, 3], "data.crous")
        >>> cn.decode_from_file("data.crous")
        [1, 2, 3]
    """
    ...

def parse_text(text: str) -> Any:
    """Parse Crous human-readable text notation into a Python object.

    Args:
        text: A string in Crous text format.

    Returns:
        The parsed Python value.

    Raises:
        ValueError: If the text is syntactically invalid.

    Example::

        >>> import _crous_native as cn
        >>> cn.parse_text('{name: "Alice"; age: 30;}')
        {'name': 'Alice', 'age': 30}
    """
    ...

def pretty_print(obj: Any, indent: int = 2) -> str:
    """Format a Python object in Crous human-readable text notation.

    Args:
        obj: The Python object to format.
        indent: Number of spaces per indentation level (default ``2``).

    Returns:
        A string in Crous text format.

    Raises:
        TypeError: If *obj* cannot be converted to a Crous value.

    Example::

        >>> import _crous_native as cn
        >>> print(cn.pretty_print({"name": "Alice", "active": True}))
        {
          name: "Alice";
          active: true;
        }
    """
    ...

# ---------------------------------------------------------------------------
# Encoder class
# ---------------------------------------------------------------------------

class Encoder:
    """Incremental Crous encoder.

    Build up encoded data by calling :meth:`encode` one or more times,
    then retrieve the final binary with :meth:`finish` (or write it
    directly with :meth:`finish_to_file`).

    Example::

        >>> enc = Encoder()
        >>> enc.enable_dedup()
        >>> enc.set_compression("lz4")
        >>> enc.encode({"key": "value"})
        >>> data = enc.finish()
    """

    def __init__(self) -> None:
        """Create a new encoder."""
        ...

    def enable_dedup(self) -> None:
        """Enable string deduplication for subsequent blocks.

        When enabled, repeated strings are stored once and referenced
        by index, reducing output size for data with many duplicate
        string values.

        Raises:
            RuntimeError: If the encoder has already been finished.
        """
        ...

    def set_compression(self, comp: str) -> None:
        """Set the compression algorithm for subsequent blocks.

        Args:
            comp: One of ``"none"``, ``"lz4"``, ``"zstd"``, or
                ``"snappy"`` (case-insensitive).

        Raises:
            ValueError: If *comp* is not a recognised algorithm name.
            RuntimeError: If the encoder has already been finished.
        """
        ...

    def encode(self, obj: Any) -> None:
        """Encode a Python value into the current block.

        Args:
            obj: The Python object to encode.

        Raises:
            TypeError: If *obj* contains an unsupported type.
            RuntimeError: If encoding fails or the encoder is finished.
        """
        ...

    def finish(self) -> bytes:
        """Finalise the encoder and return the Crous binary output.

        The encoder **cannot** be used after this call.

        Returns:
            The complete encoded binary as ``bytes``.

        Raises:
            RuntimeError: If the encoder has already been finished.
        """
        ...

    def finish_to_file(self, path: Union[str, PathLike[str]]) -> None:
        """Finalise the encoder and write the output directly to a file.

        The encoder **cannot** be used after this call.

        Args:
            path: Destination file path.

        Raises:
            RuntimeError: If the encoder has already been finished.
            OSError: If writing to *path* fails.
        """
        ...

# ---------------------------------------------------------------------------
# Decoder class
# ---------------------------------------------------------------------------

class CrousDecoder:
    """Incremental Crous decoder.

    Wraps binary data and decodes all top-level values from it.

    Example::

        >>> dec = CrousDecoder(data)
        >>> values = dec.decode_all()
        >>> print(values[0])
    """

    def __init__(self, data: bytes) -> None:
        """Create a decoder over the given binary data.

        Args:
            data: Raw Crous binary ``bytes`` to decode.
        """
        ...

    def decode_all(self) -> list[Any]:
        """Decode all values from the binary data.

        Can only be called **once** per decoder instance.

        Returns:
            A list of decoded Python objects.

        Raises:
            RuntimeError: If the decoder has already been consumed, or
                if decoding fails.
        """
        ...
