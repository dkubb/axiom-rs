//! `CanonicalPredicate`: a `BoolExpression` narrowed to canonical
//! form suitable for use as a constraint.
//!
//! The full canonicalisation pipeline (constant folding, De Morgan,
//! commutative-operand sorting, …) lands incrementally. V0 enforces
//! the two contract invariants that are load-bearing today:
//! - Bool-typed against the input schema (via `BoolExpression`).
//! - No `Expression::Agg` anywhere in the tree — aggregates are
//!   admissible only inside `Summarize`.

use alloc::collections::BTreeSet;
use alloc::vec::Vec;

use thiserror::Error;

use crate::expression::{BoolExpression, Expression, contains_aggregate};
use crate::identifier::AttributeName;
use crate::infer::InferError;
use crate::schema::Schema;

/// Boolean expression narrowed to canonical form.
///
/// Construction goes through `try_new(schema, expr)`: the
/// `BoolExpression` proof carries Bool-typing against the schema;
/// `CanonicalPredicate` adds the aggregate-free constraint on top.
/// The inner expression is private so a `CanonicalPredicate` cannot
/// be fabricated.
#[derive(Debug, Clone, PartialEq)]
pub struct CanonicalPredicate {
    inner: BoolExpression,
}

/// Construction error for `CanonicalPredicate`.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum CanonicalPredicateError {
    /// Expression failed Bool-typing against the schema.
    #[error("{0}")]
    Infer(#[source] InferError),

    /// Expression contained an aggregate. Aggregates may only
    /// appear inside `Summarize`, not in a predicate or constraint.
    #[error("aggregate not admissible inside a canonical predicate")]
    ContainsAggregate,
}

impl CanonicalPredicate {
    /// Narrow `expr` into a canonical predicate against `schema`.
    ///
    /// # Errors
    ///
    /// Returns `Infer` if the expression fails Bool-typing, and
    /// `ContainsAggregate` if it contains an `Agg` sub-expression.
    pub fn try_new(schema: &Schema, expr: Expression) -> Result<Self, CanonicalPredicateError> {
        if contains_aggregate(&expr) {
            return Err(CanonicalPredicateError::ContainsAggregate);
        }
        let inner =
            BoolExpression::try_new(schema, expr).map_err(CanonicalPredicateError::Infer)?;
        Ok(Self { inner })
    }

    /// Borrow the underlying Bool-typed expression.
    #[must_use]
    #[inline]
    pub const fn as_bool_expression(&self) -> &BoolExpression {
        &self.inner
    }

    /// Borrow the underlying raw expression for matching.
    #[must_use]
    #[inline]
    pub const fn as_expression(&self) -> &Expression {
        self.inner.as_expression()
    }

    /// Collect every attribute referenced (directly or transitively)
    /// by this predicate.
    #[must_use]
    pub fn free_attributes(&self) -> BTreeSet<AttributeName> {
        let mut out = BTreeSet::new();
        free_attrs_into(self.inner.as_expression(), &mut out);
        out
    }
}

fn free_attrs_into(expr: &Expression, out: &mut BTreeSet<AttributeName>) {
    match expr {
        Expression::Attr(name) => {
            out.insert(name.clone());
        }
        Expression::Lit(_) => {}
        Expression::BinOp(_, lhs, rhs) => {
            free_attrs_into(lhs, out);
            free_attrs_into(rhs, out);
        }
        Expression::UnOp(_, operand)
        | Expression::Like(operand, _)
        | Expression::IsNull(operand)
        | Expression::Cast(operand, _)
        | Expression::InList(operand, _) => free_attrs_into(operand, out),
        Expression::Agg(agg) => {
            // Aggregates are forbidden inside a CanonicalPredicate;
            // this branch is structurally unreachable from
            // try_from_expression but we cover it here so the
            // free-attribute walk is total over Expression.
            let names = aggregate_attribute_names(agg);
            for name in names {
                out.insert(name);
            }
        }
    }
}

fn aggregate_attribute_names(agg: &crate::op_enums::Agg) -> Vec<AttributeName> {
    use crate::op_enums::Agg;
    match agg {
        Agg::Count(maybe) => maybe.iter().cloned().collect(),
        Agg::Sum(name) | Agg::Min(name) | Agg::Max(name) | Agg::Avg(name) => {
            alloc::vec![name.clone()]
        }
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

    use super::{CanonicalPredicate, CanonicalPredicateError};
    use crate::expression::Expression;
    use crate::identifier::AttributeName;
    use crate::infer::InferError;
    use crate::op_enums::{Agg, BinOp};
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
            Attribute {
                name: attr("amount"),
                ty: Type::Int32,
            },
            Attribute {
                name: attr("x"),
                ty: Type::Int32,
            },
        ])
        .unwrap()
    }

    #[test]
    fn literal_predicate_admissible() {
        let p = CanonicalPredicate::try_new(&person_schema(), Expression::Lit(Value::Bool(true)))
            .unwrap();
        let Expression::Lit(Value::Bool(true)) = p.as_expression() else {
            unreachable!();
        };
    }

    #[test]
    fn comparison_predicate_admissible() {
        let expr = Expression::BinOp(
            BinOp::Ge,
            Box::new(Expression::Attr(attr("age"))),
            Box::new(Expression::Lit(Value::Int32(18))),
        );
        CanonicalPredicate::try_new(&person_schema(), expr).unwrap();
    }

    #[test]
    fn non_bool_expression_rejected() {
        let result =
            CanonicalPredicate::try_new(&person_schema(), Expression::Lit(Value::Int32(0)));
        assert!(matches!(
            result.unwrap_err(),
            CanonicalPredicateError::Infer(InferError::TypeMismatch {
                expected: Type::Bool,
                got: Type::Int32,
            }),
        ));
    }

    #[test]
    fn aggregate_predicate_rejected() {
        let expr = Expression::Agg(Agg::Sum(attr("amount")));
        let result = CanonicalPredicate::try_new(&person_schema(), expr);
        assert_eq!(
            result.unwrap_err(),
            CanonicalPredicateError::ContainsAggregate,
        );
    }

    #[test]
    fn aggregate_inside_binop_rejected() {
        let expr = Expression::BinOp(
            BinOp::Gt,
            Box::new(Expression::Agg(Agg::Sum(attr("x")))),
            Box::new(Expression::Lit(Value::Int32(0))),
        );
        let result = CanonicalPredicate::try_new(&person_schema(), expr);
        assert_eq!(
            result.unwrap_err(),
            CanonicalPredicateError::ContainsAggregate,
        );
    }

    #[test]
    fn free_attributes_collects_referenced_names() {
        let expr = Expression::BinOp(
            BinOp::And,
            Box::new(Expression::BinOp(
                BinOp::Ge,
                Box::new(Expression::Attr(attr("age"))),
                Box::new(Expression::Lit(Value::Int32(18))),
            )),
            Box::new(Expression::BinOp(
                BinOp::Lt,
                Box::new(Expression::Attr(attr("age"))),
                Box::new(Expression::Attr(attr("retire_at"))),
            )),
        );
        let p = CanonicalPredicate::try_new(&person_schema(), expr).unwrap();
        let attrs = p.free_attributes();
        assert_eq!(attrs.len(), 2);
        assert!(attrs.contains(&attr("age")));
        assert!(attrs.contains(&attr("retire_at")));
    }
}
