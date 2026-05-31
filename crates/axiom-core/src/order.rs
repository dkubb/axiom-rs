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

use whittle::primitive::{CollectionError, KeyOf, LenItems, UniqueByKey};
use whittle::{And, Refined};

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

type OrderKeysRule =
    And<LenItems<1, { MAX_SCHEMA_ATTRIBUTES }>, UniqueByKey<OrderKey, OrderKeyAttrKey>>;

/// Bounded, attr-unique list of order keys.
///
/// Length is `1..=MAX_SCHEMA_ATTRIBUTES`; `attr` values are
/// pairwise distinct across the list (a second key on the same
/// attribute is shadowed and carries no semantics).
#[derive(Debug, Clone, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct OrderKeys(Refined<Vec<OrderKey>, OrderKeysRule>);

/// Constructor error for `OrderKeys`.
///
/// Flat domain-shaped enum: the underlying composition is
/// `And<LenItems, UniqueByKey>`. Both inner rules report through
/// `CollectionError`, so the composition's error is `CollectionError`
/// directly — no positional `Left` / `Right` wrapping leaks.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[non_exhaustive]
pub enum OrderKeysError {
    /// Key count fell outside `1..=MAX_SCHEMA_ATTRIBUTES`.
    #[error("order-key count out of range (actual: {actual})")]
    KeyCount {
        /// Observed key count.
        actual: usize,
    },

    /// Two keys referred to the same attribute. The reported index
    /// is the second occurrence (the first wins).
    #[error("duplicate order-key attribute at index {index}")]
    DuplicateKey {
        /// Position of the duplicate (the second occurrence).
        index: usize,
    },
}

impl OrderKeys {
    /// Validate `keys` against the order-key rule and wrap.
    ///
    /// # Errors
    ///
    /// Returns `KeyCount` if the list length is outside
    /// `1..=MAX_SCHEMA_ATTRIBUTES`, or `DuplicateKey` if two keys
    /// share an attribute name.
    #[inline]
    pub fn try_new(keys: Vec<OrderKey>) -> Result<Self, OrderKeysError> {
        Refined::try_new(keys).map(Self).map_err(|err| match err {
            CollectionError::LenOutOfRange { actual } => OrderKeysError::KeyCount { actual },
            CollectionError::DuplicateKey { index } => OrderKeysError::DuplicateKey { index },
            _ => unreachable!("OrderKeysRule emits only LenOutOfRange / DuplicateKey"),
        })
    }

    /// Borrow the inner key list.
    #[must_use]
    #[inline]
    pub const fn as_slice(&self) -> &[OrderKey] {
        self.0.as_inner().as_slice()
    }

    /// Consume the wrapper and return the inner key list.
    #[must_use]
    #[inline]
    pub fn into_inner(self) -> Vec<OrderKey> {
        self.0.into_inner()
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "explicit in test code"
)]
mod tests {
    use alloc::string::ToString;
    use alloc::vec;
    use alloc::vec::Vec;

    use super::{Direction, NullOrder, OrderKey, OrderKeys, OrderKeysError};
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
        assert_eq!(result.unwrap_err(), OrderKeysError::KeyCount { actual: 0 },);
    }

    #[test]
    fn overlength_keys_rejected() {
        let mut raw = Vec::with_capacity(MAX_SCHEMA_ATTRIBUTES + 1);
        for index in 0..=MAX_SCHEMA_ATTRIBUTES {
            let name = alloc::format!("c{index}");
            raw.push(key(&name, Direction::Ascending));
        }
        let result = OrderKeys::try_new(raw);
        assert!(matches!(
            result.unwrap_err(),
            OrderKeysError::KeyCount { .. },
        ));
    }

    #[test]
    fn duplicate_attribute_rejected() {
        let result = OrderKeys::try_new(vec![
            key("id", Direction::Ascending),
            key("id", Direction::Descending),
        ]);
        assert!(matches!(
            result.unwrap_err(),
            OrderKeysError::DuplicateKey { .. },
        ));
    }
}
