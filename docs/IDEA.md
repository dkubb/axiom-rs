# Axiom Relational Algebra Library Idea Requirements

Status of This Memo

This document is an internal project specification written in an RFC-style
Markdown form. The document borrows structure and editorial discipline from
RFC 7322 and uses RFC 2026 as process vocabulary for maturity, review, and
applicability.

This document is authoritative for the Axiom Relational Algebra Library
project, a Rust port of the Ruby [AXIOM] library. When this document
conflicts with any other project artifact, this document takes precedence
for goals, scope, non-goals, and invariants. The companion
[ARCHITECTURE.md](ARCHITECTURE.md) document is authoritative for concrete
architecture and technology choices only when those choices preserve this
document's requirements.

Abstract

This document defines the goals, scope, product model, non-goals, and
invariants for a Rust port of the Ruby axiom relational-algebra library.
The library exposes the relational operators as a composable AST that
runs in memory through an iterator-based backend or compiles to
PostgreSQL through an alternative backend. The library extends the
classical flat relational model to non-first-normal-form (NF²) so nested
data structures are first-class, and fuses path-based traversal with
relational operations through an explicit AST optimizer. This document
intentionally avoids concrete implementation choices except where they
are necessary to state the required safety and invariant properties.

Table of Contents

- [Section 1: Introduction](#1-introduction)
- [Section 2: Requirements Language](#2-requirements-language)
- [Section 3: Scope](#3-scope)
- [Section 4: Product Model](#4-product-model)
- [Section 5: Normative Requirements](#5-normative-requirements)
- [Section 6: Non-Goals](#6-non-goals)
- [Section 7: Reliability and Security Considerations](#7-reliability-and-security-considerations)
- [Section 8: References](#8-references)

## 1. Introduction

The Ruby [AXIOM] library built relational queries as an AST that could
be either evaluated against in-memory data or reflected upon to produce
SQL. A separate optimizer rewrote the AST into an equivalent simpler
form. The library shipped operators for projection, restriction, join,
union, difference, rename, and aggregation, and treated relations as
enumerable sequences of tuples.

The Axiom Relational Algebra Library is a Rust port of that idea, with
two additions over the original. First, the model extends to
non-first-normal-form (NF²): a tuple attribute MAY itself be a relation,
and a path (an optic-like value composed of field, index, and traversal
steps) names the place inside a tuple where an operation applies.
Second, the optimizer treats path traversal and relational operations
as one algebra so predicate push-down, projection pruning, and
adjacent-traversal fusion work across the path/relation boundary.

The library's job is narrow:

- expose a composable relational-algebra AST whose execution is deferred;
- support both runtime-validated dynamic schemas and compile-time-typed
  schemas through one operator surface;
- support nested data through paths and the NF² model;
- run the AST either against in-memory iterators or against PostgreSQL;
- run the same optimizer ahead of every backend so the optimization
  surface does not vary by execution mode;
- encode invariants by representation wherever feasible and by smart
  constructor otherwise;
- produce deterministic results and deterministic generated SQL for
  the same inputs;
- bound identifier, schema, and inline collection sizes at
  construction.

The kernel is this: a relational query is a value that can be
transformed before it runs, and the value's representation is the same
whether it runs in memory or as SQL.

## 2. Requirements Language

The key words "MUST", "MUST NOT", "REQUIRED", "SHALL", "SHALL NOT",
"SHOULD", "SHOULD NOT", "RECOMMENDED", "NOT RECOMMENDED", "MAY", and
"OPTIONAL" in this document are to be interpreted as described in BCP 14
[RFC2119] [RFC8174] when, and only when, they appear in all capitals, as
shown here.

Lowercase uses of these words have their ordinary English meanings.

## 3. Scope

The library is intended for personal projects, learning Rust through
a domain (relational algebra) the author already understands, and as
an embedded data-transformation engine inside other personal projects.
It is a library, not a service or daemon. It does not own a network
listener and does not retain persistent state.

The PostgreSQL backend targets PostgreSQL only. SQL-dialect support for
other relational databases is deliberately out of scope for the initial
version. The relational-algebra core is dialect-agnostic so a future
backend MAY add support for another engine, subject to that backend's
own crate and the same AST contract.

The in-memory backend operates on Rust iterators and standard
collections without asynchrony, and MUST be usable on its own without
pulling in the PostgreSQL backend. The PostgreSQL backend uses an
async PostgreSQL client. Consumers that need only safe in-memory
transformations and filtering MUST be able to depend on the core and
in-memory backend without taking on Tokio, `tokio-postgres`, or any
other PostgreSQL-only dependency.

The library is a single-developer tool. There is no multi-tenant
operation, no expectation of adversarial callers, and no sandboxing of
caller-supplied predicates or expressions beyond the typed-input
discipline required by Section 5.

Correctness takes precedence over performance. The architecture MUST
NOT foreclose later performance work. Performance optimizations that
do not change observable behaviour (Section 5.8) are RECOMMENDED;
performance optimizations that weaken any normative requirement in
Section 5 are forbidden. Where a correctness-preserving optimization
already exists at design time (iterator fusion, projection pruning,
constraint-driven simplification, zero-cost smart-constructor
witnesses), the library SHOULD adopt it.

## 4. Product Model

The product model contains these concepts:

- relation;
- schema;
- attribute (name and type);
- tuple;
- dynamic tuple;
- typed tuple;
- expression;
- predicate;
- aggregate;
- constraint set;
- canonical predicate;
- operator AST node;
- path;
- path step (field, index, traversal);
- lens path;
- traversal path;
- relation transformer;
- optimizer pass;
- backend;
- in-memory backend;
- PostgreSQL backend;
- generated SQL;
- SQL cassette;
- relation error.

The model intentionally distinguishes three layers:

- the AST, which is pure data;
- the optimizer, which rewrites the AST into an equivalent simpler AST;
- the backend, which interprets the optimized AST against a concrete
  execution target.

Future backends SHOULD plug into the same model without modifying the
AST or the optimizer.

## 5. Normative Requirements

### 5.1. Relational Completeness

The library MUST support the relational-algebra operators inherited from
the original [AXIOM]: projection, restriction, join (natural and theta),
product, union, intersection, difference, rename, extension,
summarization (group-by with aggregates), order, and limit.

Operators MUST compose. The result of any operator MUST itself be a
relation accepting any other operator that the result's schema admits.

Each operator MUST be a typed AST node. The operator's semantics MUST
be defined by the AST node's interpretation rules, not by the
construction path that produced it.

### 5.2. Smart Constructors

Every public type in the library MUST expose `T::new(...)`, the
infallible constructor.

`T::try_new(...) -> Result<T, RelationError>` MUST also be exposed
when, and only when, construction can fail. Construction can fail
exactly when the constructor narrows its raw input to a strictly
smaller admissible state space — that is, when the type encodes an
invariant that not every value of its raw input domain satisfies.
Types whose raw input domain coincides with their admissible state
space do not narrow, cannot fail, and MUST NOT expose `try_new`.

For operator AST nodes whose validity depends on schema agreement of
their inputs (for example, join over a shared attribute name), the
`new` form MUST be reachable only when input schemas are compile-time
known. When schemas are dynamic, the `try_new` form is the only legal
constructor.

A constructed value MUST be a proof that its invariants hold.
Downstream code MUST NOT re-validate the same invariant. Runtime
assertions are NOT RECOMMENDED for invariants the constructor already
enforced.

Where the typed `new` and the dynamic `try_new` both exist for the same
operator, they MUST produce structurally identical AST nodes so the
optimizer and backends cannot distinguish which constructor was used.

A single declaration MUST define both the type and the runtime
smart-constructor validation that protects its invariant. The same
declaration MUST also gate deserialization: implementations of
`serde::Deserialize` (and any other deserialization path the library
exposes) MUST route through `try_new`, so that a deserialized value
carries the same proof a directly constructed value would. There is
no admissible code path that produces a `T` without running the
validation `try_new` runs. Tests MUST include a deserialization
fixture for every smart constructor proving that an invariant-
violating wire payload is rejected, not silently accepted.

The smart-constructor pair MUST be expressed through a common
construction interface (a `SmartConstructor` trait or equivalent) so
every constrained type exposes its admissible input domain, its
validation function when narrowing is required, and its proof-bearing
output type by the same shape. The trait MUST make the invariants
reflectable: code that holds a `T` MUST be able to obtain a typed
witness of the invariants `T` carries, without re-running validation.
Witness types MUST be unforgeable outside the module that validates
the invariant: the only way to produce a witness for a value whose
construction has not happened in this process is through the smart
constructor or a compile-time proof macro. Once `T` exists, recovering
its witness is a pure accessor — a witness is proof that construction
happened, not a per-call freshness check. The infallible `new` MUST
accept only proof-bearing input whose type already encodes the
invariant, not raw input.

This is **not** a compiler-assisted refinement-type system in the
Liquid-Haskell sense: Rust does not give us the type-level proof
discharge such a system needs, and we do not run an SMT solver. What
we build is the **externally observable behaviour** a refinement
system would provide — a way to define refinements, attach them to
values, and propagate them cleanly across operators — implemented
with ordinary Rust constructors that validate at construction time
and propagation rules that run at AST-construction and optimisation
time. Code that holds a `T` still needs the program to have compiled
and the constructor to have run; what it does not need is to re-check
the invariant.

The macros that introduce constrained types (`#[derive(Relational)]`,
`attr!`, `path!`, future constraint-declaration macros) MUST be
designed so they remain useful hooks for future compiler extensions
or external static analysers. Specifically, the macro inputs and
generated witness types SHOULD carry enough structured metadata that
a later rustc plugin, lint, or proof-carrying-code tool could lift
some of the runtime checks to compile time without rewriting the
public API.

The interface is designed so the trait and the constraint module of
Section 5.15 can be extracted into a standalone
constraint-propagation crate at a later milestone; the initial
implementation lives in `axiom-core` and MUST NOT take dependencies
that would prevent that extraction.

### 5.3. Most-Constrained Admissible Types

Every public type MUST use the most-constrained admissible
representation that still expresses every value the contract requires.
"Admissible" means: any value the contract allows is representable,
and no value the contract forbids is representable. When more than one
representation is admissible, the smaller state space MUST be chosen.

Specifically:

- a header (the attribute list of a schema, projection, group-by, or
  rename) MUST be a bounded ordered set with unique attribute names,
  not a bounded vector that admits duplicates;
- an aggregate list MUST be a bounded set whose elements are uniquely
  identified by output attribute name;
- an order key list MUST be a bounded ordered set whose key is the
  full canonical `OrderKey` (attribute reference plus direction plus
  null ordering); duplicate full keys are rejected at construction;
  later keys whose attribute coincides with an earlier key's
  attribute MAY be normalised away, because the earlier key already
  determines that attribute's ordering;
- a path step sequence MUST be a bounded vector (order matters), and
  the lens/traversal kind partition (Section 5.7) MAY be enforced by
  splitting the carrier into kind-specific step types where doing so
  remains expressive enough;
- a row MUST be a bounded vector whose length and per-position types
  are constrained at construction by the schema, not a free vector of
  values.

These are examples; the rule applies across the library. A
representation that admits invalid states which a more-constrained
type would forbid is not conformant with this document.

### 5.4. Schema and Tuple Representation

The library MUST support two tuple representations:

- a dynamic tuple keyed by schema position, suitable for runtime
  sources such as the PostgreSQL result set or parsed JSON;
- a typed tuple backed by a Rust struct that derives a library-supplied
  derive macro.

Both representations MUST implement the same internal tuple trait so
operators do not branch on representation.

A schema's header MUST be a bounded ordered set of `(name, type)`
attributes whose uniqueness by name is enforced by the
representation, not by a runtime check after construction. A schema
MUST be inspectable at runtime regardless of representation.

A typed tuple's schema MUST be derivable from the struct's fields at
compile time. A dynamic tuple's schema MUST be supplied at relation
construction time and MUST be validated against the supplied row data
by `try_new`.

### 5.5. Expressions and Predicates

Predicates and computed expressions MUST be values, not closures. The
opaque-closure form is admissible only as the explicit escape hatch
defined below.

The expression algebra MUST support at least:

- attribute reference;
- literal value;
- comparison (`=`, `≠`, `<`, `≤`, `>`, `≥`);
- logical connectives (`∧`, `∨`, `¬`);
- arithmetic on numeric types;
- string concatenation and pattern match;
- aggregate functions: `count`, `sum`, `min`, `max`, `avg`.

Comparison operators MUST be defined only between operands whose
static type belongs to the same comparable equivalence class
(`Bool` equality only; totally-ordered numeric types within a single
numeric class; totally-ordered string and bytes within their own
class; `DateTime` with explicit zone). The expression constructor
MUST reject incomparable operand pairs (e.g. `Bytes < String`,
`Json` against anything) with `RelationError::IncomparableTypes`.

A closure-backed predicate MAY be accepted as a convenience escape
hatch for the in-memory backend only. Such a predicate MUST be marked
opaque in the AST. The optimizer MUST NOT attempt to rewrite across an
opaque predicate. The PostgreSQL backend MUST reject an AST that
contains an opaque predicate with a typed error at compilation time.

### 5.6. Non-First-Normal-Form Data

The relational model MUST admit nested attributes, including
attributes whose type is itself a relation, a collection of
relation-like values, or another explicitly modeled nested value type.

A nested attribute MUST be addressable through a path. Operations that
target a nested attribute MUST be expressible without first flattening
the outer relation and MUST NOT require the implementation to
materialize intermediate sub-relations beyond what the optimizer's
fused execution requires.

The library MUST provide an `unnest` operator that flattens a nested
attribute into outer rows by repeating outer attributes, and a `nest`
operator that performs the inverse where the schema permits.

The library MUST provide a `modify` operator that applies a
sub-relation transformation at a given path and substitutes the result
back in place. The path supplied to `modify` MUST focus a
relation-like value. Typed paths encode this in the path type; dynamic
paths are rejected by `try_modify` when the focused schema is not
relation-like.

### 5.7. Paths and Optics

The library MUST provide a path type that names a location inside a
tuple. A path is composed of zero or more steps, where each step is
exactly one of:

- a named field access;
- a positional index access into an ordered collection;
- a traversal (`each`) over every element of a collection.

A path MUST compose by concatenation. Composition MUST be associative.
The identity path MUST be supported and MUST be a left and right
identity under composition.

A path that contains zero `each` steps MUST be a lens; a path that
contains one or more `each` steps MUST be a traversal. The library
MUST distinguish the two at the type level when the path is
constructed by the path macro. The macro's return type MUST encode
the optic class, for example as distinct `LensPath` and `TraversalPath`
types or as a sealed type parameter whose value cannot be forged by
callers.

A path applied to a typed tuple MUST be checkable against the tuple's
schema at compile time. A path applied to a dynamic tuple MUST be
checked against the schema by `try_new` at construction time.

### 5.8. AST and Optimization

A relational query MUST be representable as a value of the operator
AST. The AST MUST be opaque to the caller's execution intent: the same
AST value MUST be acceptable to every backend.

The library MUST provide an optimizer that rewrites the AST into an
equivalent simpler form. Equivalence means the optimizer preserves the
observable result for each AST: ordered operators (`order`,
sequence-preserving compositions of `order`) preserve sequence
equality, unordered relational operators preserve bag (multiset)
equality, and `limit`/`offset` are evaluated after the ordering
semantics defined for their input.

The optimizer MUST be deterministic: the same AST input MUST produce
the same AST output across runs and across machines.

The optimizer MUST be a separate pass over the AST. Backends MUST NOT
embed optimization rewrites. Backends MAY decline to execute optimized
AST shapes that they do not support, but MUST NOT introduce new
optimizations of their own.

### 5.9. Fusion of Paths and Relational Operations

The optimizer MUST implement at minimum these path/relation rewrites:

- predicate push-down through traversal, in which a restriction applied
  outside a `modify(path, ...)` is pushed down into the traversal when
  the predicate references only attributes reachable along the path;
- adjacent traversal fusion, in which two `modify` operations along
  composable paths are combined into a single `modify` along the
  composed path;
- projection pruning across paths, in which attributes not referenced
  by downstream operators are dropped during traversal rather than
  after.

Each rewrite MUST carry a domain precondition that proves it is
semantics-preserving under the equivalence in Section 5.8. In
particular, a restriction may cross a `modify` boundary only when
attribute-dependency analysis proves the predicate's free attributes
lie entirely within the target focus (for push-down) or entirely
outside it (for hoisting), so that the rewritten AST is observationally
equivalent under the ordered or bag semantics that apply.

The optimizer MAY implement additional rewrites. Backends MUST accept
every optimized AST shape without behavioural change.

Fusion is a correctness-preserving optimization. An implementation
whose output is not observationally equivalent to its input under
Section 5.8's equivalence is not conformant with this document.

### 5.10. Backend Independence

The library MUST support at least two backends:

- an in-memory backend that interprets the AST against Rust iterators
  and standard collections;
- a PostgreSQL backend that compiles the AST to a PostgreSQL query and
  executes it through an async PostgreSQL client.

A query MUST be constructible once and executable through either
backend. A backend MUST NOT require backend-specific operators in the
AST.

A backend MUST reject operators it cannot execute (for example, an
opaque closure predicate against the PostgreSQL backend) with a typed
error during backend compilation, before query execution.

### 5.11. Iterator Interop

A relation in the in-memory backend MUST implement `IntoIterator` so
the caller can iterate result tuples without an explicit execution
call. The iterator MUST yield typed tuples when the schema is typed
and dynamic tuples when the schema is dynamic.

Iteration MUST be lazy: tuples MUST NOT be materialized except as
demanded by the iterator's consumer or by an operator whose semantics
require materialization (sort, set difference, hash-join build side,
group-by accumulation).

### 5.12. Determinism

The library MUST be deterministic in the sense relevant to a query
library: for the same input data, the same query, and the same
backend, the result MUST be the same across runs.

The in-memory backend's iteration order MUST be well-defined for any
operator. Where the relational algebra does not constrain order (set
operations on unordered relations), the iteration order MUST follow a
documented stable rule, not the iteration order of the underlying
hash-keyed collection.

The PostgreSQL backend's emitted SQL string MUST be a deterministic
function of the optimized AST. Whitespace, identifier quoting,
parameter placeholder ordering, and clause ordering MUST follow a
stable canonical form so SQL cassettes are stable across runs.

### 5.13. Error Model

Errors raised at construction or at backend rejection MUST be typed.
The library MUST expose a single `RelationError` enum that covers AST
construction and backend rejection, distinguishing at minimum:

- unknown attribute;
- duplicate attribute name;
- schema mismatch;
- ambiguous join attribute;
- malformed expression;
- incomparable types;
- malformed path;
- path step out of bounds;
- duplicate aggregate output name;
- duplicate order key;
- opaque predicate inside constraint;
- predicate too large (after canonicalisation);
- cardinality bound exceeded;
- unsupported operator for backend.

Implementations MAY add further variants. Adding a variant MUST NOT
change the meaning of an existing variant.

Backend transport failures MUST be exposed through backend-specific
error types or wrappers that preserve `RelationError` as the shared
construction-or-rejection cause; transport variants MUST NOT live in
the core enum.

String errors MUST NOT be used as the primary error vocabulary.
Panics MUST NOT be used to signal invariant violations a caller could
provoke; a panic is a library-internal bug, never a documented
failure mode.

### 5.14. Bounded Inputs

Attribute names, identifier strings, and schema sizes MUST be bounded
by typed constraints where the bound is part of the contract. A
schema's attribute count and each inline collection attribute MUST
have explicit upper bounds enforced by typed collections, unless the
collection is a streaming backend cursor rather than an AST value.

Caller-supplied identifiers (attribute names, relation names) MUST be
parsed at construction time and stored in a typed identifier newtype.
Raw string identifiers MUST NOT cross the operator AST boundary.

Conformant implementations MUST accept inputs at least up to these
minimum upper bounds, and MAY reject inputs larger than the bound the
implementation chooses:

- attribute name length: at least 64 bytes;
- attributes per schema: at least 256;
- elements per `InList` value set: at least 1024;
- per-row constraints per `ConstraintSet`: at least 256;
- SQL parameters per compiled query: at least 4096;
- path step count: at least 32.

Implementations MUST document the exact upper bound each constant
takes in their build.

### 5.15. Constraint Reflection and Projection

The library MUST be able to reflect, at runtime, the constraints that
apply to a relation at two granularities:

- **per-attribute constraints**, attached to one attribute in the
  schema (for example: non-null, numeric range, string length bound,
  enum membership, foreign-key-style reference). A per-attribute
  constraint's carrier predicate MUST have a free-attribute set
  equal to `{attr}` — the smart constructor MUST reject a predicate
  whose free attributes are a strict subset (the constraint is then
  a closed proposition — trivially true or trivially false — with no
  per-attribute content) or a strict superset (the constraint is
  per-row, not per-attribute);
- **per-row constraints**, attached to the relation as a whole and
  free to reference more than one attribute (for example: `a < b`, a
  bag/set equality between attributes, a check constraint).

A constraint MUST be representable as a value of the expression
algebra in Section 5.5, so that constraints and predicates share a
single normal form and the optimizer treats them uniformly. Opaque
predicates (Section 5.5 escape hatch) MUST NOT be admitted as
reflected constraints and MUST NOT appear in a `ConstraintSet`.

A `ConstraintSet` forms a bounded, finite join-semilattice under
refinement, equipped with a partial meet whose partiality is induced
solely by the finite carrier and not by the underlying mathematical
structure. More constraints sit *below* fewer in the refinement
order, the empty set is the top (least constrained), and the
unsatisfiable set (the canonical `false` constraint) is the bottom.
The semilattice has finitely many ascending chains because the
inner bounded collections cap their cardinality.

The **meet** operation is union followed by canonicalisation and
redundant-constraint elimination. Meet is a **partial** function:
when the union of two valid `ConstraintSet`s exceeds
`MAX_ROW_CONSTRAINTS` or any other inner bound, meet returns
`RelationError::CardinalityBoundExceeded` instead of an in-band value.
This is the explicit cost of bounded carriers — the underlying
mathematical structure is closed under meet, but the bounded
representation is not, so we surface the failure as a typed error
rather than silently truncating. The **join** is the canonical
intersection of two sets' elements and is always total.

Because meet is partial, every per-operator propagation law below
that invokes meet — whether to combine two input `ConstraintSet`s
(intersection, join, product, modify) or to add new constraints to
one input (restriction, extension, summarization, unnest, nest) — is
itself partial: the propagation function returns
`Result<ConstraintSet, RelationError>` and surfaces
`CardinalityBoundExceeded` rather than silently dropping a sound
constraint. Operators whose law uses only the total `join` or pure
subset preservation (`union`, `projection`, `rename`, `order`,
`limit`, `difference`) expose an infallible propagation result,
though the propagation surface returns `Result` uniformly with an
unreachable `Err` arm.

Because propagation is fallible, the four algebraic laws above are
stated **over the `Ok` arm**: where two propagation expressions appear
on both sides of an equality, they hold when both sides return
`Ok(...)` with equal contained `ConstraintSet`s, and when either side
returns `Err(CardinalityBoundExceeded)` the whole propagation returns
that error (the implementing optimizer rewrite then declines to fire).
Monotonicity is "refining the input refines the output whenever both
sides return `Ok`"; identity and composition are likewise restricted
to the `Ok` arm.

Equality of constraints MUST be defined by `CanonicalPredicate`
(Section 5.5 escape hatch excluded), not by raw syntactic equality,
so that semantically equivalent predicates produce the same set
element.

`CanonicalPredicate` MUST close equality under every predicate
rewrite the optimizer performs: constant folding, double-negation
elimination, De Morgan to CNF (or to whatever normal form the
implementation chooses, applied consistently), commutative-operand
sorting under a canonical order, alpha-renaming of attribute
references to a schema-position canonical form, redundant-clause
elimination, and pattern-string normalisation. Implementations MAY
choose any concrete normal form (CNF, NNF + hash-consing, etc.); the
contract is that two predicates that the optimizer would treat as
equivalent compare equal as `CanonicalPredicate`, and `try_new` is
total in time and space polynomial in input size or fails with a
typed `RelationError::PredicateTooLarge`.

The library MUST project constraints through every operator. The
projection rule for each operator is the per-operator law below; the
collection of rules MUST satisfy these algebraic obligations:

- **Soundness.** For every operator `f` and every input relation `r`
  that satisfies its input `ConstraintSet`, every constraint in
  `propagate(f, input_cs)` MUST hold on every tuple of `f(r)`.
- **Monotonicity.** If `cs1` refines `cs2`, then `propagate(f, cs1)`
  refines `propagate(f, cs2)`. Adding input information never removes
  output information.
- **Identity.** For an operator that is the identity (or that the
  optimizer can prove equivalent to the identity),
  `propagate(f, cs) == cs`.
- **Composition (unary chains).** For any two unary operators `f` and
  `g` whose schemas chain (`g`'s output schema equals `f`'s input
  schema), `propagate(f ∘ g, cs) == propagate(f, propagate(g, cs))`.
- **Composition (full AST).** For an AST node `op` with child nodes
  `c1, ..., cn`, the propagation through the whole AST is the
  per-node propagation applied to the propagated `ConstraintSet`s of
  the children:
  `propagate_ast(op) == propagate_node(op, propagate_ast(c1), …, propagate_ast(cn))`.
  This is the only form composition takes for binary operators; the
  unary-chain law is a corollary.

The per-operator laws are:

- **projection** MUST preserve every per-attribute constraint whose
  attribute survives the projection, and every per-row constraint
  whose free attributes are all in the projected header;
- **restriction** MUST preserve all input constraints and MUST add
  the restriction's predicate as a new per-row constraint on the
  result;
- **rename** MUST preserve every input constraint, with attribute
  references inside renamed accordingly;
- **extension** MUST preserve every input constraint and MUST add a
  constraint that equates the new attribute with the extending
  expression (the equality constraint is the strongest the rewrite
  admits and is what makes constraint-driven simplification useful);
- **join** MUST union the constraints of its inputs after renaming
  for attribute disambiguation, MUST add the join predicate as a new
  per-row constraint on the result **when the predicate is
  non-opaque** (opaque join predicates are dropped from the result
  `ConstraintSet` per the opacity rule above), and MUST add equality
  constraints between attributes the join predicate equates (natural
  and equi-join columns) — equality across equated columns is a
  free, sound fact and uniform propagation MUST NOT be optional;
- **product** MUST union the constraints of its inputs after
  disambiguation; it adds no per-row constraint of its own;
- **union** MUST preserve the constraints that hold for every input,
  using `CanonicalPredicate` equality (not raw expression equality)
  to decide which constraints hold on both sides;
- **intersection** MUST union the input constraints (every constraint
  of either input holds on the result);
- **difference** MUST preserve the constraints of its left input;
- **summarization** MUST preserve constraints whose free attributes
  are all in the grouping header. For each aggregate `agg(e)` over an
  attribute `e` carrying an attribute constraint `pred(e)`, the rule
  MUST add the aggregate-specific transferred constraint where one is
  sound: `sum`, `min`, `max` of a non-negative input is non-negative;
  `min(e) >= lo` when `e >= lo`; `max(e) <= hi` when `e <= hi`;
  `count(_)` is always non-negative. Other transfers MAY be added;
- **order** MUST preserve all input constraints (ordering does not
  add or remove tuples);
- **limit** MUST preserve all per-attribute constraints and every
  per-row constraint whose free attributes are not cardinality-derived
  (a future cardinality-aware extension MAY tighten this);
- **modify** treats input constraints in three buckets by their free
  attributes relative to the path focus:
  pure-outer (free-attribute set disjoint from the focus) MUST be
  preserved unchanged;
  pure-focus (free-attribute set entirely within the focus) MUST be
  replaced with the constraints derived from the sub-relation; for a
  traversal path, the substitution applies pointwise to each element;
  spanning (free-attribute set straddles the focus boundary) MUST be
  discharged unless the rewrite proves the spanning constraint still
  holds after the focus replacement.
  Additionally, cardinality-derived constraints on the focus (count
  bounds, non-emptiness) MUST be discharged unless the sub-relation
  is known by construction to preserve cardinality (the optimizer
  treats `restrict` as cardinality-non-increasing, `project` as
  cardinality-non-increasing under set semantics and
  cardinality-preserving under bag semantics);
- **unnest** MUST preserve every outer constraint and MUST add the
  constraints of the nested relation as constraints on the unnested
  output attributes;
- **nest** MUST preserve every outer constraint not in the nested
  group and MUST lift the constraints whose free attributes lie
  entirely inside the nested group into per-element constraints of
  the new nested attribute.

The optimizer MAY exploit projected constraints to enable additional
rewrites (for example: discharging a restriction whose predicate is
implied by an existing per-row constraint, narrowing a range, or
eliminating an impossible join). Such rewrites MUST remain
correctness-preserving under Section 5.8's equivalence.

The constraint system MUST be designed so it can be extracted into a
standalone constraint-and-refinement-propagation crate at a later
milestone. This crate would be the surface a refinement-type system
would expose — predicate-form refinements, attached to relations,
propagated across operators — without committing to refinement-type
checking. Future relocation MUST preserve the constraint language and
projection rules above; any required import, visibility, or
package-boundary changes MUST be mechanical and behavior-preserving.

### 5.16. Testability

The library MUST be testable without contacting PostgreSQL during
default test runs. The PostgreSQL backend MUST be testable in two
layers:

- the SQL-generation layer, which converts an optimized AST to a SQL
  string and a parameter vector, MUST be tested by comparison against
  committed expected-SQL fixtures (SQL cassettes);
- the execution layer MAY be tested against a real PostgreSQL instance
  through opt-in integration tests gated by an environment variable
  recognised only by the integration-test crate.

Tests MUST prove:

- the algebraic identities each operator satisfies (where applicable:
  projection is idempotent; union and intersection are commutative
  and associative; product is commutative and associative modulo
  column reordering; restriction is idempotent under conjunction).
  Tests MUST NOT assert identities the operator does not satisfy
  (`difference` is non-commutative; `join` is in general
  non-idempotent);
- the optimizer's rewrites preserve Section 5.8's observable
  equivalence, including sequence equality where ordering is
  observable and bag equality otherwise;
- smart constructors reject every documented invalid input;
- path composition is associative and the identity path is a left and
  right identity;
- the in-memory backend and the PostgreSQL backend produce result
  shapes consistent with the AST for every committed query fixture.

Production code MUST NOT contain test-only branches, test-only
environment variables, or behaviour switches whose only purpose is to
make tests pass.

## 6. Non-Goals

The Axiom Relational Algebra Library is not:

- a multi-database query builder beyond PostgreSQL in the initial
  version;
- an Object-Relational Mapper;
- a connection pool, transaction manager, or migration tool;
- a persistent store of any kind;
- a distributed query engine;
- a streaming or push-based query system;
- a substitute for handwritten SQL where handwritten SQL is the right
  tool.

Cross-dialect SQL generation, schema migration, identity-map semantics,
adaptive query planning across collected statistics, mutation
operations (insert, update, delete), and a streaming execution model
are future extensions unless the architecture document explicitly
includes them in a later milestone.

## 7. Reliability and Security Considerations

The primary reliability concerns are silent disagreement between
in-memory and SQL execution semantics for the same AST, unsound
optimizer rewrites, non-deterministic SQL generation, hidden coupling
between backend code paths, and unbounded materialization of nested
data through `unnest`.

The requirements in Section 5 are reliability requirements.
Implementations that weaken them are not conformant with this
document.

Predicate expressions and projected attribute references are taken from
caller-supplied paths and names. Construction-time validation MUST
reject names not present in the schema. The PostgreSQL backend MUST
quote emitted identifiers so an attribute name accepted at
construction time cannot be reinterpreted as SQL syntax by the engine.

The PostgreSQL backend MUST parameterize every literal value drawn
from caller-supplied expressions. Inline literals MUST NOT be emitted
as unparameterised SQL text. The SQL cassette format MUST preserve
parameter placeholders so cassette comparison detects accidental
inlining.

## 8. References

### 8.1. Normative References

[RFC2119] Bradner, S., "Key words for use in RFCs to Indicate
Requirement Levels", BCP 14, RFC 2119, March 1997,
<https://www.rfc-editor.org/rfc/rfc2119.html>.

[RFC8174] Leiba, B., "Ambiguity of Uppercase vs Lowercase in RFC 2119
Key Words", BCP 14, RFC 8174, May 2017,
<https://www.rfc-editor.org/rfc/rfc8174.html>.

### 8.2. Informative References

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
