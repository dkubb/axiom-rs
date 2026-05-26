//! `Schema` and `Attribute`.
//!
//! A schema's header is a `Vec<Attribute>` refined by whittle's
//! `LenItems<1, MAX_SCHEMA_ATTRIBUTES>`. Per-attribute-name
//! uniqueness is enforced by `Schema::try_new` after the whittle
//! length-check passes; the uniqueness primitive
//! (`UniqueByKey<AttributeName>`) lands in whittle in a later
//! commit, at which point this manual check can be lifted into the
//! refinement chain itself.

use alloc::collections::BTreeSet;
use alloc::vec::Vec;

use thiserror::Error;
use whittle::primitive::{CollectionError, LenItems, NumericError, Within};
use whittle::Refined;

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

/// Relation header: bounded ordered list of `Attribute`s with
/// unique attribute names. Whittle enforces the length bound;
/// uniqueness is enforced by `try_new` until a `UniqueByKey`
/// primitive lands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Schema {
    attributes:
        Refined<Vec<Attribute>, LenItems<1, { MAX_SCHEMA_ATTRIBUTES }>>,
}

/// Constructor error for `Schema`.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum SchemaError {
    /// Underlying length-bound rejection from whittle.
    #[error("{0}")]
    Length(#[source] CollectionError),
    /// Two attributes share the same name. Uniqueness is enforced
    /// by `try_new`; this variant will go away once whittle's
    /// `UniqueByKey<AttributeName>` lands.
    #[error("duplicate attribute name: {name}")]
    DuplicateAttribute {
        /// The repeated name.
        name: AttributeName,
    },
}

impl From<CollectionError> for SchemaError {
    fn from(err: CollectionError) -> Self {
        Self::Length(err)
    }
}

impl Schema {
    /// Validate `raw` and wrap.
    ///
    /// # Errors
    ///
    /// Returns `SchemaError::Length` when `raw` is empty or exceeds
    /// `MAX_SCHEMA_ATTRIBUTES`. Returns
    /// `SchemaError::DuplicateAttribute` when two attributes share
    /// the same name.
    #[inline]
    pub fn try_new(raw: Vec<Attribute>) -> Result<Self, SchemaError> {
        // Run whittle's length check first so an empty/over-length
        // attribute list rejects before the uniqueness pass.
        let attributes = Refined::try_new(raw)?;
        // Uniqueness pass — pulled out of whittle for now.
        let mut seen: BTreeSet<&AttributeName> = BTreeSet::new();
        for attr in attributes.as_inner() {
            if !seen.insert(&attr.name) {
                return Err(SchemaError::DuplicateAttribute {
                    name: attr.name.clone(),
                });
            }
        }
        Ok(Self { attributes })
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

    use super::{
        Attribute, Schema, SchemaCardinality, SchemaCardinalityError,
        SchemaError,
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
        assert!(matches!(result.unwrap_err(), SchemaError::Length(_)));
    }

    #[test]
    fn duplicate_attribute_name_rejected() {
        let result = Schema::try_new(vec![
            attr("id", Type::Int64),
            attr("id", Type::String),
        ]);
        let err = result.unwrap_err();
        let SchemaError::DuplicateAttribute { name } = err else {
            unreachable!("expected DuplicateAttribute, got {err:?}");
        };
        assert_eq!(name.as_str(), "id");
    }
}
