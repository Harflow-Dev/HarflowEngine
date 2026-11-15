//! A JSON reader that matches Python's `json.loads` on the inputs definitions are made of:
//! whitespace, `NaN` and `Infinity`, duplicate keys (the last value wins, at the first
//! key's position), integers of any size up to Python's digit limit, and floats read with
//! correct rounding. It differs deliberately in two ways: a lone surrogate escape is
//! refused, and input size and nesting are bounded.
use hf_domain::workflow::json::{Json, JsonInt, JsonObject};

/// The most bytes of text read.
pub const MAX_TEXT_BYTES: usize = 1 << 20;
/// The deepest nesting of arrays and objects.
pub const MAX_DEPTH: usize = 64;
/// The most digits of an integer, as in Python's `int` conversion limit.
pub const INT_DIGIT_LIMIT: usize = 4300;

/// Why text was not read.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ParseError {
    TooLarge,
    TooDeep,
    IntegerTooLong,
    /// A `\uD800` to `\uDFFF` escape without its partner.
    LoneSurrogate,
    /// Malformed JSON, at this byte offset.
    Syntax(usize),
}

/// Bounds on what is read.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Limits {
    pub max_bytes: usize,
    pub max_depth: usize,
}

impl Limits {
    /// The bounds a definition is read under.
    pub const DEFINITION: Limits = Limits {
        max_bytes: MAX_TEXT_BYTES,
        max_depth: MAX_DEPTH,
    };
}

struct Reader<'a> {
    text: &'a str,
    bytes: &'a [u8],
    at: usize,
    max_depth: usize,
}

type Parsed<T> = Result<T, ParseError>;

impl<'a> Reader<'a> {
    fn fail<T>(&self) -> Parsed<T> {
        Err(ParseError::Syntax(self.at))
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.at).copied()
    }

    fn skip_space(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.at += 1;
        }
    }

    fn eat(&mut self, byte: u8) -> bool {
        let found = self.peek() == Some(byte);
        if found {
            self.at += 1;
        }
        found
    }

    fn literal(&mut self, word: &str) -> bool {
        let found = self.bytes[self.at..].starts_with(word.as_bytes());
        if found {
            self.at += word.len();
        }
        found
    }

    fn value(&mut self, depth: usize) -> Parsed<Json> {
        match self.peek() {
            Some(b'{') => self.object(depth + 1),
            Some(b'[') => self.array(depth + 1),
            Some(b'"') => self.string().map(Json::Str),
            Some(b'n') if self.literal("null") => Ok(Json::Null),
            Some(b't') if self.literal("true") => Ok(Json::Bool(true)),
            Some(b'f') if self.literal("false") => Ok(Json::Bool(false)),
            Some(b'N') if self.literal("NaN") => Ok(Json::Float(f64::NAN)),
            Some(b'I') if self.literal("Infinity") => Ok(Json::Float(f64::INFINITY)),
            Some(b'-') if self.literal("-Infinity") => Ok(Json::Float(f64::NEG_INFINITY)),
            Some(b'-' | b'0'..=b'9') => self.number(),
            _ => self.fail(),
        }
    }

    fn digits(&mut self) -> usize {
        let start = self.at;
        while self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
            self.at += 1;
        }
        self.at - start
    }

    fn number(&mut self) -> Parsed<Json> {
        let start = self.at;
        let negative = self.eat(b'-');
        let integer_start = self.at;
        if self.eat(b'0') {
            // A leading zero ends the integer part.
        } else if self.digits() == 0 {
            return self.fail();
        }
        let integer_end = self.at;
        let mut float = false;
        if self.peek() == Some(b'.') {
            let mark = self.at;
            self.at += 1;
            if self.digits() == 0 {
                self.at = mark;
            } else {
                float = true;
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            let mark = self.at;
            self.at += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.at += 1;
            }
            if self.digits() == 0 {
                self.at = mark;
            } else {
                float = true;
            }
        }
        if float {
            return match self.text[start..self.at].parse::<f64>() {
                Ok(number) => Ok(Json::Float(number)),
                Err(_) => self.fail(),
            };
        }
        let digits = &self.text[integer_start..integer_end];
        if digits.len() > INT_DIGIT_LIMIT {
            return Err(ParseError::IntegerTooLong);
        }
        JsonInt::parse(negative, digits)
            .map(Json::Int)
            .map_or_else(|| self.fail(), Ok)
    }

    fn hex4(&mut self) -> Parsed<u32> {
        let digits = self.text.get(self.at..self.at + 4);
        let Some(code) = digits.filter(|part| part.bytes().all(|b| b.is_ascii_hexdigit())) else {
            return self.fail();
        };
        self.at += 4;
        u32::from_str_radix(code, 16).or_else(|_| self.fail())
    }

    fn unicode_escape(&mut self) -> Parsed<char> {
        let unit = self.hex4()?;
        let code = match unit {
            0xd800..=0xdbff => {
                if !self.literal("\\u") {
                    return Err(ParseError::LoneSurrogate);
                }
                let low = self.hex4()?;
                if !(0xdc00..=0xdfff).contains(&low) {
                    return Err(ParseError::LoneSurrogate);
                }
                0x10000 + ((unit - 0xd800) << 10) + (low - 0xdc00)
            }
            0xdc00..=0xdfff => return Err(ParseError::LoneSurrogate),
            other => other,
        };
        char::from_u32(code).map_or_else(|| self.fail(), Ok)
    }

    fn string(&mut self) -> Parsed<String> {
        self.at += 1;
        let mut out = String::new();
        let mut run = self.at;
        loop {
            let Some(byte) = self.peek() else {
                return self.fail();
            };
            match byte {
                b'"' => {
                    out.push_str(&self.text[run..self.at]);
                    self.at += 1;
                    return Ok(out);
                }
                b'\\' => {
                    out.push_str(&self.text[run..self.at]);
                    self.at += 1;
                    out.push(self.escape()?);
                    run = self.at;
                }
                0..=0x1f => return self.fail(),
                _ => self.at += 1,
            }
        }
    }

    fn escape(&mut self) -> Parsed<char> {
        let Some(byte) = self.peek() else {
            return self.fail();
        };
        self.at += 1;
        Ok(match byte {
            b'"' => '"',
            b'\\' => '\\',
            b'/' => '/',
            b'b' => '\u{8}',
            b'f' => '\u{c}',
            b'n' => '\n',
            b'r' => '\r',
            b't' => '\t',
            b'u' => return self.unicode_escape(),
            _ => {
                self.at -= 1;
                return self.fail();
            }
        })
    }

    fn array(&mut self, depth: usize) -> Parsed<Json> {
        if depth > self.max_depth {
            return Err(ParseError::TooDeep);
        }
        self.at += 1;
        let mut items = Vec::new();
        self.skip_space();
        if self.eat(b']') {
            return Ok(Json::Array(items));
        }
        loop {
            self.skip_space();
            items.push(self.value(depth)?);
            self.skip_space();
            if self.eat(b',') {
                continue;
            }
            if self.eat(b']') {
                return Ok(Json::Array(items));
            }
            return self.fail();
        }
    }

    fn object(&mut self, depth: usize) -> Parsed<Json> {
        if depth > self.max_depth {
            return Err(ParseError::TooDeep);
        }
        self.at += 1;
        let mut map = JsonObject::new();
        self.skip_space();
        if self.eat(b'}') {
            return Ok(Json::Object(map));
        }
        loop {
            self.skip_space();
            if self.peek() != Some(b'"') {
                return self.fail();
            }
            let key = self.string()?;
            self.skip_space();
            if !self.eat(b':') {
                return self.fail();
            }
            self.skip_space();
            let value = self.value(depth)?;
            map.insert(&key, value);
            self.skip_space();
            if self.eat(b',') {
                continue;
            }
            if self.eat(b'}') {
                return Ok(Json::Object(map));
            }
            return self.fail();
        }
    }
}

/// Reads one JSON value from `text`, as `json.loads` does, within the definition bounds.
pub fn parse(text: &str) -> Result<Json, ParseError> {
    parse_with(text, Limits::DEFINITION)
}

/// As `parse`, with caller-chosen bounds.
pub fn parse_with(text: &str, limits: Limits) -> Result<Json, ParseError> {
    if text.len() > limits.max_bytes {
        return Err(ParseError::TooLarge);
    }
    let mut reader = Reader {
        text,
        bytes: text.as_bytes(),
        at: 0,
        max_depth: limits.max_depth,
    };
    reader.skip_space();
    let value = reader.value(0)?;
    reader.skip_space();
    if reader.at != text.len() {
        return reader.fail();
    }
    Ok(value)
}
