//! Attribute type system: `Type` and the `Value` companion.
//!
//! `Type` is a closed sum already minimised by the Rust enum.
//! `Value` carries refined inner types so its representable
//! state space matches the admissible value space variant by
//! variant: `Float64` rejects NaN and the infinities, nested
//! collection variants enforce length bounds, and so on.

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;

use whittle::Refined;
use whittle::primitive::{Finite, LenItems};

use crate::limits::{MAX_ARRAY_LEN, MAX_ROWS_IN_AST};

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
    /// `Type::Float64`. Refined to `Finite` — neither NaN nor
    /// `±INF` is admissible because both break `PartialEq`
    /// reflexivity (NaN) or well-definedness of arithmetic
    /// aggregates (`±INF`).
    Float64(Refined<f64, Finite>),
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
    /// Nested relation row collection, bounded by
    /// `MAX_ROWS_IN_AST`.
    Relation(Refined<Vec<crate::row::Row>, LenItems<0, { MAX_ROWS_IN_AST }>>),
    /// NF² array, bounded by `MAX_ARRAY_LEN`.
    Array(Refined<Vec<Self>, LenItems<0, { MAX_ARRAY_LEN }>>),
    /// `Type::Optional`.
    Optional(Option<Box<Self>>),
}
