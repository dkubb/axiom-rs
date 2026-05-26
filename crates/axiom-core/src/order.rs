//! Order key types: `Direction`, `NullOrder`, `OrderKey`,
//! `OrderKeys`.
//!
//! `OrderKeys` is the refined carrier — a bounded `Vec<OrderKey>`
//! whose length is `1..=MAX_SCHEMA_ATTRIBUTES` and whose attributes
//! are pairwise distinct (a second key on the same attribute is
//! always shadowed by the first, so admitting it would represent a
//! state with no contractual meaning).

use alloc::vec::Vec;
use core::marker::PhantomData;

use thiserror::Error;
use whittle::primitive::{CollectionError, KeyOf, LenItems, UniqueByKey};
use whittle::{And, AndError, Refined};

use crate::identifier::AttributeName;
use crate::limits::MAX_SCHEMA_ATTRIBUTES;

/// Sort direction for an `OrderKey`.
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord)]
#[non_exhaustive]
pub enum Direction {
    /// Ascending order.
    Ascending,
    /// Descending order.
    Descending,
}

/// Null placement for an `OrderKey`.
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord)]
#[non_exhaustive]
pub enum NullOrder {
    /// Nulls sort before non-null values.
    NullsFirst,
    /// Nulls sort after non-null values.
    NullsLast,
}

/// A single ordering directive: attribute, direction, and null
/// placement.
#[derive(Debug, Clone, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct OrderKey {
    /// Attribute being ordered.
    pub attr: AttributeName,
    /// Ascending or descending.
    pub direction: Direction,
    /// How nulls sort relative to non-null values.
    pub nulls: NullOrder,
}

/// Key extractor for `UniqueByKey<OrderKey, OrderKeyAttrKey>`:
/// uniqueness is on `attr` only (direction and null placement
/// don't disambiguate — `[id ASC, id DESC]` is still a redundant
/// pair of orderings on `id`).
pub struct OrderKeyAttrKey(PhantomData<()>);

impl KeyOf<OrderKey> for OrderKeyAttrKey {
    type Key = AttributeName;
    fn key_of(value: &OrderKey) -> AttributeName {
        value.attr.clone()
    }
}

type OrderKeysRule = And<
    LenItems<1, { MAX_SCHEMA_ATTRIBUTES }>,
    UniqueByKey<OrderKey, OrderKeyAttrKey>,
>;

/// Bounded, attr-unique list of order keys.
///
/// Length is `1..=MAX_SCHEMA_ATTRIBUTES`; `attr` values are
/// pairwise distinct across the list (a second key on the same
/// attribute is shadowed and carries no semantics).
#[derive(Debug, Clone, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct OrderKeys(Refined<Vec<OrderKey>, OrderKeysRule>);

/// Constructor error for `OrderKeys`.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum OrderKeysError {
    /// Length bound failed (empty list or over-length).
    #[error("{0}")]
    Length(#[source] CollectionError),
    /// Two keys reference the same attribute.
    #[error("{0}")]
    DuplicateAttribute(#[source] CollectionError),
}

impl From<AndError<CollectionError, CollectionError>> for OrderKeysError {
    fn from(err: AndError<CollectionError, CollectionError>) -> Self {
        match err {
            AndError::Left(inner) => Self::Length(inner),
            AndError::Right(inner) => Self::DuplicateAttribute(inner),
        }
    }
}

impl OrderKeys {
    /// Validate `raw` and wrap.
    ///
    /// # Errors
    ///
    /// Returns `OrderKeysError::Length` when `raw` is empty or
    /// exceeds `MAX_SCHEMA_ATTRIBUTES`, and
    /// `OrderKeysError::DuplicateAttribute` when two keys reference
    /// the same attribute.
    #[inline]
    pub fn try_new(raw: Vec<OrderKey>) -> Result<Self, OrderKeysError> {
        Refined::try_new(raw).map(Self).map_err(Into::into)
    }

    /// Borrow the inner key list.
    #[must_use]
    #[inline]
    pub const fn as_slice(&self) -> &[OrderKey] {
        self.0.as_inner().as_slice()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used,
        reason = "explicit in test code")]
mod tests {
    use alloc::string::ToString;
    use alloc::vec;
    use alloc::vec::Vec;

    use super::{
        Direction, NullOrder, OrderKey, OrderKeys, OrderKeysError,
    };
    use crate::identifier::AttributeName;
    use crate::limits::MAX_SCHEMA_ATTRIBUTES;

    fn key(name: &str, dir: Direction) -> OrderKey {
        OrderKey {
            attr: AttributeName::try_new(name.to_string()).unwrap(),
            direction: dir,
            nulls: NullOrder::NullsLast,
        }
    }

    #[test]
    fn single_key_admitted() {
        let keys = OrderKeys::try_new(vec![key("id", Direction::Ascending)]).unwrap();
        assert_eq!(keys.as_slice().len(), 1);
    }

    #[test]
    fn empty_keys_rejected() {
        let result = OrderKeys::try_new(Vec::new());
        assert!(matches!(result.unwrap_err(), OrderKeysError::Length(_)));
    }

    #[test]
    fn overlength_keys_rejected() {
        let mut raw = Vec::with_capacity(MAX_SCHEMA_ATTRIBUTES + 1);
        for index in 0..=MAX_SCHEMA_ATTRIBUTES {
            let name = alloc::format!("c{index}");
            raw.push(key(&name, Direction::Ascending));
        }
        let result = OrderKeys::try_new(raw);
        assert!(matches!(result.unwrap_err(), OrderKeysError::Length(_)));
    }

    #[test]
    fn duplicate_attribute_rejected() {
        let result = OrderKeys::try_new(vec![
            key("id", Direction::Ascending),
            key("id", Direction::Descending),
        ]);
        assert!(matches!(
            result.unwrap_err(),
            OrderKeysError::DuplicateAttribute(_),
        ));
    }
}
