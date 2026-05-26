//! `Row` — a positional value list whose length is bounded by
//! `MAX_SCHEMA_ATTRIBUTES` and whose contents will eventually be
//! type-checked against a `Schema` header.
//!
//! Today this carries just the length bound via whittle's
//! `LenItems`. The per-position type check lands when `Value` and
//! `Type` are sufficiently mature to express "this `Value` matches
//! this `Type`."

use alloc::vec::Vec;

use thiserror::Error;
use whittle::primitive::{CollectionError, LenItems};
use whittle::Refined;

use crate::limits::MAX_SCHEMA_ATTRIBUTES;
use crate::ty::Value;

/// Length-bounded row: 1..=`MAX_SCHEMA_ATTRIBUTES` values.
#[derive(Debug, Clone, PartialEq)]
pub struct Row(
    Refined<Vec<Value>, LenItems<1, { MAX_SCHEMA_ATTRIBUTES }>>,
);

/// Constructor error for `Row`.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum RowError {
    /// Underlying length-bound rejection from whittle.
    #[error("{0}")]
    Length(#[source] CollectionError),
}

impl From<CollectionError> for RowError {
    fn from(err: CollectionError) -> Self {
        Self::Length(err)
    }
}

impl Row {
    /// Validate `raw` and wrap.
    ///
    /// # Errors
    ///
    /// Returns `RowError::Length` when `raw` is empty or exceeds
    /// `MAX_SCHEMA_ATTRIBUTES` values.
    #[inline]
    pub fn try_new(raw: Vec<Value>) -> Result<Self, RowError> {
        Refined::try_new(raw).map(Self).map_err(Into::into)
    }

    /// Borrow the inner value vector.
    #[must_use]
    #[inline]
    pub const fn as_slice(&self) -> &[Value] {
        self.0.as_inner().as_slice()
    }

    /// Number of values in the row. Never zero — the rule's lower
    /// bound (`LenItems<1, _>`) makes the empty row unrepresentable.
    #[must_use]
    #[inline]
    pub const fn len(&self) -> usize {
        self.0.as_inner().len()
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
#[allow(clippy::unwrap_used, clippy::expect_used,
        reason = "explicit in test code")]
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
        assert!(matches!(result.unwrap_err(), RowError::Length(_)));
    }
}
