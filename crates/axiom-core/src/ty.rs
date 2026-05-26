//! Attribute type system: `Type` and the `Value` companion.
//!
//! Both are closed sums whose state space is already minimised by
//! the Rust enum: no refinement is needed beyond the type-level
//! discipline. They live here so other modules (`Attribute`,
//! `Schema`, `Expression`) can reference them.

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;

/// Attribute type. The NF² extensions (`Relation`, `Array`,
/// `Optional`) are recursive variants over `Schema` and `Type`.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Type {
    /// 8-bit boolean.
    Bool,
    /// Signed 32-bit integer.
    Int32,
    /// Signed 64-bit integer.
    Int64,
    /// 64-bit IEEE-754 float.
    Float64,
    /// Exact decimal (precision/scale via a future refinement).
    Decimal,
    /// UTF-8 string.
    String,
    /// Opaque byte sequence.
    Bytes,
    /// Absolute instant (timezone-aware).
    DateTime,
    /// JSON value (opaque to the optimizer outside the documented
    /// JSON-path subset).
    Json,
    /// NF² nested relation; carries its own schema header.
    Relation(Box<crate::Schema>),
    /// Ordered collection of homogeneous values.
    Array(Box<Self>),
    /// `Some` / `None` over an inner type.
    Optional(Box<Self>),
}

/// A value matching some `Type`.
///
/// The variants here mirror `Type`. `Decimal`, `DateTime`, and
/// `Json` are stored as strings until the `rust_decimal` / `chrono`
/// integrations are wired up (those land behind Cargo features in
/// later commits).
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum Value {
    /// `Type::Bool`.
    Bool(bool),
    /// `Type::Int32`.
    Int32(i32),
    /// `Type::Int64`.
    Int64(i64),
    /// `Type::Float64`. Stored as bits to keep `PartialEq` honest
    /// (NaN is intentionally not equal to itself; refined floats
    /// can use `NotNan` to forbid the case).
    Float64(f64),
    /// `Type::Decimal`. Canonical string form pending the
    /// `rust_decimal` feature.
    Decimal(crate::identifier::Pattern),
    /// `Type::String`.
    String(String),
    /// `Type::Bytes`.
    Bytes(Vec<u8>),
    /// `Type::DateTime`. ISO-8601 string pending `chrono` feature.
    DateTime(crate::identifier::Pattern),
    /// `Type::Json`. Raw JSON text pending a real JSON wrapper.
    Json(crate::identifier::Pattern),
    /// Nested relation row collection.
    Relation(Vec<crate::row::Row>),
    /// NF² array.
    Array(Vec<Self>),
    /// `Type::Optional`.
    Optional(Option<Box<Self>>),
}
