//! Axiom-rs workspace-wide bound constants.
//!
//! These match docs/ARCHITECTURE.md §4.1 (constants table). Lifting
//! them out lets the type-level rules below (which use Whittle's
//! const-generic `Within` / `AtMost` / `LenChars` / …) reference
//! them by name.

/// Maximum number of attributes in a single relation schema.
pub const MAX_SCHEMA_ATTRIBUTES: usize = 256;

/// Maximum length of an `AttributeName` in characters.
pub const MAX_ATTRIBUTE_NAME_LEN: usize = 64;

/// Maximum length of a `TableName` in characters.
pub const MAX_TABLE_NAME_LEN: usize = 128;

/// Maximum length of a `Pattern` (LIKE-style pattern) in characters.
pub const MAX_PATTERN_LEN: usize = 1024;

/// Upper bound on `Offset`. Set to `i64::MAX / 2` so the sum
/// `offset + limit_count` fits in `u64` without overflow.
pub const MAX_OFFSET: u64 = u64::MAX / 2;

/// Upper bound on the inhabited part of `LimitCount`. Same value as
/// `MAX_OFFSET` for the same overflow-avoidance reason.
pub const MAX_LIMIT_COUNT: u64 = u64::MAX / 2;

/// Upper bound on `BoundedIndex` (positional index inside a `Path`).
pub const MAX_PATH_INDEX: usize = 65536;

/// Upper bound on the number of steps in a single `Path`.
pub const MAX_PATH_STEPS: usize = 32;

/// Upper bound on rows embedded in a `Source::Memory` or a nested
/// `Value::Relation`. Matches the architecture spec
/// (`MAX_ROWS_IN_AST`).
pub const MAX_ROWS_IN_AST: usize = 16384;

/// Upper bound on `Vec<Value>` literals embedded in an
/// `Expression::InList`.
pub const MAX_IN_LIST: usize = 1024;

/// Upper bound on `Value::Array` element count.
pub const MAX_ARRAY_LEN: usize = MAX_IN_LIST;
