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

use whittle::primitive::{CollectionError, IdentityKey, LenItems, UniqueByKey};
use whittle::{And, Refined};

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
///
/// Flat domain-shaped enum: the underlying composition is
/// `And<LenItems, UniqueByKey>`. Both inner rules report through
/// `CollectionError`, so the composition's error is `CollectionError`
/// directly — no positional `Left` / `Right` wrapping leaks.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[non_exhaustive]
pub enum AttributeSetError {
    /// Attribute count fell outside `1..=MAX_SCHEMA_ATTRIBUTES`.
    #[error("attribute-set count out of range (actual: {actual})")]
    AttributeCount {
        /// Observed attribute count.
        actual: usize,
    },

    /// Two entries shared a name. The reported index is the second
    /// occurrence (the first wins).
    #[error("duplicate attribute name at index {index}")]
    DuplicateAttribute {
        /// Position of the duplicate (the second occurrence).
        index: usize,
    },
}

impl AttributeSet {
    /// Validate `attrs` against the attribute-set rule and wrap.
    ///
    /// # Errors
    ///
    /// Returns `AttributeCount` if the list length is outside
    /// `1..=MAX_SCHEMA_ATTRIBUTES`, or `DuplicateAttribute` if two
    /// entries share a name.
    #[inline]
    pub fn try_new(attrs: Vec<AttributeName>) -> Result<Self, AttributeSetError> {
        Refined::try_new(attrs).map(Self).map_err(|err| match err {
            CollectionError::LenOutOfRange { actual } => {
                AttributeSetError::AttributeCount { actual }
            }
            CollectionError::DuplicateKey { index } => {
                AttributeSetError::DuplicateAttribute { index }
            }
            _ => unreachable!("AttributeSetRule emits only LenOutOfRange / DuplicateKey"),
        })
    }

    /// Borrow the underlying name list.
    #[must_use]
    #[inline]
    pub const fn as_slice(&self) -> &[AttributeName] {
        self.0.as_inner().as_slice()
    }

    /// Consume the wrapper and return the inner name list.
    #[must_use]
    #[inline]
    pub fn into_inner(self) -> Vec<AttributeName> {
        self.0.into_inner()
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
///
/// Flat domain-shaped enum mirroring `AttributeSet`'s shape (the
/// empty set is admissible here; `GroupCount` still fires when the
/// list exceeds `MAX_SCHEMA_ATTRIBUTES`).
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[non_exhaustive]
pub enum GroupingSetError {
    /// Grouping-key count fell outside `0..=MAX_SCHEMA_ATTRIBUTES`.
    #[error("grouping-set count out of range (actual: {actual})")]
    GroupCount {
        /// Observed grouping-key count.
        actual: usize,
    },

    /// Two entries shared a grouping-key name. The reported index
    /// is the second occurrence (the first wins).
    #[error("duplicate grouping attribute at index {index}")]
    DuplicateAttribute {
        /// Position of the duplicate (the second occurrence).
        index: usize,
    },
}

impl GroupingSet {
    /// Validate `keys` against the grouping-set rule and wrap.
    ///
    /// # Errors
    ///
    /// Returns `GroupCount` if the list length is outside
    /// `0..=MAX_SCHEMA_ATTRIBUTES`, or `DuplicateAttribute` if two
    /// entries share a name.
    #[inline]
    pub fn try_new(keys: Vec<AttributeName>) -> Result<Self, GroupingSetError> {
        Refined::try_new(keys).map(Self).map_err(|err| match err {
            CollectionError::LenOutOfRange { actual } => GroupingSetError::GroupCount { actual },
            CollectionError::DuplicateKey { index } => {
                GroupingSetError::DuplicateAttribute { index }
            }
            _ => unreachable!("GroupingSetRule emits only LenOutOfRange / DuplicateKey"),
        })
    }

    /// Borrow the underlying name list.
    #[must_use]
    #[inline]
    pub const fn as_slice(&self) -> &[AttributeName] {
        self.0.as_inner().as_slice()
    }

    /// Consume the wrapper and return the inner name list.
    #[must_use]
    #[inline]
    pub fn into_inner(self) -> Vec<AttributeName> {
        self.0.into_inner()
    }
}

// ─── Aggregate output: length-bounded and unique-by-output-name. ─

type NamedAggSetRule =
    And<LenItems<1, { MAX_SCHEMA_ATTRIBUTES }>, UniqueByKey<NamedAgg, NamedAggKey>>;

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
///
/// Flat domain-shaped enum: uniqueness is on each aggregate's
/// output attribute name, so a duplicate is reported as
/// `DuplicateOutputName`.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[non_exhaustive]
pub enum NamedAggSetError {
    /// Aggregate count fell outside `1..=MAX_SCHEMA_ATTRIBUTES`.
    #[error("named-aggregate count out of range (actual: {actual})")]
    AggregateCount {
        /// Observed aggregate count.
        actual: usize,
    },

    /// Two aggregates produced the same output attribute name. The
    /// reported index is the second occurrence (the first wins).
    #[error("duplicate aggregate output name at index {index}")]
    DuplicateOutputName {
        /// Position of the duplicate (the second occurrence).
        index: usize,
    },
}

impl NamedAggSet {
    /// Validate `aggs` against the named-aggregate-set rule and wrap.
    ///
    /// # Errors
    ///
    /// Returns `AggregateCount` if the list length is outside
    /// `1..=MAX_SCHEMA_ATTRIBUTES`, or `DuplicateOutputName` if two
    /// aggregates share an output name.
    #[inline]
    pub fn try_new(aggs: Vec<NamedAgg>) -> Result<Self, NamedAggSetError> {
        Refined::try_new(aggs).map(Self).map_err(|err| match err {
            CollectionError::LenOutOfRange { actual } => {
                NamedAggSetError::AggregateCount { actual }
            }
            CollectionError::DuplicateKey { index } => {
                NamedAggSetError::DuplicateOutputName { index }
            }
            _ => unreachable!("NamedAggSetRule emits only LenOutOfRange / DuplicateKey"),
        })
    }

    /// Borrow the underlying aggregate list.
    #[must_use]
    #[inline]
    pub const fn as_slice(&self) -> &[NamedAgg] {
        self.0.as_inner().as_slice()
    }

    /// Consume the wrapper and return the inner aggregate list.
    #[must_use]
    #[inline]
    pub fn into_inner(self) -> Vec<NamedAgg> {
        self.0.into_inner()
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

    /// Set operators require the two operands to share a schema.
    #[error("set operator operands have non-matching schemas")]
    SchemaMismatch,

    /// Equi-join referenced an attribute on one side but its type
    /// did not match the corresponding attribute on the other side.
    #[error("equi-join attribute `{attribute}` has mismatched types on the two sides")]
    JoinTypeMismatch {
        /// The colliding attribute name (the side reported is the
        /// side that introduced the mismatch).
        attribute: AttributeName,
    },

    /// `Extend` or `Summarize` failed expression type inference.
    #[error("type inference: {0}")]
    Infer(#[source] crate::infer::InferError),

    /// An aggregate (`Expression::Agg`) appeared in a context that
    /// does not admit it. Aggregates are admissible only inside
    /// `Summarize`'s `aggs` set, not in `Restrict`, `Extend`, or
    /// the predicate of a theta `Join`.
    #[error("aggregate not admissible outside Summarize")]
    AggregateOutsideSummarize,

    /// `Unnest` / `Modify` reached an attribute whose type is not a
    /// nested relation.
    #[error("path target attribute `{attribute}` is not a nested relation")]
    NotARelation {
        /// The offending attribute name.
        attribute: AttributeName,
    },

    /// `Unnest` / `Modify` was given a path shape this version of
    /// the constructor does not yet support.
    ///
    /// V0 supports lens paths of the form `[Field(name)]` — a
    /// single top-level field reaching a nested relation. Deeper
    /// or traversal-kinded paths land as the path-walking schema
    /// helper is extended.
    #[error("path shape not yet supported by this constructor")]
    UnsupportedPathShape,

    /// `Product` requires the two operands' schemas to be disjoint.
    #[error("attribute `{attribute}` is present in both operands of a product")]
    DuplicateAcrossOperands {
        /// The first attribute name shared between the two schemas.
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
        Self {
            kind: OpKind::Source(src),
            schema,
        }
    }

    /// Project the input's rows down to `attrs`, in the order given.
    ///
    /// # Errors
    ///
    /// Returns `UnknownAttribute` if any name in `attrs` is missing
    /// from `input`'s schema.
    pub fn project(input: Self, attrs: AttributeSet) -> Result<Self, OpError> {
        use crate::schema::{Attribute, Schema};
        let input_schema = input.schema();
        let mut projected: Vec<Attribute> = Vec::with_capacity(attrs.as_slice().len());
        for name in attrs.as_slice() {
            let attr = input_schema
                .find(name)
                .ok_or_else(|| OpError::UnknownAttribute {
                    attribute: name.clone(),
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

    /// Restrict the input's rows by a `Predicate`. The output
    /// schema is identical to the input's.
    ///
    /// For `Predicate::Expr`, the wrapped `BoolExpression` has
    /// already been proved Bool-typed against some schema at
    /// construction. We re-verify against `input.schema()` so
    /// cross-schema misuse is rejected here, and reject any
    /// aggregate sub-expression (aggregates are admissible only
    /// inside `Summarize`). `Predicate::Opaque` skips both checks.
    ///
    /// # Errors
    ///
    /// Returns `Infer` if the wrapped expression fails inference
    /// or does not produce `Type::Bool`. Returns
    /// `AggregateOutsideSummarize` if the predicate contains an
    /// aggregate.
    pub fn restrict(input: Self, predicate: Predicate) -> Result<Self, OpError> {
        use crate::expression::contains_aggregate;
        use crate::ty::Type;

        if let Predicate::Expr(ref bool_expr) = predicate {
            let expr = bool_expr.as_expression();
            if contains_aggregate(expr) {
                return Err(OpError::AggregateOutsideSummarize);
            }
            let ty = crate::infer::infer(expr, input.schema()).map_err(OpError::Infer)?;
            if ty != Type::Bool {
                return Err(OpError::Infer(crate::infer::InferError::TypeMismatch {
                    expected: Type::Bool,
                    got: ty,
                }));
            }
        }
        let schema = input.schema().clone();
        Ok(Self {
            kind: OpKind::Restrict {
                input: Box::new(input),
                predicate,
            },
            schema,
        })
    }

    /// Rename `from` to `to`, preserving order and types.
    ///
    /// # Errors
    ///
    /// Returns `UnknownAttribute` if `from` is missing from the
    /// input schema, and `AttributeAlreadyExists` if `to` is already
    /// present.
    pub fn rename(input: Self, from: AttributeName, to: AttributeName) -> Result<Self, OpError> {
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
                    Attribute {
                        name: to.clone(),
                        ty: a.ty.clone(),
                    }
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

    /// Extend the input with a new attribute. The expression is
    /// type-checked against the input schema and the inferred type
    /// becomes the new attribute's type. Aggregates are rejected
    /// (they are admissible only inside `Summarize`).
    ///
    /// # Errors
    ///
    /// Returns `AttributeAlreadyExists` if `name` already exists in
    /// the input schema, `AggregateOutsideSummarize` if `expr`
    /// contains an aggregate, `Infer` if `expr` does not type-check
    /// against the input schema, and `Schema` if the resulting
    /// header somehow violates the schema invariants (defence in
    /// depth — the disjointness check above prevents this in
    /// practice).
    pub fn extend(input: Self, name: AttributeName, expr: Expression) -> Result<Self, OpError> {
        use crate::expression::contains_aggregate;
        use crate::schema::{Attribute, Schema};

        if input.schema().contains(&name) {
            return Err(OpError::AttributeAlreadyExists { attribute: name });
        }
        if contains_aggregate(&expr) {
            return Err(OpError::AggregateOutsideSummarize);
        }
        let ty = crate::infer::infer(&expr, input.schema()).map_err(OpError::Infer)?;
        let mut combined: Vec<Attribute> = input.schema().attributes().to_vec();
        combined.push(Attribute {
            name: name.clone(),
            ty,
        });
        let schema = Schema::try_new(combined).map_err(OpError::Schema)?;
        Ok(Self {
            kind: OpKind::Extend {
                input: Box::new(input),
                name,
                expr,
            },
            schema,
        })
    }

    /// Modify the nested relation at a path by applying a sub-`Op`.
    ///
    /// V0 supports `path = AnyPath::Lens([Field(name)])` reaching a
    /// top-level `Type::Relation(sub_in)` attribute. The nested
    /// column's sub-schema becomes `sub.schema()` in the output;
    /// callers are responsible for having constructed `sub` against
    /// the original nested sub-schema (its input).
    ///
    /// Deeper paths land once the path-walking schema helper is
    /// generalised — they raise `UnsupportedPathShape` for now.
    ///
    /// # Errors
    ///
    /// Returns `UnsupportedPathShape` for any path that is not a
    /// single-field lens, `UnknownAttribute` if the named
    /// attribute is missing from `input.schema()`, `NotARelation`
    /// if it does not carry a `Type::Relation` shape, and `Schema`
    /// if the resulting header violates an invariant.
    pub fn modify(input: Self, path: AnyPath, sub: Self) -> Result<Self, OpError> {
        use crate::path::PathStep;
        use crate::schema::{Attribute, Schema};
        use crate::ty::Type;

        let AnyPath::Lens(lens) = &path else {
            return Err(OpError::UnsupportedPathShape);
        };
        let [PathStep::Field(target)] = lens.steps() else {
            return Err(OpError::UnsupportedPathShape);
        };

        let attr_ref = input
            .schema()
            .find(target)
            .ok_or_else(|| OpError::UnknownAttribute {
                attribute: target.clone(),
            })?;
        if !matches!(attr_ref.ty, Type::Relation(_)) {
            return Err(OpError::NotARelation {
                attribute: target.clone(),
            });
        }

        let new_sub_schema = sub.schema().clone();
        let new_attrs: Vec<Attribute> = input
            .schema()
            .attributes()
            .iter()
            .map(|a| {
                if &a.name == target {
                    Attribute {
                        name: a.name.clone(),
                        ty: Type::Relation(Box::new(new_sub_schema.clone())),
                    }
                } else {
                    a.clone()
                }
            })
            .collect();
        let schema = Schema::try_new(new_attrs).map_err(OpError::Schema)?;
        Ok(Self {
            kind: OpKind::Modify {
                input: Box::new(input),
                path,
                sub: Box::new(sub),
            },
            schema,
        })
    }

    /// Flatten a nested-relation column.
    ///
    /// V0 supports `path = AnyPath::Lens([Field(name)])` reaching a
    /// top-level `Type::Relation(sub)` attribute. The column is
    /// replaced in the output header by the attributes of the
    /// nested schema (in their original order).
    ///
    /// Deeper paths and array unnest land once the path-walking
    /// schema helper is generalised — both raise
    /// `OpError::UnsupportedPathShape` for now.
    ///
    /// # Errors
    ///
    /// Returns `UnsupportedPathShape` for any path that is not a
    /// single-field lens, `UnknownAttribute` if the named
    /// attribute is missing from `input.schema()`, `NotARelation`
    /// if it does not have a `Type::Relation` shape, and `Schema`
    /// if the resulting header violates an invariant (e.g. a
    /// nested attribute name collides with a sibling).
    pub fn unnest(input: Self, path: AnyPath) -> Result<Self, OpError> {
        use crate::path::PathStep;
        use crate::schema::{Attribute, Schema};
        use crate::ty::Type;

        let AnyPath::Lens(lens) = &path else {
            return Err(OpError::UnsupportedPathShape);
        };
        let [PathStep::Field(target)] = lens.steps() else {
            return Err(OpError::UnsupportedPathShape);
        };

        let attr_ref = input
            .schema()
            .find(target)
            .ok_or_else(|| OpError::UnknownAttribute {
                attribute: target.clone(),
            })?;
        let Type::Relation(sub) = &attr_ref.ty else {
            return Err(OpError::NotARelation {
                attribute: target.clone(),
            });
        };

        let mut flattened: Vec<Attribute> = Vec::new();
        for a in input.schema().attributes() {
            if &a.name == target {
                for inner in sub.attributes() {
                    flattened.push(inner.clone());
                }
            } else {
                flattened.push(a.clone());
            }
        }
        let schema = Schema::try_new(flattened).map_err(OpError::Schema)?;
        Ok(Self {
            kind: OpKind::Unnest {
                input: Box::new(input),
                path,
            },
            schema,
        })
    }

    /// Nest a subset of attributes into a single nested-relation
    /// column.
    ///
    /// Every name in `attrs` is removed from the output header;
    /// a new attribute `into` of type `Type::Relation(sub_schema)`
    /// is appended, where `sub_schema` is the header carrying just
    /// the nested attributes (in their original order).
    ///
    /// # Errors
    ///
    /// Returns `UnknownAttribute` if any name in `attrs` is missing
    /// from `input.schema()`, `AttributeAlreadyExists` if `into`
    /// collides with an attribute that is not being nested, and
    /// `Schema` for the nested-header or output-header invariants.
    pub fn nest(input: Self, attrs: AttributeSet, into: AttributeName) -> Result<Self, OpError> {
        use crate::schema::{Attribute, Schema};
        use crate::ty::Type;

        let nested_names = attrs.as_slice();
        let mut nested: Vec<Attribute> = Vec::with_capacity(nested_names.len());
        for name in nested_names {
            let attr = input
                .schema()
                .find(name)
                .ok_or_else(|| OpError::UnknownAttribute {
                    attribute: name.clone(),
                })?;
            nested.push(attr.clone());
        }
        let sub_schema = Schema::try_new(nested).map_err(OpError::Schema)?;

        if input.schema().contains(&into) && !nested_names.contains(&into) {
            return Err(OpError::AttributeAlreadyExists { attribute: into });
        }

        let mut remaining: Vec<Attribute> = input
            .schema()
            .attributes()
            .iter()
            .filter(|a| !nested_names.contains(&a.name))
            .cloned()
            .collect();
        remaining.push(Attribute {
            name: into.clone(),
            ty: Type::Relation(Box::new(sub_schema)),
        });
        let schema = Schema::try_new(remaining).map_err(OpError::Schema)?;
        Ok(Self {
            kind: OpKind::Nest {
                input: Box::new(input),
                attrs,
                into,
            },
            schema,
        })
    }

    /// Group the input by `by` and compute the named aggregates.
    ///
    /// The output schema is the by-attributes (with their input
    /// types) followed by the aggregates (with their inferred
    /// types). Output names must be unique across by + aggs.
    ///
    /// # Errors
    ///
    /// Returns `UnknownAttribute` if a by-attribute is missing,
    /// `AttributeAlreadyExists` if a by-attribute and an aggregate
    /// share an output name, `Infer` if an aggregate's input type
    /// is not admissible, and `Schema` if the constructed header
    /// somehow fails the invariants.
    pub fn summarize(input: Self, by: GroupingSet, aggs: NamedAggSet) -> Result<Self, OpError> {
        use crate::schema::{Attribute, Schema};
        let mut combined: Vec<Attribute> =
            Vec::with_capacity(by.as_slice().len() + aggs.as_slice().len());
        for name in by.as_slice() {
            let attr = input
                .schema()
                .find(name)
                .ok_or_else(|| OpError::UnknownAttribute {
                    attribute: name.clone(),
                })?;
            combined.push(attr.clone());
        }
        for agg in aggs.as_slice() {
            // Detect by/agg output-name collisions before they
            // surface as the less-specific Schema duplicate-key
            // error.
            if by.as_slice().contains(&agg.name) {
                return Err(OpError::AttributeAlreadyExists {
                    attribute: agg.name.clone(),
                });
            }
            let ty = crate::infer::agg_ty(&agg.agg, input.schema()).map_err(OpError::Infer)?;
            combined.push(Attribute {
                name: agg.name.clone(),
                ty,
            });
        }
        let schema = Schema::try_new(combined).map_err(OpError::Schema)?;
        Ok(Self {
            kind: OpKind::Summarize {
                input: Box::new(input),
                by,
                aggs,
            },
            schema,
        })
    }

    /// Set union. Output schema = left's; right must share the same
    /// schema.
    ///
    /// # Errors
    ///
    /// Returns `SchemaMismatch` if `left.schema() != right.schema()`.
    pub fn union(left: Self, right: Self) -> Result<Self, OpError> {
        if left.schema() != right.schema() {
            return Err(OpError::SchemaMismatch);
        }
        let schema = left.schema().clone();
        Ok(Self {
            kind: OpKind::Union {
                left: Box::new(left),
                right: Box::new(right),
            },
            schema,
        })
    }

    /// Set intersection. Schema rule identical to `union`.
    ///
    /// # Errors
    ///
    /// Returns `SchemaMismatch` if the operand schemas differ.
    pub fn intersect(left: Self, right: Self) -> Result<Self, OpError> {
        if left.schema() != right.schema() {
            return Err(OpError::SchemaMismatch);
        }
        let schema = left.schema().clone();
        Ok(Self {
            kind: OpKind::Intersect {
                left: Box::new(left),
                right: Box::new(right),
            },
            schema,
        })
    }

    /// Set difference (left minus right). Schema rule identical to
    /// `union`.
    ///
    /// # Errors
    ///
    /// Returns `SchemaMismatch` if the operand schemas differ.
    pub fn difference(left: Self, right: Self) -> Result<Self, OpError> {
        if left.schema() != right.schema() {
            return Err(OpError::SchemaMismatch);
        }
        let schema = left.schema().clone();
        Ok(Self {
            kind: OpKind::Difference {
                left: Box::new(left),
                right: Box::new(right),
            },
            schema,
        })
    }

    /// Join two relations.
    ///
    /// Output schema depends on `on`:
    ///
    /// - `JoinOn::Natural`: equates every attribute that shares a
    ///   name between the two sides (their types must match) and
    ///   coalesces them in the output. Output schema is left's
    ///   attributes followed by right's minus the shared names.
    ///   If there are no shared names, the result is normalised to
    ///   `Op::product(left, right)` — natural-join over disjoint
    ///   schemas is exactly a Cartesian product, and keeping both
    ///   shapes would widen the AST's canonical state.
    /// - `JoinOn::Equi(pairs)`: each `(l, r)` pair must reference
    ///   an attribute that exists on its side and the two types
    ///   must agree. Schema is left's attributes followed by
    ///   right's (which must be name-disjoint, like `product`;
    ///   the equality is enforced at runtime, not by coalescing).
    /// - `JoinOn::Theta(pred)`: same shape as `Equi`
    ///   (name-disjoint concatenation). The predicate is
    ///   re-verified against the combined schema and must produce
    ///   `Type::Bool` — `BoolExpression`'s proof was established
    ///   against some other schema and is not transferable.
    ///
    /// # Errors
    ///
    /// Returns `UnknownAttribute` (equi-join references a missing
    /// attribute), `JoinTypeMismatch` (equi-join or natural-join
    /// joined columns have differing types),
    /// `DuplicateAcrossOperands` (theta/equi-join produced a name
    /// collision in the output), `Infer` (theta predicate fails
    /// type-checking on the combined schema), or
    /// `AggregateOutsideSummarize` (theta predicate contains an
    /// aggregate).
    pub fn join(left: Self, right: Self, kind: JoinKind, on: JoinOn) -> Result<Self, OpError> {
        use crate::expression::contains_aggregate;
        use crate::schema::{Attribute, Schema};
        use crate::ty::Type;

        let schema = match &on {
            JoinOn::Natural => {
                let mut shared: Vec<AttributeName> = Vec::new();
                for l in left.schema().attributes() {
                    if let Some(r) = right.schema().find(&l.name) {
                        if r.ty != l.ty {
                            return Err(OpError::JoinTypeMismatch {
                                attribute: l.name.clone(),
                            });
                        }
                        shared.push(l.name.clone());
                    }
                }
                if shared.is_empty() && matches!(kind, JoinKind::Inner) {
                    // Natural INNER join over disjoint schemas IS
                    // a Cartesian product; normalise. Outer-join
                    // kinds (LeftOuter / RightOuter / FullOuter)
                    // do NOT collapse this way — with an empty
                    // opposite input they still emit padded rows,
                    // which product would drop. Keep the Natural
                    // shape for those kinds so the outerness is
                    // preserved.
                    return Self::product(left, right);
                }
                let mut combined: Vec<Attribute> = left.schema().attributes().to_vec();
                for r in right.schema().attributes() {
                    if !shared.contains(&r.name) {
                        combined.push(r.clone());
                    }
                }
                Schema::try_new(combined).map_err(OpError::Schema)?
            }
            JoinOn::Equi(pairs) => {
                for pair in pairs.as_slice() {
                    let lt = left.schema().find(&pair.left).ok_or_else(|| {
                        OpError::UnknownAttribute {
                            attribute: pair.left.clone(),
                        }
                    })?;
                    let rt = right.schema().find(&pair.right).ok_or_else(|| {
                        OpError::UnknownAttribute {
                            attribute: pair.right.clone(),
                        }
                    })?;
                    if lt.ty != rt.ty {
                        return Err(OpError::JoinTypeMismatch {
                            attribute: pair.left.clone(),
                        });
                    }
                }
                concat_disjoint_schemas(&left, &right)?
            }
            JoinOn::Theta(pred) => {
                let combined = concat_disjoint_schemas(&left, &right)?;
                // Theta predicate's BoolExpression proof was
                // established against some other schema. Re-verify
                // against the joined schema; reject aggregates
                // outside Summarize.
                if let Predicate::Expr(bool_expr) = pred {
                    let expr = bool_expr.as_expression();
                    if contains_aggregate(expr) {
                        return Err(OpError::AggregateOutsideSummarize);
                    }
                    let ty = crate::infer::infer(expr, &combined).map_err(OpError::Infer)?;
                    if ty != Type::Bool {
                        return Err(OpError::Infer(crate::infer::InferError::TypeMismatch {
                            expected: Type::Bool,
                            got: ty,
                        }));
                    }
                }
                combined
            }
        };
        Ok(Self {
            kind: OpKind::Join {
                left: Box::new(left),
                right: Box::new(right),
                kind,
                on,
            },
            schema,
        })
    }

    /// Cartesian product. Output schema is the concatenation of the
    /// operands' attributes. Attribute names must be disjoint —
    /// otherwise the resulting header would violate the per-name
    /// uniqueness invariant. Disambiguate by renaming first.
    ///
    /// # Errors
    ///
    /// Returns `DuplicateAcrossOperands` if any attribute name in
    /// `right`'s schema is also present in `left`'s.
    pub fn product(left: Self, right: Self) -> Result<Self, OpError> {
        let schema = concat_disjoint_schemas(&left, &right)?;
        Ok(Self {
            kind: OpKind::Product {
                left: Box::new(left),
                right: Box::new(right),
            },
            schema,
        })
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

// Helper: concatenate two operands' schemas into a single header,
// requiring name disjointness. Shared by `Op::product`, the
// equi-join branch of `Op::join`, and the theta-join branch.
fn concat_disjoint_schemas(left: &Op, right: &Op) -> Result<crate::schema::Schema, OpError> {
    use crate::schema::{Attribute, Schema};
    for r in right.schema().attributes() {
        if left.schema().contains(&r.name) {
            return Err(OpError::DuplicateAcrossOperands {
                attribute: r.name.clone(),
            });
        }
    }
    let mut combined: Vec<Attribute> =
        Vec::with_capacity(left.schema().attributes().len() + right.schema().attributes().len());
    combined.extend(left.schema().attributes().iter().cloned());
    combined.extend(right.schema().attributes().iter().cloned());
    Schema::try_new(combined).map_err(OpError::Schema)
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

    use super::{
        AttributeSet, AttributeSetError, GroupingSet, GroupingSetError, NamedAggSet,
        NamedAggSetError, Op, OpKind,
    };
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
        assert_eq!(
            result.unwrap_err(),
            AttributeSetError::AttributeCount { actual: 0 },
        );
    }

    #[test]
    fn attribute_set_rejects_duplicate_names() {
        let result = AttributeSet::try_new(vec![attr("a"), attr("a")]);
        assert_eq!(
            result.unwrap_err(),
            AttributeSetError::DuplicateAttribute { index: 1 },
        );
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
        assert_eq!(
            result.unwrap_err(),
            GroupingSetError::DuplicateAttribute { index: 1 },
        );
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
        assert_eq!(
            result.unwrap_err(),
            NamedAggSetError::DuplicateOutputName { index: 1 },
        );
    }

    // ─── Op constructors. ────────────────────────────────────────

    fn two_attr_schema() -> Schema {
        Schema::try_new(vec![
            Attribute {
                name: attr("id"),
                ty: Type::Int64,
            },
            Attribute {
                name: attr("name"),
                ty: Type::String,
            },
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
        assert_eq!(op.schema().attributes()[0].name.as_str(), "name",);
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
        use crate::expression::{BoolExpression, Expression, Predicate};
        use crate::op_enums::BinOp;
        use crate::ty::Value;
        let input = two_attr_source();
        let bool_expr = BoolExpression::try_new(
            input.schema(),
            Expression::BinOp(
                BinOp::Gt,
                alloc::boxed::Box::new(Expression::Attr(attr("id"))),
                alloc::boxed::Box::new(Expression::Lit(Value::Int64(0))),
            ),
        )
        .unwrap();
        let op = Op::restrict(input, Predicate::Expr(bool_expr)).unwrap();
        assert_eq!(op.schema().cardinality(), 2);
    }

    #[test]
    fn op_restrict_rejects_predicate_from_incompatible_schema() {
        use super::OpError;
        use crate::expression::{BoolExpression, Expression, Predicate};
        // Build BoolExpression against a schema that has 'flag: Bool'
        // — Op::restrict's input schema (two_attr_source: id / name)
        // does not, so re-verification on use fails.
        let other_schema = Schema::try_new(vec![Attribute {
            name: attr("flag"),
            ty: Type::Bool,
        }])
        .unwrap();
        let bool_expr =
            BoolExpression::try_new(&other_schema, Expression::Attr(attr("flag"))).unwrap();
        let result = Op::restrict(two_attr_source(), Predicate::Expr(bool_expr));
        let Err(OpError::Infer(_)) = result else {
            unreachable!();
        };
    }

    #[test]
    fn op_rename_swaps_attribute_in_schema() {
        let input = two_attr_source();
        let op = Op::rename(input, attr("name"), attr("full_name")).unwrap();
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

    // ─── Set operators and product. ──────────────────────────────

    #[test]
    fn op_union_accepts_matching_schemas() {
        let op = Op::union(two_attr_source(), two_attr_source()).unwrap();
        assert_eq!(op.schema().cardinality(), 2);
    }

    #[test]
    fn op_union_rejects_mismatched_schemas() {
        use super::OpError;
        let other = Op::source(Source::Table {
            schema: Schema::try_new(vec![Attribute {
                name: attr("only"),
                ty: Type::Int64,
            }])
            .unwrap(),
            name: TableName::try_new("solo".to_string()).unwrap(),
        });
        let result = Op::union(two_attr_source(), other);
        assert_eq!(result.unwrap_err(), OpError::SchemaMismatch);
    }

    #[test]
    fn op_intersect_accepts_matching_schemas() {
        Op::intersect(two_attr_source(), two_attr_source()).unwrap();
    }

    #[test]
    fn op_difference_accepts_matching_schemas() {
        Op::difference(two_attr_source(), two_attr_source()).unwrap();
    }

    #[test]
    fn op_product_concatenates_disjoint_schemas() {
        let other = Op::source(Source::Table {
            schema: Schema::try_new(vec![Attribute {
                name: attr("city"),
                ty: Type::String,
            }])
            .unwrap(),
            name: TableName::try_new("places".to_string()).unwrap(),
        });
        let op = Op::product(two_attr_source(), other).unwrap();
        assert_eq!(op.schema().cardinality(), 3);
        let names: alloc::vec::Vec<_> = op
            .schema()
            .attributes()
            .iter()
            .map(|a| a.name.as_str())
            .collect();
        assert_eq!(names, vec!["id", "name", "city"]);
    }

    #[test]
    fn op_product_rejects_overlapping_schemas() {
        use super::OpError;
        let result = Op::product(two_attr_source(), two_attr_source());
        let Err(OpError::DuplicateAcrossOperands { attribute }) = result else {
            unreachable!();
        };
        assert_eq!(attribute.as_str(), "id");
    }

    // ─── Join. ───────────────────────────────────────────────────

    fn right_orders_source() -> Op {
        Op::source(Source::Table {
            schema: Schema::try_new(vec![
                Attribute {
                    name: attr("order_id"),
                    ty: Type::Int64,
                },
                Attribute {
                    name: attr("user_id"),
                    ty: Type::Int64,
                },
            ])
            .unwrap(),
            name: TableName::try_new("orders".to_string()).unwrap(),
        })
    }

    #[test]
    fn op_join_natural_coalesces_shared_column() {
        // Left has (id, name); right has (id, total).
        let right = Op::source(Source::Table {
            schema: Schema::try_new(vec![
                Attribute {
                    name: attr("id"),
                    ty: Type::Int64,
                },
                Attribute {
                    name: attr("total"),
                    ty: Type::Int64,
                },
            ])
            .unwrap(),
            name: TableName::try_new("orders".to_string()).unwrap(),
        });
        let op = Op::join(
            two_attr_source(),
            right,
            crate::op_enums::JoinKind::Inner,
            crate::join::JoinOn::Natural,
        )
        .unwrap();
        let names: alloc::vec::Vec<_> = op
            .schema()
            .attributes()
            .iter()
            .map(|a| a.name.as_str())
            .collect();
        assert_eq!(names, vec!["id", "name", "total"]);
    }

    #[test]
    fn op_join_natural_rejects_type_mismatch_on_shared() {
        use super::OpError;
        // Left has id: Int64; right's id is String.
        let right = Op::source(Source::Table {
            schema: Schema::try_new(vec![Attribute {
                name: attr("id"),
                ty: Type::String,
            }])
            .unwrap(),
            name: TableName::try_new("other".to_string()).unwrap(),
        });
        let result = Op::join(
            two_attr_source(),
            right,
            crate::op_enums::JoinKind::Inner,
            crate::join::JoinOn::Natural,
        );
        let Err(OpError::JoinTypeMismatch { attribute }) = result else {
            unreachable!();
        };
        assert_eq!(attribute.as_str(), "id");
    }

    #[test]
    fn op_join_equi_concatenates_when_disjoint() {
        let pairs = crate::join::EquiPairs::try_new(vec![crate::join::EquiPair {
            left: attr("id"),
            right: attr("user_id"),
        }])
        .unwrap();
        let op = Op::join(
            two_attr_source(),
            right_orders_source(),
            crate::op_enums::JoinKind::Inner,
            crate::join::JoinOn::Equi(pairs),
        )
        .unwrap();
        assert_eq!(op.schema().cardinality(), 4);
    }

    #[test]
    fn op_join_equi_rejects_unknown_attr() {
        use super::OpError;
        let pairs = crate::join::EquiPairs::try_new(vec![crate::join::EquiPair {
            left: attr("missing"),
            right: attr("user_id"),
        }])
        .unwrap();
        let result = Op::join(
            two_attr_source(),
            right_orders_source(),
            crate::op_enums::JoinKind::Inner,
            crate::join::JoinOn::Equi(pairs),
        );
        let Err(OpError::UnknownAttribute { attribute }) = result else {
            unreachable!();
        };
        assert_eq!(attribute.as_str(), "missing");
    }

    #[test]
    fn op_join_equi_rejects_type_mismatch() {
        use super::OpError;
        // Left.name: String, right.user_id: Int64 — mismatched.
        let pairs = crate::join::EquiPairs::try_new(vec![crate::join::EquiPair {
            left: attr("name"),
            right: attr("user_id"),
        }])
        .unwrap();
        let result = Op::join(
            two_attr_source(),
            right_orders_source(),
            crate::op_enums::JoinKind::Inner,
            crate::join::JoinOn::Equi(pairs),
        );
        let Err(OpError::JoinTypeMismatch { attribute }) = result else {
            unreachable!();
        };
        assert_eq!(attribute.as_str(), "name");
    }

    fn disjoint_right_source() -> Op {
        Op::source(Source::Table {
            schema: Schema::try_new(vec![Attribute {
                name: attr("city"),
                ty: Type::String,
            }])
            .unwrap(),
            name: TableName::try_new("places".to_string()).unwrap(),
        })
    }

    #[test]
    fn op_join_natural_inner_with_no_shared_attrs_normalises_to_product() {
        // Inner natural join over disjoint schemas IS a Cartesian
        // product — normalise to the canonical operator.
        let op = Op::join(
            two_attr_source(),
            disjoint_right_source(),
            crate::op_enums::JoinKind::Inner,
            crate::join::JoinOn::Natural,
        )
        .unwrap();
        assert!(matches!(op.kind(), OpKind::Product { .. }));
        assert_eq!(op.schema().cardinality(), 3);
    }

    #[test]
    fn op_join_natural_outer_with_no_shared_attrs_preserves_natural_shape() {
        // LeftOuter / RightOuter / FullOuter natural-join over
        // disjoint schemas does NOT collapse to product: when one
        // side is empty, the outer kind still emits padded rows
        // which product would drop. The smart constructor must
        // preserve the Natural shape so the outerness survives.
        for kind in [
            crate::op_enums::JoinKind::LeftOuter,
            crate::op_enums::JoinKind::RightOuter,
            crate::op_enums::JoinKind::FullOuter,
        ] {
            let op = Op::join(
                two_attr_source(),
                disjoint_right_source(),
                kind,
                crate::join::JoinOn::Natural,
            )
            .unwrap();
            assert!(
                matches!(op.kind(), OpKind::Join { .. }),
                "outer-join natural over disjoint schemas must \
                 not collapse to Product (kind: {kind:?})",
            );
            assert_eq!(op.schema().cardinality(), 3);
        }
    }

    #[test]
    fn op_join_theta_admits_bool_predicate_over_combined_schema() {
        use crate::expression::{BoolExpression, Expression, Predicate};
        use crate::op_enums::BinOp;

        let left = two_attr_source();
        let right = right_orders_source();
        let combined = Schema::try_new(vec![
            Attribute {
                name: attr("id"),
                ty: Type::Int64,
            },
            Attribute {
                name: attr("name"),
                ty: Type::String,
            },
            Attribute {
                name: attr("order_id"),
                ty: Type::Int64,
            },
            Attribute {
                name: attr("user_id"),
                ty: Type::Int64,
            },
        ])
        .unwrap();
        let bool_expr = BoolExpression::try_new(
            &combined,
            Expression::BinOp(
                BinOp::Eq,
                alloc::boxed::Box::new(Expression::Attr(attr("id"))),
                alloc::boxed::Box::new(Expression::Attr(attr("user_id"))),
            ),
        )
        .unwrap();
        let op = Op::join(
            left,
            right,
            crate::op_enums::JoinKind::Inner,
            crate::join::JoinOn::Theta(Predicate::Expr(bool_expr)),
        )
        .unwrap();
        assert_eq!(op.schema().cardinality(), 4);
    }

    #[test]
    fn op_join_theta_rejects_predicate_with_unknown_attr() {
        use super::OpError;
        use crate::expression::{BoolExpression, Expression, Predicate};

        // Predicate refers to 'flag' which is in neither side's
        // schema. The BoolExpression built against an unrelated
        // schema reaches Op::join's re-verification on the
        // combined schema and fails.
        let other_schema = Schema::try_new(vec![Attribute {
            name: attr("flag"),
            ty: Type::Bool,
        }])
        .unwrap();
        let bool_expr =
            BoolExpression::try_new(&other_schema, Expression::Attr(attr("flag"))).unwrap();
        let result = Op::join(
            two_attr_source(),
            right_orders_source(),
            crate::op_enums::JoinKind::Inner,
            crate::join::JoinOn::Theta(Predicate::Expr(bool_expr)),
        );
        let Err(OpError::Infer(_)) = result else {
            unreachable!();
        };
    }

    #[test]
    fn op_join_theta_rejects_aggregate_predicate() {
        use super::OpError;
        use crate::expression::{BoolExpression, Expression, Predicate};
        use crate::op_enums::{Agg, BinOp};
        use crate::ty::Value;

        // BoolExpression with an Agg sub-expression. The proof at
        // BoolExpression construction passed only because the
        // wrapping comparison still infers Bool against a schema
        // that has 'count' as Int64. Op::join's theta branch
        // rejects the aggregate in this non-summarize context.
        let bool_expr_schema = Schema::try_new(vec![Attribute {
            name: attr("count"),
            ty: Type::Int64,
        }])
        .unwrap();
        let expr = Expression::BinOp(
            BinOp::Gt,
            alloc::boxed::Box::new(Expression::Agg(Agg::Sum(attr("count")))),
            alloc::boxed::Box::new(Expression::Lit(Value::Int64(0))),
        );
        let bool_expr = BoolExpression::try_new(&bool_expr_schema, expr).unwrap();
        let result = Op::join(
            two_attr_source(),
            right_orders_source(),
            crate::op_enums::JoinKind::Inner,
            crate::join::JoinOn::Theta(Predicate::Expr(bool_expr)),
        );
        assert_eq!(result.unwrap_err(), OpError::AggregateOutsideSummarize);
    }

    // ─── Extend. ─────────────────────────────────────────────────

    #[test]
    fn op_extend_adds_inferred_attribute() {
        use crate::expression::Expression;
        use crate::op_enums::BinOp;
        use crate::ty::Value;
        let expr = Expression::BinOp(
            BinOp::Add,
            alloc::boxed::Box::new(Expression::Attr(attr("id"))),
            alloc::boxed::Box::new(Expression::Lit(Value::Int64(1))),
        );
        let op = Op::extend(two_attr_source(), attr("plus_one"), expr).unwrap();
        assert_eq!(op.schema().cardinality(), 3);
        let added = op.schema().find(&attr("plus_one")).unwrap();
        assert_eq!(added.ty, crate::ty::Type::Int64);
    }

    #[test]
    fn op_extend_rejects_existing_name() {
        use super::OpError;
        use crate::expression::Expression;
        use crate::ty::Value;
        let expr = Expression::Lit(Value::Int64(0));
        let result = Op::extend(two_attr_source(), attr("id"), expr);
        let Err(OpError::AttributeAlreadyExists { attribute }) = result else {
            unreachable!();
        };
        assert_eq!(attribute.as_str(), "id");
    }

    #[test]
    fn op_extend_propagates_infer_error() {
        use super::OpError;
        use crate::expression::Expression;
        // age does not exist on `two_attr_source()` (which has id /
        // name) → infer reports UnknownAttribute, Op::extend wraps
        // it in OpError::Infer.
        let expr = Expression::Attr(attr("age"));
        let result = Op::extend(two_attr_source(), attr("new"), expr);
        let Err(OpError::Infer(_)) = result else {
            unreachable!();
        };
    }

    #[test]
    fn op_extend_rejects_aggregate_expression() {
        use super::OpError;
        use crate::expression::Expression;
        use crate::op_enums::Agg;
        let expr = Expression::Agg(Agg::Sum(attr("id")));
        let result = Op::extend(two_attr_source(), attr("total"), expr);
        assert_eq!(result.unwrap_err(), OpError::AggregateOutsideSummarize);
    }

    #[test]
    fn op_restrict_rejects_aggregate_predicate() {
        use super::OpError;
        use crate::expression::{BoolExpression, Expression, Predicate};
        use crate::op_enums::{Agg, BinOp};
        use crate::ty::Value;
        // Build a Bool-typed expression that contains an aggregate
        // against a schema where 'id' is Int64.
        let bool_expr = BoolExpression::try_new(
            two_attr_source().schema(),
            Expression::BinOp(
                BinOp::Gt,
                alloc::boxed::Box::new(Expression::Agg(Agg::Sum(attr("id")))),
                alloc::boxed::Box::new(Expression::Lit(Value::Int64(0))),
            ),
        )
        .unwrap();
        let result = Op::restrict(two_attr_source(), Predicate::Expr(bool_expr));
        assert_eq!(result.unwrap_err(), OpError::AggregateOutsideSummarize);
    }

    // ─── Summarize. ──────────────────────────────────────────────

    #[test]
    fn op_summarize_builds_schema_with_by_and_aggs() {
        let by = GroupingSet::try_new(vec![attr("name")]).unwrap();
        let aggs = NamedAggSet::try_new(vec![
            NamedAgg {
                name: attr("count"),
                agg: Agg::Count(None),
            },
            NamedAgg {
                name: attr("total"),
                agg: Agg::Sum(attr("id")),
            },
        ])
        .unwrap();
        let op = Op::summarize(two_attr_source(), by, aggs).unwrap();
        let names: alloc::vec::Vec<_> = op
            .schema()
            .attributes()
            .iter()
            .map(|a| a.name.as_str())
            .collect();
        assert_eq!(names, vec!["name", "count", "total"]);
        let total = op.schema().find(&attr("total")).unwrap();
        assert_eq!(total.ty, crate::ty::Type::Int64);
    }

    #[test]
    fn op_summarize_admits_empty_by() {
        let by = GroupingSet::try_new(vec![]).unwrap();
        let aggs = NamedAggSet::try_new(vec![NamedAgg {
            name: attr("count"),
            agg: Agg::Count(None),
        }])
        .unwrap();
        let op = Op::summarize(two_attr_source(), by, aggs).unwrap();
        assert_eq!(op.schema().cardinality(), 1);
    }

    #[test]
    fn op_summarize_rejects_unknown_by_attr() {
        use super::OpError;
        let by = GroupingSet::try_new(vec![attr("missing")]).unwrap();
        let aggs = NamedAggSet::try_new(vec![NamedAgg {
            name: attr("count"),
            agg: Agg::Count(None),
        }])
        .unwrap();
        let result = Op::summarize(two_attr_source(), by, aggs);
        let Err(OpError::UnknownAttribute { attribute }) = result else {
            unreachable!();
        };
        assert_eq!(attribute.as_str(), "missing");
    }

    #[test]
    fn op_summarize_rejects_agg_name_colliding_with_by() {
        use super::OpError;
        let by = GroupingSet::try_new(vec![attr("name")]).unwrap();
        let aggs = NamedAggSet::try_new(vec![NamedAgg {
            name: attr("name"),
            agg: Agg::Count(None),
        }])
        .unwrap();
        let result = Op::summarize(two_attr_source(), by, aggs);
        let Err(OpError::AttributeAlreadyExists { attribute }) = result else {
            unreachable!();
        };
        assert_eq!(attribute.as_str(), "name");
    }

    #[test]
    fn op_summarize_propagates_infer_error_for_string_sum() {
        use super::OpError;
        let by = GroupingSet::try_new(vec![]).unwrap();
        let aggs = NamedAggSet::try_new(vec![NamedAgg {
            name: attr("total_names"),
            agg: Agg::Sum(attr("name")),
        }])
        .unwrap();
        let result = Op::summarize(two_attr_source(), by, aggs);
        let Err(OpError::Infer(_)) = result else {
            unreachable!();
        };
    }

    // ─── Nest. ───────────────────────────────────────────────────

    fn three_attr_source() -> Op {
        Op::source(Source::Table {
            schema: Schema::try_new(vec![
                Attribute {
                    name: attr("id"),
                    ty: Type::Int64,
                },
                Attribute {
                    name: attr("name"),
                    ty: Type::String,
                },
                Attribute {
                    name: attr("city"),
                    ty: Type::String,
                },
            ])
            .unwrap(),
            name: TableName::try_new("users".to_string()).unwrap(),
        })
    }

    #[test]
    fn op_nest_bundles_attributes_under_target_name() {
        let attrs = AttributeSet::try_new(vec![attr("name"), attr("city")]).unwrap();
        let op = Op::nest(three_attr_source(), attrs, attr("profile")).unwrap();
        let names: alloc::vec::Vec<_> = op
            .schema()
            .attributes()
            .iter()
            .map(|a| a.name.as_str())
            .collect();
        assert_eq!(names, vec!["id", "profile"]);
        let profile = op.schema().find(&attr("profile")).unwrap();
        let crate::ty::Type::Relation(sub) = &profile.ty else {
            unreachable!();
        };
        let sub_names: alloc::vec::Vec<_> =
            sub.attributes().iter().map(|a| a.name.as_str()).collect();
        assert_eq!(sub_names, vec!["name", "city"]);
    }

    #[test]
    fn op_nest_rejects_unknown_attr() {
        use super::OpError;
        let attrs = AttributeSet::try_new(vec![attr("missing")]).unwrap();
        let result = Op::nest(three_attr_source(), attrs, attr("nested"));
        let Err(OpError::UnknownAttribute { attribute }) = result else {
            unreachable!();
        };
        assert_eq!(attribute.as_str(), "missing");
    }

    #[test]
    fn op_nest_rejects_into_colliding_with_kept_attr() {
        use super::OpError;
        let attrs = AttributeSet::try_new(vec![attr("name"), attr("city")]).unwrap();
        let result = Op::nest(three_attr_source(), attrs, attr("id"));
        let Err(OpError::AttributeAlreadyExists { attribute }) = result else {
            unreachable!();
        };
        assert_eq!(attribute.as_str(), "id");
    }

    #[test]
    fn op_nest_allows_into_reusing_a_nested_name() {
        // When 'name' is itself being nested, reusing 'name' as
        // the target name does not collide (the old 'name' attr is
        // removed before the new nested attr is appended).
        let attrs = AttributeSet::try_new(vec![attr("name"), attr("city")]).unwrap();
        let op = Op::nest(three_attr_source(), attrs, attr("name")).unwrap();
        let names: alloc::vec::Vec<_> = op
            .schema()
            .attributes()
            .iter()
            .map(|a| a.name.as_str())
            .collect();
        assert_eq!(names, vec!["id", "name"]);
    }

    // ─── Unnest. ─────────────────────────────────────────────────

    fn nested_source() -> Op {
        // Build via Op::nest so the nested-relation shape is
        // identical to what a user would produce.
        let attrs = AttributeSet::try_new(vec![attr("name"), attr("city")]).unwrap();
        Op::nest(three_attr_source(), attrs, attr("profile")).unwrap()
    }

    #[test]
    fn op_unnest_flattens_nested_relation() {
        use crate::path::{AnyPath, LensPath, PathStep};
        let path =
            AnyPath::Lens(LensPath::try_new(vec![PathStep::Field(attr("profile"))]).unwrap());
        let op = Op::unnest(nested_source(), path).unwrap();
        let names: alloc::vec::Vec<_> = op
            .schema()
            .attributes()
            .iter()
            .map(|a| a.name.as_str())
            .collect();
        assert_eq!(names, vec!["id", "name", "city"]);
    }

    #[test]
    fn op_unnest_rejects_traversal_path() {
        use super::OpError;
        use crate::path::{AnyPath, PathStep, TraversalPath};
        let path = AnyPath::Traversal(
            TraversalPath::try_new(vec![PathStep::Field(attr("profile")), PathStep::Each]).unwrap(),
        );
        let result = Op::unnest(nested_source(), path);
        assert_eq!(result.unwrap_err(), OpError::UnsupportedPathShape);
    }

    #[test]
    fn op_unnest_rejects_unknown_attr() {
        use super::OpError;
        use crate::path::{AnyPath, LensPath, PathStep};
        let path =
            AnyPath::Lens(LensPath::try_new(vec![PathStep::Field(attr("missing"))]).unwrap());
        let result = Op::unnest(nested_source(), path);
        let Err(OpError::UnknownAttribute { attribute }) = result else {
            unreachable!();
        };
        assert_eq!(attribute.as_str(), "missing");
    }

    #[test]
    fn op_unnest_rejects_scalar_target() {
        use super::OpError;
        use crate::path::{AnyPath, LensPath, PathStep};
        // three_attr_source has `id` as Int64 — not a relation.
        let path = AnyPath::Lens(LensPath::try_new(vec![PathStep::Field(attr("id"))]).unwrap());
        let result = Op::unnest(three_attr_source(), path);
        let Err(OpError::NotARelation { attribute }) = result else {
            unreachable!();
        };
        assert_eq!(attribute.as_str(), "id");
    }

    // ─── Modify. ─────────────────────────────────────────────────

    #[test]
    fn op_modify_replaces_nested_subschema() {
        use crate::path::{AnyPath, LensPath, PathStep};
        let input = nested_source();
        // Build a sub-Op whose output schema is just `city`. Use a
        // freshly-constructed source so the test doesn't care
        // about how the user gets to that schema.
        let sub = Op::source(Source::Table {
            schema: Schema::try_new(vec![Attribute {
                name: attr("city"),
                ty: Type::String,
            }])
            .unwrap(),
            name: TableName::try_new("inner".to_string()).unwrap(),
        });
        let path =
            AnyPath::Lens(LensPath::try_new(vec![PathStep::Field(attr("profile"))]).unwrap());
        let op = Op::modify(input, path, sub).unwrap();
        let profile = op.schema().find(&attr("profile")).unwrap();
        let crate::ty::Type::Relation(sub_schema) = &profile.ty else {
            unreachable!();
        };
        let inner_names: alloc::vec::Vec<_> = sub_schema
            .attributes()
            .iter()
            .map(|a| a.name.as_str())
            .collect();
        assert_eq!(inner_names, vec!["city"]);
    }

    #[test]
    fn op_modify_rejects_unknown_attr() {
        use super::OpError;
        use crate::path::{AnyPath, LensPath, PathStep};
        let sub = Op::source(Source::Table {
            schema: Schema::try_new(vec![Attribute {
                name: attr("x"),
                ty: Type::Int64,
            }])
            .unwrap(),
            name: TableName::try_new("inner".to_string()).unwrap(),
        });
        let path =
            AnyPath::Lens(LensPath::try_new(vec![PathStep::Field(attr("missing"))]).unwrap());
        let result = Op::modify(nested_source(), path, sub);
        let Err(OpError::UnknownAttribute { attribute }) = result else {
            unreachable!();
        };
        assert_eq!(attribute.as_str(), "missing");
    }

    #[test]
    fn op_modify_rejects_scalar_target() {
        use super::OpError;
        use crate::path::{AnyPath, LensPath, PathStep};
        let sub = Op::source(Source::Table {
            schema: Schema::try_new(vec![Attribute {
                name: attr("x"),
                ty: Type::Int64,
            }])
            .unwrap(),
            name: TableName::try_new("inner".to_string()).unwrap(),
        });
        let path = AnyPath::Lens(LensPath::try_new(vec![PathStep::Field(attr("id"))]).unwrap());
        let result = Op::modify(three_attr_source(), path, sub);
        let Err(OpError::NotARelation { attribute }) = result else {
            unreachable!();
        };
        assert_eq!(attribute.as_str(), "id");
    }
}
