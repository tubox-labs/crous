"""Human-readable Crous text format: parser and pretty-printer."""

from __future__ import annotations

import base64

from crous.error import ParseError
from crous.value import Value, ValueType


# ---------------------------------------------------------------------------
# Parser
# ---------------------------------------------------------------------------


def parse(input_text: str) -> Value:
    """Parse a Crous text document into a Value.

    >>> parse('null')
    Value.null()
    >>> parse('42')
    Value.uint(42)
    >>> parse('"hello"')
    Value.str_('hello')
    """
    parser = _Parser(input_text)
    value = parser.parse_value()
    parser.skip_ws()
    return value


class _Parser:
    def __init__(self, text: str):
        self._text = text
        self._pos = 0
        self._line = 1
        self._col = 1

    def _peek(self) -> str | None:
        if self._pos >= len(self._text):
            return None
        return self._text[self._pos]

    def _advance(self) -> str | None:
        ch = self._peek()
        if ch is None:
            return None
        self._pos += 1
        if ch == "\n":
            self._line += 1
            self._col = 1
        else:
            self._col += 1
        return ch

    def _remaining(self) -> str:
        return self._text[self._pos :]

    def _error(self, msg: str) -> ParseError:
        return ParseError(self._line, self._col, msg)

    def skip_ws(self) -> None:
        while True:
            # Skip whitespace
            while self._peek() is not None and self._peek() in " \t\r\n":
                self._advance()

            # Skip line comments
            if self._remaining().startswith("//"):
                while True:
                    ch = self._advance()
                    if ch is None or ch == "\n":
                        break
                continue

            # Skip block comments
            if self._remaining().startswith("/*"):
                self._advance()  # /
                self._advance()  # *
                depth = 1
                while depth > 0:
                    ch = self._advance()
                    if ch is None:
                        break
                    if ch == "*" and self._peek() == "/":
                        self._advance()
                        depth -= 1
                    elif ch == "/" and self._peek() == "*":
                        self._advance()
                        depth += 1
                continue

            break

    def _expect_char(self, expected: str) -> None:
        self.skip_ws()
        ch = self._advance()
        if ch != expected:
            got = repr(ch) if ch else "EOF"
            raise self._error(f"expected '{expected}', got {got}")

    def parse_value(self) -> Value:
        self.skip_ws()
        ch = self._peek()

        if ch is None:
            raise self._error("unexpected end of input")
        if ch == "{":
            return self._parse_object()
        if ch == "[":
            return self._parse_array()
        if ch == '"':
            return self._parse_string_value()
        if self._remaining().startswith("b64#"):
            return self._parse_bytes()
        if self._remaining().startswith("true"):
            return self._parse_true()
        if self._remaining().startswith("false"):
            return self._parse_false()
        if self._remaining().startswith("null"):
            return self._parse_null()
        if ch in "-+0123456789":
            return self._parse_number()

        raise self._error(f"unexpected character: '{ch}'")

    def _parse_null(self) -> Value:
        for _ in range(4):
            self._advance()
        self._skip_type_annotation()
        return Value.null()

    def _parse_true(self) -> Value:
        for _ in range(4):
            self._advance()
        self._skip_type_annotation()
        return Value.bool_(True)

    def _parse_false(self) -> Value:
        for _ in range(5):
            self._advance()
        self._skip_type_annotation()
        return Value.bool_(False)

    def _parse_number(self) -> Value:
        start = self._pos
        is_negative = False
        is_float = False

        if self._peek() == "-":
            is_negative = True
            self._advance()
        elif self._peek() == "+":
            self._advance()

        while self._peek() is not None:
            ch = self._peek()
            if ch is not None and ch.isdigit():
                self._advance()
            elif ch == ".":
                is_float = True
                self._advance()
            elif ch in "eE":
                is_float = True
                self._advance()
                if self._peek() in "+-":
                    self._advance()
            else:
                break

        num_str = self._text[start : self._pos]
        self._skip_type_annotation()

        if is_float:
            return Value.float_(float(num_str))
        elif is_negative:
            return Value.int_(int(num_str))
        else:
            return Value.uint(int(num_str))

    def _parse_string_value(self) -> Value:
        s = self._parse_quoted_string()
        self._skip_type_annotation()
        return Value.str_(s)

    def _parse_quoted_string(self) -> str:
        self._expect_char('"')
        parts: list[str] = []
        while True:
            ch = self._advance()
            if ch is None:
                raise self._error("unterminated string")
            if ch == '"':
                break
            if ch == "\\":
                esc = self._advance()
                if esc == "n":
                    parts.append("\n")
                elif esc == "t":
                    parts.append("\t")
                elif esc == "r":
                    parts.append("\r")
                elif esc == "\\":
                    parts.append("\\")
                elif esc == '"':
                    parts.append('"')
                elif esc is None:
                    raise self._error("unterminated string escape")
                else:
                    parts.append("\\")
                    parts.append(esc)
            else:
                parts.append(ch)
        return "".join(parts)

    def _parse_bytes(self) -> Value:
        # Consume "b64#"
        for _ in range(4):
            self._advance()
        start = self._pos
        while self._peek() is not None and self._peek() != ";":
            self._advance()
        b64_str = self._text[start : self._pos].strip()
        data = base64.b64decode(b64_str)
        return Value.bytes_(data)

    def _parse_array(self) -> Value:
        self._expect_char("[")
        items: list[Value] = []
        while True:
            self.skip_ws()
            if self._peek() == "]":
                self._advance()
                break
            items.append(self.parse_value())
            self.skip_ws()
            if self._peek() == ",":
                self._advance()
        return Value.array(items)

    def _parse_object(self) -> Value:
        self._expect_char("{")
        entries: list[tuple[str, Value]] = []
        while True:
            self.skip_ws()
            if self._peek() == "}":
                self._advance()
                break
            key = self._parse_key()
            self._expect_char(":")
            value = self.parse_value()
            self._expect_char(";")
            entries.append((key, value))
        return Value.object(entries)

    def _parse_key(self) -> str:
        self.skip_ws()
        if self._peek() == '"':
            return self._parse_quoted_string()
        return self._parse_identifier()

    def _parse_identifier(self) -> str:
        start = self._pos
        ch = self._peek()
        if ch is None or not (ch.isalpha() or ch == "_"):
            raise self._error("expected identifier")
        self._advance()
        while self._peek() is not None:
            ch = self._peek()
            if ch is not None and (ch.isalnum() or ch == "_"):
                self._advance()
            else:
                break
        return self._text[start : self._pos]

    def _skip_type_annotation(self) -> None:
        if self._remaining().startswith("::"):
            self._advance()  # :
            self._advance()  # :
            while self._peek() is not None:
                ch = self._peek()
                if ch is not None and (ch.isalnum() or ch == "_"):
                    self._advance()
                else:
                    break


# ---------------------------------------------------------------------------
# Pretty-printer
# ---------------------------------------------------------------------------


def pretty_print(value: Value, indent: int = 4) -> str:
    """Pretty-print a Value in canonical Crous text notation.

    >>> pretty_print(Value.uint(42))
    '42'
    """
    parts: list[str] = []
    _write_value(parts, value, indent, 0)
    return "".join(parts)


def _write_value(out: list[str], value: Value, indent: int, depth: int) -> None:
    ind = " " * (indent * depth)
    inner = " " * (indent * (depth + 1))

    vt = value.type

    if vt == ValueType.NULL:
        out.append("null")
    elif vt == ValueType.BOOL:
        out.append("true" if value.data else "false")
    elif vt == ValueType.UINT:
        out.append(str(value.data))
    elif vt == ValueType.INT:
        out.append(str(value.data))
    elif vt == ValueType.FLOAT:
        s = repr(value.data)
        # Ensure always has decimal point
        if "." not in s and "e" not in s and "E" not in s and "inf" not in s.lower() and "nan" not in s.lower():
            s += ".0"
        out.append(s)
    elif vt == ValueType.STR:
        out.append('"')
        for ch in value.data:
            if ch == '"':
                out.append('\\"')
            elif ch == "\\":
                out.append("\\\\")
            elif ch == "\n":
                out.append("\\n")
            elif ch == "\r":
                out.append("\\r")
            elif ch == "\t":
                out.append("\\t")
            else:
                out.append(ch)
        out.append('"')
    elif vt == ValueType.BYTES:
        out.append("b64#")
        out.append(base64.b64encode(value.data).decode("ascii"))
    elif vt == ValueType.ARRAY:
        items = value.data
        if not items:
            out.append("[]")
        elif _is_simple_array(items):
            out.append("[")
            for i, item in enumerate(items):
                if i > 0:
                    out.append(", ")
                _write_value(out, item, indent, depth)
            out.append("]")
        else:
            out.append("[\n")
            for i, item in enumerate(items):
                out.append(inner)
                _write_value(out, item, indent, depth + 1)
                if i < len(items) - 1:
                    out.append(",")
                out.append("\n")
            out.append(ind)
            out.append("]")
    elif vt == ValueType.OBJECT:
        entries = value.data
        if not entries:
            out.append("{}")
        else:
            out.append("{\n")
            for key, val in entries:
                out.append(inner)
                if _is_valid_identifier(key):
                    out.append(key)
                else:
                    out.append('"')
                    out.append(key)
                    out.append('"')
                out.append(": ")
                _write_value(out, val, indent, depth + 1)
                out.append(";\n")
            out.append(ind)
            out.append("}")


def _is_simple_array(items: list[Value]) -> bool:
    return len(items) <= 8 and all(
        v.type
        in (
            ValueType.NULL,
            ValueType.BOOL,
            ValueType.UINT,
            ValueType.INT,
            ValueType.FLOAT,
            ValueType.STR,
        )
        for v in items
    )


def _is_valid_identifier(s: str) -> bool:
    if not s:
        return False
    if not (s[0].isalpha() or s[0] == "_"):
        return False
    return all(c.isalnum() or c == "_" for c in s[1:])
