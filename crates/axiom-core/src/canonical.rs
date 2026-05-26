//! `CanonicalPredicate`: an `Expression` narrowed to canonical form
//! suitable for use as a constraint.
//!
//! The full canonicalisation pipeline (constant folding, De Morgan,
//! commutative-operand sorting, …) lands incrementally. The v0
//! invariant is the only one that's load-bearing for constraints:
//! an `Expression::Agg` is not admissible as a constraint because
//! aggregates can only appear inside `Summarize`.

use alloc::collections::BTreeSet;
use alloc::vec::Vec;

use thiserror::Error;

use crate::expression::Expression;
use crate::identifier::AttributeName;

/// Boolean expression narrowed to canonical form.
///
/// Construction goes through `try_from_expression`. The inner
/// `Expression` is private so the only path to a `CanonicalPredicate`
/// is through the narrowing morphism.
#[derive(Debug, Clone, PartialEq)]
pub struct CanonicalPredicate {
    inner: Expression,
}

/// Construction error for `CanonicalPredicate`.
#[derive(Debug, Error, PartialEq, Eq)]
#[non_exhaustive]
pub enum CanonicalPredicateError {
    /// Expression contained an aggregate. Aggregates may only
    /// appear inside `Summarize`, not in a predicate or constraint.
    #[error("aggregate not admissible inside a canonical predicate")]
    ContainsAggregate,
}

impl CanonicalPredicate {
    /// Narrow `expr` into a canonical predicate.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalPredicateError::ContainsAggregate` when
    /// `expr` (or any sub-expression) contains an `Agg` variant.
    #[inline]
    pub fn try_from_expression(
        expr: Expression,
    ) -> Result<Self, CanonicalPredicateError> {
        if contains_aggregate(&expr) {
            return Err(CanonicalPredicateError::ContainsAggregate);
        }
        Ok(Self { inner: expr })
    }

    /// Borrow the underlying expression.
    #[must_use]
    #[inline]
    pub const fn as_expression(&self) -> &Expression {
        &self.inner
    }

    /// Collect every attribute referenced (directly or transitively)
    /// by this predicate.
    #[must_use]
    pub fn free_attributes(&self) -> BTreeSet<AttributeName> {
        let mut out = BTreeSet::new();
        free_attrs_into(&self.inner, &mut out);
        out
    }
}

fn contains_aggregate(expr: &Expression) -> bool {
    match expr {
        Expression::Attr(_) | Expression::Lit(_) => false,
        Expression::BinOp(_, lhs, rhs) => {
            contains_aggregate(lhs) || contains_aggregate(rhs)
        }
        Expression::UnOp(_, operand)
        | Expression::Like(operand, _)
        | Expression::IsNull(operand)
        | Expression::Cast(operand, _)
        | Expression::InList(operand, _) => contains_aggregate(operand),
        Expression::Agg(_) => true,
    }
}

fn free_attrs_into(
    expr: &Expression,
    out: &mut BTreeSet<AttributeName>,
) {
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

fn aggregate_attribute_names(
    agg: &crate::op_enums::Agg,
) -> Vec<AttributeName> {
    use crate::op_enums::Agg;
    match agg {
        Agg::Count(maybe) => maybe.iter().cloned().collect(),
        Agg::Sum(name)
        | Agg::Min(name)
        | Agg::Max(name)
        | Agg::Avg(name) => alloc::vec![name.clone()],
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used,
        reason = "explicit in test code")]
mod tests {
    use alloc::boxed::Box;
    use alloc::string::ToString;

    use super::{CanonicalPredicate, CanonicalPredicateError};
    use crate::expression::Expression;
    use crate::identifier::AttributeName;
    use crate::op_enums::{Agg, BinOp};
    use crate::ty::Value;

    fn attr(name: &str) -> AttributeName {
        AttributeName::try_new(name.to_string()).unwrap()
    }

    #[test]
    fn literal_predicate_admissible() {
        let p = CanonicalPredicate::try_from_expression(
            Expression::Lit(Value::Bool(true)),
        )
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
        CanonicalPredicate::try_from_expression(expr).unwrap();
    }

    #[test]
    fn aggregate_predicate_rejected() {
        let expr = Expression::Agg(Agg::Sum(attr("amount")));
        let result = CanonicalPredicate::try_from_expression(expr);
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
        let result = CanonicalPredicate::try_from_expression(expr);
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
        let p = CanonicalPredicate::try_from_expression(expr).unwrap();
        let attrs = p.free_attributes();
        assert_eq!(attrs.len(), 2);
        assert!(attrs.contains(&attr("age")));
        assert!(attrs.contains(&attr("retire_at")));
    }
}
