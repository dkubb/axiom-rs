//! Axiom-rs workspace-wide bound constants.
//!
//! These match docs/ARCHITECTURE.md §4.1 (constants table). Lifting
//! them out lets the type-level rules below (which use Whittle's
//! const-generic `Within`/`AtMost`/etc.) reference them by name.

/// Maximum number of attributes in a single relation schema.
///
/// Used by `SchemaCardinality` to bound the count, and (later) by
/// every operator whose admissible attribute set is keyed off the
/// schema header.
pub const MAX_SCHEMA_ATTRIBUTES: usize = 256;
