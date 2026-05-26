//! Expression algebra and `Predicate` boundary.
//!
//! `Expression` is the pure algebra: attribute reference, literal,
//! `BinOp`, `UnOp`, and so on. `Predicate` is the layer at which the
//! opaque escape hatch lives — `Predicate::Opaque(OpaqueId)` carries
//! a closure registered with an `ExecutionContext` (introduced when
//! the in-memory backend lands).
//!
//! Per docs/ARCHITECTURE.md §11.1, opacity is at the `Predicate`
//! boundary, not in `Expression`. Opaque operands to `Cast`, `Agg`,
//! `InList`, arithmetic, or `Extend` are unrepresentable.

use alloc::boxed::Box;
use alloc::vec::Vec;

use crate::identifier::{AttributeName, Pattern};
use crate::op_enums::{Agg, BinOp, UnOp};
use crate::ty::{Type, Value};

/// Boolean-or-other-typed expression. Refinement to `Type::Bool` is
/// the job of `Predicate`'s constructor; this enum is the
/// untyped-at-construction algebra that the optimizer rewrites.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum Expression {
    /// Reference to a schema attribute by name.
    Attr(AttributeName),
    /// Literal value.
    Lit(Value),
    /// Binary operation.
    BinOp(BinOp, Box<Self>, Box<Self>),
    /// Unary operation.
    UnOp(UnOp, Box<Self>),
    /// `lhs LIKE pattern`.
    Like(Box<Self>, Pattern),
    /// `expr IN (literals...)`.
    InList(Box<Self>, Vec<Value>),
    /// `expr IS NULL`.
    IsNull(Box<Self>),
    /// `CAST(expr AS ty)`.
    Cast(Box<Self>, Type),
    /// Aggregate function (only admissible inside summarisation).
    Agg(Agg),
}

/// Opaque-predicate registry id.
///
/// An `OpaqueId` is minted by `axiom-mem::ExecutionContext::restrict_with`
/// (which lands when the in-memory backend lands) and carries no
/// public constructor. Holding one means a closure has been
/// registered against a specific execution context; the closure
/// itself is consumed only during evaluation.
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct OpaqueId(u32);

impl OpaqueId {
    /// Constructor for backend code. Kept `pub(crate)` until the
    /// backend crate lands; external callers cannot mint an
    /// `OpaqueId`, so the only way `Predicate::Opaque` reaches the
    /// AST is through `axiom-mem`'s `restrict_with`.
    #[must_use]
    #[inline]
    #[cfg_attr(not(test), allow(dead_code, reason = "in use once backend lands"))]
    pub(crate) const fn new(raw: u32) -> Self {
        Self(raw)
    }

    /// Read the underlying id, for diagnostics.
    #[must_use]
    #[inline]
    pub const fn get(self) -> u32 {
        self.0
    }
}

/// A predicate. Either a Bool-typed expression or an opaque
/// closure-backed predicate registered with an execution context.
///
/// Predicate's `Expr` arm holds an `Expression` whose static type
/// must be Bool; the type check belongs to `Predicate::try_new`,
/// which lands when type inference is implemented.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum Predicate {
    /// Boolean expression.
    Expr(Expression),
    /// Opaque closure-backed predicate. Only constructible through
    /// the in-memory backend's `restrict_with` API.
    Opaque(OpaqueId),
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used,
        reason = "explicit in test code")]
mod tests {
    use alloc::boxed::Box;
    use alloc::string::ToString;

    use super::{Expression, OpaqueId, Predicate};
    use crate::identifier::AttributeName;
    use crate::op_enums::BinOp;
    use crate::ty::Value;

    #[test]
    fn expression_attr_and_literal() {
        let attr = AttributeName::try_new("age".to_string()).unwrap();
        let expr = Expression::BinOp(
            BinOp::Ge,
            Box::new(Expression::Attr(attr)),
            Box::new(Expression::Lit(Value::Int32(18))),
        );
        let Expression::BinOp(BinOp::Ge, ..) = expr else {
            unreachable!();
        };
    }

    #[test]
    fn predicate_expr_round_trip() {
        let p = Predicate::Expr(Expression::Lit(Value::Bool(true)));
        let Predicate::Expr(_) = p else { unreachable!() };
    }

    #[test]
    fn predicate_opaque_carries_id() {
        // OpaqueId::new is crate-private; this test uses it because
        // the test module is inside the same crate.
        let id = OpaqueId::new(42);
        let p = Predicate::Opaque(id);
        let Predicate::Opaque(got) = p else { unreachable!() };
        assert_eq!(got.get(), 42);
    }
}
