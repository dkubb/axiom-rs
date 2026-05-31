//! `Row` — a positional value list whose length is bounded by
//! `MAX_SCHEMA_ATTRIBUTES` and whose contents will eventually be
//! type-checked against a `Schema` header.
//!
//! Today this carries just the length bound via whittle's
//! `LenItems`. The per-position type check lands when `Value` and
//! `Type` are sufficiently mature to express "this `Value` matches
//! this `Type`."

use alloc::vec::Vec;

use whittle::primitive::{CollectionError, LenItems};
use whittle::refinement;

use crate::limits::MAX_SCHEMA_ATTRIBUTES;
use crate::ty::Value;

type RowRule = LenItems<1, { MAX_SCHEMA_ATTRIBUTES }>;

refinement! {
    /// Length-bounded row: 1..=`MAX_SCHEMA_ATTRIBUTES` values.
    #[derive(Debug, Clone, PartialEq)]
    pub Row: Vec<Value>, RowRule;
}

/// Constructor error for `Row`.
pub type RowError = CollectionError;

impl Row {
    /// Borrow the inner value vector.
    #[must_use]
    #[inline]
    pub const fn as_slice(&self) -> &[Value] {
        self.as_inner().as_slice()
    }

    /// Number of values in the row. Never zero — the rule's lower
    /// bound (`LenItems<1, _>`) makes the empty row unrepresentable.
    #[must_use]
    #[inline]
    pub const fn len(&self) -> usize {
        self.as_inner().len()
    }

    /// `false` by construction. Present to satisfy
    /// `clippy::len_without_is_empty`; a `Row` whose length is in
    /// `1..=MAX_SCHEMA_ATTRIBUTES` is never empty.
    #[must_use]
    #[inline]
    pub const fn is_empty(&self) -> bool {
        false
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "explicit in test code"
)]
mod tests {
    use alloc::vec;
    use alloc::vec::Vec;

    use super::{Row, RowError};
    use crate::ty::Value;

    #[test]
    fn single_value_row_admitted() {
        let row = Row::try_new(vec![Value::Bool(true)]).unwrap();
        assert_eq!(row.len(), 1);
    }

    #[test]
    fn empty_row_rejected() {
        let result = Row::try_new(Vec::new());
        assert!(matches!(
            result.unwrap_err(),
            RowError::LenOutOfRange { actual: 0 },
        ));
    }
}
