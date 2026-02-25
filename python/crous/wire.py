"""Wire types, block types, and compression types for the Crous binary format."""

from enum import IntEnum


class WireType(IntEnum):
    """Wire type identifiers (low 4 bits of tag byte)."""
    NULL = 0x00
    BOOL = 0x01
    VAR_UINT = 0x02
    VAR_INT = 0x03
    FIXED64 = 0x04
    LEN_DELIMITED = 0x05
    START_OBJECT = 0x06
    END_OBJECT = 0x07
    START_ARRAY = 0x08
    END_ARRAY = 0x09
    REFERENCE = 0x0A

    @classmethod
    def from_tag(cls, tag: int) -> "WireType | None":
        """Extract wire type from tag byte (low 4 bits)."""
        try:
            return cls(tag & 0x0F)
        except ValueError:
            return None

    def to_tag(self, flags: int = 0) -> int:
        """Encode as tag byte with optional flags in high nibble."""
        return (flags << 4) | self.value


class BlockType(IntEnum):
    """Block type identifiers."""
    DATA = 0x01
    INDEX = 0x02
    SCHEMA = 0x03
    STRING_DICT = 0x04
    TRAILER = 0xFF

    @classmethod
    def from_byte(cls, b: int) -> "BlockType | None":
        try:
            return cls(b)
        except ValueError:
            return None


class CompressionType(IntEnum):
    """Compression type identifiers."""
    NONE = 0x00
    ZSTD = 0x01
    SNAPPY = 0x02

    @classmethod
    def from_byte(cls, b: int) -> "CompressionType | None":
        try:
            return cls(b)
        except ValueError:
            return None


class TagFlags:
    """Tag byte flags (high 4 bits)."""
    NONE = 0x00
    STRING_DICT_REF = 0x01
    HAS_SCHEMA_ANNOTATION = 0x02
