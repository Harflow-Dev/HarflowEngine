//! A bounded reader for the JSON of a cursor, with the reach of Python's `json.loads` on
//! bytes: the encodings `json.detect_encoding` finds (UTF-8 with or without a byte order
//! mark, UTF-16 and UTF-32), integers of any size, `NaN` and `Infinity`, lone surrogates in
//! strings and the last of duplicate keys. The result serves only the cursor checks.

use super::{py_codec::decode, py_value::Json};

const MAX_DEPTH: usize = 4096;

/// Decodes `bytes` as `json.loads` does; `None` where it would raise `ValueError`.
pub fn parse(bytes: &[u8]) -> Option<Json> {
    let text = decode(bytes)?;
    if text.first() == Some(&0xFEFF) {
        return None;
    }
    let mut reader = Reader { text: &text, at: 0 };
    reader.space();
    let value = reader.value(0)?;
    reader.space();
    (reader.at == text.len()).then_some(value)
}

struct Reader<'a> {
    text: &'a [u32],
    at: usize,
}

impl Reader<'_> {
    fn peek(&self) -> Option<char> {
        self.text.get(self.at).and_then(|c| char::from_u32(*c))
    }

    fn space(&mut self) {
        while matches!(self.peek(), Some(' ' | '\t' | '\n' | '\r')) {
            self.at += 1;
        }
    }

    fn take(&mut self, word: &str) -> bool {
        let fits = word
            .chars()
            .enumerate()
            .all(|(index, ch)| self.text.get(self.at + index) == Some(&u32::from(ch)));
        if fits {
            self.at += word.chars().count();
        }
        fits
    }

    fn value(&mut self, depth: usize) -> Option<Json> {
        if depth > MAX_DEPTH {
            return None;
        }
        match self.peek()? {
            '"' => self.string().map(Json::Text),
            '{' => self.object(depth),
            '[' => self.array(depth),
            'n' => self.take("null").then_some(Json::Null),
            't' => self.take("true").then_some(Json::Bool(true)),
            'f' => self.take("false").then_some(Json::Bool(false)),
            'N' => self.take("NaN").then_some(Json::Float(f64::NAN)),
            'I' => self.take("Infinity").then_some(Json::Float(f64::INFINITY)),
            '-' if self.text.get(self.at + 1) == Some(&u32::from('I')) => self
                .take("-Infinity")
                .then_some(Json::Float(f64::NEG_INFINITY)),
            _ => self.number(),
        }
    }

    fn digits(&mut self) -> String {
        let mut out = String::new();
        while let Some(ch) = self.peek().filter(char::is_ascii_digit) {
            out.push(ch);
            self.at += 1;
        }
        out
    }

    fn number(&mut self) -> Option<Json> {
        let negative = self.peek() == Some('-');
        if negative {
            self.at += 1;
        }
        let whole = match self.peek()? {
            '0' => {
                self.at += 1;
                "0".to_string()
            }
            '1'..='9' => self.digits(),
            _ => return None,
        };
        let mut float = String::from(if negative { "-" } else { "" });
        float.push_str(&whole);
        let mut is_float = false;
        if self.peek() == Some('.')
            && self
                .text
                .get(self.at + 1)
                .is_some_and(|c| (0x30..=0x39).contains(c))
        {
            self.at += 1;
            float.push('.');
            float.push_str(&self.digits());
            is_float = true;
        }
        if matches!(self.peek(), Some('e' | 'E')) {
            let mark = self.at;
            self.at += 1;
            let mut exponent = String::from("e");
            if let Some(sign @ ('+' | '-')) = self.peek() {
                exponent.push(sign);
                self.at += 1;
            }
            let digits = self.digits();
            if digits.is_empty() {
                self.at = mark;
            } else {
                float.push_str(&exponent);
                float.push_str(&digits);
                is_float = true;
            }
        }
        if is_float {
            return float.parse().ok().map(Json::Float);
        }
        Some(Json::Int {
            negative,
            digits: whole,
        })
    }

    fn hex4(&mut self) -> Option<u32> {
        let mut value = 0;
        for _ in 0..4 {
            let digit = self.peek()?.to_digit(16)?;
            value = value * 16 + digit;
            self.at += 1;
        }
        Some(value)
    }

    fn string(&mut self) -> Option<Vec<u32>> {
        self.at += 1;
        let mut out = Vec::new();
        loop {
            let unit = *self.text.get(self.at)?;
            self.at += 1;
            match unit {
                0x22 => return Some(out),
                0..=0x1F => return None,
                0x5C => {
                    let escape = self.peek()?;
                    self.at += 1;
                    let plain = match escape {
                        '"' => 0x22,
                        '\\' => 0x5C,
                        '/' => 0x2F,
                        'b' => 8,
                        'f' => 12,
                        'n' => 10,
                        'r' => 13,
                        't' => 9,
                        'u' => {
                            let first = self.hex4()?;
                            let paired = self.text.get(self.at) == Some(&0x5C)
                                && self.text.get(self.at + 1) == Some(&u32::from('u'));
                            if (0xD800..0xDC00).contains(&first) && paired {
                                let mark = self.at;
                                self.at += 2;
                                match self.hex4() {
                                    Some(low) if (0xDC00..0xE000).contains(&low) => {
                                        out.push(
                                            0x10000 + ((first - 0xD800) << 10) + (low - 0xDC00),
                                        );
                                        continue;
                                    }
                                    Some(_) => self.at = mark,
                                    None => return None,
                                }
                            }
                            first
                        }
                        _ => return None,
                    };
                    out.push(plain);
                }
                other => out.push(other),
            }
        }
    }

    fn array(&mut self, depth: usize) -> Option<Json> {
        self.at += 1;
        let mut items = Vec::new();
        self.space();
        if self.peek() == Some(']') {
            self.at += 1;
            return Some(Json::Array(items));
        }
        loop {
            self.space();
            items.push(self.value(depth + 1)?);
            self.space();
            match self.peek()? {
                ',' => self.at += 1,
                ']' => {
                    self.at += 1;
                    return Some(Json::Array(items));
                }
                _ => return None,
            }
        }
    }

    fn object(&mut self, depth: usize) -> Option<Json> {
        self.at += 1;
        let mut members: Vec<(Vec<u32>, Json)> = Vec::new();
        self.space();
        if self.peek() == Some('}') {
            self.at += 1;
            return Some(Json::Object(members));
        }
        loop {
            self.space();
            if self.peek()? != '"' {
                return None;
            }
            let key = self.string()?;
            self.space();
            if self.peek()? != ':' {
                return None;
            }
            self.at += 1;
            self.space();
            let value = self.value(depth + 1)?;
            match members.iter_mut().find(|pair| pair.0 == key) {
                Some(pair) => pair.1 = value,
                None => members.push((key, value)),
            }
            self.space();
            match self.peek()? {
                ',' => self.at += 1,
                '}' => {
                    self.at += 1;
                    return Some(Json::Object(members));
                }
                _ => return None,
            }
        }
    }
}
