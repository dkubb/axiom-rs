//! Closed-sum companion enums for the operator AST.
//!
//! These are pure algebraic data types — the type-level discipline
//! already minimises their state space, so they need no
//! refinement. They live here so other modules (`Op`, `Expression`,
//! `Schema`) can reference them.

use crate::identifier::AttributeName;

/// Aggregate function over a single named attribute.
///
/// `Count(None)` denotes `COUNT(*)`; `Count(Some(attr))` denotes
/// `COUNT(attr)` (which counts non-null values of `attr`). All
/// other variants name the attribute they aggregate.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Agg {
    /// `COUNT(*)` when `None`, `COUNT(attr)` when `Some`.
    Count(Option<AttributeName>),
    /// `SUM(attr)`.
    Sum(AttributeName),
    /// `MIN(attr)`.
    Min(AttributeName),
    /// `MAX(attr)`.
    Max(AttributeName),
    /// `AVG(attr)`.
    Avg(AttributeName),
}

/// Named aggregate: the output attribute name plus the aggregate
/// function. Used by `Summarize` operator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NamedAgg {
    /// Output attribute name.
    pub name: AttributeName,
    /// Aggregate function.
    pub agg: Agg,
}

/// Binary operators in the expression algebra.
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
#[non_exhaustive]
pub enum BinOp {
    // Arithmetic.
    /// `a + b`.
    Add,
    /// `a - b`.
    Sub,
    /// `a * b`.
    Mul,
    /// `a / b`.
    Div,
    // Comparison.
    /// `a = b`.
    Eq,
    /// `a != b`.
    Ne,
    /// `a < b`.
    Lt,
    /// `a <= b`.
    Le,
    /// `a > b`.
    Gt,
    /// `a >= b`.
    Ge,
    // Logical.
    /// `a AND b`.
    And,
    /// `a OR b`.
    Or,
    // String.
    /// `a || b` (string concatenation).
    Concat,
}

/// Unary operators in the expression algebra.
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
#[non_exhaustive]
pub enum UnOp {
    /// `-a` (arithmetic negation).
    Neg,
    /// `NOT a` (boolean negation).
    Not,
}

/// Join kind. Natural-join column extraction lives on `JoinOn`, not
/// here, so each variant is just an outerness/innerness tag.
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
#[non_exhaustive]
pub enum JoinKind {
    /// Inner join.
    Inner,
    /// Left outer join.
    LeftOuter,
    /// Right outer join.
    RightOuter,
    /// Full outer join.
    FullOuter,
}
