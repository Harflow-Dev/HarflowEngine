//! Flat JSON arena: both parsing and error cleanup are independent of input depth.
use hf_application::ctl::state_recover::journal::JournalError;
use std::collections::BTreeMap;

pub enum Node {
    Object(BTreeMap<String, usize>),
    Array(Vec<usize>),
    Scalar(serde_json::Value),
}
pub struct Tree(pub Vec<Node>);
struct Frame {
    id: usize,
    state: u8,
    key: Option<String>,
}
impl Tree {
    pub fn parse(bytes: &[u8]) -> Result<Self, JournalError> {
        std::str::from_utf8(bytes).map_err(|_| JournalError::Invalid)?;
        let mut tree = Self(Vec::new());
        let mut stack: Vec<Frame> = Vec::new();
        let mut pos = 0;
        loop {
            skip(bytes, &mut pos);
            if tree.0.is_empty() || stack.last().is_some_and(|f| f.state == 2) {
                let (id, container) = tree.value(bytes, &mut pos)?;
                if let Some(frame) = stack.last_mut() {
                    match &mut tree.0[frame.id] {
                        Node::Object(map) => {
                            let key = frame.key.take().ok_or(JournalError::Invalid)?;
                            if map.insert(key, id).is_some() {
                                return Err(JournalError::Invalid);
                            }
                        }
                        Node::Array(items) => items.push(id),
                        Node::Scalar(_) => return Err(JournalError::Invalid),
                    }
                    frame.state = 3;
                }
                if container {
                    if stack.len() >= 512 {
                        return Err(JournalError::TooLarge);
                    }
                    stack.push(Frame {
                        id,
                        state: 0,
                        key: None,
                    });
                }
            } else if let Some(frame) = stack.last_mut() {
                let byte = *bytes.get(pos).ok_or(JournalError::Invalid)?;
                let object = matches!(tree.0[frame.id], Node::Object(_));
                let close = if object { b'}' } else { b']' };
                match (frame.state, byte) {
                    (0 | 3, b) if b == close => {
                        stack.pop();
                        pos += 1;
                    }
                    (3, b',') => {
                        frame.state = 1;
                        pos += 1;
                    }
                    (0 | 1, b'"') if object => {
                        frame.key = Some(string(bytes, &mut pos)?);
                        skip(bytes, &mut pos);
                        if bytes.get(pos) != Some(&b':') {
                            return Err(JournalError::Invalid);
                        }
                        pos += 1;
                        frame.state = 2;
                    }
                    (0 | 1, _) if !object => frame.state = 2,
                    _ => return Err(JournalError::Invalid),
                }
            }
            if !tree.0.is_empty() && stack.is_empty() {
                skip(bytes, &mut pos);
                return if pos == bytes.len() {
                    Ok(tree)
                } else {
                    Err(JournalError::Invalid)
                };
            }
        }
    }
    fn value(&mut self, bytes: &[u8], pos: &mut usize) -> Result<(usize, bool), JournalError> {
        let byte = *bytes.get(*pos).ok_or(JournalError::Invalid)?;
        let container = matches!(byte, b'{' | b'[');
        let node = match byte {
            b'{' => {
                *pos += 1;
                Node::Object(BTreeMap::new())
            }
            b'[' => {
                *pos += 1;
                Node::Array(Vec::new())
            }
            b'"' => Node::Scalar(serde_json::Value::String(string(bytes, pos)?)),
            _ => {
                let start = *pos;
                while bytes.get(*pos).is_some_and(|b| !b" \t\r\n,]}".contains(b)) {
                    *pos += 1;
                }
                let value = serde_json::from_slice(&bytes[start..*pos])
                    .map_err(|_| JournalError::Invalid)?;
                if matches!(
                    value,
                    serde_json::Value::Array(_) | serde_json::Value::Object(_)
                ) {
                    return Err(JournalError::Invalid);
                }
                Node::Scalar(value)
            }
        };
        let id = self.0.len();
        self.0.push(node);
        Ok((id, container))
    }
    pub fn object(&self, id: usize) -> Result<&BTreeMap<String, usize>, JournalError> {
        match &self.0[id] {
            Node::Object(v) => Ok(v),
            _ => Err(JournalError::Invalid),
        }
    }
    pub fn array(&self, id: usize) -> Result<&[usize], JournalError> {
        match &self.0[id] {
            Node::Array(v) => Ok(v),
            _ => Err(JournalError::Invalid),
        }
    }
    pub fn scalar(&self, id: usize) -> Result<&serde_json::Value, JournalError> {
        match &self.0[id] {
            Node::Scalar(v) => Ok(v),
            _ => Err(JournalError::Invalid),
        }
    }
    pub fn text(&self, id: usize) -> Result<String, JournalError> {
        self.scalar(id)?
            .as_str()
            .map(str::to_owned)
            .ok_or(JournalError::Invalid)
    }
    pub fn unsigned(&self, id: usize) -> Result<u64, JournalError> {
        self.scalar(id)?.as_u64().ok_or(JournalError::Invalid)
    }
    pub fn signed(&self, id: usize) -> Result<i64, JournalError> {
        self.scalar(id)?.as_i64().ok_or(JournalError::Invalid)
    }
    pub fn null(&self, id: usize) -> bool {
        matches!(&self.0[id], Node::Scalar(v) if v.is_null())
    }
}
fn skip(bytes: &[u8], pos: &mut usize) {
    while bytes.get(*pos).is_some_and(|b| b" \t\r\n".contains(b)) {
        *pos += 1;
    }
}
fn string(bytes: &[u8], pos: &mut usize) -> Result<String, JournalError> {
    let start = *pos;
    *pos += 1;
    loop {
        match bytes.get(*pos) {
            Some(b'"') => {
                *pos += 1;
                break;
            }
            Some(b'\\') => *pos += 2,
            Some(_) => *pos += 1,
            None => return Err(JournalError::Invalid),
        }
    }
    serde_json::from_slice(&bytes[start..*pos]).map_err(|_| JournalError::Invalid)
}
pub fn field(map: &BTreeMap<String, usize>, name: &str) -> Result<usize, JournalError> {
    map.get(name).copied().ok_or(JournalError::Invalid)
}
pub fn fields(map: &BTreeMap<String, usize>, allowed: &[&str]) -> Result<(), JournalError> {
    if map.keys().any(|k| !allowed.contains(&k.as_str())) {
        return Err(JournalError::UnsupportedExtension);
    }
    if map.len() != allowed.len() {
        return Err(JournalError::Invalid);
    }
    Ok(())
}
