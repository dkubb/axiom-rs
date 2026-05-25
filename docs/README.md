# Axiom-rs Documentation

This directory contains the project specification for axiom-rs, a Rust
port of the Ruby [axiom](https://github.com/dkubb/axiom) relational-
algebra library.

## Contents

- [IDEA.md](IDEA.md) — authoritative project specification (RFC-style):
  goals, scope, product model, normative requirements, non-goals,
  reliability and security considerations.
- [ARCHITECTURE.md](ARCHITECTURE.md) — concrete architecture derived
  from IDEA.md: workspace layout, dependency direction, schema and
  tuple representation, operator AST, paths, expressions, optimizer
  pipeline, in-memory and PostgreSQL backends, testing architecture,
  build sequence, open issues.
- [DESIGN.md](DESIGN.md) — early high-level design sketch. Superseded
  for architectural commitments by ARCHITECTURE.md and retained as a
  design-rationale historical record. Where it conflicts with
  IDEA.md or ARCHITECTURE.md, the other docs win.

[IDEA.md](IDEA.md) is authoritative for goals, scope, non-goals, and
invariants. [ARCHITECTURE.md](ARCHITECTURE.md) is authoritative for
concrete technology and structural choices only when those choices
preserve IDEA.md's requirements; if the two conflict,
[IDEA.md](IDEA.md) takes precedence.
