//! Parser for the Lua-like text stored in the `header` chunk.
//!
//! Real payload (abridged):
//!
//! ```text
//! pyramid = {
//!     colorProfile = "sRGB",
//!     croppedHeight = 1077,
//!     croppedWidth = 1616,
//!     digest = "bd012a8e...",
//!     formatVersion = 3,
//!     levels = { { height = 68, width = 101, }, ... },
//!     quality = "embedded",
//!     uuid = "1418F22B-...",
//! }
//! ```
//!
//! This is deliberately a hand-written, allocation-light parser rather than an
//! embedded Lua runtime: the input is untrusted disk data and must never be
//! executed.

use crate::error::{Error, Result};

/// The parsed contents of a `header` chunk.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PyramidHeader {
    pub color_profile: String,
    pub cropped_width: u32,
    pub cropped_height: u32,
    pub digest: String,
    pub quality: String,
    pub uuid: String,
    /// Declared `(width, height)` per level, ascending.
    pub levels: Vec<(u32, u32)>,
}

impl PyramidHeader {
    /// Parse the header text. Unknown keys are ignored; a missing key is not
    /// an error (older Lightroom versions omit different fields).
    pub fn parse(text: &str) -> Result<Self> {
        let tokens = tokenize(text)?;
        if tokens.is_empty() {
            return Err(err("header payload is empty"));
        }
        let root = Parser::new(tokens).parse_document()?;

        // Real payloads wrap everything in `pyramid = { ... }`; fall back to
        // the top level so a bare table is still usable.
        let table = match root.get("pyramid") {
            Some(Value::Table(t)) => t,
            Some(_) => return Err(err("`pyramid` is not a table")),
            None => &root,
        };

        Ok(Self {
            color_profile: table.string("colorProfile").unwrap_or_default(),
            cropped_width: table.u32("croppedWidth").unwrap_or_default(),
            cropped_height: table.u32("croppedHeight").unwrap_or_default(),
            digest: table.string("digest").unwrap_or_default(),
            quality: table.string("quality").unwrap_or_default(),
            uuid: table.string("uuid").unwrap_or_default(),
            levels: parse_levels(table),
        })
    }
}

/// `levels` is positional: `{ { height = .., width = .. }, ... }`, one table
/// per level, in ascending order. Entries missing either dimension are skipped.
fn parse_levels(table: &Table) -> Vec<(u32, u32)> {
    let Some(Value::Table(levels)) = table.get("levels") else {
        return Vec::new();
    };
    levels
        .items
        .iter()
        .filter_map(|item| match item {
            Value::Table(level) => Some((level.u32("width")?, level.u32("height")?)),
            _ => None,
        })
        .collect()
}

fn err(message: impl Into<String>) -> Error {
    Error::PyramidHeader(message.into())
}

/// A parsed value. Numbers stay as text so unknown keys (which may be floats
/// or huge timestamps) never need to be interpreted.
#[derive(Debug, Clone, PartialEq)]
enum Value {
    Str(String),
    Num(String),
    Bool(bool),
    Table(Table),
}

/// One Lua-ish table: named entries plus positional (array) items.
#[derive(Debug, Clone, Default, PartialEq)]
struct Table {
    entries: Vec<(String, Value)>,
    items: Vec<Value>,
}

impl Table {
    /// Last write wins, matching Lua.
    fn get(&self, key: &str) -> Option<&Value> {
        self.entries
            .iter()
            .rev()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v)
    }

    fn string(&self, key: &str) -> Option<String> {
        match self.get(key) {
            Some(Value::Str(s)) => Some(s.clone()),
            _ => None,
        }
    }

    /// Integers only: floats, negatives and out-of-range values are ignored.
    fn u32(&self, key: &str) -> Option<u32> {
        match self.get(key) {
            Some(Value::Num(n)) => u32::try_from(n.parse::<u64>().ok()?).ok(),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Tok<'a> {
    Ident(&'a str),
    Str(String),
    Num(&'a str),
    LBrace,
    RBrace,
    Eq,
    Comma,
    Semi,
}

/// Split the header into tokens. Deliberately tiny: no Lua runtime, no
/// evaluation of any kind.
fn tokenize(src: &str) -> Result<Vec<Tok<'_>>> {
    let bytes = src.as_bytes();
    let mut tokens = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b if b.is_ascii_whitespace() => i += 1,
            // `--` line comment (the payload never uses long comments).
            b'-' if bytes.get(i + 1) == Some(&b'-') => {
                i += 2;
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            b'{' => {
                tokens.push(Tok::LBrace);
                i += 1;
            }
            b'}' => {
                tokens.push(Tok::RBrace);
                i += 1;
            }
            b'=' => {
                tokens.push(Tok::Eq);
                i += 1;
            }
            b',' => {
                tokens.push(Tok::Comma);
                i += 1;
            }
            b';' => {
                tokens.push(Tok::Semi);
                i += 1;
            }
            quote @ (b'"' | b'\'') => {
                let (value, next) = scan_string(src, i + 1, quote)?;
                tokens.push(Tok::Str(value));
                i = next;
            }
            b'0'..=b'9' | b'-' | b'+' | b'.' => {
                let start = i;
                i += 1;
                while i < bytes.len()
                    && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'.' || bytes[i] == b'_')
                {
                    i += 1;
                }
                tokens.push(Tok::Num(&src[start..i]));
            }
            _ if bytes[i].is_ascii_alphabetic() || bytes[i] == b'_' => {
                let start = i;
                while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                    i += 1;
                }
                tokens.push(Tok::Ident(&src[start..i]));
            }
            other => {
                return Err(err(format!(
                    "unexpected character {:?} at byte {i}",
                    other as char
                )));
            }
        }
    }
    Ok(tokens)
}

/// Scan a quoted string starting just after the opening quote. Returns the
/// decoded value and the index just past the closing quote.
fn scan_string(src: &str, start: usize, quote: u8) -> Result<(String, usize)> {
    let bytes = src.as_bytes();
    let mut out = String::new();
    let mut i = start;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => {
                let Some(&escaped) = bytes.get(i + 1) else {
                    break;
                };
                if escaped < 0x80 {
                    match escaped {
                        b'n' => out.push('\n'),
                        b't' => out.push('\t'),
                        b'r' => out.push('\r'),
                        b'0' => out.push('\0'),
                        b'"' => out.push('"'),
                        b'\'' => out.push('\''),
                        b'\\' => out.push('\\'),
                        other => out.push(other as char),
                    }
                    i += 2;
                } else {
                    // Unknown escape of a non-ASCII character: keep the
                    // character literally. Decode whole, never split UTF-8.
                    let ch = src[i + 1..].chars().next().ok_or_else(|| err("bad text"))?;
                    out.push(ch);
                    i += 1 + ch.len_utf8();
                }
            }
            b if b == quote => return Ok((out, i + 1)),
            b if b < 0x80 => {
                out.push(b as char);
                i += 1;
            }
            _ => {
                // Copy one full UTF-8 character (the input is a &str, so `i`
                // is a character boundary here).
                let ch = src[i..].chars().next().ok_or_else(|| err("bad text"))?;
                out.push(ch);
                i += ch.len_utf8();
            }
        }
    }
    Err(err("unterminated string"))
}

const MAX_DEPTH: usize = 32;

struct Parser<'a> {
    tokens: Vec<Tok<'a>>,
    pos: usize,
}

impl<'a> Parser<'a> {
    fn new(tokens: Vec<Tok<'a>>) -> Self {
        Self { tokens, pos: 0 }
    }

    fn peek(&self) -> Option<&Tok<'a>> {
        self.tokens.get(self.pos)
    }

    fn peek_next(&self) -> Option<&Tok<'a>> {
        self.tokens.get(self.pos + 1)
    }

    fn skip_separators(&mut self) {
        while matches!(self.peek(), Some(Tok::Comma) | Some(Tok::Semi)) {
            self.pos += 1;
        }
    }

    fn take_ident(&mut self) -> Option<String> {
        match self.peek() {
            Some(Tok::Ident(name)) => {
                let name = (*name).to_string();
                self.pos += 1;
                Some(name)
            }
            _ => None,
        }
    }

    /// `key = value` entries until EOF.
    fn parse_document(mut self) -> Result<Table> {
        let mut table = Table::default();
        loop {
            self.skip_separators();
            match self.peek() {
                None => return Ok(table),
                Some(Tok::Ident(_)) => {
                    if !matches!(self.peek_next(), Some(Tok::Eq)) {
                        return Err(err(format!("expected `=` after key at token {}", self.pos)));
                    }
                    let key = self.take_ident().expect("peeked an identifier");
                    self.pos += 1; // '='
                    let value = self.parse_value(0)?;
                    table.entries.push((key, value));
                }
                Some(_) => {
                    return Err(err(format!("expected `key = value` at token {}", self.pos)));
                }
            }
        }
    }

    fn parse_value(&mut self, depth: usize) -> Result<Value> {
        if depth > MAX_DEPTH {
            return Err(err("tables are nested too deeply"));
        }
        match self.peek() {
            Some(Tok::Str(s)) => {
                let s = s.clone();
                self.pos += 1;
                Ok(Value::Str(s))
            }
            Some(Tok::Num(n)) => {
                let n = (*n).to_string();
                self.pos += 1;
                Ok(Value::Num(n))
            }
            Some(Tok::LBrace) => {
                self.pos += 1;
                Ok(Value::Table(self.parse_table(depth)?))
            }
            Some(Tok::Ident(name)) => {
                let name = *name;
                self.pos += 1;
                match name {
                    "true" => Ok(Value::Bool(true)),
                    "false" => Ok(Value::Bool(false)),
                    other => Err(err(format!("unsupported bare value `{other}`"))),
                }
            }
            Some(_) => Err(err(format!("expected a value at token {}", self.pos))),
            None => Err(err("unexpected end of header, expected a value")),
        }
    }

    /// Parse a table body; the opening brace has already been consumed.
    fn parse_table(&mut self, depth: usize) -> Result<Table> {
        let mut table = Table::default();
        loop {
            self.skip_separators();
            match self.peek() {
                None => return Err(err("unbalanced `{`: table is not closed")),
                Some(Tok::RBrace) => {
                    self.pos += 1;
                    return Ok(table);
                }
                Some(Tok::Ident(_)) if matches!(self.peek_next(), Some(Tok::Eq)) => {
                    let key = self.take_ident().expect("peeked an identifier");
                    self.pos += 1; // '='
                    let value = self.parse_value(depth + 1)?;
                    table.entries.push((key, value));
                }
                Some(_) => {
                    let value = self.parse_value(depth + 1)?;
                    table.items.push(value);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Error;

    const REAL: &str = "pyramid = {\n\
\tcolorProfile = \"sRGB\",\n\
\tcroppedHeight = 1077,\n\
\tcroppedWidth = 1616,\n\
\tdigest = \"bd012a8e0e0b5b73ddaf34aeceb8bf48\",\n\
\tformatVersion = 3,\n\
\tfromProxy = false,\n\
\tfileTimeStamp = 18446744073709551615,\n\
\tlevels = {\n\
\t\t{ height = 68, width = 101, },\n\
\t\t{ height = 1077, width = 1616, },\n\
\t},\n\
\tquality = \"embedded\",\n\
\tuuid = \"1418F22B-41B0-4786-9DF0-D5081559CAD1\",\n\
}\n";

    #[test]
    fn parses_a_real_shaped_header_including_unknown_and_boolean_keys() {
        let header = PyramidHeader::parse(REAL).unwrap();
        assert_eq!(header.color_profile, "sRGB");
        assert_eq!(header.cropped_width, 1616);
        assert_eq!(header.cropped_height, 1077);
        assert_eq!(header.digest, "bd012a8e0e0b5b73ddaf34aeceb8bf48");
        assert_eq!(header.quality, "embedded");
        assert_eq!(header.uuid, "1418F22B-41B0-4786-9DF0-D5081559CAD1");
        // `(width, height)` per level, in declared (ascending) order.
        assert_eq!(header.levels, vec![(101, 68), (1616, 1077)]);
    }

    #[test]
    fn missing_keys_are_not_errors() {
        let header = PyramidHeader::parse("pyramid = { }").unwrap();
        assert_eq!(header, PyramidHeader::default());
        assert!(header.levels.is_empty());

        // A pyramid with one level and nothing else.
        let one =
            PyramidHeader::parse("pyramid = { levels = { { height = 68, width = 101, }, }, }")
                .unwrap();
        assert_eq!(one.levels, vec![(101, 68)]);
        assert_eq!(one.color_profile, "");
        assert_eq!(one.cropped_width, 0);
    }

    #[test]
    fn ignores_unknown_keys_and_nested_tables_everywhere() {
        let text = "pyramid = {\n\
            colorProfile = \"AdobeRGB\",\n\
            future = { alpha = 1, beta = \"two\", gamma = { }, delta = false, },\n\
            levels = {\n\
                { width = 101, height = 68, extra = 7, },\n\
                { height = 1077, width = 1616, },\n\
            },\n\
            anotherUnknown = -12.5,\n\
        }";
        let header = PyramidHeader::parse(text).unwrap();
        assert_eq!(header.color_profile, "AdobeRGB");
        // Key order inside a level table does not matter.
        assert_eq!(header.levels, vec![(101, 68), (1616, 1077)]);
    }

    #[test]
    fn decodes_escapes_and_skips_lua_comments() {
        let text = "pyramid = {\n\
            -- a comment with { unbalanced braces\n\
            uuid = \"a\\\"b\\\\c\",\n\
            quality = \"embedded\", -- trailing comment\n\
        }";
        let header = PyramidHeader::parse(text).unwrap();
        assert_eq!(header.uuid, "a\"b\\c");
        assert_eq!(header.quality, "embedded");
    }

    #[test]
    fn malformed_input_is_an_error_not_a_panic() {
        for bad in [
            "pyramid = {",
            "pyramid = { levels = { { height = 1, width = 2, },",
            "pyramid = { quality = \"embedded }",
            "pyramid = { levels = { { height = 1, width = } } }",
            "{ not an assignment }",
            "pyramid = { levels = { { height = 68, width = 101, }, }, ",
            "",
        ] {
            let result = PyramidHeader::parse(bad);
            assert!(
                matches!(result, Err(Error::PyramidHeader(_))),
                "expected PyramidHeader error for {bad:?}, got {result:?}"
            );
        }
    }

    #[test]
    fn non_ascii_text_after_a_backslash_is_not_a_panic() {
        // An unknown escape of a non-ASCII character is kept literally.
        let header =
            PyramidHeader::parse("pyramid = { uuid = \"x\\é\", quality = \"q\", }").unwrap();
        assert_eq!(header.uuid, "xé");
        assert_eq!(header.quality, "q");
        // Unterminated UTF-8 strings still error cleanly.
        assert!(PyramidHeader::parse("pyramid = { uuid = \"xé }").is_err());
    }

    #[test]
    fn every_prefix_of_a_valid_header_is_answered_without_panicking() {
        for end in 0..=REAL.len() {
            let _ = PyramidHeader::parse(&REAL[..end]);
        }
        // Arbitrary binary-ish garbage must not panic either.
        for junk in ["\0\0\0", "}}}}", "{{{{", "=", "\"", "pyramid"] {
            let _ = PyramidHeader::parse(junk);
        }
    }
}
