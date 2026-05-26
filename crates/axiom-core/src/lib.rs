//! Axiom kernel.
//!
//! See `docs/IDEA.md` and `docs/ARCHITECTURE.md` at the repo root
//! for the specification this crate implements.

#![no_std]

extern crate alloc;

mod canonical;
mod constraint;
mod expression;
mod identifier;
mod join;
mod limit;
mod limits;
mod op;
mod op_enums;
mod order;
mod path;
mod row;
mod schema;
mod source;
mod ty;

pub use canonical::{CanonicalPredicate, CanonicalPredicateError};
pub use constraint::{
    AttributeConstraint, AttributeConstraintError, ConstraintSet,
    ConstraintSetError, RowConstraint,
};
pub use expression::{Expression, OpaqueId, Predicate};
pub use identifier::{
    AttributeName, AttributeNameError, Pattern, PatternError,
    TableName, TableNameError,
};
pub use join::{EquiPair, EquiPairs, EquiPairsError, JoinOn};
pub use op::{
    AttributeSet, AttributeSetError, GroupingSet, GroupingSetError,
    NamedAggKey, NamedAggSet, NamedAggSetError, Op, OpError,
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
pub use op_enums::{Agg, BinOp, JoinKind, NamedAgg, UnOp};
pub use order::{
    Direction, NullOrder, OrderKey, OrderKeys, OrderKeysError,
};
pub use path::{
    AnyPath, Kind, Lens, LensPath, Path, PathError, PathStep,
    Traversal, TraversalPath,
};
pub use row::{Row, RowError};
pub use schema::{
    Attribute, AttributeKey, Schema, SchemaCardinality,
    SchemaCardinalityError, SchemaError,
};
pub use source::{Rows, RowsError, Source};
pub use ty::{Type, Value};
