//! Strict bounded JSON reader; duplicate keys are checked after escape decoding.
use super::Limits;
use hf_domain::backend::BackendError;
use serde_json::{Map, Value};
type Checked<T> = Result<T, BackendError>;
struct Reader<'a> {
    raw: &'a [u8],
    at: usize,
    limits: Limits,
}
pub(super) fn parse(raw: &[u8], limits: Limits) -> Checked<Value> {
    std::str::from_utf8(raw).map_err(|_| BackendError::Shape)?;
    let mut reader = Reader { raw, at: 0, limits };
    let value = reader.value(0)?;
    reader.space();
    if reader.at != raw.len() {
        return Err(BackendError::Shape);
    }
    Ok(value)
}
impl Reader<'_> {
    fn space(&mut self) {
        while self
            .raw
            .get(self.at)
            .is_some_and(|b| b" \t\r\n".contains(b))
        {
            self.at += 1;
        }
    }
    fn eat(&mut self, byte: u8) -> bool {
        self.space();
        if self.raw.get(self.at) == Some(&byte) {
            self.at += 1;
            true
        } else {
            false
        }
    }
    fn value(&mut self, depth: usize) -> Checked<Value> {
        self.space();
        match self.raw.get(self.at) {
            Some(b'{') => self.object(depth + 1),
            Some(b'[') => self.array(depth + 1),
            Some(b'"') => self.string().map(Value::String),
            Some(_) => self.scalar(),
            None => Err(BackendError::Shape),
        }
    }
    fn scalar(&mut self) -> Checked<Value> {
        let start = self.at;
        while self
            .raw
            .get(self.at)
            .is_some_and(|b| !b" \t\r\n,]}".contains(b))
        {
            self.at += 1;
        }
        serde_json::from_slice(&self.raw[start..self.at]).map_err(|_| BackendError::Shape)
    }
    fn string(&mut self) -> Checked<String> {
        let start = self.at;
        self.at += 1;
        while let Some(byte) = self.raw.get(self.at) {
            match byte {
                b'"' => {
                    self.at += 1;
                    return serde_json::from_slice(&self.raw[start..self.at])
                        .map_err(|_| BackendError::Shape);
                }
                b'\\' => self.at += 2,
                _ => self.at += 1,
            }
        }
        Err(BackendError::Shape)
    }
    fn depth(&self, depth: usize) -> Checked<()> {
        if depth > self.limits.max_depth {
            Err(BackendError::Budget)
        } else {
            Ok(())
        }
    }
    fn object(&mut self, depth: usize) -> Checked<Value> {
        self.depth(depth)?;
        self.at += 1;
        let mut fields = Map::new();
        if self.eat(b'}') {
            return Ok(Value::Object(fields));
        }
        loop {
            if fields.len() >= self.limits.max_collection_members {
                return Err(BackendError::Budget);
            }
            self.space();
            if self.raw.get(self.at) != Some(&b'"') {
                return Err(BackendError::Shape);
            }
            let key = self.string()?;
            if key.len() > self.limits.max_key_bytes {
                return Err(BackendError::Budget);
            }
            if fields.contains_key(&key) || !self.eat(b':') {
                return Err(BackendError::Shape);
            }
            fields.insert(key, self.value(depth)?);
            if self.eat(b'}') {
                return Ok(Value::Object(fields));
            }
            if !self.eat(b',') {
                return Err(BackendError::Shape);
            }
        }
    }
    fn array(&mut self, depth: usize) -> Checked<Value> {
        self.depth(depth)?;
        self.at += 1;
        let mut items = Vec::new();
        if self.eat(b']') {
            return Ok(Value::Array(items));
        }
        loop {
            if items.len() >= self.limits.max_collection_members {
                return Err(BackendError::Budget);
            }
            items.push(self.value(depth)?);
            if self.eat(b']') {
                return Ok(Value::Array(items));
            }
            if !self.eat(b',') {
                return Err(BackendError::Shape);
            }
        }
    }
}
