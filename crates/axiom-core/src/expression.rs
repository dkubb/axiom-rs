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

use whittle::primitive::{CollectionError, LenItems};
use whittle::Refined;

use crate::identifier::{AttributeName, Pattern};
use crate::limits::MAX_IN_LIST;
use crate::op_enums::{Agg, BinOp, UnOp};
use crate::ty::{Type, Value};

type InListValuesRule = LenItems<1, { MAX_IN_LIST }>;

/// Length-bounded list of literals for `Expression::InList`.
///
/// Length is `1..=MAX_IN_LIST`. Duplicate literals are NOT yet
/// rejected at construction (e.g. `x IN (1, 1)` is representable
/// alongside `x IN (1)`); enforcing it requires `Value: Ord`,
/// which is non-trivial across `Float64` / `Array` / `Relation` /
/// `Optional` and lands when those variants get canonical
/// comparison semantics. The architecture's
/// `BoundedOrderedSet<Value, 1, MAX_IN_LIST>` shape is the target.
#[derive(Debug, Clone, PartialEq)]
pub struct InListValues(Refined<Vec<Value>, InListValuesRule>);

/// Constructor error for `InListValues`.
///
/// The underlying rule is a single `LenItems<1, MAX_IN_LIST>`, so
/// the only failure mode is a length-bound violation. The flat
/// variant gives call sites a domain-named match target without
/// having to thread `CollectionError` through.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[non_exhaustive]
pub enum InListValuesError {
    /// Value count fell outside `1..=MAX_IN_LIST`.
    #[error("in-list value count out of range (actual: {actual})")]
    ValueCount {
        /// Observed value count.
        actual: usize,
    },
}

impl InListValues {
    /// Validate `values` against the in-list rule and wrap.
    ///
    /// # Errors
    ///
    /// Returns `ValueCount` if the list length is outside
    /// `1..=MAX_IN_LIST`.
    #[inline]
    pub fn try_new(
        values: Vec<Value>,
    ) -> Result<Self, InListValuesError> {
        Refined::try_new(values).map(Self).map_err(|err| match err {
            CollectionError::LenOutOfRange { actual } => {
                InListValuesError::ValueCount { actual }
            }
            _ => unreachable!(
                "InListValuesRule (LenItems) emits only LenOutOfRange"
            ),
        })
    }

    /// Borrow the underlying value list.
    #[must_use]
    #[inline]
    pub const fn as_slice(&self) -> &[Value] {
        self.0.as_inner().as_slice()
    }

    /// Consume the wrapper and return the inner value list.
    #[must_use]
    #[inline]
    pub fn into_inner(self) -> Vec<Value> {
        self.0.into_inner()
    }
}

/// `true` iff `expr` (or any sub-expression) is an
/// `Expression::Agg`.
///
/// Aggregates are only admissible inside `Summarize`. Smart
/// constructors of other operators (`Op::restrict`, `Op::extend`,
/// `Op::join` theta predicate) call this to reject aggregate-
/// bearing expressions before they reach a context that does not
/// know what to do with them.
#[must_use]
pub fn contains_aggregate(expr: &Expression) -> bool {
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
    /// `expr IN (literals...)`. Length-bounded by `MAX_IN_LIST`;
    /// empty IN-lists are unrepresentable (an empty IN is always
    /// `false`, which the optimiser produces by constant folding
    /// rather than carrying as a degenerate state).
    InList(Box<Self>, InListValues),
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

/// An `Expression` proved to infer to `Type::Bool` against a
/// `Schema`.
///
/// The proof is established once at construction through
/// `BoolExpression::try_new(schema, expr)`; the inner expression
/// is private so a `BoolExpression` cannot be fabricated.
///
/// The proof is relative to the schema used at construction. Op
/// constructors that consume a `BoolExpression` (only
/// `Op::restrict` today) re-verify against their input schema —
/// the carrier proof removes "syntactically-non-Bool" from the
/// representable state space, but cross-schema use is still
/// guarded at the use site.
#[derive(Debug, Clone, PartialEq)]
pub struct BoolExpression {
    expr: Expression,
}

impl BoolExpression {
    /// Type-check `expr` against `schema` and require `Type::Bool`.
    ///
    /// # Errors
    ///
    /// Returns the underlying `InferError` if inference fails, or
    /// `InferError::TypeMismatch { expected: Bool, got: <inferred> }`
    /// when the expression has a non-Bool inferred type.
    pub fn try_new(
        schema: &crate::schema::Schema,
        expr: Expression,
    ) -> Result<Self, crate::infer::InferError> {
        let ty = crate::infer::infer(&expr, schema)?;
        if ty != Type::Bool {
            return Err(crate::infer::InferError::TypeMismatch {
                expected: Type::Bool,
                got: ty,
            });
        }
        Ok(Self { expr })
    }

    /// Borrow the underlying expression.
    #[must_use]
    #[inline]
    pub const fn as_expression(&self) -> &Expression {
        &self.expr
    }

    /// Consume and return the underlying expression.
    #[must_use]
    #[inline]
    pub fn into_expression(self) -> Expression {
        self.expr
    }
}

/// A predicate. Either a Bool-typed expression or an opaque
/// closure-backed predicate registered with an execution context.
///
/// `Predicate::Expr` wraps a `BoolExpression` whose payload has
/// already been proved to be Bool-typed against some schema. That
/// removes the bare-Expression-with-no-Bool-proof state from the
/// representable space. `Op::restrict` re-verifies against its own
/// input schema, so cross-schema misuse is still caught.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum Predicate {
    /// Bool-typed expression (proved at `BoolExpression`
    /// construction).
    Expr(BoolExpression),
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

    use super::{BoolExpression, Expression, OpaqueId, Predicate};
    use crate::identifier::AttributeName;
    use crate::infer::InferError;
    use crate::op_enums::BinOp;
    use crate::schema::{Attribute, Schema};
    use crate::ty::{Type, Value};

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

    fn attr(name: &str) -> AttributeName {
        AttributeName::try_new(name.to_string()).unwrap()
    }

    fn bool_schema() -> Schema {
        Schema::try_new(alloc::vec![Attribute {
            name: attr("flag"),
            ty: Type::Bool,
        }])
        .unwrap()
    }

    #[test]
    fn bool_expression_accepts_bool_typed_expression() {
        let e = BoolExpression::try_new(
            &bool_schema(),
            Expression::Lit(Value::Bool(true)),
        )
        .unwrap();
        assert!(matches!(
            e.as_expression(),
            Expression::Lit(Value::Bool(true)),
        ));
    }

    #[test]
    fn bool_expression_rejects_non_bool_expression() {
        let err = BoolExpression::try_new(
            &bool_schema(),
            Expression::Lit(Value::Int32(0)),
        )
        .unwrap_err();
        assert!(matches!(
            err,
            InferError::TypeMismatch {
                expected: Type::Bool,
                got: Type::Int32,
            },
        ));
    }

    #[test]
    fn bool_expression_rejects_unknown_attribute() {
        let err = BoolExpression::try_new(
            &bool_schema(),
            Expression::Attr(attr("missing")),
        )
        .unwrap_err();
        assert!(matches!(err, InferError::UnknownAttribute(_)));
    }

    #[test]
    fn predicate_expr_wraps_bool_expression() {
        let e = BoolExpression::try_new(
            &bool_schema(),
            Expression::Attr(attr("flag")),
        )
        .unwrap();
        let p = Predicate::Expr(e);
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
