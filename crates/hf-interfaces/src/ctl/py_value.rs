//! The value a cursor's JSON reads as, with the comparisons Python makes of it.

#[derive(Clone, Debug, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    /// An integer of any size: its sign and decimal digits without leading zeros.
    Int {
        negative: bool,
        digits: String,
    },
    Float(f64),
    /// Code points, which may include lone surrogates.
    Text(Vec<u32>),
    Array(Vec<Json>),
    Object(Vec<(Vec<u32>, Json)>),
}

impl Json {
    pub fn get(&self, key: &str) -> Option<&Json> {
        let Json::Object(members) = self else {
            return None;
        };
        let wanted: Vec<u32> = key.chars().map(u32::from).collect();
        members
            .iter()
            .find(|(name, _)| *name == wanted)
            .map(|pair| &pair.1)
    }

    /// The member names of an object.
    pub fn keys(&self) -> Option<Vec<String>> {
        let Json::Object(members) = self else {
            return None;
        };
        let name = |codes: &[u32]| {
            codes
                .iter()
                .map(|c| char::from_u32(*c).unwrap_or('\u{fffd}'))
                .collect()
        };
        Some(members.iter().map(|pair| name(&pair.0)).collect())
    }

    /// Python's `value == 1`: `true`, `1` and `1.0`.
    pub fn equals_one(&self) -> bool {
        match self {
            Json::Bool(flag) => *flag,
            Json::Int { negative, digits } => !negative && digits == "1",
            Json::Float(number) => *number == 1.0,
            _ => false,
        }
    }

    /// Python's `value == number` for a whole number.
    pub fn equals_integer(&self, number: i64) -> bool {
        match self {
            Json::Int { negative, digits } => {
                let text = if *negative && digits != "0" {
                    format!("-{digits}")
                } else {
                    digits.clone()
                };
                text == number.to_string()
            }
            Json::Float(value) => value.fract() == 0.0 && *value as i128 == i128::from(number),
            Json::Bool(flag) => i64::from(*flag) == number,
            _ => false,
        }
    }

    pub fn equals_text(&self, text: &str) -> bool {
        match self {
            Json::Text(codes) => codes.iter().copied().eq(text.chars().map(u32::from)),
            _ => false,
        }
    }

    /// The characters of text, with a lone surrogate read as U+FFFD.
    pub fn text(&self) -> Option<String> {
        match self {
            Json::Text(codes) => Some(
                codes
                    .iter()
                    .map(|c| char::from_u32(*c).unwrap_or('\u{fffd}'))
                    .collect(),
            ),
            _ => None,
        }
    }

    pub fn code_points(&self) -> Option<usize> {
        match self {
            Json::Text(codes) => Some(codes.len()),
            _ => None,
        }
    }

    /// A non-negative integer that is not a boolean, saturated at `u64::MAX`.
    pub fn count(&self) -> Option<u64> {
        let Json::Int { negative, digits } = self else {
            return None;
        };
        if *negative && digits != "0" {
            return None;
        }
        Some(digits.parse().unwrap_or(u64::MAX))
    }
}
