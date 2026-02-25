"""Custom error types for Crous operations."""


class CrousError(Exception):
    """Base exception for all Crous encoding/decoding errors."""
    pass


class InvalidMagicError(CrousError):
    """File does not start with valid Crous magic bytes."""
    pass


class ChecksumMismatchError(CrousError):
    """Block checksum verification failed."""
    def __init__(self, expected: int, actual: int):
        self.expected = expected
        self.actual = actual
        super().__init__(
            f"Checksum mismatch: expected 0x{expected:016x}, got 0x{actual:016x}"
        )


class VarintOverflowError(CrousError):
    """Varint exceeds 64-bit limit."""
    pass


class NestingTooDeepError(CrousError):
    """Nesting depth exceeds maximum."""
    def __init__(self, depth: int, max_depth: int):
        self.depth = depth
        self.max_depth = max_depth
        super().__init__(f"Nesting depth {depth} exceeds maximum {max_depth}")


class UnexpectedEofError(CrousError):
    """Unexpected end of input."""
    def __init__(self, offset: int):
        self.offset = offset
        super().__init__(f"Unexpected end of input at offset {offset}")


class MemoryLimitError(CrousError):
    """Memory limit exceeded."""
    def __init__(self, requested: int, limit: int):
        self.requested = requested
        self.limit = limit
        super().__init__(f"Memory limit exceeded: {requested} bytes, limit {limit}")


class ParseError(CrousError):
    """Error parsing Crous text format."""
    def __init__(self, line: int, col: int, message: str):
        self.line = line
        self.col = col
        super().__init__(f"Parse error at line {line}, col {col}: {message}")
