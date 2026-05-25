# Axiom Relational Algebra Library: High-Level Design Sketch

> Status: high-level narrative that summarises the normative
> documents at sketch level. [IDEA.md](IDEA.md) is the authoritative
> specification; [ARCHITECTURE.md](ARCHITECTURE.md) is the concrete
> technical design. Where this sketch differs from either of those
> documents, the other documents win. The role of this file is to
> read top-to-bottom in one sitting and to convey the shape of the
> library; it is not a substitute for the normative texts.

## Goals

1. Express relational-algebra queries as a composable value (an AST)
   that runs either in memory (over Rust iterators) or compiles to
   PostgreSQL.
2. Stay idiomatic for Rust: borrow-friendly surface, iterator-based
   in-memory execution, types that carry as many invariants as the
   language admits.
3. Define smart constructors uniformly: every constrained type goes
   through a common `SmartConstructor` interface, with a separate
   `Narrowing` trait for types whose construction can fail. The same
   declaration drives runtime validation **and** deserialization, so
   no `Deserialize` path bypasses the invariant.
4. Operate on nested data structures as a first-class case — not as
   an afterthought retro-fitted onto a flat tabular model.
5. Fuse path traversal with relational operations so filter/transform
   pushes down through paths instead of materialising intermediate
   collections.
6. Propagate refinement-like constraints across every operator so the
   optimiser can discharge predicates that follow from earlier
   constraints, without ever consulting an SMT solver.

## Non-Goals (for now)

- SQL dialects beyond PostgreSQL.
- ORM-style identity maps, sessions, schema migrations, connection
  pooling.
- Distributed execution, streaming push-based pipelines.
- Compiler-assisted refinement types in the Liquid-Haskell sense (we
  reach for the externally-observable behaviour of such a system at
  runtime, not the type-level proof discharge).

---

## Core Concepts

### A query is a value

The central type is `Op` (Section 9.1 of ARCH): an opaque wrapper
around a crate-private `OpKind` discriminated sum. External callers
never see `OpKind`; they build queries through the smart constructors
on the `Relation` / `RelationExt` traits and pass the resulting `Op`
into a backend.

```rust
pub trait Relation: Sized {
    fn into_op(self) -> Op;
    fn schema(&self) -> &Schema;
}
```

Two construction styles, both required:

```rust
// Infallible: accepts proof-bearing input only. The type system
// already knows the input is valid for `T`.
let r: Op = Source::Memory::new(proven_source);

// Fallible: accepts raw input and validates it against the schema,
// failing fast on mismatch. Only types that *narrow* expose this.
let r: Op = Source::Memory::try_new(schema, rows)?;
```

`try_new` exists when, and only when, construction can fail —
i.e. when the constructor narrows its raw input domain to a smaller
admissible state space. Types whose raw input domain coincides with
their admissible state space (no narrowing) expose only `new`.

### Smart constructors as a trait

The constructor pair sits behind a uniform trait so every constrained
type advertises its admissible domain, its validation function, and
its witness in the same shape:

```rust
pub trait SmartConstructor: Sized {
    type Admissible;
    type Witness: Copy;        // sealed; downstream cannot mint a fresh one
    fn new(admissible: Self::Admissible) -> Self;
    fn witness(&self) -> Self::Witness;
}

pub trait Narrowing: SmartConstructor {
    type Raw;
    type Error;
    fn try_new(raw: Self::Raw) -> Result<Self, Self::Error>;
}
```

A single declaration — sketched as a `refinement! { … }` macro —
drives the type, the `SmartConstructor` impl (plus the narrowing
extension when narrowing applies), the sealed `Witness`, and a
`Deserialize` impl that routes through `try_new`. There is no
admissible code path that produces a `T` without running the
validation `try_new` runs. The system is the externally-observable
behaviour a refinement-type system would expose, implemented with
ordinary Rust constructors and runtime predicates.

Across the library we pick the **most-constrained admissible
representation**: a header (the attribute list of a schema,
projection, or rename) is a bounded ordered *set* keyed by
`AttributeName`, not a bounded vector that admits duplicates; an
aggregate list is keyed by output attribute name; a row's width and
per-position types are constrained at construction by its schema.
Where a tighter type still expresses every legal value, that's the
one we use.

### Schema and tuple

`Schema` is an ordered set of `(name, Type)` attributes keyed by
name, so duplicate attribute names are unrepresentable. A tuple comes
in two flavours behind one internal trait:

- **Dynamic** (`DynamicTuple`): a `Row` whose width and per-position
  value types are validated against its `Schema` at construction;
  used when columns come from runtime sources such as SQL result
  rows or parsed JSON.
- **Typed** (`TypedTuple<T>`): a Rust struct deriving `Relational`.
  The schema is reflected from the type at compile time.

### Paths

Nested data is addressed through a path — a small optic-like value
composed of field-access, index, and traversal (`[*]`) steps:

```rust
pub enum PathStep {
    Field(AttributeName),
    Index(BoundedIndex),
    Each,                              // promotes a path to a traversal
}

pub struct Path<K: Kind>(BoundedVec<PathStep, 0, MAX_PATH_STEPS>, PhantomData<K>);
pub type LensPath      = Path<Lens>;
pub type TraversalPath = Path<Traversal>;
```

`Kind` is sealed, so the only inhabitants are `Lens` and `Traversal`.
A path with zero `Each` steps is a `LensPath`; one or more makes it a
`TraversalPath`. The `path!` macro returns the narrower kind it can
prove from the literal syntax:

```rust
path!(.posts[*].comments)        // TraversalPath
path!(.address.city)             // LensPath
```

Composition is kind-preserving: lens ∘ lens = lens; any composition
involving a traversal is a traversal. The composition is fallible
because the concatenated step count can exceed `MAX_PATH_STEPS`.

### Expressions, predicates, and the opaque escape hatch

The expression algebra (`Attr`, `Lit`, `BinOp`, `UnOp`, `Like`,
`InList`, `IsNull`, `Cast`, `Agg`) is a value, not a closure — that
is what lets the optimiser rewrite predicates and what lets the
PostgreSQL backend translate them.

```rust
pub enum Predicate {
    Expr(Expression),                  // typed to Bool
    Opaque(OpaqueId),                  // in-memory backend only
}
```

The opaque escape hatch lives at the **predicate** boundary, not in
the general `Expression` algebra: opaque operands to `Cast`, `Agg`,
arithmetic, or `Extend` are unrepresentable rather than relying on
every constructor to reject them. `axiom-pg` rejects an AST that
contains `Predicate::Opaque` at compile time; the in-memory backend
holds the closure in a per-execution registry obtained through
`execute_with`.

### Constraints that propagate

Constraints are predicates attached to a relation at one of two
granularities:

- **per-attribute** constraints (free-attribute set exactly `{attr}`)
  for facts like non-null, numeric range, enum membership.
- **per-row** constraints (free attributes drawn from the schema) for
  facts like `a < b` or check predicates.

Both use a `CanonicalPredicate` wrapping `Expression`, so equality is
decided by canonical form — two semantically equivalent predicates
compare equal regardless of how they were written. Opaque predicates
cannot enter a `ConstraintSet` (they're not in `Expression`).

`ConstraintSet` is a **bounded, finite join-semilattice with a
partial meet**: the top is the empty set, the bottom is the canonical
`false` constraint, and meet (union + canonicalisation) is partial
because the bounded carrier can overflow. Every operator has a
constraint-projection rule (Section 5.15 of IDEA / Section 11.2 of
ARCH) that derives the output `ConstraintSet` from its inputs',
following five laws on the `Ok` arm: soundness, monotonicity,
identity preservation, full-AST composition, and unary-chain
composition. The optimiser consumes the propagated constraints to
discharge restrictions implied by them, eliminate impossible joins,
and narrow ranges where it can prove the rewrite is safe.

---

## Operations

The familiar relational operators, each producing an `Op`:

```rust
people.project(attrs![id, name])?;            // Projection
people.restrict(col("age").gte(18))?;         // Restriction (σ)
orders.join(people, on!(orders.user_id == people.id))?;  // ⋈
a.union(b)?;
a.difference(b)?;
left.group_by(attrs![customer_id])
    .agg(sum(col("total")).as_("revenue"))?;
```

Smart constructors come in pairs: a typed `project` whose argument
type already proves admissibility, and a dynamic `try_project` that
validates a runtime attribute list. The fluent style above threads
the dynamic form with `?`.

---

## Nested data: NF² + paths

A tuple's attribute can itself be a relation or another modelled
nested value. Backends never have to guess whether a value is
algebraic or opaque because the type system partitions the cases:

- `Type::Relation(schema)` and `Type::Array(Type::Relation(_))` are
  **algebraic NF² values** — paths and `modify`/`unnest`/`nest`
  operate on them directly.
- `Type::Array(t)` for non-relation `t` is an algebraic collection
  addressable by `[*]`/`[i]` steps.
- `Type::Json` is addressable only through the PostgreSQL backend's
  documented JSON-path subset; outside that subset it's opaque and
  the optimiser treats it as a black box.

Operating on nested data uses paths:

```rust
let pruned = users.modify(
    path!(.posts[*]),
    Source::from(Post::schema())
        .restrict(col("published").eq(true))?
)?;

let user_posts = users.unnest(path!(.posts[*]))?;
```

`modify` is the key NF² primitive. It takes a path whose focus is
relation-like and a sub-`Op` that operates on the focused
sub-relation; references in the sub-`Op` to attributes outside the
focus are rejected at smart-constructor time. Typed paths encode the
relation-likeness in the path type; dynamic paths are rejected by
`try_modify` when the focused schema is not relation-like.

---

## Fusion: where the two worlds meet

The AST is data, so the optimiser rewrites it. The rewrites that
matter most for nested data are:

### 1a. Filter nested elements in place

```text
modify(.posts[*], restrict(p, sub))   ⟿   modify(.posts[*], restrict(p, sub))
                                          with `p` applied during traversal
```

Precondition: `p`'s free attributes lie entirely inside the path
focus. Outer cardinality is preserved; iteration replaces
materialisation.

### 1b. Filter outer rows by an existential path predicate

```text
restrict(exists(.posts[*], p), users)
    ⟿ semi_join(users, restrict(p, posts))
```

Precondition: `p`'s free attributes lie entirely inside the focus,
and the outer query is an existence question. Outer cardinality
shrinks; matching inner collections are kept intact.

### 2. Fuse adjacent traversals

```text
modify(p, modify(q, f))   ⟿   modify(p ++ q, f)
```

One walk instead of two. The same idea is used by the SQL backend to
emit a single `LATERAL` expression rather than nested subqueries.

### 3. Project pruning across paths

If the outer query only reads `posts[*].id`, the traversal never
loads `posts[*].body`. Standard projection pushdown, extended to
paths.

### And: constraint propagation

The optimiser runs a constraint-propagation pass (ARCH §12.1 step 3)
that attaches each operator's output `ConstraintSet`. A later pass
discharges restrictions whose predicate is implied by the propagated
constraints and eliminates impossible joins. Propagation is fallible
because meet is partial; the optimiser as a whole therefore returns
`Result<Op, RelationError>`.

---

## Backends

Backends are crates that consume an `Op`. There is no single
`Backend` trait — each backend exposes its own entry points because
their lifetimes, async-ness, and execution shapes genuinely differ:

```rust
// In-memory: synchronous, iterator-based.
pub fn axiom_mem::execute(op: Op)
    -> Result<Box<dyn Iterator<Item = DynamicTuple>>, RelationError>;
pub fn axiom_mem::execute_typed<T: Relational>(op: Op)
    -> Result<impl Iterator<Item = T>, RelationError>;

// PostgreSQL: async client; SQL is compiled deterministically.
pub fn axiom_pg::compile(op: Op)
    -> Result<CompiledSql, RelationError>;
pub async fn axiom_pg::execute(client: &tokio_postgres::Client, op: Op)
    -> Result<Vec<DynamicTuple>, PgError>;
// PgError = Relation(RelationError) | Transport(tokio_postgres::Error)
```

Both backends internally call `axiom_optimizer::optimize(op)` before
interpretation/compilation, so the same optimiser runs ahead of every
execution mode and the optimised AST is the only thing a backend
sees. A relation built once can be handed to either backend — that is
the payoff for keeping the AST a value.

Default Cargo features wire only `axiom-mem`. Consumers that only
need safe in-memory transformations and filtering can use the core
and in-memory backend without pulling in Tokio, `tokio-postgres`, or
any other PostgreSQL-only dependency.

### Iterator interop

The in-memory result type is iterator-based; the facade exposes
`IntoIterator` over `execute`/`execute_typed`'s output so a query
can be consumed with `for`/`collect`/`...`. Iteration is lazy:
tuples are materialised only when a consumer demands them, except in
operators whose semantics require materialisation (sort, set
difference, hash-join build side, group-by accumulator).

---

## Errors

A single `RelationError` covers AST construction and backend
rejection: unknown attribute, duplicate attribute name, schema
mismatch, ambiguous join, malformed expression, incomparable types,
malformed path, path step out of bounds, duplicate aggregate output
name, duplicate order key, opaque predicate inside constraint,
predicate too large, cardinality bound exceeded, unsupported operator
for backend. Implementations MAY add further variants without changing
the meaning of existing ones.

Backend transport failures (network, server error, decode failure)
live in backend-specific error types or wrappers that preserve
`RelationError` as the shared construction-or-rejection cause; they
never live in the core enum.

Panics are NOT RECOMMENDED for invariant violations a caller could
provoke — a panic is a library-internal bug, not a documented failure
mode.

---

## Open questions

Open questions are tracked authoritatively in
[ARCHITECTURE.md](ARCHITECTURE.md) Section 17. Highlights:

- The shape of the refinement-declaration macro (declarative,
  derive, attribute, or hybrid).
- Aggregation over paths (`sum(path!(.posts[*].score))`) and how it
  composes with the summarisation constraint-transfer rule.
- Whether `Relation<T>` is generic over the tuple type or whether
  typed and dynamic surfaces are separate traits sharing the same
  AST.
- Mutation (insert/update/delete). Out of scope today, but `modify`
  is suspiciously close to what an update needs.
- A future `axiom-refine` extraction of the constraint /
  smart-constructor system once a second consumer appears.

---

## Minimal example, end to end

```rust
use axiom_core::{attrs, col, path};

#[derive(Relational)]
struct User { id: i64, name: String, posts: Vec<Post> }

#[derive(Relational)]
struct Post { id: i64, title: String, published: bool, score: i32 }

let users: Op = Source::Memory::try_new(User::schema(), load_users()?)?;

let query: Op = users
    .modify(
        path!(.posts[*]),
        Source::from(Post::schema())
            .restrict(col("published").eq(true))?,
    )?
    .project(attrs![id, name, posts])?;

// In memory.
for u in axiom_mem::execute_typed::<User>(query.clone())? {
    println!("{} has {} posts", u.name, u.posts.len());
}

// Or compile to SQL.
let sql = axiom_pg::compile(query)?;
println!("{}", sql.sql());
```

Both paths run through the same optimiser; the user writes the query
once.
