//! Operator AST.
//!
//! `Op` is the public opaque wrapper; `OpKind` is the
//! `pub(crate)`, `#[non_exhaustive]` discriminated sum that the
//! optimiser and backends match on. Per docs/ARCHITECTURE.md §9.1
//! the only external construction path is the smart constructors
//! exposed on the `Op` impl block (and convenience builders for
//! refined attribute sets), not bare variant syntax.

use alloc::boxed::Box;
use alloc::vec::Vec;

use whittle::primitive::{
    CollectionError, IdentityKey, LenItems, UniqueByKey,
};
use whittle::{And, AndError, Refined};

use crate::expression::{Expression, Predicate};
use crate::identifier::AttributeName;
use crate::join::JoinOn;
use crate::limit::{LimitCount, Offset};
use crate::limits::MAX_SCHEMA_ATTRIBUTES;
use crate::op_enums::{JoinKind, NamedAgg};
use crate::order::OrderKeys;
use crate::path::AnyPath;
use crate::source::Source;

// ─── Refined attribute set used by Project / Summarize / Nest. ───

// Length-bounded and per-name-unique. Reuses whittle's IdentityKey
// since AttributeName is its own ordering key.
type AttributeSetRule = And<
    LenItems<1, { MAX_SCHEMA_ATTRIBUTES }>,
    UniqueByKey<AttributeName, IdentityKey<AttributeName>>,
>;

/// Bounded, ordered, name-unique set of attributes used by
/// `Project`, the `by` clause of `Summarize`, and `Nest`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttributeSet(Refined<Vec<AttributeName>, AttributeSetRule>);

/// Constructor error for `AttributeSet`.
pub type AttributeSetError = AndError<CollectionError, CollectionError>;

impl AttributeSet {
    /// Validate `raw` (non-empty, bounded, no duplicate names) and
    /// wrap.
    ///
    /// # Errors
    ///
    /// Returns `AndError::Left` on length-bound violation,
    /// `AndError::Right(CollectionError::DuplicateKey)` on a
    /// duplicate name.
    #[inline]
    pub fn try_new(
        raw: Vec<AttributeName>,
    ) -> Result<Self, AttributeSetError> {
        Refined::try_new(raw).map(Self)
    }

    /// Borrow the underlying name list.
    #[must_use]
    #[inline]
    pub const fn as_slice(&self) -> &[AttributeName] {
        self.0.as_inner().as_slice()
    }
}

// Same shape but admits an empty header (zero-key summarisation).
type GroupingSetRule = And<
    LenItems<0, { MAX_SCHEMA_ATTRIBUTES }>,
    UniqueByKey<AttributeName, IdentityKey<AttributeName>>,
>;

/// `Summarize.by` set: like `AttributeSet` but the empty set is
/// admissible (grand-total grouping).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupingSet(Refined<Vec<AttributeName>, GroupingSetRule>);

/// Constructor error for `GroupingSet`.
pub type GroupingSetError = AndError<CollectionError, CollectionError>;

impl GroupingSet {
    /// Validate `raw` (bounded, no duplicate names; empty is OK).
    ///
    /// # Errors
    ///
    /// Returns `AndError::Left` on length-bound violation,
    /// `AndError::Right(CollectionError::DuplicateKey)` on duplicate
    /// names.
    #[inline]
    pub fn try_new(
        raw: Vec<AttributeName>,
    ) -> Result<Self, GroupingSetError> {
        Refined::try_new(raw).map(Self)
    }

    /// Borrow the underlying name list.
    #[must_use]
    #[inline]
    pub const fn as_slice(&self) -> &[AttributeName] {
        self.0.as_inner().as_slice()
    }
}

// ─── Aggregate output: length-bounded and unique-by-output-name. ─

type NamedAggSetRule = And<
    LenItems<1, { MAX_SCHEMA_ATTRIBUTES }>,
    UniqueByKey<NamedAgg, NamedAggKey>,
>;

/// Key extractor for `UniqueByKey<NamedAgg, NamedAggKey>` —
/// uniqueness is on the output attribute name.
pub struct NamedAggKey(core::marker::PhantomData<()>);

impl whittle::primitive::KeyOf<NamedAgg> for NamedAggKey {
    type Key = AttributeName;
    fn key_of(value: &NamedAgg) -> AttributeName {
        value.name.clone()
    }
}

/// Bounded, output-name-unique aggregate list used by `Summarize`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NamedAggSet(Refined<Vec<NamedAgg>, NamedAggSetRule>);

/// Constructor error for `NamedAggSet`.
pub type NamedAggSetError = AndError<CollectionError, CollectionError>;

impl NamedAggSet {
    /// Validate `raw` and wrap.
    ///
    /// # Errors
    ///
    /// Returns `AndError::Left` on length-bound violation,
    /// `AndError::Right(CollectionError::DuplicateKey)` on a
    /// duplicate output name.
    #[inline]
    pub fn try_new(
        raw: Vec<NamedAgg>,
    ) -> Result<Self, NamedAggSetError> {
        Refined::try_new(raw).map(Self)
    }

    /// Borrow the underlying aggregate list.
    #[must_use]
    #[inline]
    pub const fn as_slice(&self) -> &[NamedAgg] {
        self.0.as_inner().as_slice()
    }
}

// ─── The operator AST. ───────────────────────────────────────────

/// Operator AST node — opaque to callers; matched on internally
/// through `Op::kind()`.
///
/// Each node caches its computed output `Schema`. Smart constructors
/// validate every operator's invariants against the input schemas
/// and compute the output schema once, so consumers — the optimiser,
/// type checker, backends — can read `Op::schema()` in O(1) without
/// recomputing through the tree.
#[derive(Debug, Clone, PartialEq)]
pub struct Op {
    kind: OpKind,
    schema: crate::schema::Schema,
}

/// Errors common to every schema-aware `Op` smart constructor.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[non_exhaustive]
pub enum OpError {
    /// A referenced attribute is not in the input schema.
    #[error("attribute `{attribute}` is not in the input schema")]
    UnknownAttribute {
        /// The offending name.
        attribute: AttributeName,
    },

    /// `Rename`'s target name is already present in the input schema.
    #[error("target attribute `{attribute}` already exists in the input schema")]
    AttributeAlreadyExists {
        /// The colliding target name.
        attribute: AttributeName,
    },

    /// `Project`/etc. produced a schema invariant violation.
    #[error("output schema: {0}")]
    Schema(#[source] crate::schema::SchemaError),
}

impl Op {
    /// Build a leaf from a `Source`. The output schema is the
    /// source's schema verbatim.
    #[must_use]
    #[inline]
    pub fn source(src: Source) -> Self {
        let schema = src.schema().clone();
        Self { kind: OpKind::Source(src), schema }
    }

    /// Project the input's rows down to `attrs`, in the order given.
    ///
    /// # Errors
    ///
    /// Returns `UnknownAttribute` if any name in `attrs` is missing
    /// from `input`'s schema.
    pub fn project(
        input: Self,
        attrs: AttributeSet,
    ) -> Result<Self, OpError> {
        use crate::schema::{Attribute, Schema};
        let input_schema = input.schema();
        let mut projected: Vec<Attribute> = Vec::with_capacity(attrs.as_slice().len());
        for name in attrs.as_slice() {
            let attr = input_schema.find(name).ok_or_else(|| {
                OpError::UnknownAttribute { attribute: name.clone() }
            })?;
            projected.push(attr.clone());
        }
        let schema = Schema::try_new(projected).map_err(OpError::Schema)?;
        Ok(Self {
            kind: OpKind::Project {
                input: Box::new(input),
                attrs,
            },
            schema,
        })
    }

    /// Restrict the input's rows by a `Predicate`. The output schema
    /// is identical to the input's.
    #[must_use]
    pub fn restrict(input: Self, predicate: Predicate) -> Self {
        let schema = input.schema().clone();
        Self {
            kind: OpKind::Restrict {
                input: Box::new(input),
                predicate,
            },
            schema,
        }
    }

    /// Rename `from` to `to`, preserving order and types.
    ///
    /// # Errors
    ///
    /// Returns `UnknownAttribute` if `from` is missing from the
    /// input schema, and `AttributeAlreadyExists` if `to` is already
    /// present.
    pub fn rename(
        input: Self,
        from: AttributeName,
        to: AttributeName,
    ) -> Result<Self, OpError> {
        use crate::schema::{Attribute, Schema};
        let input_schema = input.schema();
        if !input_schema.contains(&from) {
            return Err(OpError::UnknownAttribute { attribute: from });
        }
        if from != to && input_schema.contains(&to) {
            return Err(OpError::AttributeAlreadyExists { attribute: to });
        }
        let renamed: Vec<Attribute> = input_schema
            .attributes()
            .iter()
            .map(|a| {
                if a.name == from {
                    Attribute { name: to.clone(), ty: a.ty.clone() }
                } else {
                    a.clone()
                }
            })
            .collect();
        let schema = Schema::try_new(renamed).map_err(OpError::Schema)?;
        Ok(Self {
            kind: OpKind::Rename {
                input: Box::new(input),
                from,
                to,
            },
            schema,
        })
    }

    /// Order the input by `by`. Output schema unchanged.
    #[must_use]
    pub fn order(input: Self, by: OrderKeys) -> Self {
        let schema = input.schema().clone();
        Self {
            kind: OpKind::Order {
                input: Box::new(input),
                by,
            },
            schema,
        }
    }

    /// Window over the input. Output schema unchanged.
    #[must_use]
    pub fn limit(input: Self, offset: Offset, count: LimitCount) -> Self {
        let schema = input.schema().clone();
        Self {
            kind: OpKind::Limit {
                input: Box::new(input),
                offset,
                count,
            },
            schema,
        }
    }

    /// Borrow the cached output schema. O(1) — computed once at
    /// construction.
    #[must_use]
    #[inline]
    pub const fn schema(&self) -> &crate::schema::Schema {
        &self.schema
    }

    /// Internal accessor used by the optimiser and backends. The
    /// returned type lives in a private module, so external callers
    /// cannot name `OpKind`; only sibling modules inside this crate
    /// can match on it.
    #[must_use]
    #[inline]
    pub const fn kind(&self) -> &OpKind {
        &self.kind
    }
}

/// Discriminated sum of operator shapes. Public at the type level
/// but only nameable inside this crate, because the `op` module is
/// private. External callers see `Op` as opaque and construct only
/// through the smart constructors on `Op`.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum OpKind {
    Source(Source),
    Project {
        input: Box<Op>,
        attrs: AttributeSet,
    },
    Restrict {
        input: Box<Op>,
        predicate: Predicate,
    },
    Rename {
        input: Box<Op>,
        from: AttributeName,
        to: AttributeName,
    },
    Extend {
        input: Box<Op>,
        name: AttributeName,
        expr: Expression,
    },
    Join {
        left: Box<Op>,
        right: Box<Op>,
        kind: JoinKind,
        on: JoinOn,
    },
    Product {
        left: Box<Op>,
        right: Box<Op>,
    },
    Union {
        left: Box<Op>,
        right: Box<Op>,
    },
    Intersect {
        left: Box<Op>,
        right: Box<Op>,
    },
    Difference {
        left: Box<Op>,
        right: Box<Op>,
    },
    Summarize {
        input: Box<Op>,
        by: GroupingSet,
        aggs: NamedAggSet,
    },
    Order {
        input: Box<Op>,
        by: OrderKeys,
    },
    Limit {
        input: Box<Op>,
        offset: Offset,
        count: LimitCount,
    },
    Modify {
        input: Box<Op>,
        path: AnyPath,
        sub: Box<Op>,
    },
    Unnest {
        input: Box<Op>,
        path: AnyPath,
    },
    Nest {
        input: Box<Op>,
        attrs: AttributeSet,
        into: AttributeName,
    },
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

    use super::{AttributeSet, GroupingSet, NamedAggSet, Op, OpKind};
    use crate::identifier::{AttributeName, TableName};
    use crate::op_enums::{Agg, NamedAgg};
    use crate::schema::{Attribute, Schema};
    use crate::source::Source;
    use crate::ty::Type;

    fn attr(name: &str) -> AttributeName {
        AttributeName::try_new(name.to_string()).unwrap()
    }

    fn schema() -> Schema {
        Schema::try_new(vec![Attribute {
            name: attr("id"),
            ty: Type::Int64,
        }])
        .unwrap()
    }

    // ─── AttributeSet. ───────────────────────────────────────────

    #[test]
    fn attribute_set_admits_distinct_names() {
        let s = AttributeSet::try_new(vec![attr("a"), attr("b")]).unwrap();
        assert_eq!(s.as_slice().len(), 2);
    }

    #[test]
    fn attribute_set_rejects_empty() {
        let result = AttributeSet::try_new(Vec::new());
        assert!(matches!(
            result.unwrap_err(),
            AndError::Left(CollectionError::LenOutOfRange { actual: 0 }),
        ));
    }

    #[test]
    fn attribute_set_rejects_duplicate_names() {
        let result = AttributeSet::try_new(vec![attr("a"), attr("a")]);
        assert!(matches!(
            result.unwrap_err(),
            AndError::Right(CollectionError::DuplicateKey { index: 1 }),
        ));
    }

    // ─── GroupingSet. ────────────────────────────────────────────

    #[test]
    fn grouping_set_admits_empty() {
        let s = GroupingSet::try_new(Vec::new()).unwrap();
        assert!(s.as_slice().is_empty());
    }

    #[test]
    fn grouping_set_rejects_duplicates() {
        let result = GroupingSet::try_new(vec![attr("k"), attr("k")]);
        assert!(matches!(
            result.unwrap_err(),
            AndError::Right(CollectionError::DuplicateKey { index: 1 }),
        ));
    }

    // ─── NamedAggSet. ────────────────────────────────────────────

    #[test]
    fn named_agg_set_rejects_duplicate_output_names() {
        let aggs = vec![
            NamedAgg {
                name: attr("total"),
                agg: Agg::Sum(attr("x")),
            },
            NamedAgg {
                name: attr("total"),
                agg: Agg::Avg(attr("y")),
            },
        ];
        let result = NamedAggSet::try_new(aggs);
        assert!(matches!(
            result.unwrap_err(),
            AndError::Right(CollectionError::DuplicateKey { index: 1 }),
        ));
    }

    // ─── Op constructors. ────────────────────────────────────────

    fn two_attr_schema() -> Schema {
        Schema::try_new(vec![
            Attribute { name: attr("id"), ty: Type::Int64 },
            Attribute { name: attr("name"), ty: Type::String },
        ])
        .unwrap()
    }

    fn two_attr_source() -> Op {
        Op::source(Source::Table {
            schema: two_attr_schema(),
            name: TableName::try_new("users".to_string()).unwrap(),
        })
    }

    #[test]
    fn op_source_builds_a_leaf() {
        let src = Source::Table {
            schema: schema(),
            name: TableName::try_new("orders".to_string()).unwrap(),
        };
        let op = Op::source(src);
        let OpKind::Source(_) = op.kind() else {
            unreachable!();
        };
        assert_eq!(op.schema().cardinality(), 1);
    }

    #[test]
    fn op_project_admits_known_attrs() {
        let input = two_attr_source();
        let attrs = AttributeSet::try_new(vec![attr("name")]).unwrap();
        let op = Op::project(input, attrs).unwrap();
        assert_eq!(op.schema().cardinality(), 1);
        assert_eq!(
            op.schema().attributes()[0].name.as_str(),
            "name",
        );
    }

    #[test]
    fn op_project_rejects_unknown_attr() {
        use super::OpError;
        let input = two_attr_source();
        let attrs = AttributeSet::try_new(vec![attr("missing")]).unwrap();
        let result = Op::project(input, attrs);
        let Err(OpError::UnknownAttribute { attribute }) = result else {
            unreachable!();
        };
        assert_eq!(attribute.as_str(), "missing");
    }

    #[test]
    fn op_restrict_preserves_schema() {
        use crate::expression::{Expression, Predicate};
        use crate::op_enums::BinOp;
        use crate::ty::Value;
        let input = two_attr_source();
        let predicate = Predicate::Expr(Expression::BinOp(
            BinOp::Gt,
            alloc::boxed::Box::new(Expression::Attr(attr("id"))),
            alloc::boxed::Box::new(Expression::Lit(Value::Int64(0))),
        ));
        let op = Op::restrict(input, predicate);
        assert_eq!(op.schema().cardinality(), 2);
    }

    #[test]
    fn op_rename_swaps_attribute_in_schema() {
        let input = two_attr_source();
        let op =
            Op::rename(input, attr("name"), attr("full_name")).unwrap();
        let names: alloc::vec::Vec<_> = op
            .schema()
            .attributes()
            .iter()
            .map(|a| a.name.as_str())
            .collect();
        assert_eq!(names, vec!["id", "full_name"]);
    }

    #[test]
    fn op_rename_rejects_unknown_source() {
        use super::OpError;
        let input = two_attr_source();
        let result = Op::rename(input, attr("missing"), attr("x"));
        let Err(OpError::UnknownAttribute { attribute }) = result else {
            unreachable!();
        };
        assert_eq!(attribute.as_str(), "missing");
    }

    #[test]
    fn op_rename_rejects_target_collision() {
        use super::OpError;
        let input = two_attr_source();
        let result = Op::rename(input, attr("name"), attr("id"));
        let Err(OpError::AttributeAlreadyExists { attribute }) = result else {
            unreachable!();
        };
        assert_eq!(attribute.as_str(), "id");
    }
}
