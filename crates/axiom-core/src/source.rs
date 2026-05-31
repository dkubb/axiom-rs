//! `Source`: the AST leaf that introduces a relation.
//!
//! Two variants per docs/ARCHITECTURE.md §9.3:
//! - `Memory(MemorySource)` carries inline data proved to match
//!   the supplied schema in both width and per-attribute type.
//! - `Table { schema, name }` is a symbolic reference for backends
//!   that consult an external store.
//!
//! The `MemorySource` carrier's fields are private so the schema /
//! rows pair always carries its proof: external code cannot
//! construct a `Source::Memory` whose rows mismatch the schema.

use alloc::vec::Vec;

use thiserror::Error;
use whittle::primitive::{CollectionError, LenItems};
use whittle::refinement;

use crate::identifier::TableName;
use crate::infer::ValueTypeError;
use crate::limits::MAX_ROWS_IN_AST;
use crate::row::Row;
use crate::schema::Schema;

// Schema-validated rows are simply a length-bounded `Vec<Row>` for
// now. Per-row value-type checking against the schema lands once
// the schema-aware Source::try_memory constructor is wired in; the
// structural bound is already enforced by whittle.
type SourceRowsRule = LenItems<0, { MAX_ROWS_IN_AST }>;

refinement! {
    /// Length-bounded list of `Row`s, the inline data for a memory source.
    #[derive(Debug, Clone, PartialEq)]
    pub Rows: Vec<Row>, SourceRowsRule;
}

/// Constructor error for `Rows`.
pub type RowsError = CollectionError;

impl Rows {
    /// Borrow the underlying row list.
    #[must_use]
    #[inline]
    pub const fn as_slice(&self) -> &[Row] {
        self.as_inner().as_slice()
    }

    /// Number of rows.
    #[must_use]
    #[inline]
    pub const fn len(&self) -> usize {
        self.as_inner().len()
    }

    /// `true` if the source has no rows.
    #[must_use]
    #[inline]
    pub const fn is_empty(&self) -> bool {
        self.as_inner().is_empty()
    }
}

/// Inline data with its schema, proved to match by construction.
///
/// Fields are private; the only construction path is
/// `MemorySource::try_new(schema, raw_rows)`, which checks
/// per-row width and per-position value-against-type matching.
#[derive(Debug, Clone, PartialEq)]
pub struct MemorySource {
    schema: Schema,
    rows: Rows,
}

/// Constructor error for `MemorySource`.
#[derive(Debug, Error, PartialEq, Eq)]
#[non_exhaustive]
pub enum MemorySourceError {
    /// `Rows` length bound failed.
    #[error("rows: {0}")]
    Rows(#[source] CollectionError),

    /// A row failed to match the schema.
    #[error("row at index {row_index}: {source}")]
    RowMismatch {
        /// Position of the offending row in the input list.
        row_index: usize,
        /// The structural failure inside that row.
        #[source]
        source: ValueTypeError,
    },
}

impl MemorySource {
    /// Validate `raw_rows` against `schema` and wrap.
    ///
    /// # Errors
    ///
    /// Returns `Rows` for a length-bound violation on the row
    /// collection, and `RowMismatch { row_index, source }` for the
    /// first row whose values do not conform to the schema (width
    /// or per-position type).
    pub fn try_new(schema: Schema, raw_rows: Vec<Row>) -> Result<Self, MemorySourceError> {
        for (row_index, row) in raw_rows.iter().enumerate() {
            crate::infer::row_matches_schema(row, &schema)
                .map_err(|source| MemorySourceError::RowMismatch { row_index, source })?;
        }
        let rows = Rows::try_new(raw_rows).map_err(MemorySourceError::Rows)?;
        Ok(Self { schema, rows })
    }

    /// Borrow the header.
    #[must_use]
    #[inline]
    pub const fn schema(&self) -> &Schema {
        &self.schema
    }

    /// Borrow the validated row collection.
    #[must_use]
    #[inline]
    pub const fn rows(&self) -> &Rows {
        &self.rows
    }
}

/// AST leaf introducing a relation.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum Source {
    /// Inline data whose rows match the carried schema (proved at
    /// `MemorySource::try_new`).
    Memory(MemorySource),
    /// Symbolic reference to a named external table.
    Table {
        /// Header of the external table.
        schema: Schema,
        /// Identifier the backend uses to look the table up.
        name: TableName,
    },
}

impl Source {
    /// Convenience constructor: build a `Source::Memory` from raw
    /// pieces by delegating to `MemorySource::try_new`.
    ///
    /// # Errors
    ///
    /// Returns the underlying `MemorySourceError`.
    pub fn try_memory(schema: Schema, raw_rows: Vec<Row>) -> Result<Self, MemorySourceError> {
        MemorySource::try_new(schema, raw_rows).map(Self::Memory)
    }

    /// Borrow the header that describes the rows this source
    /// produces. Cheap accessor used by `Op` smart constructors to
    /// drive schema-aware validation.
    #[must_use]
    #[inline]
    pub const fn schema(&self) -> &Schema {
        match self {
            Self::Memory(memory) => memory.schema(),
            Self::Table { schema, .. } => schema,
        }
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "explicit in test code"
)]
mod tests {
    use alloc::string::ToString;
    use alloc::vec;
    use alloc::vec::Vec;

    use whittle::primitive::CollectionError;

    use super::{MemorySource, MemorySourceError, Rows, Source};
    use crate::identifier::{AttributeName, TableName};
    use crate::infer::ValueTypeError;
    use crate::limits::MAX_ROWS_IN_AST;
    use crate::row::Row;
    use crate::schema::{Attribute, Schema};
    use crate::ty::{Type, Value};

    fn schema() -> Schema {
        Schema::try_new(vec![Attribute {
            name: AttributeName::try_new("id".to_string()).unwrap(),
            ty: Type::Int64,
        }])
        .unwrap()
    }

    #[test]
    fn empty_rows_admissible() {
        let rows = Rows::try_new(Vec::new()).unwrap();
        assert!(rows.is_empty());
    }

    #[test]
    fn single_row_admissible() {
        let row = Row::try_new(vec![Value::Int64(1)]).unwrap();
        let rows = Rows::try_new(vec![row]).unwrap();
        assert_eq!(rows.len(), 1);
    }

    #[test]
    fn overlength_rows_rejected() {
        let row = Row::try_new(vec![Value::Int64(1)]).unwrap();
        let too_many: Vec<Row> = (0..=MAX_ROWS_IN_AST).map(|_| row.clone()).collect();
        let result = Rows::try_new(too_many);
        assert!(matches!(
            result.unwrap_err(),
            CollectionError::LenOutOfRange { .. },
        ));
    }

    #[test]
    fn memory_source_admits_matching_rows() {
        let row = Row::try_new(vec![Value::Int64(1)]).unwrap();
        let src = Source::try_memory(schema(), vec![row]).unwrap();
        let Source::Memory(memory) = &src else {
            unreachable!();
        };
        assert_eq!(memory.rows().len(), 1);
    }

    #[test]
    fn memory_source_rejects_row_with_wrong_width() {
        // schema() has one Int64 attr. A two-value row mismatches.
        let bad_row = Row::try_new(vec![Value::Int64(1), Value::Int64(2)]).unwrap();
        let result = Source::try_memory(schema(), vec![bad_row]);
        let Err(MemorySourceError::RowMismatch {
            row_index: 0,
            source,
        }) = result
        else {
            unreachable!();
        };
        assert!(matches!(
            source,
            ValueTypeError::RowWidth {
                expected: 1,
                actual: 2
            },
        ));
    }

    #[test]
    fn memory_source_rejects_row_with_wrong_value_type() {
        // schema() has one Int64 attr; we hand in a Bool.
        let bad_row = Row::try_new(vec![Value::Bool(true)]).unwrap();
        let result = Source::try_memory(schema(), vec![bad_row]);
        let Err(MemorySourceError::RowMismatch {
            row_index: 0,
            source: ValueTypeError::RelationField { position: 0, .. },
        }) = result
        else {
            unreachable!();
        };
    }

    #[test]
    fn memory_source_construct_via_typed_constructor() {
        // Round-trip via MemorySource::try_new directly.
        let row = Row::try_new(vec![Value::Int64(1)]).unwrap();
        let memory = MemorySource::try_new(schema(), vec![row]).unwrap();
        assert_eq!(memory.schema().cardinality(), 1);
        assert_eq!(memory.rows().len(), 1);
    }

    #[test]
    fn memory_source_rejects_nested_array_element_type_mismatch() {
        use alloc::boxed::Box;
        use whittle::Refined;
        // Schema declares position 0 is Array<Int32>; supply a row
        // whose array element is a String. The walk should descend
        // into the array and report a RelationField -> ArrayElement
        // -> Mismatch path.
        let nested_schema = Schema::try_new(vec![Attribute {
            name: AttributeName::try_new("tags".to_string()).unwrap(),
            ty: Type::Array(Box::new(Type::Int32)),
        }])
        .unwrap();
        let bad_value =
            Value::Array(Refined::try_new(vec![Value::String("oops".to_string())]).unwrap());
        let bad_row = Row::try_new(vec![bad_value]).unwrap();
        let result = Source::try_memory(nested_schema, vec![bad_row]);
        let Err(MemorySourceError::RowMismatch {
            row_index: 0,
            source:
                ValueTypeError::RelationField {
                    position: 0,
                    source: nested,
                },
        }) = result
        else {
            unreachable!();
        };
        let ValueTypeError::ArrayElement {
            index: 0,
            source: leaf,
        } = *nested
        else {
            unreachable!();
        };
        assert!(matches!(*leaf, ValueTypeError::Mismatch));
    }

    #[test]
    fn table_source_carries_schema_and_name() {
        let src = Source::Table {
            schema: schema(),
            name: TableName::try_new("orders".to_string()).unwrap(),
        };
        let Source::Table { name, .. } = src else {
            unreachable!();
        };
        assert_eq!(name.as_str(), "orders");
    }
}
