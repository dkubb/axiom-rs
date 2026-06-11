//! `Schema` and `Attribute`.
//!
//! A schema's header is a `Vec<Attribute>` refined by whittle's
//! `LenItems<1, MAX_SCHEMA_ATTRIBUTES>` (length) composed via
//! `whittle::And` with `UniqueByKey<Attribute, AttributeKey>`
//! (per-attribute-name uniqueness). The entire header invariant is
//! a single whittle refinement; there is no manual second pass.

use alloc::vec::Vec;
use core::marker::PhantomData;

use whittle::primitive::{CollectionError, KeyOf, LenItems, NumericError, UniqueByKey, Within};
use whittle::{And, Refined};

use crate::identifier::AttributeName;
use crate::limits::MAX_SCHEMA_ATTRIBUTES;
use crate::ty::Type;

/// Number of attributes in a `Schema`: at least one (the empty
/// schema is not admissible), at most `MAX_SCHEMA_ATTRIBUTES`.
pub type SchemaCardinality = Refined<usize, Within<1, { MAX_SCHEMA_ATTRIBUTES as i128 }>>;

/// Constructor error for `SchemaCardinality`. `Within<MIN, MAX>`
/// in whittle is a nominal domain newtype with a flat
/// `NumericError`, so the composition machinery does not leak.
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
// the composition's error is `CollectionError` directly — no
// positional `Left` / `Right` wrapping is exposed.
type SchemaHeaderRule =
    And<LenItems<1, { MAX_SCHEMA_ATTRIBUTES }>, UniqueByKey<Attribute, AttributeKey>>;

/// Relation header: bounded ordered list of `Attribute`s with
/// unique attribute names. The entire invariant is a single
/// `whittle::Refined` value — there is no manual second pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Schema(Refined<Vec<Attribute>, SchemaHeaderRule>);

/// Constructor error for `Schema`.
///
/// Flat domain-shaped enum: the underlying composition is
/// `And<LenItems, UniqueByKey>` and both inner rules report through
/// `CollectionError`, so the composition surfaces `CollectionError`
/// directly — call sites match on `Cardinality` or
/// `DuplicateAttribute` here without seeing the rule shape.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum SchemaError {
    /// Attribute count fell outside `1..=MAX_SCHEMA_ATTRIBUTES`.
    #[error("schema cardinality out of range (actual: {actual})")]
    Cardinality {
        /// Observed attribute count.
        actual: usize,
    },

    /// Two attributes shared a name. The reported index is the
    /// second occurrence (the first wins).
    #[error("duplicate attribute name at index {index}")]
    DuplicateAttribute {
        /// Position of the duplicate (the second occurrence).
        index: usize,
    },
}

impl Schema {
    /// Validate `attributes` against the schema-header rule and wrap.
    ///
    /// # Errors
    ///
    /// Returns `Cardinality` if the list length is outside
    /// `1..=MAX_SCHEMA_ATTRIBUTES`, or `DuplicateAttribute` if two
    /// entries share a name.
    #[inline]
    pub fn try_new(attributes: Vec<Attribute>) -> Result<Self, SchemaError> {
        Refined::try_new(attributes)
            .map(Self)
            .map_err(|err| match err {
                CollectionError::LenOutOfRange { actual } => SchemaError::Cardinality { actual },
                CollectionError::DuplicateKey { index } => {
                    SchemaError::DuplicateAttribute { index }
                }
                // The underlying composition only produces the two
                // variants matched above; the remaining `CollectionError`
                // variants belong to rules not used here.
                CollectionError::BadItem { .. }
                | CollectionError::MatchingItem { .. }
                | CollectionError::NoMatchingItem
                | CollectionError::NotSorted { .. } => {
                    unreachable!("SchemaHeaderRule emits only LenOutOfRange / DuplicateKey")
                }
            })
    }

    /// Borrow the attribute list.
    #[must_use]
    #[inline]
    pub const fn attributes(&self) -> &[Attribute] {
        self.0.as_inner().as_slice()
    }

    /// Number of attributes in the schema. Never zero — the rule's
    /// lower bound makes the empty header unrepresentable.
    #[must_use]
    #[inline]
    pub const fn cardinality(&self) -> usize {
        self.0.as_inner().len()
    }

    /// Consume the wrapper and return the inner attribute list.
    #[must_use]
    #[inline]
    pub fn into_inner(self) -> Vec<Attribute> {
        self.0.into_inner()
    }

    /// Look up an attribute by name. `None` if absent.
    #[must_use]
    pub fn find(&self, name: &AttributeName) -> Option<&Attribute> {
        self.attributes()
            .iter()
            .find(|attribute| &attribute.name == name)
    }

    /// `true` iff an attribute with this name is in the schema.
    #[must_use]
    pub fn contains(&self, name: &AttributeName) -> bool {
        self.find(name).is_some()
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

    use super::{Attribute, Schema, SchemaCardinality, SchemaCardinalityError, SchemaError};
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
        // `Within` exposes a flat domain `NumericError` aliased
        // here as `SchemaCardinalityError`; no composition
        // machinery leaks through the surface.
        let err: SchemaCardinalityError = result.unwrap_err();
        assert!(matches!(
            err,
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
        let s = Schema::try_new(vec![attr("id", Type::Int64), attr("name", Type::String)]).unwrap();
        assert_eq!(s.cardinality(), 2);
    }

    #[test]
    fn empty_schema_rejected_by_length_check() {
        let result = Schema::try_new(Vec::new());
        assert_eq!(result.unwrap_err(), SchemaError::Cardinality { actual: 0 },);
    }

    #[test]
    fn duplicate_attribute_name_rejected() {
        let result = Schema::try_new(vec![attr("id", Type::Int64), attr("id", Type::String)]);
        assert_eq!(
            result.unwrap_err(),
            SchemaError::DuplicateAttribute { index: 1 },
        );
    }
}
