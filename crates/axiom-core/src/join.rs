//! `JoinOn`: how a `Join` operator equates rows.
//!
//! Lives separately from `JoinKind` because it depends on
//! `Predicate` and the bounded attribute-pair set, while `JoinKind`
//! is a closed sum with no dependencies.

use alloc::vec::Vec;
use core::marker::PhantomData;

use whittle::primitive::{
    CollectionError, KeyOf, LenItems, UniqueByKey,
};
use whittle::{And, Refined};

use crate::expression::Predicate;
use crate::identifier::AttributeName;
use crate::limits::MAX_SCHEMA_ATTRIBUTES;

/// Pair of attributes to be equated by an equi-join.
///
/// Stored as a (left, right) tuple. Uniqueness checks key off the
/// left attribute name only.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct EquiPair {
    /// Attribute from the left input.
    pub left: AttributeName,
    /// Attribute from the right input.
    pub right: AttributeName,
}

/// Key extractor for `UniqueByKey<EquiPair, EquiPairLeftKey>`.
pub struct EquiPairLeftKey(PhantomData<()>);

impl KeyOf<EquiPair> for EquiPairLeftKey {
    type Key = AttributeName;
    fn key_of(pair: &EquiPair) -> AttributeName {
        pair.left.clone()
    }
}

// The refined equi-pair set: 1..=MAX_SCHEMA_ATTRIBUTES pairs, with
// left-side names unique. Right-side duplicates are admitted (the
// same right-side column may be equated to multiple left-side
// columns in conjunction).
type EquiPairsRule = And<
    LenItems<1, { MAX_SCHEMA_ATTRIBUTES }>,
    UniqueByKey<EquiPair, EquiPairLeftKey>,
>;

/// Bounded equi-join pair set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EquiPairs(Refined<Vec<EquiPair>, EquiPairsRule>);

/// Constructor error for `EquiPairs`.
///
/// Flat domain-shaped enum: the underlying composition is
/// `And<LenItems, UniqueByKey>` keyed on the left attribute name.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[non_exhaustive]
pub enum EquiPairsError {
    /// Pair count fell outside `1..=MAX_SCHEMA_ATTRIBUTES`.
    #[error("equi-pair count out of range (actual: {actual})")]
    PairCount {
        /// Observed pair count.
        actual: usize,
    },

    /// Two pairs shared the same left-side attribute. The reported
    /// index is the second occurrence (the first wins).
    #[error("duplicate left-side attribute at index {index}")]
    DuplicateLeftAttribute {
        /// Position of the duplicate (the second occurrence).
        index: usize,
    },
}

impl EquiPairs {
    /// Validate `pairs` against the equi-pair rule and wrap.
    ///
    /// # Errors
    ///
    /// Returns `PairCount` if the list length is outside
    /// `1..=MAX_SCHEMA_ATTRIBUTES`, or `DuplicateLeftAttribute` if
    /// two pairs share a left-side attribute.
    #[inline]
    pub fn try_new(
        pairs: Vec<EquiPair>,
    ) -> Result<Self, EquiPairsError> {
        Refined::try_new(pairs).map(Self).map_err(|err| match err {
            CollectionError::LenOutOfRange { actual } => {
                EquiPairsError::PairCount { actual }
            }
            CollectionError::DuplicateKey { index } => {
                EquiPairsError::DuplicateLeftAttribute { index }
            }
            _ => unreachable!(
                "EquiPairsRule emits only LenOutOfRange / DuplicateKey"
            ),
        })
    }

    /// Borrow the underlying pair list.
    #[must_use]
    #[inline]
    pub const fn as_slice(&self) -> &[EquiPair] {
        self.0.as_inner().as_slice()
    }

    /// Consume the wrapper and return the inner pair list.
    #[must_use]
    #[inline]
    pub fn into_inner(self) -> Vec<EquiPair> {
        self.0.into_inner()
    }
}

/// How a `Join` operator equates rows.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum JoinOn {
    /// Natural join: equate columns that share a name in both
    /// inputs. With no shared columns, the smart constructor of
    /// `Op::join` rewrites this to a cartesian product (per
    /// docs/ARCHITECTURE.md §9.0).
    Natural,
    /// Equi-join on a bounded set of (left, right) attribute pairs.
    Equi(EquiPairs),
    /// Theta-join on an arbitrary boolean predicate.
    Theta(Predicate),
}


#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used,
        reason = "explicit in test code")]
mod tests {
    use alloc::string::ToString;
    use alloc::vec;
    use alloc::vec::Vec;

    use super::{EquiPair, EquiPairs, EquiPairsError};
    use crate::identifier::AttributeName;

    fn attr(name: &str) -> AttributeName {
        AttributeName::try_new(name.to_string()).unwrap()
    }

    fn pair(left: &str, right: &str) -> EquiPair {
        EquiPair {
            left: attr(left),
            right: attr(right),
        }
    }

    #[test]
    fn equi_pairs_admit_distinct_left_keys() {
        let pairs = EquiPairs::try_new(vec![
            pair("user_id", "id"),
            pair("order_id", "id"),
        ])
        .unwrap();
        assert_eq!(pairs.as_slice().len(), 2);
    }

    #[test]
    fn equi_pairs_reject_empty() {
        let result = EquiPairs::try_new(Vec::new());
        assert_eq!(
            result.unwrap_err(),
            EquiPairsError::PairCount { actual: 0 },
        );
    }

    #[test]
    fn equi_pairs_reject_duplicate_left_key() {
        let result = EquiPairs::try_new(vec![
            pair("user_id", "a"),
            pair("user_id", "b"),
        ]);
        assert_eq!(
            result.unwrap_err(),
            EquiPairsError::DuplicateLeftAttribute { index: 1 },
        );
    }
}
