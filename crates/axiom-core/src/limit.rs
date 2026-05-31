//! Numeric domain types: `Offset`, `LimitCount`, `BoundedIndex`.
//!
//! Each one is a `whittle::Refined` newtype with a bounded numeric
//! rule keyed off a constant in `crate::limits`.

use whittle::primitive::{NumericError, Within};
use whittle::{Refined, refinement};

use crate::limits::{MAX_LIMIT_COUNT, MAX_OFFSET, MAX_PATH_INDEX};

// Whittle's `Within` is parameterised over `i128` — wide enough to
// hold every value we need. Casts from `u64`/`usize` round-trip
// losslessly into `i128`.

refinement! {
    /// `OFFSET` value in a `Limit` operator: `0..=MAX_OFFSET`.
    #[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord)]
    pub Offset: u64, Within<0, { MAX_OFFSET as i128 }>;
}

impl Offset {
    /// Borrow the inner `u64`.
    #[must_use]
    #[inline]
    pub const fn get(&self) -> u64 {
        *self.as_inner()
    }
}

/// Constructor error for `Offset`. `Within<MIN, MAX>` in whittle
/// is a nominal domain newtype with a flat `NumericError`, so the
/// composition machinery does not leak through the domain surface.
pub type OffsetError = NumericError;

/// `LIMIT n` count in the `Limit` operator. `Bounded(0)` is
/// admissible and denotes `LIMIT 0`.
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub enum LimitCount {
    /// No limit (the entire input passes through).
    Unbounded,
    /// At most this many tuples.
    Bounded(Refined<u64, Within<0, { MAX_LIMIT_COUNT as i128 }>>),
}

/// Constructor error for `LimitCount::Bounded`. `Within` exposes
/// a flat `NumericError`; see `OffsetError` for the same rationale.
pub type LimitCountError = NumericError;

impl LimitCount {
    /// Build a `Bounded` variant from a raw `u64`.
    ///
    /// # Errors
    ///
    /// Returns `NumericError::OutOfRange` if `raw > MAX_LIMIT_COUNT`.
    #[inline]
    pub fn bounded(raw: u64) -> Result<Self, LimitCountError> {
        Refined::try_new(raw).map(Self::Bounded)
    }

    /// `LIMIT 0` shortcut: always admissible by construction.
    #[must_use]
    #[inline]
    pub fn zero() -> Self {
        // SAFETY by rule: 0 is in 0..=MAX_LIMIT_COUNT for every
        // sensible value of MAX_LIMIT_COUNT.
        Self::bounded(0).unwrap_or_else(|_| -> Self { unreachable!() })
    }

    /// Borrow the inner count, returning `None` for `Unbounded`.
    #[must_use]
    #[inline]
    pub const fn get(&self) -> Option<u64> {
        match self {
            Self::Unbounded => None,
            Self::Bounded(refined) => Some(*refined.as_inner()),
        }
    }
}

refinement! {
    /// Positional index inside a `Path` step:
    /// `0..=MAX_PATH_INDEX`.
    #[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord)]
    pub BoundedIndex: usize, Within<0, { MAX_PATH_INDEX as i128 }>;
}

impl BoundedIndex {
    /// Borrow the inner `usize`.
    #[must_use]
    #[inline]
    pub const fn get(&self) -> usize {
        *self.as_inner()
    }
}

/// Constructor error for `BoundedIndex`. `Within` exposes a flat
/// `NumericError`; see `OffsetError` for the same rationale.
pub type BoundedIndexError = NumericError;

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "explicit in test code"
)]
mod tests {
    use whittle::primitive::NumericError;

    use super::{BoundedIndex, LimitCount, Offset};
    use crate::limits::{MAX_LIMIT_COUNT, MAX_OFFSET, MAX_PATH_INDEX};

    #[test]
    fn offset_accepts_zero() {
        let o = Offset::try_new(0).unwrap();
        assert_eq!(o.get(), 0);
    }

    #[test]
    fn offset_accepts_max_inclusive() {
        let o = Offset::try_new(MAX_OFFSET).unwrap();
        assert_eq!(o.get(), MAX_OFFSET);
    }

    #[test]
    fn offset_rejects_above_max() {
        // u64::MAX > MAX_OFFSET = u64::MAX / 2
        let result = Offset::try_new(u64::MAX);
        // `Within` exposes a flat `NumericError`; no composition
        // machinery leaks through.
        assert!(matches!(
            result.unwrap_err(),
            NumericError::OutOfRange { .. },
        ));
    }

    #[test]
    fn limit_count_unbounded_has_no_inner_value() {
        let lc = LimitCount::Unbounded;
        assert!(lc.get().is_none());
    }

    #[test]
    fn limit_count_bounded_zero_is_admissible() {
        let zero = LimitCount::zero();
        assert_eq!(zero.get(), Some(0));
        let zero_explicit = LimitCount::bounded(0).unwrap();
        assert_eq!(zero, zero_explicit);
    }

    #[test]
    fn limit_count_bounded_round_trips_admissible() {
        let lc = LimitCount::bounded(42).unwrap();
        assert_eq!(lc.get(), Some(42));
    }

    #[test]
    fn limit_count_bounded_rejects_above_max() {
        let result = LimitCount::bounded(MAX_LIMIT_COUNT + 1);
        // `Within` exposes a flat `NumericError`; no composition
        // machinery leaks through.
        assert!(matches!(
            result.unwrap_err(),
            NumericError::OutOfRange { .. },
        ));
    }

    #[test]
    fn bounded_index_accepts_zero() {
        let i = BoundedIndex::try_new(0).unwrap();
        assert_eq!(i.get(), 0);
    }

    #[test]
    fn bounded_index_accepts_max_inclusive() {
        let i = BoundedIndex::try_new(MAX_PATH_INDEX).unwrap();
        assert_eq!(i.get(), MAX_PATH_INDEX);
    }

    #[test]
    fn bounded_index_rejects_above_max() {
        let result = BoundedIndex::try_new(MAX_PATH_INDEX + 1);
        // `Within` exposes a flat `NumericError`; no composition
        // machinery leaks through.
        assert!(matches!(
            result.unwrap_err(),
            NumericError::OutOfRange { .. },
        ));
    }
}
