//! Axiom kernel.
//!
//! See `docs/IDEA.md` and `docs/ARCHITECTURE.md` at the repo root
//! for the specification this crate implements.

#![no_std]

extern crate alloc;

mod limits;
mod schema;

pub use limits::MAX_SCHEMA_ATTRIBUTES;
pub use schema::{Schema, SchemaCardinality, SchemaCardinalityError};
