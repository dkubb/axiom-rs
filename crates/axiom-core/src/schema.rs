//! `Schema` and `Attribute`.
//!
//! A schema's header is a `Vec<Attribute>` refined by whittle's
//! `LenItems<1, MAX_SCHEMA_ATTRIBUTES>` (length) composed via
//! `whittle::And` with `UniqueByKey<Attribute, AttributeKey>`
//! (per-attribute-name uniqueness). The entire header invariant is
//! a single whittle refinement; there is no manual second pass.

use alloc::vec::Vec;
use core::marker::PhantomData;

use whittle::primitive::{
    CollectionError, KeyOf, LenItems, NumericError, UniqueByKey, Within,
};
use whittle::{And, AndError, Refined};

use crate::identifier::AttributeName;
use crate::limits::MAX_SCHEMA_ATTRIBUTES;
use crate::ty::Type;

/// Number of attributes in a `Schema`: at least one (the empty
/// schema is not admissible), at most `MAX_SCHEMA_ATTRIBUTES`.
pub type SchemaCardinality =
    Refined<usize, Within<1, { MAX_SCHEMA_ATTRIBUTES as i128 }>>;

/// Constructor error for `SchemaCardinality`.
pub type SchemaCardinalityError = NumericError;

/// One column in a relation schema: its name and type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attribute {
    /// Column name.
    pub name: AttributeName,
    /// Column type.
    pub ty: Type,
}

/// Key extractor that pulls an `AttributeName` out of an `Attribute`
/// for uniqueness checks.
pub struct AttributeKey(PhantomData<()>);

impl KeyOf<Attribute> for AttributeKey {
    type Key = AttributeName;
    fn key_of(attr: &Attribute) -> AttributeName {
        attr.name.clone()
    }
}

// The composite rule: length-bound first, then per-attribute-name
// uniqueness. Both inner rules report through `CollectionError`, so
// the wrapping `AndError` carries the same error type on both sides.
type SchemaHeaderRule = And<
    LenItems<1, { MAX_SCHEMA_ATTRIBUTES }>,
    UniqueByKey<Attribute, AttributeKey>,
>;

/// Relation header: bounded ordered list of `Attribute`s with
/// unique attribute names. The entire invariant is a single
/// `whittle::Refined` value — there is no manual second pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Schema {
    attributes: Refined<Vec<Attribute>, SchemaHeaderRule>,
}

/// Constructor error for `Schema`. Wraps whittle's
/// `AndError<CollectionError, CollectionError>` under the axiom-rs
/// vocabulary so call sites need not name the rule type.
pub type SchemaError = AndError<CollectionError, CollectionError>;

impl Schema {
    /// Validate `raw` and wrap.
    ///
    /// # Errors
    ///
    /// Returns `AndError::Left(CollectionError::LenOutOfRange ..)`
    /// when `raw` is empty or exceeds `MAX_SCHEMA_ATTRIBUTES`.
    /// Returns `AndError::Right(CollectionError::DuplicateKey ..)`
    /// when two attributes share the same name.
    #[inline]
    pub fn try_new(raw: Vec<Attribute>) -> Result<Self, SchemaError> {
        Refined::try_new(raw).map(|attributes| Self { attributes })
    }

    /// Borrow the attribute list.
    #[must_use]
    #[inline]
    pub const fn attributes(&self) -> &[Attribute] {
        self.attributes.as_inner().as_slice()
    }

    /// Number of attributes in the schema. Never zero — the rule's
    /// lower bound makes the empty header unrepresentable.
    #[must_use]
    #[inline]
    pub const fn cardinality(&self) -> usize {
        self.attributes.as_inner().len()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used,
        reason = "explicit in test code")]
mod tests {
    use alloc::string::ToString;
    use alloc::vec;
    use alloc::vec::Vec;

    use whittle::primitive::CollectionError;
    use whittle::AndError;

    use super::{
        Attribute, Schema, SchemaCardinality, SchemaCardinalityError,
    };
    use crate::identifier::AttributeName;
    use crate::limits::MAX_SCHEMA_ATTRIBUTES;
    use crate::ty::Type;

    fn attr(name: &str, ty: Type) -> Attribute {
        Attribute {
            name: AttributeName::try_new(name.to_string()).unwrap(),
            ty,
        }
    }

    // ─── SchemaCardinality. ──────────────────────────────────────

    #[test]
    fn cardinality_zero_rejected() {
        let result = SchemaCardinality::try_new(0_usize);
        assert!(matches!(
            result.unwrap_err(),
            SchemaCardinalityError::OutOfRange { value: 0 },
        ));
    }

    #[test]
    fn cardinality_one_accepted() {
        let count = SchemaCardinality::try_new(1_usize).unwrap();
        assert_eq!(*count.as_inner(), 1_usize);
    }

    #[test]
    fn cardinality_max_inclusive_accepted() {
        let count = SchemaCardinality::try_new(MAX_SCHEMA_ATTRIBUTES).unwrap();
        assert_eq!(*count.as_inner(), MAX_SCHEMA_ATTRIBUTES);
    }

    // ─── Schema. ─────────────────────────────────────────────────

    #[test]
    fn schema_single_attribute_admitted() {
        let s = Schema::try_new(vec![attr("id", Type::Int64)]).unwrap();
        assert_eq!(s.cardinality(), 1);
    }

    #[test]
    fn schema_two_distinct_attributes_admitted() {
        let s = Schema::try_new(vec![
            attr("id", Type::Int64),
            attr("name", Type::String),
        ])
        .unwrap();
        assert_eq!(s.cardinality(), 2);
    }

    #[test]
    fn empty_schema_rejected_by_length_check() {
        let result = Schema::try_new(Vec::new());
        assert!(matches!(
            result.unwrap_err(),
            AndError::Left(CollectionError::LenOutOfRange { actual: 0 }),
        ));
    }

    #[test]
    fn duplicate_attribute_name_rejected() {
        let result = Schema::try_new(vec![
            attr("id", Type::Int64),
            attr("id", Type::String),
        ]);
        assert!(matches!(
            result.unwrap_err(),
            AndError::Right(CollectionError::DuplicateKey { index: 1 }),
        ));
    }
}
