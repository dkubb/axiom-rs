//! Axiom kernel.
//!
//! See `docs/IDEA.md` and `docs/ARCHITECTURE.md` at the repo root
//! for the specification this crate implements.

#![no_std]

extern crate alloc;

mod identifier;
mod limit;
mod limits;
mod order;
mod row;
mod schema;
mod ty;

pub use identifier::{
    AttributeName, AttributeNameError, Pattern, PatternError,
    TableName, TableNameError,
};
pub use limit::{
    BoundedIndex, BoundedIndexError, LimitCount, LimitCountError,
    Offset, OffsetError,
};
pub use limits::{
    MAX_ATTRIBUTE_NAME_LEN, MAX_LIMIT_COUNT, MAX_OFFSET,
    MAX_PATH_INDEX, MAX_PATH_STEPS, MAX_PATTERN_LEN,
    MAX_SCHEMA_ATTRIBUTES, MAX_TABLE_NAME_LEN,
};
pub use order::{
    Direction, NullOrder, OrderKey, OrderKeys, OrderKeysError,
};
pub use row::{Row, RowError};
pub use schema::{
    Attribute, Schema, SchemaCardinality, SchemaCardinalityError,
    SchemaError,
};
pub use ty::{Type, Value};
