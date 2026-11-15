//! 随拓扑增长的点、边、面选区；快照共享未改动的位集，取消与后台预览使用同一份选择。

use std::sync::Arc;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct MeshSelection(Arc<Vec<u64>>);

impl MeshSelection {
    pub(super) fn single(index: usize) -> Self {
        let mut selection = Self::default();
        selection.insert(index);
        selection
    }

    pub(super) fn all(count: usize) -> Self {
        let mut words = vec![u64::MAX; count / 64];
        if count % 64 != 0 {
            words.push((1u64 << (count % 64)) - 1);
        }
        Self(Arc::new(words))
    }

    pub(super) fn contains(&self, index: usize) -> bool {
        self.0.get(index / 64).is_some_and(|word| word & (1u64 << (index % 64)) != 0)
    }

    pub(super) fn insert(&mut self, index: usize) {
        let words = Arc::make_mut(&mut self.0);
        let word = index / 64;
        if words.len() <= word {
            words.resize(word + 1, 0);
        }
        words[word] |= 1u64 << (index % 64);
    }

    pub(super) fn remove(&mut self, index: usize) {
        if self.contains(index) {
            let words = Arc::make_mut(&mut self.0);
            words[index / 64] &= !(1u64 << (index % 64));
            trim(words);
        }
    }

    pub(super) fn toggle(&mut self, index: usize) {
        if self.contains(index) {
            self.remove(index);
        } else {
            self.insert(index);
        }
    }

    pub(super) fn union_with(&mut self, other: &Self) {
        let words = Arc::make_mut(&mut self.0);
        if words.len() < other.0.len() {
            words.resize(other.0.len(), 0);
        }
        for (word, rhs) in words.iter_mut().zip(other.0.iter()) {
            *word |= rhs;
        }
    }

    pub(super) fn subtract(&mut self, other: &Self) {
        let words = Arc::make_mut(&mut self.0);
        for (word, rhs) in words.iter_mut().zip(other.0.iter()) {
            *word &= !rhs;
        }
        trim(words);
    }

    pub(super) fn len(&self) -> usize {
        self.0.iter().map(|word| word.count_ones() as usize).sum()
    }

    pub(super) fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub(super) fn same_storage(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }

    pub(super) fn iter(&self) -> impl Iterator<Item = usize> + '_ {
        self.0.iter().copied().enumerate().flat_map(|(index, mut word)| {
            std::iter::from_fn(move || {
                if word == 0 {
                    return None;
                }
                let next = word.trailing_zeros() as usize;
                word &= word - 1;
                Some(index * 64 + next)
            })
        })
    }
}

impl FromIterator<usize> for MeshSelection {
    fn from_iter<T: IntoIterator<Item = usize>>(indices: T) -> Self {
        let mut selection = Self::default();
        for index in indices {
            selection.insert(index);
        }
        selection
    }
}

#[cfg(any(test, feature = "validation"))]
impl From<u128> for MeshSelection {
    fn from(mask: u128) -> Self {
        let mut words = vec![mask as u64, (mask >> 64) as u64];
        trim(&mut words);
        Self(Arc::new(words))
    }
}

#[cfg(test)]
impl PartialEq<u128> for MeshSelection {
    fn eq(&self, mask: &u128) -> bool {
        self == &Self::from(*mask)
    }
}

fn trim(words: &mut Vec<u64>) {
    while words.last() == Some(&0) {
        words.pop();
    }
}
