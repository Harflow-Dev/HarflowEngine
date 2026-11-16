//! A fixed-size set of node indices.

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BitSet {
    words: Vec<u64>,
    size: usize,
}

impl BitSet {
    pub fn empty(size: usize) -> Self {
        Self {
            words: vec_of(size.div_ceil(64), 0),
            size,
        }
    }

    pub fn full(size: usize) -> Self {
        let mut set = Self {
            words: vec_of(size.div_ceil(64), u64::MAX),
            size,
        };
        let tail = size % 64;
        if tail != 0
            && let Some(last) = set.words.last_mut()
        {
            *last = (1u64 << tail) - 1;
        }
        set
    }

    pub fn insert(&mut self, index: usize) {
        if index < self.size {
            self.words[index / 64] |= 1 << (index % 64);
        }
    }

    pub fn contains(&self, index: usize) -> bool {
        index < self.size && self.words[index / 64] & (1 << (index % 64)) != 0
    }

    pub fn intersect_with(&mut self, other: &BitSet) {
        for (word, theirs) in self.words.iter_mut().zip(&other.words) {
            *word &= theirs;
        }
    }

    pub fn is_subset(&self, other: &BitSet) -> bool {
        self.words
            .iter()
            .zip(&other.words)
            .all(|(word, theirs)| word & !theirs == 0)
    }

    pub fn intersects(&self, other: &BitSet) -> bool {
        self.words
            .iter()
            .zip(&other.words)
            .any(|(word, theirs)| word & theirs != 0)
    }

    pub fn is_empty(&self) -> bool {
        self.words.iter().all(|word| *word == 0)
    }

    /// The members in ascending order, without allocating.
    pub fn iter(&self) -> impl Iterator<Item = usize> + '_ {
        self.words.iter().enumerate().flat_map(|(at, word)| {
            let mut rest = *word;
            std::iter::from_fn(move || {
                if rest == 0 {
                    return None;
                }
                let bit = rest.trailing_zeros() as usize;
                rest &= rest - 1;
                Some(at * 64 + bit)
            })
        })
    }

    /// The members in ascending order.
    pub fn members(&self) -> Vec<usize> {
        self.iter().collect()
    }
}

fn vec_of(count: usize, value: u64) -> Vec<u64> {
    let mut out = Vec::with_capacity(count);
    out.resize(count, value);
    out
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;

    #[test]
    fn set_algebra() {
        let mut a = BitSet::empty(70);
        a.insert(1);
        a.insert(69);
        let mut b = BitSet::full(70);
        assert!(a.is_subset(&b));
        b.intersect_with(&a);
        assert_eq!(b.members(), [1, 69]);
        assert!(a.intersects(&b));
        assert!(!BitSet::empty(70).intersects(&a));
        assert!(!a.contains(70));
        assert_eq!(BitSet::full(70).members().len(), 70);
        assert_eq!(BitSet::full(64).members().len(), 64);
        assert!(BitSet::full(0).is_empty());
    }
}
