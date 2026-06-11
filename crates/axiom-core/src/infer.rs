//! Expression type inference.
//!
//! Per docs/ARCHITECTURE.md §11, an `Expression` carries no
//! type-level discipline at construction — its tree is the algebra
//! the optimiser rewrites. The static type of a leaf or composite
//! node is *inferred* against a `Schema`: `Attr(name)` looks the
//! attribute up; `Lit(value)` is determined by the variant;
//! everything else dispatches on operator and operand types.
//!
//! `Predicate::try_new` (added in this commit) lifts the
//! inference result into the only construction path: a
//! `Predicate::Expr` must infer to `Type::Bool`. Aggregates use
//! `agg_ty` to determine their output type for `Op::summarize`.

use thiserror::Error;

use crate::expression::Expression;
use crate::identifier::AttributeName;
use crate::op_enums::{Agg, BinOp, UnOp};
use crate::schema::Schema;
use crate::ty::{Type, Value};

/// Failure modes for type inference. Variants are precise — each
/// reports the inferred operand types so call sites can produce
/// useful diagnostics without re-traversing the AST.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum InferError {
    /// `Attr(name)` referenced an attribute that is not in the
    /// schema.
    #[error("attribute `{0}` is not in the schema")]
    UnknownAttribute(AttributeName),

    /// A literal's type cannot be determined from the value alone.
    /// Reaches this case only for compound `Value` shapes —
    /// `Optional`, `Array`, `Relation` — that need surrounding
    /// schema context.
    #[error("compound literal type cannot be inferred without schema context")]
    AmbiguousLiteral,

    /// Operator received an operand of the wrong type.
    #[error("operator received unexpected operand type")]
    TypeMismatch {
        /// Type the operator required.
        expected: Type,
        /// Type the operand actually produced.
        got: Type,
    },

    /// Operator does not admit operands of the given type. Used
    /// when more than one type would have been admissible (e.g.
    /// arithmetic on any of several numeric types) but the actual
    /// operand was none of them.
    #[error("operator does not admit operand of this type")]
    OperatorNotApplicable {
        /// The offending operand type.
        got: Type,
    },

    /// Binary operator received operands whose types disagree
    /// where they must match (arithmetic, comparison, concat).
    #[error("binary operator received operands of disagreeing types")]
    BinOpTypeMismatch {
        /// Left operand's inferred type.
        left: Type,
        /// Right operand's inferred type.
        right: Type,
    },

    /// `IN (list)` had a list value whose type disagrees with the
    /// scrutinee.
    #[error("IN-list element disagrees with scrutinee type")]
    InListElementMismatch {
        /// Index of the offending list element.
        index: usize,
        /// Scrutinee's inferred type.
        scrutinee: Type,
        /// The list element's type.
        got: Type,
    },
}

/// Infer the static type of an expression against `schema`.
///
/// # Errors
///
/// Returns `InferError` for any of the documented failure modes.
pub fn infer(expr: &Expression, schema: &Schema) -> Result<Type, InferError> {
    match expr {
        Expression::Attr(name) => schema
            .find(name)
            .map(|a| a.ty.clone())
            .ok_or_else(|| InferError::UnknownAttribute(name.clone())),
        Expression::Lit(value) => infer_value(value),
        Expression::BinOp(op, lhs, rhs) => {
            let lt = infer(lhs, schema)?;
            let rt = infer(rhs, schema)?;
            infer_binop(*op, lt, rt)
        }
        Expression::UnOp(op, operand) => {
            let ot = infer(operand, schema)?;
            infer_unop(*op, ot)
        }
        Expression::Like(operand, _) => {
            let ot = infer(operand, schema)?;
            if ot != Type::String {
                return Err(InferError::TypeMismatch {
                    expected: Type::String,
                    got: ot,
                });
            }
            Ok(Type::Bool)
        }
        Expression::InList(operand, values) => {
            let ot = infer(operand, schema)?;
            for (index, v) in values.as_slice().iter().enumerate() {
                let vt = infer_value(v)?;
                if vt != ot {
                    return Err(InferError::InListElementMismatch {
                        index,
                        scrutinee: ot,
                        got: vt,
                    });
                }
            }
            Ok(Type::Bool)
        }
        Expression::IsNull(operand) => {
            let _ = infer(operand, schema)?;
            Ok(Type::Bool)
        }
        Expression::Cast(operand, to) => {
            let _ = infer(operand, schema)?;
            Ok(to.clone())
        }
        Expression::Agg(agg) => agg_ty(agg, schema),
    }
}

/// Failure path returned by `value_matches_type` when a `Value`
/// does not conform to a `Type`. Carries the structural location
/// inside the value tree so callers can produce precise
/// diagnostics.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum ValueTypeError {
    /// Scalar mismatch (or compound-value-vs-scalar-type) at the
    /// current node.
    #[error("value does not match type at this position")]
    Mismatch,

    /// Mismatch nested inside an `Array`'s element position.
    #[error("array element at index {index}: {source}")]
    ArrayElement {
        /// Position inside the array.
        index: usize,
        /// The nested failure.
        #[source]
        source: alloc::boxed::Box<Self>,
    },

    /// Mismatch nested inside a relation row's attribute position.
    #[error("relation field at position {position}: {source}")]
    RelationField {
        /// Position inside the row.
        position: usize,
        /// The nested failure.
        #[source]
        source: alloc::boxed::Box<Self>,
    },

    /// Mismatch nested inside a `Relation`'s row position.
    #[error("relation row at index {index}: {source}")]
    RelationRow {
        /// Position inside the relation.
        index: usize,
        /// The nested failure.
        #[source]
        source: alloc::boxed::Box<Self>,
    },

    /// A relation row had the wrong width for the nested schema.
    #[error("row width {actual} differs from schema width {expected}")]
    RowWidth {
        /// Width the schema requires.
        expected: usize,
        /// Width of the row we received.
        actual: usize,
    },

    /// Mismatch nested inside an `Optional`'s inner value.
    #[error("optional value: {0}")]
    Optional(#[source] alloc::boxed::Box<Self>),
}

/// `Ok(())` iff `value`'s shape conforms to `ty` recursively.
///
/// Scalars are compared variant-by-variant; compound values
/// (`Array`, `Relation`, `Optional`) are walked structurally,
/// recursing into the inner type and surfacing the path of the
/// first mismatch.
///
/// # Errors
///
/// Returns `ValueTypeError::Mismatch` at the root level for a
/// shape disagreement, with `ArrayElement` / `RelationRow` /
/// `Optional` wrappers naming the path of the first descent that
/// diverged.
pub fn value_matches_type(value: &Value, ty: &Type) -> Result<(), ValueTypeError> {
    use alloc::boxed::Box;
    match (value, ty) {
        (Value::Bool(_), Type::Bool)
        | (Value::Int32(_), Type::Int32)
        | (Value::Int64(_), Type::Int64)
        | (Value::Float64(_), Type::Float64)
        | (Value::Decimal(_), Type::Decimal)
        | (Value::String(_), Type::String)
        | (Value::Bytes(_), Type::Bytes)
        | (Value::DateTime(_), Type::DateTime)
        | (Value::Json(_), Type::Json)
        | (Value::Optional(None), Type::Optional(_)) => Ok(()),
        (Value::Optional(Some(inner_val)), Type::Optional(inner_ty)) => {
            value_matches_type(inner_val, inner_ty)
                .map_err(|e| ValueTypeError::Optional(Box::new(e)))
        }
        (Value::Array(values), Type::Array(inner_ty)) => {
            for (index, v) in values.as_inner().iter().enumerate() {
                value_matches_type(v, inner_ty).map_err(|e| ValueTypeError::ArrayElement {
                    index,
                    source: Box::new(e),
                })?;
            }
            Ok(())
        }
        (Value::Relation(rows), Type::Relation(schema)) => {
            for (index, row) in rows.as_inner().iter().enumerate() {
                row_matches_schema(row, schema).map_err(|e| ValueTypeError::RelationRow {
                    index,
                    source: Box::new(e),
                })?;
            }
            Ok(())
        }
        _ => Err(ValueTypeError::Mismatch),
    }
}

/// `Ok(())` iff `row` has the same width as `schema` and every
/// position's value matches its attribute's type.
///
/// # Errors
///
/// Returns `RowWidth` for a width mismatch, otherwise the
/// structural error from the first non-matching value.
pub fn row_matches_schema(row: &crate::row::Row, schema: &Schema) -> Result<(), ValueTypeError> {
    use alloc::boxed::Box;
    let expected = schema.cardinality();
    let actual = row.len();
    if actual != expected {
        return Err(ValueTypeError::RowWidth { expected, actual });
    }
    for (position, (value, attribute)) in row
        .as_slice()
        .iter()
        .zip(schema.attributes().iter())
        .enumerate()
    {
        value_matches_type(value, &attribute.ty).map_err(|e| ValueTypeError::RelationField {
            position,
            source: Box::new(e),
        })?;
    }
    Ok(())
}

/// Infer the type of a `Value` literal. Compound values
/// (`Relation`, `Array`, `Optional`) require surrounding schema
/// context; this function reports `AmbiguousLiteral` for them.
///
/// # Errors
///
/// Returns `AmbiguousLiteral` for compound values.
pub const fn infer_value(value: &Value) -> Result<Type, InferError> {
    match value {
        Value::Bool(_) => Ok(Type::Bool),
        Value::Int32(_) => Ok(Type::Int32),
        Value::Int64(_) => Ok(Type::Int64),
        Value::Float64(_) => Ok(Type::Float64),
        Value::Decimal(_) => Ok(Type::Decimal),
        Value::String(_) => Ok(Type::String),
        Value::Bytes(_) => Ok(Type::Bytes),
        Value::DateTime(_) => Ok(Type::DateTime),
        Value::Json(_) => Ok(Type::Json),
        Value::Relation(_) | Value::Array(_) | Value::Optional(_) => {
            Err(InferError::AmbiguousLiteral)
        }
    }
}

/// Infer the result type of an aggregate against `schema`.
///
/// # Errors
///
/// Returns `UnknownAttribute` if the aggregate references a
/// missing attribute, `OperatorNotApplicable` if the aggregate's
/// input type is not admissible (e.g. SUM of a String).
pub fn agg_ty(agg: &Agg, schema: &Schema) -> Result<Type, InferError> {
    match agg {
        Agg::Count(_) => Ok(Type::Int64),
        Agg::Sum(name) => {
            let input = lookup(schema, name)?;
            match input {
                Type::Int32 | Type::Int64 => Ok(Type::Int64),
                Type::Float64 => Ok(Type::Float64),
                Type::Decimal => Ok(Type::Decimal),
                other => Err(InferError::OperatorNotApplicable { got: other }),
            }
        }
        Agg::Min(name) | Agg::Max(name) => {
            let input = lookup(schema, name)?;
            // MIN/MAX admit anything orderable. The closed set of
            // axiom-rs types are all orderable except Json and
            // nested Relation.
            match input {
                Type::Json | Type::Relation(_) => {
                    Err(InferError::OperatorNotApplicable { got: input })
                }
                other => Ok(other),
            }
        }
        Agg::Avg(name) => {
            let input = lookup(schema, name)?;
            match input {
                Type::Int32 | Type::Int64 | Type::Float64 => Ok(Type::Float64),
                Type::Decimal => Ok(Type::Decimal),
                other => Err(InferError::OperatorNotApplicable { got: other }),
            }
        }
    }
}

fn lookup(schema: &Schema, name: &AttributeName) -> Result<Type, InferError> {
    schema
        .find(name)
        .map(|a| a.ty.clone())
        .ok_or_else(|| InferError::UnknownAttribute(name.clone()))
}

fn infer_binop(op: BinOp, lt: Type, rt: Type) -> Result<Type, InferError> {
    use BinOp::{Add, And, Concat, Div, Eq, Ge, Gt, Le, Lt, Mul, Ne, Or, Sub};
    match op {
        Add | Sub | Mul | Div => {
            require_same(&lt, &rt)?;
            if is_numeric(&lt) {
                Ok(lt)
            } else {
                Err(InferError::OperatorNotApplicable { got: lt })
            }
        }
        Concat => {
            require_same(&lt, &rt)?;
            if lt == Type::String {
                Ok(Type::String)
            } else {
                Err(InferError::OperatorNotApplicable { got: lt })
            }
        }
        Eq | Ne | Lt | Le | Gt | Ge => {
            require_same(&lt, &rt)?;
            Ok(Type::Bool)
        }
        And | Or => {
            if lt != Type::Bool {
                return Err(InferError::TypeMismatch {
                    expected: Type::Bool,
                    got: lt,
                });
            }
            if rt != Type::Bool {
                return Err(InferError::TypeMismatch {
                    expected: Type::Bool,
                    got: rt,
                });
            }
            Ok(Type::Bool)
        }
    }
}

fn infer_unop(op: UnOp, ot: Type) -> Result<Type, InferError> {
    match op {
        UnOp::Neg => {
            if is_numeric(&ot) {
                Ok(ot)
            } else {
                Err(InferError::OperatorNotApplicable { got: ot })
            }
        }
        UnOp::Not => {
            if ot == Type::Bool {
                Ok(Type::Bool)
            } else {
                Err(InferError::TypeMismatch {
                    expected: Type::Bool,
                    got: ot,
                })
            }
        }
    }
}

fn require_same(lt: &Type, rt: &Type) -> Result<(), InferError> {
    if lt == rt {
        Ok(())
    } else {
        Err(InferError::BinOpTypeMismatch {
            left: lt.clone(),
            right: rt.clone(),
        })
    }
}

const fn is_numeric(ty: &Type) -> bool {
    matches!(
        ty,
        Type::Int32 | Type::Int64 | Type::Float64 | Type::Decimal
    )
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

    use super::{InferError, agg_ty, infer, infer_value};
    use crate::expression::Expression;
    use crate::identifier::AttributeName;
    use crate::op_enums::{Agg, BinOp, UnOp};
    use crate::schema::{Attribute, Schema};
    use crate::ty::{Type, Value};

    fn attr(name: &str) -> AttributeName {
        AttributeName::try_new(name.to_string()).unwrap()
    }

    fn two_attr_schema() -> Schema {
        Schema::try_new(vec![
            Attribute {
                name: attr("age"),
                ty: Type::Int32,
            },
            Attribute {
                name: attr("name"),
                ty: Type::String,
            },
        ])
        .unwrap()
    }

    // ─── infer_value: scalar literals. ───────────────────────────

    #[test]
    fn literal_bool_infers_bool() {
        assert_eq!(infer_value(&Value::Bool(true)).unwrap(), Type::Bool,);
    }

    #[test]
    fn literal_int32_infers_int32() {
        assert_eq!(infer_value(&Value::Int32(42_i32)).unwrap(), Type::Int32,);
    }

    #[test]
    fn compound_literal_is_ambiguous() {
        assert_eq!(
            infer_value(&Value::Optional(None)).unwrap_err(),
            InferError::AmbiguousLiteral,
        );
    }

    // ─── infer: attribute lookups. ───────────────────────────────

    #[test]
    fn attr_resolved_from_schema() {
        let s = two_attr_schema();
        let ty = infer(&Expression::Attr(attr("age")), &s).unwrap();
        assert_eq!(ty, Type::Int32);
    }

    #[test]
    fn unknown_attr_reported() {
        let s = two_attr_schema();
        let err = infer(&Expression::Attr(attr("missing")), &s).unwrap_err();
        let InferError::UnknownAttribute(name) = err else {
            unreachable!();
        };
        assert_eq!(name.as_str(), "missing");
    }

    // ─── BinOp. ──────────────────────────────────────────────────

    #[test]
    fn arithmetic_admits_matching_numeric() {
        let s = two_attr_schema();
        let expr = Expression::BinOp(
            BinOp::Add,
            Box::new(Expression::Attr(attr("age"))),
            Box::new(Expression::Lit(Value::Int32(1))),
        );
        assert_eq!(infer(&expr, &s).unwrap(), Type::Int32);
    }

    #[test]
    fn arithmetic_rejects_string_operand() {
        let s = two_attr_schema();
        let expr = Expression::BinOp(
            BinOp::Add,
            Box::new(Expression::Attr(attr("name"))),
            Box::new(Expression::Lit(Value::String("x".to_string()))),
        );
        assert!(matches!(
            infer(&expr, &s).unwrap_err(),
            InferError::OperatorNotApplicable { got: Type::String },
        ));
    }

    #[test]
    fn arithmetic_rejects_mismatched_widths() {
        let s = two_attr_schema();
        let expr = Expression::BinOp(
            BinOp::Add,
            Box::new(Expression::Attr(attr("age"))),
            Box::new(Expression::Lit(Value::Int64(1))),
        );
        let err = infer(&expr, &s).unwrap_err();
        assert!(matches!(err, InferError::BinOpTypeMismatch { .. }));
    }

    #[test]
    fn comparison_returns_bool() {
        let s = two_attr_schema();
        let expr = Expression::BinOp(
            BinOp::Ge,
            Box::new(Expression::Attr(attr("age"))),
            Box::new(Expression::Lit(Value::Int32(18))),
        );
        assert_eq!(infer(&expr, &s).unwrap(), Type::Bool);
    }

    #[test]
    fn logical_rejects_non_bool_operand() {
        let s = two_attr_schema();
        let expr = Expression::BinOp(
            BinOp::And,
            Box::new(Expression::Attr(attr("age"))),
            Box::new(Expression::Lit(Value::Bool(true))),
        );
        assert!(matches!(
            infer(&expr, &s).unwrap_err(),
            InferError::TypeMismatch {
                expected: Type::Bool,
                ..
            },
        ));
    }

    // ─── UnOp. ───────────────────────────────────────────────────

    #[test]
    fn neg_admits_numeric() {
        let s = two_attr_schema();
        let expr = Expression::UnOp(UnOp::Neg, Box::new(Expression::Attr(attr("age"))));
        assert_eq!(infer(&expr, &s).unwrap(), Type::Int32);
    }

    #[test]
    fn not_admits_bool() {
        let s = two_attr_schema();
        let expr = Expression::UnOp(UnOp::Not, Box::new(Expression::Lit(Value::Bool(true))));
        assert_eq!(infer(&expr, &s).unwrap(), Type::Bool);
    }

    // ─── Like / IsNull / Cast / InList. ──────────────────────────

    #[test]
    fn like_requires_string_operand() {
        use crate::identifier::Pattern;
        let s = two_attr_schema();
        let pat = Pattern::try_new("Mr.%".to_string()).unwrap();
        let expr = Expression::Like(Box::new(Expression::Attr(attr("name"))), pat);
        assert_eq!(infer(&expr, &s).unwrap(), Type::Bool);
    }

    #[test]
    fn isnull_returns_bool() {
        let s = two_attr_schema();
        let expr = Expression::IsNull(Box::new(Expression::Attr(attr("age"))));
        assert_eq!(infer(&expr, &s).unwrap(), Type::Bool);
    }

    #[test]
    fn cast_returns_target_type() {
        let s = two_attr_schema();
        let expr = Expression::Cast(Box::new(Expression::Attr(attr("age"))), Type::Int64);
        assert_eq!(infer(&expr, &s).unwrap(), Type::Int64);
    }

    fn in_list(values: alloc::vec::Vec<Value>) -> crate::expression::InListValues {
        crate::expression::InListValues::try_new(values).unwrap()
    }

    #[test]
    fn in_list_returns_bool_when_elements_match() {
        let s = two_attr_schema();
        let expr = Expression::InList(
            Box::new(Expression::Attr(attr("age"))),
            in_list(vec![Value::Int32(1), Value::Int32(2)]),
        );
        assert_eq!(infer(&expr, &s).unwrap(), Type::Bool);
    }

    #[test]
    fn in_list_rejects_element_mismatch() {
        let s = two_attr_schema();
        let expr = Expression::InList(
            Box::new(Expression::Attr(attr("age"))),
            in_list(vec![Value::Int32(1), Value::String("x".to_string())]),
        );
        assert!(matches!(
            infer(&expr, &s).unwrap_err(),
            InferError::InListElementMismatch { index: 1, .. },
        ));
    }

    // ─── Agg. ────────────────────────────────────────────────────

    #[test]
    fn count_is_int64() {
        let s = two_attr_schema();
        assert_eq!(agg_ty(&Agg::Count(None), &s).unwrap(), Type::Int64,);
    }

    #[test]
    fn sum_widens_int32_to_int64() {
        let s = two_attr_schema();
        assert_eq!(agg_ty(&Agg::Sum(attr("age")), &s).unwrap(), Type::Int64,);
    }

    #[test]
    fn avg_returns_float64_for_int_input() {
        let s = two_attr_schema();
        assert_eq!(agg_ty(&Agg::Avg(attr("age")), &s).unwrap(), Type::Float64,);
    }

    #[test]
    fn min_returns_input_type() {
        let s = two_attr_schema();
        assert_eq!(agg_ty(&Agg::Min(attr("name")), &s).unwrap(), Type::String,);
    }

    #[test]
    fn sum_rejects_string() {
        let s = two_attr_schema();
        assert!(matches!(
            agg_ty(&Agg::Sum(attr("name")), &s).unwrap_err(),
            InferError::OperatorNotApplicable { got: Type::String },
        ));
    }
}
