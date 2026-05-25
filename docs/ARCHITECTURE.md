# Axiom Relational Algebra Library Architecture Specification

Status of This Memo

This document is an internal project specification written in an
RFC-style Markdown form. The document borrows structure and editorial
discipline from RFC 7322 and uses RFC 2026 as process vocabulary for
maturity, review, and applicability.

This document is the concrete architecture for the Axiom Relational
Algebra Library. It is derived from [IDEA.md](IDEA.md), which is
authoritative for goals, scope, non-goals, and invariants. When this
document conflicts with [IDEA.md](IDEA.md), [IDEA.md](IDEA.md) takes
precedence.

Section 16 is non-normative sequencing guidance. Section 17 is a
non-normative open-issue list. Staged decisions MUST NOT weaken the
invariants in [IDEA.md](IDEA.md).

Abstract

This document specifies the architecture for a Rust library that
exposes a relational-algebra AST executable through either an
in-memory iterator backend or a PostgreSQL backend. The architecture
uses a Cargo workspace with one thin facade package and five member
crates: a pure core crate that holds the AST, schema, paths, and
expression DSL; a derive-macro crate; an optimizer crate; an in-memory
backend crate; and a PostgreSQL backend crate. The architecture is
cassette-driven for SQL output, encodes domain invariants by
representation where feasible, and treats new backends as strictly
local additions.

Table of Contents

- [Section 1: Introduction](#1-introduction)
- [Section 2: Requirements Language](#2-requirements-language)
- [Section 3: Sources](#3-sources)
- [Section 4: Foundations](#4-foundations)
- [Section 5: Toolchain and Gates](#5-toolchain-and-gates)
- [Section 6: Crate and Module Shape](#6-crate-and-module-shape)
- [Section 7: Dependency Direction](#7-dependency-direction)
- [Section 8: Schema and Tuple](#8-schema-and-tuple)
- [Section 9: Algebra and AST](#9-algebra-and-ast)
- [Section 10: Paths](#10-paths)
- [Section 11: Expressions and Constraint System](#11-expressions-and-constraint-system)
- [Section 12: Optimizer](#12-optimizer)
- [Section 13: In-Memory Backend](#13-in-memory-backend)
- [Section 14: PostgreSQL Backend](#14-postgresql-backend)
- [Section 15: Testing Architecture](#15-testing-architecture)
- [Section 16: Build Sequence](#16-build-sequence)
- [Section 17: Open Issues](#17-open-issues)
- [Section 18: References](#18-references)

## 1. Introduction

The Axiom Relational Algebra Library is a Rust library that hosts a
relational-algebra AST and one or more backends that execute it. The
library validates inputs at construction, treats the query value as
data the optimizer can rewrite, and exposes only the same operator
surface to every backend.

This document specifies the concrete mechanisms that realize the
requirements in [IDEA.md](IDEA.md). The architecture is a Technical
Specification in the RFC 2026 sense: it describes concrete
procedures, conventions, and formats for this library.

## 2. Requirements Language

The key words "MUST", "MUST NOT", "REQUIRED", "SHALL", "SHALL NOT",
"SHOULD", "SHOULD NOT", "RECOMMENDED", "NOT RECOMMENDED", "MAY", and
"OPTIONAL" in this document are to be interpreted as described in
BCP 14 [RFC2119] [RFC8174] when, and only when, they appear in all
capitals, as shown here.

Lowercase uses of these words have their ordinary English meanings.

## 3. Sources

The authoritative source is:

- [IDEA.md](IDEA.md), which is authoritative for goals, scope,
  non-goals, and invariants.

Reference inputs are:

- the Ruby [AXIOM] library, which establishes the operator surface,
  the deferred-execution model, and the term vocabulary;
- the [AXIOM-OPT] optimizer, which establishes the AST-rewrite
  separation from the operator interpretation;
- [DATE2005] and [DATE2011], which establish the relational-theory
  reading of the algebra;
- [JAESCHKE1982], which establishes the non-first-normal-form
  extension;
- [KING2019] for the parse-don't-validate principle applied to smart
  constructors;
- earlier design notes recorded in [DESIGN.md](DESIGN.md), which this
  document supersedes for architectural commitments.

## 4. Foundations

The implementation uses these concrete foundations:

- Language: Rust, edition 2024.
- Toolchain: pinned per commit via `rust-toolchain.toml`.
- Workspace shape: one Cargo workspace containing a thin facade
  package at the workspace root and five member crates under
  `crates/`.
- Async runtime: Tokio, used by the PostgreSQL backend only. The
  core, optimizer, and in-memory backend are intentionally async-free.
- PostgreSQL client: `tokio-postgres` with `rustls-tls`.
- Serialization: `serde` with derive support.
- Numeric representation for SQL `NUMERIC` and exact arithmetic:
  `rust_decimal`.
- Time representation: `chrono` with `clock` and `serde`.
- Bounded collections: `bounded-vec`, `non-empty-string`, `nonempty`,
  shared across crates via `workspace.dependencies`.
- Error handling: `thiserror` for typed error definitions.
- Proc-macro support: `proc-macro2`, `syn`, `quote`.

Local `axiom-core` domain types built on the foundations above include
`BoundedOrderedSet`, `BoundedOrderedSetByKey`, `BoundedIndex`,
`Offset`, `LimitCount`, `Row`, `OrderKeys`, `CanonicalPredicate`, and
the sealed-witness types backing `SmartConstructor`. These are part
of the crate's public surface, not external dependencies. `SqlText`,
`SqlParam`, and `CompiledSql` are PostgreSQL-backend-specific and
live in `axiom-pg`, not in `axiom-core`.

### 4.1. Constants

The numeric limits referenced throughout the architecture live in
`axiom-core::limits` as `pub const` items. The initial values satisfy
the minimum upper bounds required by [IDEA.md](IDEA.md) Section 5.14:

| Constant | Initial value |
| --- | --- |
| `MAX_ATTRS` | 256 |
| `MAX_PATH_STEPS` | 32 |
| `MAX_IN_LIST` | 1024 |
| `MAX_ROW_CONSTRAINTS` | 256 |
| `MAX_PREDICATE_CLAUSES` | 4096 (canonical-form size budget, §11.2) |
| `MAX_PATH_INDEX` | 65536 (positional index limit inside `BoundedIndex`) |
| `MAX_PARAMS` | 4096 |
| `MAX_OFFSET` | `u64::MAX / 2` |
| `MAX_LIMIT_COUNT` | `u64::MAX / 2` |
| `MAX_ROWS_IN_AST` | 16384 |
| `MAX_ATTRIBUTE_NAME_LEN` | 64 |

Implementations MAY raise these constants for their own build; they
MUST NOT lower any constant below the minimum upper bound required by
[IDEA.md](IDEA.md) Section 5.14.

## 5. Toolchain and Gates

This section specifies the gate vocabulary the implementation will
adopt. The files it references (`justfile`, `.cargo/clippy.toml`,
`.cargo/config.toml`, `.cargo/deny.toml`, `rust-toolchain.toml`) are
the target scaffold and MAY not exist in the repository yet; the
docs-only commits in the initial history deliberately ship before the
implementation scaffold.

The local gate vocabulary is provided by `just` and cargo aliases:

- `just fmt-check` runs `cargo fmt --all --check`.
- `just lint` runs the `clippy-all` alias, which expands to
  `cargo clippy --all-features --all-targets --tests --workspace`.
- `just test` runs the `test-all` alias (nextest under the hood) plus
  `cargo test --doc --workspace --all-features`.
- `just docs` runs `mado check` against the committed Markdown
  documents using the repository's `.mado.toml` configuration.
- `just deny` runs `cargo deny check` against `.cargo/deny.toml`.
- `just coverage` runs `cargo llvm-cov` and asserts zero uncovered
  regions, functions, or lines.
- `just ci` runs the full gate: `check deny`, where `check` itself is
  `fmt-check lint test docs coverage`.
- `just it` runs the opt-in PostgreSQL integration test crate against
  a configured local database. It is excluded from `just ci`.

Lint posture: every default Clippy lint, plus `pedantic`, `nursery`,
`cargo`, and `restriction`, is denied. Every Rustdoc lint is denied.
Suppressions MUST use `#![expect(LINT, reason = "…")]` with a reason
string. The unfulfilled-lint-expectations lint is denied so that an
`expect` whose lint does not fire becomes a build failure.

Dependency-license posture: `.cargo/deny.toml` allows the standard
permissive set (MIT, Apache-2.0, Apache-2.0 WITH LLVM-exception,
BSD-3-Clause, ISC, Unicode-3.0, BSL-1.0, CC0-1.0, CDLA-Permissive-2.0,
0BSD, Unlicense, Zlib). Unknown registries are denied. Unknown git
sources are denied.

## 6. Crate and Module Shape

The repository layout is:

```text
axiom-rs/
├── Cargo.toml                  workspace + thin facade package
├── justfile                    gate recipes
├── rust-toolchain.toml
├── .mado.toml                  Markdown lint configuration
├── .cargo/
│   ├── clippy.toml             disallowed-methods configuration
│   ├── config.toml             cargo aliases + build flags
│   └── deny.toml               license allowlist, registry rules
├── src/lib.rs                  re-exports core + selected backend(s)
├── crates/
│   ├── axiom-core/             AST, Schema, Tuple, Path, Expression, Error
│   ├── axiom-derive/           proc macros: Relational, path!, col!, on!
│   ├── axiom-optimizer/        AST rewrites (the AXIOM-OPT analogue)
│   ├── axiom-mem/              in-memory iterator backend
│   └── axiom-pg/               PostgreSQL SQL generator + executor
└── docs/
    ├── README.md
    ├── IDEA.md
    ├── ARCHITECTURE.md
    └── DESIGN.md
```

A second backend MUST be introduced as a new `axiom-<target>` crate.
Shared-crate changes are allowed only for backend registration, facade
feature wiring, tests, or contract-preserving AST support required by
[IDEA.md](IDEA.md); functional rewrite or interpretation logic for the
new backend MUST live in its own crate.

## 7. Dependency Direction

The dependency graph MUST be one-way through the backend boundary:

- `axiom-core` depends on `serde`, `thiserror`, `rust_decimal`,
  `chrono`, `bounded-vec`, `non-empty-string`. It MUST NOT depend on
  any other axiom crate.
- `axiom-derive` depends on `proc-macro2`, `syn`, `quote`. It MUST
  NOT depend on `axiom-core`; proc-macro crates are compiled for the
  host and cannot share types with the target crate.
- `axiom-optimizer` depends on `axiom-core` only. It MUST NOT depend
  on any backend crate.
- `axiom-mem` depends on `axiom-core` and `axiom-optimizer`. It MUST
  NOT depend on `axiom-pg`.
- `axiom-pg` depends on `axiom-core`, `axiom-optimizer`,
  `tokio-postgres`, `rustls`, and `tokio`. It MUST NOT depend on
  `axiom-mem`.
- The root facade `axiom` depends on `axiom-core`, `axiom-optimizer`,
  and on each backend crate behind a Cargo feature. Default features
  enable `axiom-mem` only. A consumer that only wants the in-memory
  backend MUST NOT pull in Tokio, `tokio-postgres`, or any other
  PostgreSQL-only dependency through default features or the
  transitive build graph.

## 8. Schema and Tuple

### 8.1. Attribute Names

```rust
pub struct AttributeName(NonEmptyString);
```

`AttributeName::try_new(s)` enforces the documented identifier grammar
(letters, digits, underscores; not starting with a digit; bounded
length). `AttributeName::new` accepts a compile-time literal validated
by the `attr!` macro in `axiom-derive`.

### 8.2. Types

```rust
pub enum Type {
    Bool, Int32, Int64, Float64,
    Decimal,
    String, Bytes,
    DateTime,
    Json,
    Relation(Box<Schema>),                 // NF² nested relation
    Array(Box<Type>),                      // ordered collection
    Optional(Box<Type>),
}
```

`Type::Relation` is the NF² extension point. An attribute whose type
is `Relation(s)` carries its own schema. `Type::Array(t)` is the
collection form for non-relation nested data such as JSON arrays.

### 8.3. Schema

```rust
pub struct Schema {
    /// Ordered set of attributes keyed by name. Uniqueness is enforced
    /// by the representation, not by a runtime check at access time.
    attributes: BoundedOrderedSetByKey<Attribute, AttributeName, 1, MAX_ATTRS>,
}

pub struct Attribute {
    pub name: AttributeName,
    pub ty: Type,
}
```

`BoundedOrderedSetByKey<T, K, MIN, MAX>` is an ordered collection
whose elements have a key `K` extracted from `T` by a trait method.
Insertion at construction rejects duplicate keys. Iteration order is
insertion order so schema position is stable. `Schema::try_new`
collects attributes through the set's `try_extend`, returning
`RelationError::DuplicateAttribute` on the first collision.
`Schema::new` is reachable only from the derive macro, which has
already verified uniqueness at compile time.

### 8.4. Tuple Trait

```rust
pub trait Tuple {
    fn schema(&self) -> &Schema;
    fn get(&self, name: &AttributeName) -> Option<&Value>;
    fn get_by_index(&self, idx: usize) -> Option<&Value>;
}

pub struct DynamicTuple { schema: Schema, row: Row }
pub struct TypedTuple<T: Relational>(T);
```

`Value` is a closed sum over the variants admitted by `Type`. The
`Relational` trait is derived by `#[derive(Relational)]` and provides
the schema and the position-keyed accessor.

### 8.5. Smart Constructor Trait

Every public type implements one base trait so the `new` constructor,
the admissible input domain, and the invariant witness are exposed
uniformly. Types that narrow (construction can fail) additionally
implement a narrowing extension trait that adds `try_new` over a raw
input domain:

```rust
pub trait SmartConstructor: Sized {
    /// Admissible input domain — a type whose inhabitants are already
    /// proven valid by their own type (typically built from prior
    /// witnesses, derive macros, or composition of narrower types).
    /// The infallible `new` accepts only this domain.
    type Admissible;
    /// Proof type. Unforgeable outside `mod sc::sealed`; downstream
    /// code may hold and copy a `Witness`, but only this crate's
    /// sealed constructor can mint the first one.
    type Witness: Copy;

    /// Infallible constructor. Accepts only a proof-bearing input
    /// whose type already encodes the invariant. There is no path
    /// from a raw input to `Self` that bypasses validation.
    fn new(admissible: Self::Admissible) -> Self;

    /// Recover the witness from an already-constructed value, without
    /// re-running validation.
    fn witness(&self) -> Self::Witness;
}

/// Implemented only by types whose construction narrows a strictly
/// larger raw input domain. Per [IDEA.md](IDEA.md) Section 5.2, types
/// that do not narrow MUST NOT implement this trait.
pub trait Narrowing: SmartConstructor {
    /// Raw input domain — values that may or may not satisfy the
    /// invariant. `try_new` is the only constructor that accepts this.
    type Raw;
    /// Construction-time error variant.
    type Error;

    /// Fallible constructor. Validates `raw` and returns the proven
    /// value; the witness is recoverable via `SmartConstructor::witness`.
    fn try_new(raw: Self::Raw) -> Result<Self, Self::Error>;
}
```

`Witness` is a phantom-typed zero-sized type, distinct per invariant,
whose constructor is sealed inside the validating module; existence
of a `Witness` is proof that the invariants held at construction. The
type MUST NOT be `()` or any other type a downstream crate can
construct on its own, because the sealing requirement is what makes
`Admissible` inputs (and therefore `new`) safe. The only ways an
`Admissible` input arises are: a prior `try_new`, the derive macro, a
compile-time proof macro, or a composition rule whose own correctness
is checked by this trait. Holding a `T` and calling `T::witness` MUST
be cheaper than calling `try_new`.

A single declaration is intended to define both the Rust type and
the runtime validation. The precise syntax is open (see Section 17);
the direction is one type-level declaration per constrained type,
either as a declarative macro, a derive macro on a thin struct
definition, or some hybrid. A sketch of the intended ergonomics:

```rust
refinement! {
    /// Non-empty bounded ASCII identifier.
    pub struct AttributeName(String) {
        max_len: 64,
        pattern: r"^[A-Za-z_][A-Za-z0-9_]*$",
    }
}
```

Whatever the final form, the declaration MUST emit, from one source
of truth:

- the public type and its private representation;
- the `SmartConstructor` impl (with the matching `Raw`,
  `Admissible`, sealed `Witness`, and generated `Error` variant);
- the sealed witness type;
- a `Deserialize` impl (and any other deserialization path the
  library exposes) that routes through `try_new`, so a wire payload
  satisfying the schema but violating the refinement is rejected
  with the same typed error a direct `try_new` would return;
- a `Display`/`AsRef`-style read-only surface that exposes the
  underlying representation without re-validation.

No code path produces a `T` without running the validation. The macro
metadata MUST be structured (not opaque tokens) so a future rustc
extension or external analyser can read it back and lift checks where
the host language allows.

The trait MUST live behind a `pub mod sc` (or equivalent) in
`axiom-core` so it can be relocated wholesale to a future
`axiom-refine` crate (a constraint-and-refinement-propagation crate,
not a refinement-type-checker — see Section 17) without changing any
call site beyond the import path. The trait MUST NOT depend on any
other axiom module, and types that implement it MUST NOT add backend-,
optimizer-, or schema-specific bounds to the trait itself.

## 9. Algebra and AST

### 9.0. Companion Types

The operator AST uses the following supporting types, all defined in
`axiom-core::algebra` unless noted otherwise:

```rust
pub struct TableName(NonEmptyString);
pub enum JoinKind { Inner, LeftOuter, RightOuter, FullOuter }
pub enum JoinOn {
    /// Natural join on shared attribute names; the smart constructor
    /// computes the equated set from the inputs' schemas. When the
    /// schemas share no attributes, classical relational algebra
    /// reduces natural join to cartesian product; the smart
    /// constructor returns `OpKind::Product { .. }` instead of
    /// `OpKind::Join { on: JoinOn::Natural, .. }` in that case.
    Natural,
    /// Equi-join on an explicit set of attribute pairs.
    Equi(BoundedOrderedSet<(AttributeName, AttributeName), 1, MAX_ATTRS>),
    /// Theta-join on an arbitrary boolean predicate. `Predicate` is
    /// defined in §11.1; forward-referenced here.
    Theta(Predicate),
}
pub struct NamedAgg { pub name: AttributeName, pub agg: Agg }
/// Aggregate function. The operand is an `AttributeName` baked into
/// each variant rather than a nested `Expression`; the
/// `Expression::Agg(Agg)` variant in §11.1 wraps this same `Agg`
/// whole.
pub enum Agg {
    /// `COUNT(*)` when None; `COUNT(attr)` when Some.
    Count(Option<AttributeName>),
    Sum(AttributeName),
    Min(AttributeName),
    Max(AttributeName),
    Avg(AttributeName),
}
pub enum BinOp { Add, Sub, Mul, Div, Eq, Ne, Lt, Le, Gt, Ge, And, Or, Concat }
pub enum UnOp  { Neg, Not }
pub struct Pattern(NonEmptyString);  // LIKE-style pattern
pub struct OrderKey {
    pub attr: AttributeName,
    pub dir:  Direction,
    pub nulls: NullOrder,
}
pub enum Direction { Asc, Desc }
pub enum NullOrder { NullsFirst, NullsLast }
/// Ordered set of order keys, keyed on the full canonical `OrderKey`
/// (attribute + direction + null ordering) so duplicate full keys are
/// rejected at construction (per [IDEA.md](IDEA.md) Section 5.3).
/// After insertion the smart constructor MAY drop any later key whose
/// `attr` already appears in an earlier position, because the earlier
/// key already determines that attribute's ordering.
pub struct OrderKeys(
    BoundedOrderedSetByKey<OrderKey, OrderKey, 1, MAX_ATTRS>,
);
pub struct BoundedU64<const MIN: u64, const MAX: u64>(u64);
pub struct BoundedIndex(BoundedU64<0, MAX_PATH_INDEX>);
```

The `Relation` trait that `RelationExt` (§9.2) extends is the
static-dispatch surface for "anything that can be promoted to an
`Op`". It is not object-safe; callers that need a heterogeneous
collection of relations should hold `Op` values directly.

```rust
pub trait Relation: Sized {
    fn into_op(self) -> Op;
    fn schema(&self) -> &Schema;
}
impl Relation for Op { /* identity */ }
```

### 9.1. Operator AST

The `MAX_ATTRS` constant referenced below is the workspace-wide upper
bound on attribute count per operator, exported from `axiom-core::limits`
and equal to the `Schema` upper bound (`256` in the initial design).

Operator fields use the most-constrained admissible collection type
per [IDEA.md](IDEA.md) Section 5.3: `BoundedOrderedSet` for headers
(unique attribute names, order matters); `BoundedOrderedSetByKey` for
aggregate lists (uniqueness by output attribute name); the dedicated
`OrderKeys` newtype (a bounded ordered set keyed by full canonical
`OrderKey`) for order-by lists; `BoundedVec` only where duplicates
are admissible and order matters (`Row` values, `Path` step
sequences).

The public type is `Op`, an opaque wrapper around the crate-private
`OpKind` discriminated sum. External callers see `Op` and the smart
constructors only; optimizer and backend code inside the crate match
on `OpKind` via the `pub(crate) fn kind(&self) -> &OpKind` accessor.
Public backward-compatibility comes from `Op` being an opaque
wrapper around a `pub(crate)` discriminant; the `#[non_exhaustive]`
attribute on `OpKind` is there so crate-internal match sites must
keep a wildcard arm even when a new operator is added.

```rust
pub struct Op { kind: OpKind }

impl Op {
    pub(crate) fn kind(&self) -> &OpKind { &self.kind }
}

#[non_exhaustive]
pub(crate) enum OpKind {
    Source(Source),
    Project { input: Box<Op>, attrs: BoundedOrderedSet<AttributeName, 1, MAX_ATTRS> },
    Restrict { input: Box<Op>, predicate: Predicate },
    Rename { input: Box<Op>, from: AttributeName, to: AttributeName },
    Extend { input: Box<Op>, name: AttributeName, expr: Expression },
    Join { left: Box<Op>, right: Box<Op>, kind: JoinKind, on: JoinOn },
    Product { left: Box<Op>, right: Box<Op> },
    Union { left: Box<Op>, right: Box<Op> },
    Intersect { left: Box<Op>, right: Box<Op> },
    Difference { left: Box<Op>, right: Box<Op> },
    Summarize {
        input: Box<Op>,
        by:   BoundedOrderedSet<AttributeName, 0, MAX_ATTRS>,
        aggs: BoundedOrderedSetByKey<NamedAgg, AttributeName, 1, MAX_ATTRS>,
    },
    Order  { input: Box<Op>, by: OrderKeys },
    Limit  { input: Box<Op>, offset: Offset, count: LimitCount },
    Modify { input: Box<Op>, path: AnyPath, sub: Box<Op> },
    Unnest { input: Box<Op>, path: AnyPath },
    Nest   {
        input: Box<Op>,
        attrs: BoundedOrderedSet<AttributeName, 1, MAX_ATTRS>,
        into:  AttributeName,
    },
}
```

Each variant carries the operator's identity. All `OpKind`-producing
call sites (crate-internal) MUST go through the smart constructors
in Section 9.2; external callers cannot reach `OpKind` because it
is `pub(crate)`.

`Modify::sub` is interpreted against an implicit source whose schema
is the path-focus type; references in `sub` to attributes outside the
focus are rejected at smart-constructor time. The optimizer treats
`sub` as opaque to outer attribute names.

`Offset` is a newtype around `BoundedU64<0, MAX_OFFSET>`. `LimitCount`
is a closed sum:

```rust
pub struct Offset(BoundedU64<0, MAX_OFFSET>);
pub enum LimitCount {
    Unbounded,
    Bounded(BoundedU64<0, MAX_LIMIT_COUNT>),
}
```

`LimitCount::Bounded(0)` is admissible and denotes an empty result
(`LIMIT 0` in SQL). The optimizer MAY rewrite a `Limit` whose count
is `Bounded(0)` to an empty-relation node. `MAX_OFFSET` and
`MAX_LIMIT_COUNT` are each `u64::MAX / 2` (see Section 4.1) so the
sum `offset + limit_count` is computable in `u64` without overflow.

### 9.2. Smart Constructors

Each operator variant has a corresponding builder on the relation
surface. A constructor exposes the fallible `try_*` form when, and
only when, validation can fail (Section 5.2 of [IDEA.md](IDEA.md)):

```rust
pub trait RelationExt: Relation + Sized {
    fn project(self, attrs: impl ProvenAttrs) -> Op;
    fn try_project(self, attrs: impl AttrsRuntime)
        -> Result<Op, RelationError>;
    // similarly for restrict / rename / extend / join / ...
}
impl<R: Relation> RelationExt for R {}
```

Builders return `Op` directly (the public sealed wrapper introduced
above); there are no per-variant wrapper types. The typed `project`
accepts a `ProvenAttrs` impl whose validity is enforced at compile
time by the derive macro. The dynamic `try_project` accepts runtime
strings and returns `RelationError::UnknownAttribute` when one is
absent from the schema.

### 9.3. Source

```rust
pub enum Source {
    Memory {
        schema: Schema,
        rows:   BoundedVec<Row, 0, MAX_ROWS_IN_AST>,
    },
    Table {
        schema: Schema,
        name:   TableName,
    },
}

pub struct Row {
    /// Crate-private. Values in schema order. Constructor-validated
    /// against the owning `Source::Memory`'s `Schema` so width and
    /// per-position types are guaranteed by the *only* constructor.
    values: BoundedVec<Value, 1, MAX_ATTRS>,
}
```

`Row::try_new(schema, values) -> Result<Row, RelationError>` is the
only public path to a `Row`; the inner field is crate-private and no
mutating accessor is exposed, so a `Row` whose width or per-position
types disagree with its owning schema is unrepresentable rather than
merely unvalidated. The witness (`RowWitness`) is recoverable via
`SmartConstructor::witness(&row)`. `Source::Memory::try_new`
constructs each `Row` from caller-supplied tuples and rejects the
entire source on the first mismatch.

`Source::Memory` carries inline data for the in-memory backend.
`Source::Table` is the symbolic table reference for the PostgreSQL
backend. The schema lives in the AST so the optimizer can reason
about types without consulting the backend.

## 10. Paths

### 10.1. Path Type

```rust
pub enum PathStep {
    Field(AttributeName),
    Index(BoundedIndex),
    Each,
}

/// Optic kind. `Kind` is sealed so the only inhabitants are `Lens`
/// and `Traversal`; this is what makes the `Compose` type-level
/// function below total. Adding a third kind requires changes inside
/// this module.
// Private module containing a `pub` trait — the standard sealed-trait
// idiom: external crates cannot name `sealed::Kind`, so they cannot
// satisfy its bound on the public `Kind` trait below.
mod sealed { pub trait Kind {} }
pub trait Kind: sealed::Kind + Sized {}
pub enum Lens {}
pub enum Traversal {}
impl sealed::Kind for Lens {}
impl sealed::Kind for Traversal {}
impl Kind for Lens {}
impl Kind for Traversal {}

// The carrier permits any step in any kind; the `K` tag is enforced
// predicatively by the smart constructor (a `Path<Lens>` is rejected
// at construction if its step sequence contains an `Each`, and a
// `Path<Traversal>` if it contains none). Pushing this invariant into
// the carrier itself (separate step types per kind) is a Section-17
// future tightening; doing so now would force `PathStep` to know its
// kind at the variant level and complicate the `path!` macro.
pub struct Path<K: Kind>(
    BoundedVec<PathStep, 0, MAX_PATH_STEPS>,
    PhantomData<K>,
);

pub type LensPath      = Path<Lens>;
pub type TraversalPath = Path<Traversal>;

/// AST-side path. `AnyPath` is a discriminated sum, not a kind-erased
/// `dyn`-style box: the variant *is* the kind tag, and matching on the
/// variant recovers the proof. Operators that need a specific kind
/// MUST consume the kinded `Path<K>` directly; only operators that
/// genuinely accept either kind (`Modify`, `Unnest`) take `AnyPath`.
pub enum AnyPath { Lens(LensPath), Traversal(TraversalPath) }
```

`Path::<K>::identity()` is the empty-step path; `identity` exists only
for `K = Lens` because identity contains no `Each` step. Composition
is kind-preserving:

```rust
/// Type-level composition rule for optic kinds. Total because `Kind`
/// is sealed: there are exactly four `(K1, K2)` impls, listed below.
///
///   Lens      ∘ Lens      = Lens
///   Lens      ∘ Traversal = Traversal
///   Traversal ∘ Lens      = Traversal
///   Traversal ∘ Traversal = Traversal
pub trait Compose<Rhs: Kind>: Kind { type Out: Kind; }
impl Compose<Lens>      for Lens      { type Out = Lens;      }
impl Compose<Traversal> for Lens      { type Out = Traversal; }
impl Compose<Lens>      for Traversal { type Out = Traversal; }
impl Compose<Traversal> for Traversal { type Out = Traversal; }

pub fn compose<K1: Compose<K2>, K2: Kind>(a: Path<K1>, b: Path<K2>)
    -> Result<Path<<K1 as Compose<K2>>::Out>, RelationError>;
```

`compose` is partial because the concatenated step count can exceed
`MAX_PATH_STEPS`; the failure is returned as
`RelationError::PathStepOutOfBounds`. The kind-composition itself is
total (it is a type-level function); only the length cap is runtime.

`Compose<K1, K2>` is a type-level function whose only `Out = Lens`
case is `Compose<Lens, Lens>`. All other combinations produce
`Traversal`, so any composition involving a traversal is statically a
traversal. Associativity follows from the underlying step-sequence
concatenation.

`is_lens()` remains as a runtime inspection on `AnyPath` for cases
where the AST has erased the kind, but operator signatures that need a
specific kind MUST consume the kinded `Path<K>` directly so the proof
is not lost.

### 10.2. Path Macro

```rust
path!(.posts[*].comments)    // TraversalPath (contains Each)
path!(.address.city)         // LensPath (no Each)
```

The `path!` macro in `axiom-derive` returns the narrower kind it can
prove from the literal syntax: `LensPath` when no `[*]` step is
present, `TraversalPath` otherwise. The macro performs compile-time
syntactic validation but does not verify the path against a schema.

Schema-level validation happens at the typed operator boundary
through trait bounds emitted by `#[derive(Relational)]`. For each
typed-tuple struct, the derive macro emits typestate marker traits
of the form `HasField<NAME>` for every field name and `HasPath<P>`
for every compile-time-known path. Typed operators (`project`,
`restrict`, `modify`, ...) are bounded by these marker traits, so an
attempt such as `users.project::<("missing",)>()` against a struct
without a `missing` field is a compile error. Dynamic operators
(`try_project` over runtime strings) carry no such bound and instead
validate at construction time, returning the typed error.

### 10.3. Path Application

A `Path` does not interpret data directly. Path interpretation is the
responsibility of the backend, which walks the AST and applies path
steps to its representation of the tuple. The path's role in the AST
is purely declarative.

## 11. Expressions and Constraint System

### 11.1. Expression Algebra

```rust
pub enum Expression {
    Attr(AttributeName),
    Lit(Value),
    BinOp(BinOp, Box<Expression>, Box<Expression>),
    UnOp(UnOp, Box<Expression>),
    Like(Box<Expression>, Pattern),
    InList(Box<Expression>, BoundedOrderedSet<Value, 1, MAX_IN_LIST>),
    IsNull(Box<Expression>),
    Cast(Box<Expression>, Type),
    Agg(Agg),
}

pub enum Predicate {
    Expr(Expression),                      // typed to `Type::Bool`
    Opaque(OpaqueId),                      // in-memory backend only
}
```

The opaque escape hatch lives at the `Predicate` boundary, not in
the general `Expression` algebra: opaque operands to `Cast`, `Agg`,
`InList`, arithmetic, or `Extend` are unrepresentable rather than
relying on every constructor to reject them. `Predicate::Expr::try_new`
validates that the expression's inferred type is `Type::Bool` against
a supplied schema; `Predicate::Expr::new` is reachable only through
the derive-macro-emitted typed predicate builder.

`Predicate::Opaque` carries a registry id pointing to a closure held
in a side table owned by the in-memory backend. `axiom-pg` rejects an
AST containing `Predicate::Opaque` at compile time with
`RelationError::UnsupportedOperator`.

`Expression::BinOp` smart constructors enforce
[IDEA.md](IDEA.md) Section 5.5's comparable-equivalence-class rule:
the comparison operators (`Eq`, `Ne`, `Lt`, `Le`, `Gt`, `Ge`) accept
operand pairs only inside one of these classes — `Bool` equality
only, total-ordered numeric within a single numeric class, total-
ordered string and bytes within their own class, or `DateTime` with
explicit zone. The smart constructor rejects an incomparable operand
pair (`Bytes < String`, `Json` against anything, etc.) with
`RelationError::IncomparableTypes`.

### 11.2. Constraint System

Constraints are represented in the same expression algebra as
predicates so the optimizer treats them uniformly. This is not a
compiler-assisted refinement-type checker: there is no SMT solver and
no type-level discharge of proof obligations. It is the externally
observable behaviour a refinement system would provide —
predicate-form refinements attached to relations and propagated
cleanly across every operator — implemented by ordinary constructors
that validate at construction time and a propagation function that
runs at AST-construction and optimisation time. The macros that
introduce constraints are designed to be future hooks for rustc
extensions or external analysers that could lift some of the runtime
checking to compile time without changing the public API.

```rust
pub struct CanonicalPredicate(/* private */ Expression);
// Inner field is crate-private. `Expression` here is the boolean
// expression algebra from §11.1; opacity lives at the `Predicate`
// boundary and never enters `Expression`, so a `CanonicalPredicate`
// is by construction free of opaque holes.
//
// CanonicalPredicate::try_new normalises the expression so equality
// is closed under every rewrite the optimizer performs: constant
// folding, double-negation elimination, De Morgan to the chosen
// canonical form (CNF or NNF + hash-consing — pick one, apply
// consistently), commutative-operand sorting under a canonical
// order, alpha-renaming of attribute references to a schema-position
// canonical form, redundant-clause elimination, and pattern-string
// normalisation. If normalisation would produce more than
// MAX_PREDICATE_CLAUSES clauses, try_new returns
// RelationError::PredicateTooLarge.

pub struct AttributeConstraint {
    pub attr: AttributeName,
    /// Refinement whose free-attribute set MUST equal {attr}.
    /// The smart constructor rejects predicates whose free
    /// attributes are a strict subset (trivially true) or a strict
    /// superset (per-row, not per-attribute).
    pub pred: CanonicalPredicate,
}

pub struct RowConstraint {
    /// Refinement over zero or more free attributes from the schema.
    /// Typed to Bool. Free-attribute set computable in O(expr size).
    /// `Eq` and `Ord` on `RowConstraint` defer to `CanonicalPredicate`
    /// so set deduplication uses Section 11.2's canonical equality.
    pub pred: CanonicalPredicate,
}

pub struct ConstraintSet {
    /// Keyed by attribute name. The smart constructor conjoins
    /// multiple per-attribute constraints over the same attribute
    /// into a single CanonicalPredicate before insertion, so the
    /// keyed set is the canonical home of all constraints over a
    /// given attribute.
    pub per_attr: BoundedOrderedSetByKey<AttributeConstraint,
                                         AttributeName,
                                         0, MAX_ATTRS>,
    pub per_row:  BoundedOrderedSet<RowConstraint, 0, MAX_ROW_CONSTRAINTS>,
}
```

`Predicate::Opaque` is unrepresentable inside `CanonicalPredicate`:
the inner field is an `Expression`, and `Predicate::Opaque` is not
an `Expression` variant. Lifting a `Predicate` into a
`CanonicalPredicate` via `CanonicalPredicate::try_from_predicate`
returns `Err(RelationError::OpaqueInConstraint)` for the
`Predicate::Opaque` arm and `Ok(...)` for the `Predicate::Expr(e)`
arm after canonicalising `e`. That keeps constraint equality
decidable and propagation sound.

`ConstraintSet` forms a bounded, finite **join-semilattice** under
refinement with a partial meet (per [IDEA.md](IDEA.md) Section 5.15):
more constraints sit *below* fewer, the empty set is the top (least
constrained), and the canonical `false` constraint is the bottom
(unsatisfiable). The bounded inner collections impose a finiteness
cap so ascending chains have bounded length; the cap is enforcement,
not a semilattice element.

```rust
impl ConstraintSet {
    /// Union followed by canonicalisation and redundant-constraint
    /// elimination. Partial: returns `Err(RelationError::CardinalityBoundExceeded)`
    /// when the union of the two operands would exceed
    /// `MAX_ROW_CONSTRAINTS` or any other inner bound after
    /// canonicalisation.
    pub fn try_meet(&self, other: &Self)
        -> Result<Self, RelationError>;

    /// Canonical intersection on `CanonicalPredicate` equality; total.
    pub fn join(&self, other: &Self) -> Self;
}
```

Meet is the partial operation per [IDEA.md](IDEA.md) Section 5.15;
the underlying mathematical structure (the unbounded lattice) is
closed under meet, but the bounded
carrier is not, so we surface the cap as a typed error rather than
silently truncating.

`ConstraintSet` attaches to a `Schema` (or to a `Source`) at
construction and travels alongside the relation through the operator
AST. Each operator MUST implement a `project_constraints` rule that
derives the output `ConstraintSet` from its inputs' `ConstraintSet`s,
following [IDEA.md](IDEA.md) Section 5.15:

```rust
fn project_constraints(node: &OpKind, inputs: &[ConstraintSet])
    -> Result<ConstraintSet, RelationError>;
```

The signature is fallible exactly when the per-operator law invokes
the partial `try_meet` — either to combine two input `ConstraintSet`s
(intersection, join, product, modify) or to add new per-row
constraints to one input (restriction, extension, summarization,
unnest, nest). For operators whose law uses only the total `join` or
pure subset preservation (union, projection, rename, order, limit,
difference), the `Err` arm is unreachable but kept for uniformity.
The propagation function MUST satisfy the following laws. Each law
is stated over the `Ok` arm of the `Result`; when either side
returns `Err(CardinalityBoundExceeded)`, the whole propagation
returns that error and the optimizer rewrite that would have consumed
the missing constraint MUST decline to fire:

- **sound:** every constraint in the `Ok` output holds on every
  output tuple of every input that satisfies the input constraints;
- **monotone (on `Ok`):** if `cs1` refines `cs2` and both sides
  return `Ok`, the output of `cs1` refines the output of `cs2`;
- **identity-preserving (on `Ok`):** for an operator equivalent to
  the identity, propagation returns `Ok(cs)` for input `cs`;
- **compositional, full AST (on `Ok`):** propagation through an AST
  node equals the per-node propagation applied to the propagated
  constraint sets of its children, whenever every contributing
  propagation returns `Ok` ([IDEA.md](IDEA.md) Section 5.15);
- **compositional, unary chains (on `Ok`):** the special case for
  arity-1 chains: when `propagate(g, cs)` is `Ok(c)`, then
  `propagate(f ∘ g, cs) == propagate(f, c)`.

Property tests in Section 15.2 assert all five laws over random ASTs
and random input fixtures, plus a separate property that asserts the
`Err(CardinalityBoundExceeded)` path is exercised by AST shapes whose
total constraint count would exceed `MAX_ROW_CONSTRAINTS`.

The constraint module lives in `axiom-core::constraint` and depends
only on `axiom-core::expression`, `axiom-core::schema`, and the
smart-constructor trait in Section 8.5. The module's public surface
MUST be relocatable wholesale into a future `axiom-refine`
constraint-propagation crate without changing any other module beyond
import paths; see Section 17.

### 11.3. RelationError

The single core error enum lives in `axiom-core::error`:

```rust
#[non_exhaustive]
pub enum RelationError {
    UnknownAttribute(AttributeName),
    DuplicateAttribute(AttributeName),
    SchemaMismatch { expected: Schema, actual: Schema },
    AmbiguousJoinAttribute(AttributeName),
    MalformedExpression(String),
    IncomparableTypes { lhs: Type, rhs: Type },
    MalformedPath(String),
    PathStepOutOfBounds { limit: usize, actual: usize },
    DuplicateAggregateOutputName(AttributeName),
    DuplicateOrderKey(OrderKey),
    OpaqueInConstraint,
    PredicateTooLarge { limit: usize, actual: usize },
    CardinalityBoundExceeded { bound: &'static str, limit: usize },
    UnsupportedOperator { backend: &'static str, op: &'static str },
}
```

This is the minimum set required by [IDEA.md](IDEA.md) Section 5.13;
the `#[non_exhaustive]` attribute lets implementations add further
variants without breaking exhaustive matches in downstream code.
Adding a variant MUST NOT change the meaning of an existing one.

Backend transport failures live in backend-specific error types or
wrappers that preserve `RelationError` as the shared
construction-or-rejection cause; transport variants MUST NOT enter
this enum. See Section 14.4 for the PostgreSQL wrapper.

## 12. Optimizer

### 12.1. Pipeline

`axiom-optimizer` exposes one entry point:

```rust
pub fn optimize(op: Op) -> Result<Op, RelationError>;
```

The return type is fallible because the constraint-propagation pass
can hit the partial-meet overflow surfaced by
`ConstraintSet::try_meet` (Section 11.2). The structural rewrites
(folding, push-down, pruning, fusion, simplification, elimination)
remain infallible; only propagation can fail.

`optimize` runs a fixed sequence of passes to a fixed point. Each
structural pass is a pure function `Op -> Op`; the constraint
propagation pass is a pure function `Op -> Result<Op, RelationError>`
that threads the `Err` upward through the pipeline. The pipeline is:

1. Constant folding in expressions.
2. Predicate normalisation (CNF push, double-negation removal).
3. Constraint propagation: every operator's output `ConstraintSet` is
   computed from its inputs per Section 11.2 and [IDEA.md](IDEA.md)
   Section 5.15. Computed constraints are attached to the operator
   for later passes; they do not change AST shape on their own.
4. Restriction push-down across `Project`, `Rename`, `Extend`, and
   `Join` boundaries where attribute references permit.
5. Restriction push-down through `Modify`: outer predicates that
   reference only path-reachable attributes are pushed down into the
   inner sub-query; inner predicates that reference only outer
   attributes are hoisted out of the sub-query.
6. Adjacent `Modify` fusion: `Modify(p, Modify(q, f))` becomes
   `Modify(p ++ q, f)`.
7. Projection pruning: every operator's output attribute set is
   trimmed to attributes referenced downstream. The pruning crosses
   `Modify` boundaries so traversal drops unreferenced sub-attributes.
8. Constraint-driven simplification: a `Restrict` whose predicate is
   implied by the input operator's `ConstraintSet` is replaced by its
   input; a `Join` known by constraint analysis to be empty becomes
   the empty relation; range-narrowing rewrites that the constraint
   set proves safe MAY fire here.
9. Trivial-operator elimination (`Project` over the full schema,
   `Restrict` over the constant `true`, empty `Union`).

Each pass MUST either strictly decrease a documented termination
measure or be proven idempotent in the pass order. The initial
measure is the lexicographic tuple, smaller is better:

1. operator count,
2. traversal-step count across all paths in the AST,
3. expression-node count,
4. unresolved-constraint-fact count (constraints not yet propagated
   to the operator's output `ConstraintSet`).

Constraint propagation is monotone over the bounded `ConstraintSet`
join-semilattice of Section 11.2; that semilattice is finite for a
given AST
because `per_attr` is bounded above by `MAX_ATTRS` distinct keys and
`per_row` is bounded above by `MAX_ROW_CONSTRAINTS` distinct
`CanonicalPredicate`s, so the constraint-state contribution to the
fixed point converges in a bounded number of steps. The fixed point
includes both AST shape and attached `ConstraintSet`s.

The pipeline runs to a fixed point: passes are applied in the listed
order; if any pass made a change, the pipeline restarts from step 1.
Restart count is bounded by the lexicographic termination measure
above. Each individual pass MUST be idempotent or strictly decrease
that measure within one application. The full pipeline MUST be
idempotent on success: a property test asserts that if
`optimize(op) == Ok(op')` then `optimize(op') == Ok(op')`
structurally (including attached constraint state). ASTs whose
`optimize` returns `Err(CardinalityBoundExceeded)` are covered by a
separate test that asserts the same `Err` value is returned
deterministically for the same input.
"Unresolved-constraint-fact count" is defined as the number of AST
nodes whose attached `ConstraintSet` is strictly weaker than
(i.e. refined by) the constraint set the propagation function would
derive from the operator's children right now; monotonicity plus
finiteness of the join-semilattice guarantees this count decreases
on every constraint-changing pass.

### 12.2. Equivalence

Every rewrite MUST preserve the observable result defined in
[IDEA.md](IDEA.md) Section 5.8: ordered operators preserve sequence
equality, unordered relational operators preserve bag equality, and
`limit`/`offset` are evaluated after the ordering semantics of their
input. Equivalence is asserted by:

- a property-test suite that generates random ASTs over a small
  schema, conditions on `optimize(op) == Ok(op')`, and checks
  `eval(op, data) == eval(op', data)` for the in-memory backend,
  using sequence equality below an `order` node and bag equality
  above its absence. ASTs for which `optimize` returns
  `Err(CardinalityBoundExceeded)` are exercised by a separate test
  that asserts `execute` surfaces the same error;
- a SQL-cassette suite that checks `compile(op)` and, conditional on
  `optimize(op) == Ok(op')`, `compile(op')` over committed example
  databases: for unordered results, the oracle is empty `EXCEPT ALL`
  symmetric difference; for ordered results (any AST whose root chain
  contains an `Order` node not later wrapped by a set operator), the
  oracle compares the full ordered tuple sequence row-for-row. ASTs
  whose `optimize` returns `Err` are covered by a dedicated cassette
  that asserts both `compile` and `execute` propagate the same
  error.
  Nested-relation column values are compared by recursively applying
  the same equivalence rule (bag equality for unordered nested
  relations, sequence equality below a nested `Order` node). The
  cassette suite MUST exercise this recursion at least one level
  deep.

### 12.3. Determinism

`optimize` is deterministic. Pass order is fixed. Iteration over
attribute sets within a pass uses a `BTreeSet`-style canonical order,
not a hash-set order.

## 13. In-Memory Backend

### 13.1. Execution

`axiom-mem` exposes:

```rust
pub fn execute(op: Op)
    -> Result<Box<dyn Iterator<Item = DynamicTuple>>, RelationError>;
pub fn execute_typed<T: Relational>(op: Op)
    -> Result<impl Iterator<Item = T>, RelationError>;
```

Both entry points fail when `optimize` returns
`Err(CardinalityBoundExceeded)` — the partial-meet overflow surfaced
by `ConstraintSet::try_meet` (Section 11.2).
`execute_typed` additionally fails with
`RelationError::SchemaMismatch` when `op`'s inferred output schema
does not match `T::schema()`; that check happens once at entry, not
per row.

`execute` runs `optimize` internally before interpretation. The
returned iterator is lazy: it materializes only the tuples its
consumer demands, except for operators whose semantics require
materialization (sort, set difference, hash-join build side,
group-by accumulator).

### 13.2. Iterator Composition

Each operator is implemented as an `Iterator` adapter over its input.
The adapter chain mirrors the (already-optimized) AST structure, so
per-row fusion is provided by the Rust iterator combinators without
additional work.

### 13.3. Closure Registry

The opaque-predicate registry is owned by an `ExecutionContext`
obtained from `axiom-mem::execute_with`. `ExecutionContext::restrict_with`
takes a Rust closure, inserts it into the context's registry, and
returns the `Predicate::Opaque(OpaqueId)` AST node. This is the
only public source of `Predicate::Opaque`; the registry's lifetime
is the context's, so opaque ids cannot leak across queries.

The simple `axiom-mem::execute` and `execute_typed` entry points
construct a fresh `ExecutionContext` internally and discard it after
the iterator is consumed; callers that need to inject closures use
`execute_with` instead.

### 13.4. Determinism

Per [IDEA.md](IDEA.md) Section 5.12 the in-memory backend's iteration
order MUST be well-defined for every operator. Where the relational
algebra does not constrain order (set operations on unordered
relations: `Union`, `Intersect`, `Difference`), iteration MUST follow
a documented stable rule, not the iteration order of the underlying
hash-keyed collection used internally for set semantics. Concretely:
the implementation MUST iterate the result in a canonical order
derived from the operator's output schema (lexicographic on schema
position over each tuple's `CanonicalValue` representation), or any
other order specified in tracked configuration. The default order is
fixed at compile time so the same in-memory program produces the same
iteration order across runs and across machines.

## 14. PostgreSQL Backend

### 14.1. Compile

`axiom-pg` exposes:

```rust
pub fn compile(op: Op) -> Result<CompiledSql, RelationError>;

pub struct CompiledSql {
    sql:    SqlText,
    params: BoundedVec<SqlParam, 0, MAX_PARAMS>,
}

impl CompiledSql {
    pub fn sql(&self)    -> &SqlText;
    pub fn params(&self) -> &[SqlParam];
}

/// The only constructor of `CompiledSql`. The builder emits
/// `$1, $2, ...` placeholders as a side effect of pushing parameters,
/// so placeholder count and parameter count are equal by construction
/// and never need to be cross-validated.
pub struct SqlBuilder { /* ... */ }
impl SqlBuilder {
    /// Append literal SQL text. Callers MUST NOT include `$<digits>`
    /// placeholder tokens in `s`; placeholders are the exclusive
    /// output of `placeholder`. Debug builds SHOULD assert.
    pub fn write_str(&mut self, s: &str);
    /// Append `$N` and push `value` into the parameter vector.
    pub fn placeholder(&mut self, value: SqlParam);
    pub fn finish(self) -> Result<CompiledSql, RelationError>;
}
```

`SqlText` is a newtype around a non-empty string. `SqlParam` is a
backend-typed parameter value carrying the inferred PostgreSQL type
tag so binding is unambiguous. Because every placeholder is emitted
through `SqlBuilder::placeholder`, a `CompiledSql` whose placeholder
count differs from its parameter count is unrepresentable, and the
backend never needs to parse SQL text to count `$N` occurrences.

`compile` delegates to `axiom-optimizer::optimize`, rejects
unsupported operators (`Predicate::Opaque`, `Source::Memory`) with
typed errors, and emits a deterministic SQL string with positional
parameter placeholders (`$1`, `$2`, ...) in left-to-right traversal
order. The backend owns no rewrite logic of its own.

### 14.2. Canonical SQL Form

The emitted SQL form MUST be deterministic and stable for cassette
comparison:

- identifier quoting uses double quotes uniformly;
- keyword case is uppercase;
- clause order is `WITH`, `SELECT`, `FROM`, `WHERE`, `GROUP BY`,
  `HAVING`, `ORDER BY`, `LIMIT`, `OFFSET`;
- subqueries are rendered with consistent indentation;
- parameter placeholders are numbered in lexical order of appearance
  in the final SQL string.

### 14.3. Nested Data and NF²

Nested operations compile as follows:

- `Modify(path, sub)` over a JSON-typed attribute compiles to a
  correlated subquery against `jsonb_array_elements` (for `Each`
  steps) or `->` and `->>` (for `Field` steps) projected back through
  `jsonb_build_object`.
- `Unnest(path)` compiles to `LATERAL` joins against
  `jsonb_array_elements` or array `UNNEST`, depending on the source
  type.
- `Nest(attrs, into)` compiles to `jsonb_agg` plus `jsonb_build_object`
  within a `GROUP BY` over the non-nested attributes.

### 14.4. Execution

Execution is provided by an async function:

```rust
pub async fn execute(client: &tokio_postgres::Client, op: Op)
    -> Result<Vec<DynamicTuple>, PgError>;
```

`execute` calls `compile`, sends the parameterized query, and decodes
the result rows into `DynamicTuple` against the AST's inferred output
schema. A typed `execute_typed::<T>` form is provided for typed
output.

Per [IDEA.md](IDEA.md) Section 5.13, transport failures (network
error, server error envelope, decode failure) MUST NOT enter
`RelationError`. The PG backend exposes them through `PgError`:

```rust
pub enum PgError {
    /// AST construction or backend rejection: identifier quoting,
    /// unsupported operator, schema decode mismatch.
    Relation(RelationError),
    /// Live connection / wire / decode failure from `tokio-postgres`.
    Transport(tokio_postgres::Error),
}

impl From<RelationError> for PgError { /* … */ }
```

`compile` returns `Result<CompiledSql, RelationError>` (no transport
surface); `execute` returns `Result<_, PgError>` so a caller can
distinguish "the AST is invalid for this backend" from "the network
dropped". The optimizer's
`Err(RelationError::CardinalityBoundExceeded)` reaches the caller
through `PgError::Relation` unchanged.

## 15. Testing Architecture

### 15.1. Core Tests

The core crate MUST contain:

- unit tests for every smart-constructor success and rejection path;
- property tests proving path composition is associative and the
  identity path is a left and right identity;
- property tests proving expression type inference is sound for the
  values it admits;
- property tests proving schema construction rejects duplicate
  attribute names.

### 15.2. Optimizer Tests

The optimizer crate MUST contain:

- property tests proving that for every random AST `op` over a
  random in-memory data set scoped to a small schema vocabulary,
  either `optimize(op) == Err(CardinalityBoundExceeded)` or
  `optimize(op) == Ok(op')` with `eval(op, data) == eval(op', data)`;
- golden tests proving each documented rewrite produces the expected
  output AST for a representative input AST;
- property tests proving `optimize` is idempotent on success: if
  `optimize(op) == Ok(op')`, then `optimize(op') == Ok(op')`
  structurally;
- property tests proving constraint propagation is **sound on `Ok`**
  ([IDEA.md](IDEA.md) Section 5.15): for every constraint in the
  `Ok` output `ConstraintSet`, the predicate holds on every tuple
  produced by `eval(op, data)` for every committed input fixture;
- property tests proving constraint propagation is **monotone on
  `Ok`** (Section 5.15): if both sides return `Ok`, refining the
  input `ConstraintSet` refines the output;
- property tests proving constraint propagation is
  **identity-preserving on `Ok`** for identity rewrites
  (Section 5.15);
- property tests proving constraint propagation is **compositional
  (full AST) on `Ok`**: the fold-over-AST law of Section 5.15;
- property tests proving constraint propagation is **compositional
  (unary chains) on `Ok`**: the corollary of the full-AST law for
  arity-1 operator chains;
- a property test asserting that propagation surfaces
  `Err(CardinalityBoundExceeded)` deterministically for AST shapes
  whose accumulated constraint count would exceed
  `MAX_ROW_CONSTRAINTS`, and that the error is the same on repeated
  evaluation of the same input.

### 15.3. In-Memory Backend Tests

The in-memory backend crate MUST contain:

- per-operator unit tests against tabular fixtures committed in the
  crate;
- nested-data tests exercising `Modify`, `Unnest`, and `Nest` against
  fixtures that include `Type::Relation`, `Type::Array`, and
  `Type::Json` attributes;
- iterator-laziness tests asserting that operators which can stream
  do not pull more from their input than the consumer demanded.

### 15.4. PostgreSQL Backend Tests

The PostgreSQL backend crate MUST contain two test layers.

The **SQL-generation layer** uses cassettes: a committed directory of
`(ast.json, expected.sql, expected_params.json)` triples. Each test
loads the AST, compiles it, and asserts byte-equality against the
committed SQL string and a structural equality against the committed
parameter vector. Cassette regeneration is opt-in through an
environment variable recognised only by the test binary, never by
production code. The exact variable name is open (see Section 17,
"Cassette regeneration ergonomics").

The **execution layer** is an opt-in integration test crate
(`axiom-pg-it`) that connects to a local PostgreSQL instance
configured by environment variables. The crate is excluded from
`just ci` and runs only under `just it`.

### 15.5. Derive-Macro Tests

The derive crate MUST contain trybuild-style tests for:

- successful `#[derive(Relational)]` over flat structs;
- successful `#[derive(Relational)]` over nested structs (`Vec<T>`,
  `Option<T>`);
- compile-failure tests for misuse (`path!(.foo)` against a type
  lacking the `foo` field).

### 15.6. No Test-Only Branches

Production code MUST NOT contain test-only branches or test-only
environment-variable switches. Test substitutions happen through
documented production boundaries: the AST as an input, the iterator
as an output, the parameterized SQL as a string. The PostgreSQL
integration test's environment-variable gate lives in the
integration-test crate, not in `axiom-pg` itself.

## 16. Build Sequence

The dependency graph dictates a topologically sortable build order.
The recommended sequence is two phases.

Phase A — core algebra through in-memory execution:

1. `axiom-core::sc` (`SmartConstructor` + `Narrowing` traits, sealed
   `Witness`, sealed-module convention), `axiom-core::limits`, and
   the bounded numeric and ordered collection wrappers
   (`BoundedU64`, `Offset`, `LimitCount`, `BoundedOrderedSet`,
   `BoundedOrderedSetByKey`, `BoundedIndex`, `OrderKeys`).
2. Core leaf types: `AttributeName`, `Type`, `Value`, `Schema`,
   `Attribute`, `RelationError`; `Row` (constructor-validated against
   `Schema`); `DynamicTuple` + the `Tuple` trait.
3. `Path` and `PathStep` with kind-preserving composition; property
   tests for associativity and identity.
4. `Expression`, `Predicate`, and `CanonicalPredicate` with type
   inference; property tests for inference soundness and for
   canonical-form equality.
5. `ConstraintSet` with per-attribute and per-row constraints;
   property tests for the bounded join-semilattice structure
   (top, bottom, partial meet, total join).
6. `Op` opaque wrapper plus the `pub(crate) OpKind` discriminated
   sum, and per-operator smart constructors (typed and dynamic).
7. `axiom-derive`: `attr!`, `path!`, `col!`, `on!`, and
   `#[derive(Relational)]` for flat structs; first cut of the
   refinement-declaration macro emitting `SmartConstructor` /
   `Narrowing` impls plus the `Deserialize` route through `try_new`.
8. `axiom-mem`: per-operator iterator adapters in order Project,
   Restrict, Extend, Rename, Order, Limit, Product, Join, Union,
   Difference, Intersect, Summarize, Modify, Unnest, Nest.
9. `IntoIterator` interop on the facade.

Phase B — optimizer through SQL execution:

1. `axiom-optimizer`: constant folding, restriction push-down,
   projection pruning, trivial-operator elimination. Property tests
   against the in-memory backend at each step.
2. `axiom-optimizer`: constraint propagation pass following the laws
   in Section 11.2; property tests for soundness, monotonicity,
   identity, composition (full AST), composition (unary chains).
3. `axiom-optimizer`: `Modify`-aware passes (predicate push-down
   through traversal, adjacent traversal fusion, projection pruning
   across paths).
4. `axiom-optimizer`: constraint-driven simplification (discharge
   restrictions implied by the propagated `ConstraintSet`, prove
   empty joins).
5. `axiom-pg`: SQL writer infrastructure (identifier quoting,
   parameter accumulator, canonical clause emitter); per-operator
   SQL compilation for flat operators.
6. `axiom-pg`: nested-data SQL compilation (`Modify`, `Unnest`,
   `Nest`) over `jsonb` and `LATERAL`.
7. `axiom-pg`: cassette test scaffolding and the first cassettes.
8. `axiom-pg`: `execute` against `tokio-postgres`; opt-in
   integration tests.
9. Derive macro support for nested structs (`Vec<T>`, `Option<T>`).

Each step is its own commit. Each commit passes the full gate.

This sequence is non-normative guidance. Implementations MAY reorder
if the dependency graph permits and the invariants in
[IDEA.md](IDEA.md) are preserved.

## 17. Open Issues

The items in this section are unresolved questions that affect the
architecture but not yet the implementation. They will be resolved as
implementation evidence accumulates or as decisions are required.

- **Typed-tuple ergonomics.** Whether `Relation<T>` is generic over
  the tuple type and the operator surface is parameterised, or
  whether typed and dynamic surfaces are separate traits sharing the
  same AST, is undecided. The decision depends on how cleanly the
  derive macro can emit operator overloads.
- **Aggregation over paths.** `sum(path!(.posts[*].score))` is
  appealing because it makes aggregations expressions that traverse.
  The expression algebra in Section 11 does not yet have a path-typed
  variant; whether to extend `Expression` or to introduce a separate
  `PathExpression` is open.
- **Mutation operations.** Insert, update, delete are out of scope.
  The `Modify` operator is suspiciously close to what an update needs.
  The architecture should avoid painting `Modify` into a corner that
  rules out a future mutation path.
- **Opaque-predicate scope.** The current design admits opaque
  predicates only in the in-memory backend. Whether opaque
  *expressions* (not just predicates) are allowed, and whether the
  optimizer can treat them as side-effect-free for reordering inside
  the in-memory backend, is open.
- **Coverage threshold start position.** The `ci` gate asserts zero
  uncovered regions, functions, and lines. The initial scaffold
  cannot meet this threshold. The threshold direction is up; the
  start position is to be set from the actual current value once the
  first batch of tests exists.
- **Cassette regeneration ergonomics.** Cassette regeneration through
  an environment variable inside the test binary is the chosen
  approach. The exact variable name, the regeneration audit trail,
  and the cassette diff-review workflow remain to be specified.
- **Second backend candidates.** A future backend over SQLite, DuckDB,
  or an Apache Arrow compute graph is plausible. The architecture
  treats this as additive (a new `axiom-<target>` crate) and defers
  the decision until a concrete use case justifies it.
- **Constraint-system extraction (`axiom-refine`).** Sections 8.5 and
  11.2 are designed so the smart-constructor trait and the
  constraint module can be lifted into a standalone refinement-types
  crate without touching the rest of axiom-rs. The trigger for the
  extraction is a second consumer (another crate that wants the
  constraint vocabulary independent of relational algebra) or a
  decision to publish the type system on its own. Until then, the
  modules live in `axiom-core` and are kept dependency-clean so the
  move is a rename, not a refactor.
- **Constraint expressiveness.** The initial constraint language is
  the existing expression algebra of Section 11.1. Whether to admit
  richer shapes — quantifiers (`forall`, `exists` over subset rows),
  cross-relation references, or arithmetic theories — depends on what
  the optimizer can usefully discharge. Defer until the first set of
  constraint-driven rewrites is in.
- **Refinement-declaration macro shape.** Section 8.5 sketches a
  `refinement! { ... }` declarative macro, but the precise syntax —
  declarative macro vs. derive macro on a thin struct vs. attribute
  macro vs. some hybrid — is undecided. The deliverables it MUST
  emit are fixed (type, `SmartConstructor`, sealed witness, narrowing
  impl when applicable, `Deserialize` routed through `try_new`,
  read-only surface) but the surface syntax is open until at least
  three constrained types have been built by hand and compared.

## 18. References

### 18.1. Normative References

[RFC2119] Bradner, S., "Key words for use in RFCs to Indicate
Requirement Levels", BCP 14, RFC 2119, March 1997,
<https://www.rfc-editor.org/rfc/rfc2119.html>.

[RFC8174] Leiba, B., "Ambiguity of Uppercase vs Lowercase in RFC 2119
Key Words", BCP 14, RFC 8174, May 2017,
<https://www.rfc-editor.org/rfc/rfc8174.html>.

### 18.2. Informative References

[RFC2026] Bradner, S., "The Internet Standards Process -- Revision 3",
BCP 9, RFC 2026, October 1996,
<https://www.rfc-editor.org/rfc/rfc2026.html>.

[RFC7322] Flanagan, H. and S. Ginoza, "RFC Style Guide", RFC 7322,
September 2014, <https://www.rfc-editor.org/rfc/rfc7322.html>.

[AXIOM] Kubb, D., "axiom: Ruby relational algebra library",
<https://github.com/dkubb/axiom>.

[AXIOM-OPT] Kubb, D., "axiom-optimizer: AST rewriter for axiom",
<https://github.com/dkubb/axiom-optimizer>.

[KING2019] King, A., "Parse, don't validate", November 2019,
<https://lexi-lambda.github.io/blog/2019/11/05/parse-don-t-validate/>.

[DATE2005] Date, C. J., "Database in Depth: Relational Theory for
Practitioners", O'Reilly Media, 2005.

[DATE2011] Date, C. J., "SQL and Relational Theory: How to Write
Accurate SQL Code", O'Reilly Media, 2011.

[JAESCHKE1982] Jaeschke, G. and Schek, H.-J., "Remarks on the algebra
of non first normal form relations", Proceedings of the 1st ACM
SIGACT-SIGMOD Symposium on Principles of Database Systems, 1982.
