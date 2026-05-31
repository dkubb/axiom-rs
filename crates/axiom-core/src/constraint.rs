//! Constraints: predicates that must hold for every row of a
//! relation.
//!
//! Per docs/ARCHITECTURE.md §10, a constraint is a
//! `CanonicalPredicate` plus a scope: either attached to a single
//! attribute (`AttributeConstraint`) or applying to the whole row
//! (`RowConstraint`). A `ConstraintSet` bundles both kinds and is
//! the per-relation invariant store the optimiser consults during
//! predicate push-down, join planning, and pruning.

use alloc::vec::Vec;
use core::marker::PhantomData;

use thiserror::Error;
use whittle::primitive::{CollectionError, KeyOf, LenItems, UniqueByKey};
use whittle::{And, Refined};

use crate::canonical::CanonicalPredicate;
use crate::identifier::AttributeName;
use crate::limits::MAX_SCHEMA_ATTRIBUTES;

/// A constraint over a single attribute: `predicate` must hold for
/// the value of `attr` on every admissible row.
///
/// Invariant: `predicate.free_attributes() == {attr}` — exactly the
/// scoped attribute, no more, no less. Constant predicates (empty
/// free set) belong in a row-level constraint or should be folded
/// away; foreign references mean the constraint is mis-scoped.
#[derive(Debug, Clone, PartialEq)]
pub struct AttributeConstraint {
    attr: AttributeName,
    predicate: CanonicalPredicate,
}

/// Construction error for `AttributeConstraint`.
#[derive(Debug, Error, PartialEq, Eq)]
#[non_exhaustive]
pub enum AttributeConstraintError {
    /// Predicate mentions an attribute other than the scoped one.
    #[error("attribute constraint on `{scope}` referenced foreign attribute `{foreign}`")]
    ForeignAttribute {
        /// The attribute this constraint is scoped to.
        scope: AttributeName,
        /// The first attribute name that escaped the scope.
        foreign: AttributeName,
    },
    /// Predicate references no attributes — it's a constant
    /// predicate and does not belong on a per-attribute scope.
    #[error("attribute constraint on `{scope}` has constant predicate (no free attributes)")]
    ConstantPredicate {
        /// The attribute the constant predicate was supposed to scope.
        scope: AttributeName,
    },
    /// Predicate does not reference the scoped attribute.
    #[error("attribute constraint on `{scope}` does not reference its own attribute")]
    MissingScope {
        /// The attribute the constraint was supposed to scope.
        scope: AttributeName,
    },
}

impl AttributeConstraint {
    /// Build a per-attribute constraint, enforcing
    /// `predicate.free_attributes() == {attr}`.
    ///
    /// # Errors
    ///
    /// - `ForeignAttribute` if the predicate references any
    ///   attribute other than `attr`.
    /// - `MissingScope` if the predicate does not reference `attr`.
    /// - `ConstantPredicate` if the predicate references no
    ///   attributes at all.
    pub fn try_new(
        attr: AttributeName,
        predicate: CanonicalPredicate,
    ) -> Result<Self, AttributeConstraintError> {
        let free = predicate.free_attributes();
        if free.is_empty() {
            return Err(AttributeConstraintError::ConstantPredicate { scope: attr });
        }
        if !free.contains(&attr) {
            return Err(AttributeConstraintError::MissingScope { scope: attr });
        }
        for name in &free {
            if name != &attr {
                return Err(AttributeConstraintError::ForeignAttribute {
                    scope: attr,
                    foreign: name.clone(),
                });
            }
        }
        Ok(Self { attr, predicate })
    }

    /// Borrow the attribute this constraint is scoped to.
    #[must_use]
    #[inline]
    pub const fn attribute(&self) -> &AttributeName {
        &self.attr
    }

    /// Borrow the predicate.
    #[must_use]
    #[inline]
    pub const fn predicate(&self) -> &CanonicalPredicate {
        &self.predicate
    }
}

/// A constraint over an entire row: `predicate` must hold across
/// any combination of attributes in the schema. No scoping check
/// applies — the predicate may reference any attribute.
#[derive(Debug, Clone, PartialEq)]
pub struct RowConstraint {
    predicate: CanonicalPredicate,
}

impl RowConstraint {
    /// Wrap a predicate as a row-level constraint.
    #[must_use]
    #[inline]
    pub const fn new(predicate: CanonicalPredicate) -> Self {
        Self { predicate }
    }

    /// Borrow the underlying predicate.
    #[must_use]
    #[inline]
    pub const fn predicate(&self) -> &CanonicalPredicate {
        &self.predicate
    }
}

/// Key extractor: per-attribute constraint uniqueness is on the
/// scoped attribute name. Two `AttributeConstraint`s on the same
/// attribute are redundant — a future commit will conjoin them
/// into a single `CanonicalPredicate` for that attribute, but
/// admitting two distinct entries is a state with no contractual
/// meaning today.
pub struct AttributeConstraintAttr(PhantomData<()>);

impl KeyOf<AttributeConstraint> for AttributeConstraintAttr {
    type Key = AttributeName;
    fn key_of(value: &AttributeConstraint) -> AttributeName {
        value.attr.clone()
    }
}

// Bounded by `MAX_SCHEMA_ATTRIBUTES` for symmetry with the schema —
// no relation can practically need more per-attribute constraints
// than it has columns. Per-attribute uniqueness on the scoped
// attribute name closes the duplicate-entry state.
type AttrConstraintsRule = And<
    LenItems<0, { MAX_SCHEMA_ATTRIBUTES }>,
    UniqueByKey<AttributeConstraint, AttributeConstraintAttr>,
>;
type RowConstraintsRule = LenItems<0, { MAX_SCHEMA_ATTRIBUTES }>;

/// Set of constraints attached to a relation.
///
/// Constraints fall into two scopes: per-attribute (validated to
/// reference only their own attribute) and per-row (free to
/// reference anything in the schema). Both vectors are bounded but
/// admit the empty set (the default for an unconstrained relation).
#[derive(Debug, Clone, PartialEq)]
pub struct ConstraintSet {
    per_attr: Refined<Vec<AttributeConstraint>, AttrConstraintsRule>,
    per_row: Refined<Vec<RowConstraint>, RowConstraintsRule>,
}

/// Constructor error for `ConstraintSet`.
#[derive(Debug, Error, PartialEq, Eq)]
#[non_exhaustive]
pub enum ConstraintSetError {
    /// Per-attribute constraint vector violated its length bound.
    #[error("per-attribute constraint list: length: {0}")]
    PerAttributeLength(#[source] CollectionError),

    /// Per-attribute constraint vector contains two entries on the
    /// same attribute.
    #[error("per-attribute constraint list: duplicate: {0}")]
    PerAttributeDuplicate(#[source] CollectionError),

    /// Per-row constraint vector violated its length bound.
    #[error("per-row constraint list: {0}")]
    PerRow(#[source] CollectionError),
}

impl From<CollectionError> for ConstraintSetError {
    fn from(err: CollectionError) -> Self {
        // Both inner rules of `AttrConstraintsRule` report through
        // `CollectionError`, so the composition's error is the flat
        // enum: a length failure surfaces as `LenOutOfRange`, a
        // per-attribute uniqueness failure as `DuplicateKey`.
        match err {
            CollectionError::LenOutOfRange { .. } => Self::PerAttributeLength(err),
            CollectionError::DuplicateKey { .. } => Self::PerAttributeDuplicate(err),
            _ => unreachable!("AttrConstraintsRule emits only LenOutOfRange / DuplicateKey"),
        }
    }
}

impl ConstraintSet {
    /// Build a constraint set from the two scope vectors.
    ///
    /// # Errors
    ///
    /// Returns `PerAttributeLength` or `PerRow` for length bound
    /// violations, and `PerAttributeDuplicate` if two
    /// per-attribute constraints scope the same attribute.
    pub fn try_new(
        per_attr: Vec<AttributeConstraint>,
        per_row: Vec<RowConstraint>,
    ) -> Result<Self, ConstraintSetError> {
        let per_attr = Refined::try_new(per_attr)?;
        let per_row = Refined::try_new(per_row).map_err(ConstraintSetError::PerRow)?;
        Ok(Self { per_attr, per_row })
    }

    /// The empty constraint set. Used as the default when a source
    /// declares no invariants.
    #[must_use]
    pub fn empty() -> Self {
        Self::try_new(Vec::new(), Vec::new()).unwrap_or_else(|_| unreachable!("0 fits in 0..MAX"))
    }

    /// Per-attribute constraints.
    #[must_use]
    #[inline]
    pub const fn per_attribute(&self) -> &[AttributeConstraint] {
        self.per_attr.as_inner().as_slice()
    }

    /// Per-row constraints.
    #[must_use]
    #[inline]
    pub const fn per_row(&self) -> &[RowConstraint] {
        self.per_row.as_inner().as_slice()
    }

    /// `true` iff both constraint lists are empty.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.per_attribute().is_empty() && self.per_row().is_empty()
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "explicit in test code"
)]
mod tests {
    use alloc::boxed::Box;
    use alloc::string::ToString;
    use alloc::vec;

    use super::{
        AttributeConstraint, AttributeConstraintError, ConstraintSet, ConstraintSetError,
        RowConstraint,
    };
    use crate::canonical::CanonicalPredicate;
    use crate::expression::Expression;
    use crate::identifier::AttributeName;
    use crate::op_enums::BinOp;
    use crate::schema::{Attribute, Schema};
    use crate::ty::{Type, Value};

    fn attr(name: &str) -> AttributeName {
        AttributeName::try_new(name.to_string()).unwrap()
    }

    fn person_schema() -> Schema {
        Schema::try_new(vec![
            Attribute {
                name: attr("age"),
                ty: Type::Int32,
            },
            Attribute {
                name: attr("retire_at"),
                ty: Type::Int32,
            },
        ])
        .unwrap()
    }

    fn pred_age_ge_18() -> CanonicalPredicate {
        CanonicalPredicate::try_new(
            &person_schema(),
            Expression::BinOp(
                BinOp::Ge,
                Box::new(Expression::Attr(attr("age"))),
                Box::new(Expression::Lit(Value::Int32(18))),
            ),
        )
        .unwrap()
    }

    fn pred_age_lt_retire() -> CanonicalPredicate {
        CanonicalPredicate::try_new(
            &person_schema(),
            Expression::BinOp(
                BinOp::Lt,
                Box::new(Expression::Attr(attr("age"))),
                Box::new(Expression::Attr(attr("retire_at"))),
            ),
        )
        .unwrap()
    }

    #[test]
    fn attribute_constraint_accepts_matching_scope() {
        let c = AttributeConstraint::try_new(attr("age"), pred_age_ge_18()).unwrap();
        assert_eq!(c.attribute().as_str(), "age");
    }

    #[test]
    fn attribute_constraint_rejects_foreign_reference() {
        let result = AttributeConstraint::try_new(attr("age"), pred_age_lt_retire());
        assert!(matches!(
            result.unwrap_err(),
            AttributeConstraintError::ForeignAttribute { .. },
        ));
    }

    #[test]
    fn attribute_constraint_rejects_constant_predicate() {
        let pred =
            CanonicalPredicate::try_new(&person_schema(), Expression::Lit(Value::Bool(true)))
                .unwrap();
        let result = AttributeConstraint::try_new(attr("age"), pred);
        assert!(matches!(
            result.unwrap_err(),
            AttributeConstraintError::ConstantPredicate { .. },
        ));
    }

    #[test]
    fn attribute_constraint_rejects_missing_scope() {
        // Predicate references retire_at only; scope is age.
        let pred = CanonicalPredicate::try_new(
            &person_schema(),
            Expression::BinOp(
                BinOp::Lt,
                Box::new(Expression::Attr(attr("retire_at"))),
                Box::new(Expression::Lit(Value::Int32(65))),
            ),
        )
        .unwrap();
        let result = AttributeConstraint::try_new(attr("age"), pred);
        assert!(matches!(
            result.unwrap_err(),
            AttributeConstraintError::MissingScope { .. },
        ));
    }

    #[test]
    fn row_constraint_accepts_multi_attr_predicate() {
        let rc = RowConstraint::new(pred_age_lt_retire());
        let Expression::BinOp(_, _, _) = rc.predicate().as_expression() else {
            unreachable!();
        };
    }

    #[test]
    fn empty_constraint_set_is_empty() {
        let cs = ConstraintSet::empty();
        assert!(cs.is_empty());
        assert!(cs.per_attribute().is_empty());
        assert!(cs.per_row().is_empty());
    }

    #[test]
    fn constraint_set_holds_both_scopes() {
        let attr_c = AttributeConstraint::try_new(attr("age"), pred_age_ge_18()).unwrap();
        let row_c = RowConstraint::new(pred_age_lt_retire());
        let cs = ConstraintSet::try_new(vec![attr_c], vec![row_c]).unwrap();
        assert!(!cs.is_empty());
        assert_eq!(cs.per_attribute().len(), 1);
        assert_eq!(cs.per_row().len(), 1);
    }

    #[test]
    fn constraint_set_rejects_duplicate_attribute_constraints() {
        let c1 = AttributeConstraint::try_new(attr("age"), pred_age_ge_18()).unwrap();
        let c2 = AttributeConstraint::try_new(attr("age"), pred_age_ge_18()).unwrap();
        let result = ConstraintSet::try_new(vec![c1, c2], vec![]);
        assert!(matches!(
            result.unwrap_err(),
            ConstraintSetError::PerAttributeDuplicate(_),
        ));
    }
}
