//! Field definitions, types, and the [`FieldSet`] bitset.

use crate::id::FieldId;

/// Classification of a field's data type.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FieldType {
    /// A single floating-point value per cell.
    Scalar,
    /// A fixed-size vector of floating-point values per cell.
    Vector {
        /// Number of components in the vector (e.g., 3 for velocity).
        dims: u32,
    },
    /// A categorical (discrete) value per cell, stored as a single f32 index.
    Categorical {
        /// Number of possible categories.
        n_values: u32,
    },
}

impl FieldType {
    /// Returns the number of f32 storage slots this field type requires per cell.
    pub fn components(&self) -> u32 {
        match self {
            Self::Scalar => 1,
            Self::Vector { dims } => *dims,
            Self::Categorical { .. } => 1,
        }
    }
}

/// Boundary behavior when field values exceed declared bounds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BoundaryBehavior {
    /// Clamp the value to the nearest bound.
    Clamp,
    /// Reflect the value off the bound.
    Reflect,
    /// Absorb at the boundary (value is set to the bound).
    Absorb,
    /// Wrap around to the opposite bound.
    Wrap,
}

/// How a field's allocation is managed across ticks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldMutability {
    /// Generation 0 forever. Shared across all snapshots and vectorized envs.
    Static,
    /// New allocation each tick if modified. Per-generation.
    PerTick,
    /// New allocation only when modified. Shared until mutation.
    Sparse,
}

/// Definition of a field registered in a simulation world.
///
/// Fields are the fundamental unit of per-cell state. Each field has a type,
/// mutability class, optional bounds, and boundary behavior. Fields are
/// registered at world creation; `FieldId` is the index into the field list.
#[derive(Clone, Debug, PartialEq)]
pub struct FieldDef {
    /// Human-readable name for debugging and logging.
    pub name: String,
    /// Data type and dimensionality.
    pub field_type: FieldType,
    /// Allocation strategy across ticks.
    pub mutability: FieldMutability,
    /// Optional unit annotation (e.g., `"meters/sec"`).
    pub units: Option<String>,
    /// Optional `(min, max)` bounds for field values.
    pub bounds: Option<(f32, f32)>,
    /// Behavior when values exceed declared bounds.
    pub boundary_behavior: BoundaryBehavior,
}

/// A set of field IDs implemented as a dynamically-sized bitset.
///
/// Used by propagators to declare which fields they read and write,
/// enabling the engine to validate the dependency graph and compute
/// overlay resolution plans.
#[derive(Clone, Debug)]
pub struct FieldSet {
    bits: Vec<u64>,
}

impl FieldSet {
    const BITS_PER_WORD: usize = 64;

    /// Create an empty field set.
    pub fn empty() -> Self {
        Self { bits: Vec::new() }
    }

    /// Insert a field ID into the set.
    pub fn insert(&mut self, field: FieldId) {
        let word = field.0 as usize / Self::BITS_PER_WORD;
        let bit = field.0 as usize % Self::BITS_PER_WORD;
        if word >= self.bits.len() {
            self.bits.resize(word + 1, 0);
        }
        self.bits[word] |= 1u64 << bit;
    }

    /// Check whether the set contains a field ID.
    pub fn contains(&self, field: FieldId) -> bool {
        let word = field.0 as usize / Self::BITS_PER_WORD;
        let bit = field.0 as usize % Self::BITS_PER_WORD;
        word < self.bits.len() && (self.bits[word] & (1u64 << bit)) != 0
    }

    /// Return the union of two sets (`self | other`).
    pub fn union(&self, other: &Self) -> Self {
        let max_len = self.bits.len().max(other.bits.len());
        let mut bits = Vec::with_capacity(max_len);
        for i in 0..max_len {
            let a = self.bits.get(i).copied().unwrap_or(0);
            let b = other.bits.get(i).copied().unwrap_or(0);
            bits.push(a | b);
        }
        Self { bits }
    }

    /// Return the intersection of two sets (`self & other`).
    pub fn intersection(&self, other: &Self) -> Self {
        let min_len = self.bits.len().min(other.bits.len());
        let mut bits = Vec::with_capacity(min_len);
        for i in 0..min_len {
            bits.push(self.bits[i] & other.bits[i]);
        }
        while bits.last() == Some(&0) {
            bits.pop();
        }
        Self { bits }
    }

    /// Return the set difference (`self - other`): elements in `self` but not `other`.
    pub fn difference(&self, other: &Self) -> Self {
        let mut bits = Vec::with_capacity(self.bits.len());
        for i in 0..self.bits.len() {
            let b = other.bits.get(i).copied().unwrap_or(0);
            bits.push(self.bits[i] & !b);
        }
        while bits.last() == Some(&0) {
            bits.pop();
        }
        Self { bits }
    }

    /// Check whether `self` is a subset of `other`.
    pub fn is_subset(&self, other: &Self) -> bool {
        for i in 0..self.bits.len() {
            let b = other.bits.get(i).copied().unwrap_or(0);
            if self.bits[i] & !b != 0 {
                return false;
            }
        }
        true
    }

    /// Returns `true` if the set contains no fields.
    pub fn is_empty(&self) -> bool {
        self.bits.iter().all(|&w| w == 0)
    }

    /// Returns the number of fields in the set.
    pub fn len(&self) -> usize {
        self.bits.iter().map(|w| w.count_ones() as usize).sum()
    }

    /// Iterate over the field IDs in the set, in ascending order.
    pub fn iter(&self) -> FieldSetIter<'_> {
        FieldSetIter {
            bits: &self.bits,
            word_idx: 0,
            bit_idx: 0,
        }
    }
}

impl PartialEq for FieldSet {
    fn eq(&self, other: &Self) -> bool {
        let max_len = self.bits.len().max(other.bits.len());
        for i in 0..max_len {
            let a = self.bits.get(i).copied().unwrap_or(0);
            let b = other.bits.get(i).copied().unwrap_or(0);
            if a != b {
                return false;
            }
        }
        true
    }
}

impl Eq for FieldSet {}

impl FromIterator<FieldId> for FieldSet {
    fn from_iter<I: IntoIterator<Item = FieldId>>(iter: I) -> Self {
        let mut set = Self::empty();
        for field in iter {
            set.insert(field);
        }
        set
    }
}

impl<'a> IntoIterator for &'a FieldSet {
    type Item = FieldId;
    type IntoIter = FieldSetIter<'a>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

/// Iterator over field IDs in a [`FieldSet`], yielding IDs in ascending order.
pub struct FieldSetIter<'a> {
    bits: &'a [u64],
    word_idx: usize,
    bit_idx: usize,
}

impl Iterator for FieldSetIter<'_> {
    type Item = FieldId;

    fn next(&mut self) -> Option<Self::Item> {
        while self.word_idx < self.bits.len() {
            let word = self.bits[self.word_idx];
            while self.bit_idx < 64 {
                let bit = self.bit_idx;
                self.bit_idx += 1;
                if word & (1u64 << bit) != 0 {
                    return Some(FieldId((self.word_idx * 64 + bit) as u32));
                }
            }
            self.word_idx += 1;
            self.bit_idx = 0;
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn arb_field_set() -> impl Strategy<Value = FieldSet> {
        prop::collection::vec(0u32..128, 0..32)
            .prop_map(|ids| ids.into_iter().map(FieldId).collect::<FieldSet>())
    }

    proptest! {
        #[test]
        fn union_commutative(a in arb_field_set(), b in arb_field_set()) {
            prop_assert_eq!(a.union(&b), b.union(&a));
        }

        #[test]
        fn intersection_commutative(a in arb_field_set(), b in arb_field_set()) {
            prop_assert_eq!(a.intersection(&b), b.intersection(&a));
        }

        #[test]
        fn union_associative(
            a in arb_field_set(),
            b in arb_field_set(),
            c in arb_field_set(),
        ) {
            prop_assert_eq!(a.union(&b).union(&c), a.union(&b.union(&c)));
        }

        #[test]
        fn intersection_associative(
            a in arb_field_set(),
            b in arb_field_set(),
            c in arb_field_set(),
        ) {
            prop_assert_eq!(
                a.intersection(&b).intersection(&c),
                a.intersection(&b.intersection(&c))
            );
        }

        #[test]
        fn union_identity(a in arb_field_set()) {
            prop_assert_eq!(a.union(&FieldSet::empty()), a.clone());
        }

        #[test]
        fn union_idempotent(a in arb_field_set()) {
            prop_assert_eq!(a.union(&a), a.clone());
        }

        #[test]
        fn intersection_idempotent(a in arb_field_set()) {
            prop_assert_eq!(a.intersection(&a), a.clone());
        }

        #[test]
        fn intersection_with_empty(a in arb_field_set()) {
            prop_assert_eq!(a.intersection(&FieldSet::empty()), FieldSet::empty());
        }

        #[test]
        fn difference_removes_common(a in arb_field_set(), b in arb_field_set()) {
            let diff = a.difference(&b);
            for field in diff.iter() {
                prop_assert!(a.contains(field), "diff element {field:?} not in a");
                prop_assert!(!b.contains(field), "diff element {field:?} in b");
            }
        }

        #[test]
        fn distributive_intersection_over_union(
            a in arb_field_set(),
            b in arb_field_set(),
            c in arb_field_set(),
        ) {
            prop_assert_eq!(
                a.intersection(&b.union(&c)),
                a.intersection(&b).union(&a.intersection(&c))
            );
        }

        #[test]
        fn subset_reflexive(a in arb_field_set()) {
            prop_assert!(a.is_subset(&a));
        }

        #[test]
        fn empty_is_subset(a in arb_field_set()) {
            prop_assert!(FieldSet::empty().is_subset(&a));
        }

        #[test]
        fn insert_contains(id in 0u32..256) {
            let mut set = FieldSet::empty();
            set.insert(FieldId(id));
            prop_assert!(set.contains(FieldId(id)));
            prop_assert_eq!(set.len(), 1);
        }

        #[test]
        fn len_matches_iter_count(a in arb_field_set()) {
            prop_assert_eq!(a.len(), a.iter().count());
        }
    }
}
