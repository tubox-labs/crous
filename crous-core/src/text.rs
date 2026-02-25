//! Human-readable Crous text format: parser and pretty-printer.
//!
//! # Crous Text Syntax (summary)
//!
//! The Crous textual notation is a unique, deterministic syntax that maps
//! 1:1 to the binary format. It is NOT a JSON clone — it has its own rules:
//!
//! - Objects use `{ key: value; key2: value2; }` with `;` as mandatory terminator.
//! - Arrays use `[ value, value, value ]` with `,` separator.
//! - Strings are double-quoted: `"hello world"`.
//! - Binary data uses `b64#<base64>;` marker.
//! - Integers: unsigned are bare digits, signed use `+` or `-` prefix.
//! - Floats use decimal point: `3.14`, `-2.718`.
//! - Null is `null`, booleans are `true`/`false`.
//! - Optional type annotations: `42::u32`, `"hello"::str`.
//! - Comments: `// line comment` and `/* block comment */`.
//!
//! # ABNF Grammar
//!
//! ```abnf
//! document     = value
//! value        = null / boolean / integer / float / string / bytes
//!              / array / object
//! null         = "null"
//! boolean      = "true" / "false"
//! integer      = [sign] 1*DIGIT [type-ann]
//! float        = [sign] 1*DIGIT "." 1*DIGIT [exponent] [type-ann]
//! exponent     = ("e" / "E") [sign] 1*DIGIT
//! sign         = "+" / "-"
//! string       = DQUOTE *char DQUOTE [type-ann]
//! bytes        = "b64#" base64-data ";"
//! base64-data  = *( ALPHA / DIGIT / "+" / "/" / "=" )
//! array        = "[" [value *("," value)] "]"
//! object       = "{" *field "}"
//! field        = key ":" value ";"
//! key          = identifier / string
//! identifier   = (ALPHA / "_") *(ALPHA / DIGIT / "_")
//! type-ann     = "::" type-name
//! type-name    = identifier
//! comment      = line-comment / block-comment
//! line-comment = "//" *(%x20-7E) LF
//! block-comment= "/*" *(comment-char) "*/"
//! ```

use crate::error::{CrousError, Result};
use crate::value::Value;
use base64::Engine;

// ---------------------------------------------------------------------------
// Parser
// ---------------------------------------------------------------------------

/// Parse a Crous text document into a `Value`.
///
/// ```
/// use crous_core::text::parse;
/// use crous_core::Value;
///
/// let v = parse(r#"{ name: "Alice"; age: 30; }"#).unwrap();
/// assert_eq!(
///     v,
///     Value::Object(vec![
///         ("name".into(), Value::Str("Alice".into())),
///         ("age".into(), Value::UInt(30)),
///     ])
/// );
/// ```
pub fn parse(input: &str) -> Result<Value> {
    let mut parser = Parser::new(input);
    let value = parser.parse_value()?;
    parser.skip_whitespace_and_comments();
    Ok(value)
}

struct Parser<'a> {
    input: &'a str,
    pos: usize,
    line: usize,
    col: usize,
}

impl<'a> Parser<'a> {
    fn new(input: &'a str) -> Self {
        Self {
            input,
            pos: 0,
            line: 1,
            col: 1,
        }
    }

    fn peek(&self) -> Option<char> {
        self.input[self.pos..].chars().next()
    }

    fn advance(&mut self) -> Option<char> {
        let ch = self.peek()?;
        self.pos += ch.len_utf8();
        if ch == '\n' {
            self.line += 1;
            self.col = 1;
        } else {
            self.col += 1;
        }
        Some(ch)
    }

    fn remaining(&self) -> &'a str {
        &self.input[self.pos..]
    }

    fn error(&self, msg: impl Into<String>) -> CrousError {
        CrousError::ParseError {
            line: self.line,
            col: self.col,
            message: msg.into(),
        }
    }

    fn skip_whitespace_and_comments(&mut self) {
        loop {
            // Skip whitespace.
            while let Some(ch) = self.peek() {
                if ch.is_whitespace() {
                    self.advance();
                } else {
                    break;
                }
            }
            // Skip line comments.
            if self.remaining().starts_with("//") {
                while let Some(ch) = self.advance() {
                    if ch == '\n' {
                        break;
                    }
                }
                continue;
            }
            // Skip block comments.
            if self.remaining().starts_with("/*") {
                self.advance(); // '/'
                self.advance(); // '*'
                let mut depth = 1;
                while depth > 0 {
                    match self.advance() {
                        Some('*') if self.peek() == Some('/') => {
                            self.advance();
                            depth -= 1;
                        }
                        Some('/') if self.peek() == Some('*') => {
                            self.advance();
                            depth += 1;
                        }
                        Some(_) => {}
                        None => break,
                    }
                }
                continue;
            }
            break;
        }
    }

    fn expect_char(&mut self, expected: char) -> Result<()> {
        self.skip_whitespace_and_comments();
        match self.advance() {
            Some(ch) if ch == expected => Ok(()),
            Some(ch) => Err(self.error(format!("expected '{expected}', got '{ch}'"))),
            None => Err(self.error(format!("expected '{expected}', got EOF"))),
        }
    }

    fn parse_value(&mut self) -> Result<Value> {
        self.skip_whitespace_and_comments();

        match self.peek() {
            None => Err(self.error("unexpected end of input")),
            Some('{') => self.parse_object(),
            Some('[') => self.parse_array(),
            Some('"') => self.parse_string_value(),
            Some('b') if self.remaining().starts_with("b64#") => self.parse_bytes(),
            Some('t') if self.remaining().starts_with("true") => self.parse_true(),
            Some('f') if self.remaining().starts_with("false") => self.parse_false(),
            Some('n') if self.remaining().starts_with("null") => self.parse_null(),
            Some(ch) if ch == '-' || ch == '+' || ch.is_ascii_digit() => self.parse_number(),
            Some(ch) => Err(self.error(format!("unexpected character: '{ch}'"))),
        }
    }

    fn parse_null(&mut self) -> Result<Value> {
        for _ in 0..4 {
            self.advance();
        }
        self.skip_type_annotation();
        Ok(Value::Null)
    }

    fn parse_true(&mut self) -> Result<Value> {
        for _ in 0..4 {
            self.advance();
        }
        self.skip_type_annotation();
        Ok(Value::Bool(true))
    }

    fn parse_false(&mut self) -> Result<Value> {
        for _ in 0..5 {
            self.advance();
        }
        self.skip_type_annotation();
        Ok(Value::Bool(false))
    }

    fn parse_number(&mut self) -> Result<Value> {
        let start = self.pos;
        let mut is_negative = false;
        let mut is_float = false;

        if self.peek() == Some('-') {
            is_negative = true;
            self.advance();
        } else if self.peek() == Some('+') {
            self.advance();
        }

        while let Some(ch) = self.peek() {
            if ch.is_ascii_digit() {
                self.advance();
            } else if ch == '.' {
                is_float = true;
                self.advance();
            } else if ch == 'e' || ch == 'E' {
                is_float = true;
                self.advance();
                if self.peek() == Some('+') || self.peek() == Some('-') {
                    self.advance();
                }
            } else {
                break;
            }
        }

        let num_str = &self.input[start..self.pos];
        self.skip_type_annotation();

        if is_float {
            let f: f64 = num_str
                .parse()
                .map_err(|_| self.error(format!("invalid float: {num_str}")))?;
            Ok(Value::Float(f))
        } else if is_negative {
            let i: i64 = num_str
                .parse()
                .map_err(|_| self.error(format!("invalid integer: {num_str}")))?;
            Ok(Value::Int(i))
        } else {
            let u: u64 = num_str
                .parse()
                .map_err(|_| self.error(format!("invalid integer: {num_str}")))?;
            Ok(Value::UInt(u))
        }
    }

    fn parse_string_value(&mut self) -> Result<Value> {
        let s = self.parse_quoted_string()?;
        self.skip_type_annotation();
        Ok(Value::Str(s))
    }

    fn parse_quoted_string(&mut self) -> Result<String> {
        self.expect_char('"')?;
        let mut s = String::new();
        loop {
            match self.advance() {
                Some('"') => break,
                Some('\\') => match self.advance() {
                    Some('n') => s.push('\n'),
                    Some('t') => s.push('\t'),
                    Some('r') => s.push('\r'),
                    Some('\\') => s.push('\\'),
                    Some('"') => s.push('"'),
                    Some(ch) => {
                        s.push('\\');
                        s.push(ch);
                    }
                    None => return Err(self.error("unterminated string escape")),
                },
                Some(ch) => s.push(ch),
                None => return Err(self.error("unterminated string")),
            }
        }
        Ok(s)
    }

    fn parse_bytes(&mut self) -> Result<Value> {
        // Consume "b64#"
        for _ in 0..4 {
            self.advance();
        }
        let start = self.pos;
        // Read until ';' but do NOT consume the ';' — it's the statement terminator
        // and will be consumed by the object/array parser.
        while let Some(ch) = self.peek() {
            if ch == ';' {
                break;
            }
            self.advance();
        }
        let b64_str = &self.input[start..self.pos];
        // Do NOT advance past ';' — let the caller handle it.

        let bytes = base64::engine::general_purpose::STANDARD
            .decode(b64_str.trim())
            .map_err(|e| self.error(format!("invalid base64: {e}")))?;
        Ok(Value::Bytes(bytes))
    }

    fn parse_array(&mut self) -> Result<Value> {
        self.expect_char('[')?;
        let mut items = Vec::new();

        loop {
            self.skip_whitespace_and_comments();
            if self.peek() == Some(']') {
                self.advance();
                break;
            }
            items.push(self.parse_value()?);
            self.skip_whitespace_and_comments();
            if self.peek() == Some(',') {
                self.advance();
            }
        }

        Ok(Value::Array(items))
    }

    fn parse_object(&mut self) -> Result<Value> {
        self.expect_char('{')?;
        let mut entries = Vec::new();

        loop {
            self.skip_whitespace_and_comments();
            if self.peek() == Some('}') {
                self.advance();
                break;
            }

            // Parse key: either an identifier or a quoted string.
            let key = self.parse_key()?;
            self.expect_char(':')?;
            let value = self.parse_value()?;
            self.expect_char(';')?;

            entries.push((key, value));
        }

        Ok(Value::Object(entries))
    }

    fn parse_key(&mut self) -> Result<String> {
        self.skip_whitespace_and_comments();
        if self.peek() == Some('"') {
            self.parse_quoted_string()
        } else {
            self.parse_identifier()
        }
    }

    fn parse_identifier(&mut self) -> Result<String> {
        let start = self.pos;
        match self.peek() {
            Some(ch) if ch.is_alphabetic() || ch == '_' => {
                self.advance();
            }
            _ => return Err(self.error("expected identifier")),
        }
        while let Some(ch) = self.peek() {
            if ch.is_alphanumeric() || ch == '_' {
                self.advance();
            } else {
                break;
            }
        }
        Ok(self.input[start..self.pos].to_string())
    }

    /// Skip optional type annotations like `::u32`, `::str`.
    fn skip_type_annotation(&mut self) {
        if self.remaining().starts_with("::") {
            self.advance(); // ':'
            self.advance(); // ':'
            while let Some(ch) = self.peek() {
                if ch.is_alphanumeric() || ch == '_' {
                    self.advance();
                } else {
                    break;
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Pretty-printer
// ---------------------------------------------------------------------------

/// Pretty-print a `Value` in canonical Crous text notation.
///
/// The output is deterministic: the same `Value` always produces the same text.
///
/// ```
/// use crous_core::text::pretty_print;
/// use crous_core::Value;
///
/// let v = Value::Object(vec![
///     ("name".into(), Value::Str("Alice".into())),
///     ("age".into(), Value::UInt(30)),
/// ]);
/// let text = pretty_print(&v, 0);
/// assert!(text.contains("name: \"Alice\";"));
/// assert!(text.contains("age: 30;"));
/// ```
pub fn pretty_print(value: &Value, indent: usize) -> String {
    let mut out = String::new();
    write_value(&mut out, value, indent, 0);
    out
}

fn write_value(out: &mut String, value: &Value, indent_size: usize, depth: usize) {
    let indent = " ".repeat(indent_size * depth);
    let inner_indent = " ".repeat(indent_size * (depth + 1));

    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::UInt(n) => out.push_str(&n.to_string()),
        Value::Int(n) => out.push_str(&n.to_string()),
        Value::Float(f) => {
            // Ensure float always has a decimal point for deterministic output.
            let s = format!("{f}");
            if s.contains('.') || s.contains('e') || s.contains('E') {
                out.push_str(&s);
            } else {
                out.push_str(&format!("{f}.0"));
            }
        }
        Value::Str(s) => {
            out.push('"');
            for ch in s.chars() {
                match ch {
                    '"' => out.push_str("\\\""),
                    '\\' => out.push_str("\\\\"),
                    '\n' => out.push_str("\\n"),
                    '\r' => out.push_str("\\r"),
                    '\t' => out.push_str("\\t"),
                    c => out.push(c),
                }
            }
            out.push('"');
        }
        Value::Bytes(b) => {
            out.push_str("b64#");
            out.push_str(&base64::engine::general_purpose::STANDARD.encode(b));
        }
        Value::Array(items) => {
            if items.is_empty() {
                out.push_str("[]");
            } else if is_simple_array(items) {
                // Inline for simple arrays.
                out.push('[');
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    write_value(out, item, indent_size, depth);
                }
                out.push(']');
            } else {
                out.push_str("[\n");
                for (i, item) in items.iter().enumerate() {
                    out.push_str(&inner_indent);
                    write_value(out, item, indent_size, depth + 1);
                    if i < items.len() - 1 {
                        out.push(',');
                    }
                    out.push('\n');
                }
                out.push_str(&indent);
                out.push(']');
            }
        }
        Value::Object(entries) => {
            if entries.is_empty() {
                out.push_str("{}");
            } else {
                out.push_str("{\n");
                for (key, val) in entries {
                    out.push_str(&inner_indent);
                    if is_valid_identifier(key) {
                        out.push_str(key);
                    } else {
                        out.push('"');
                        out.push_str(key);
                        out.push('"');
                    }
                    out.push_str(": ");
                    write_value(out, val, indent_size, depth + 1);
                    out.push_str(";\n");
                }
                out.push_str(&indent);
                out.push('}');
            }
        }
    }
}

/// Check if an array contains only simple scalar values (no nesting).
fn is_simple_array(items: &[Value]) -> bool {
    items.len() <= 8
        && items.iter().all(|v| {
            matches!(
                v,
                Value::Null
                    | Value::Bool(_)
                    | Value::UInt(_)
                    | Value::Int(_)
                    | Value::Float(_)
                    | Value::Str(_)
            )
        })
}

/// Check if a string is a valid unquoted identifier.
fn is_valid_identifier(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_alphanumeric() || c == '_')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_null() {
        assert_eq!(parse("null").unwrap(), Value::Null);
    }

    #[test]
    fn parse_bool() {
        assert_eq!(parse("true").unwrap(), Value::Bool(true));
        assert_eq!(parse("false").unwrap(), Value::Bool(false));
    }

    #[test]
    fn parse_uint() {
        assert_eq!(parse("42").unwrap(), Value::UInt(42));
        assert_eq!(parse("0").unwrap(), Value::UInt(0));
    }

    #[test]
    fn parse_int() {
        assert_eq!(parse("-1").unwrap(), Value::Int(-1));
        assert_eq!(parse("-42").unwrap(), Value::Int(-42));
    }

    #[test]
    fn parse_float() {
        assert_eq!(parse("3.125").unwrap(), Value::Float(3.125));
        assert_eq!(parse("-2.5").unwrap(), Value::Float(-2.5));
    }

    #[test]
    fn parse_string() {
        assert_eq!(parse(r#""hello""#).unwrap(), Value::Str("hello".into()));
        assert_eq!(
            parse(r#""with \"quotes\"""#).unwrap(),
            Value::Str("with \"quotes\"".into())
        );
    }

    #[test]
    fn parse_bytes() {
        let v = parse("b64#AQID;").unwrap();
        assert_eq!(v, Value::Bytes(vec![1, 2, 3]));
    }

    #[test]
    fn parse_array() {
        let v = parse("[1, 2, 3]").unwrap();
        assert_eq!(
            v,
            Value::Array(vec![Value::UInt(1), Value::UInt(2), Value::UInt(3)])
        );
    }

    #[test]
    fn parse_object() {
        let v = parse(r#"{ name: "Alice"; age: 30; }"#).unwrap();
        assert_eq!(
            v,
            Value::Object(vec![
                ("name".into(), Value::Str("Alice".into())),
                ("age".into(), Value::UInt(30)),
            ])
        );
    }

    #[test]
    fn parse_nested() {
        let input = r#"{
            users: [
                { name: "Bob"; scores: [100, 95, 87]; }
            ];
            count: 1;
        }"#;
        let v = parse(input).unwrap();
        let expected = Value::Object(vec![
            (
                "users".into(),
                Value::Array(vec![Value::Object(vec![
                    ("name".into(), Value::Str("Bob".into())),
                    (
                        "scores".into(),
                        Value::Array(vec![Value::UInt(100), Value::UInt(95), Value::UInt(87)]),
                    ),
                ])]),
            ),
            ("count".into(), Value::UInt(1)),
        ]);
        assert_eq!(v, expected);
    }

    #[test]
    fn parse_comments() {
        let input = r#"{
            // This is a comment
            name: "Alice"; /* inline comment */
            age: 30;
        }"#;
        let v = parse(input).unwrap();
        assert_eq!(
            v,
            Value::Object(vec![
                ("name".into(), Value::Str("Alice".into())),
                ("age".into(), Value::UInt(30)),
            ])
        );
    }

    #[test]
    fn parse_type_annotation() {
        let v = parse("42::u32").unwrap();
        assert_eq!(v, Value::UInt(42));
    }

    #[test]
    fn pretty_print_roundtrip() {
        let original = Value::Object(vec![
            ("name".into(), Value::Str("Alice".into())),
            ("age".into(), Value::UInt(30)),
            (
                "tags".into(),
                Value::Array(vec![Value::Str("admin".into()), Value::Str("user".into())]),
            ),
        ]);
        let text = pretty_print(&original, 4);
        let parsed = parse(&text).unwrap();
        assert_eq!(parsed, original);
    }

    #[test]
    fn pretty_print_bytes() {
        let v = Value::Bytes(vec![0xDE, 0xAD, 0xBE, 0xEF]);
        let text = pretty_print(&v, 0);
        assert!(text.starts_with("b64#"));
        // b64# values no longer include trailing ';' — the terminator is handled
        // by the enclosing object/array syntax.
        let parsed = parse(&text).unwrap();
        assert_eq!(parsed, v);
    }

    #[test]
    fn text_binary_text_roundtrip() {
        // Parse text → encode binary → decode binary → pretty-print text → parse text
        let input = r#"{ name: "Alice"; age: 30; active: true; }"#;
        let val1 = parse(input).unwrap();

        let mut enc = crate::encoder::Encoder::new();
        enc.encode_value(&val1).unwrap();
        let binary = enc.finish().unwrap();

        let mut dec = crate::decoder::Decoder::new(&binary);
        let val2 = dec.decode_next().unwrap().to_owned_value();
        assert_eq!(val1, val2);

        let text2 = pretty_print(&val2, 4);
        let val3 = parse(&text2).unwrap();
        assert_eq!(val1, val3);
    }
}
