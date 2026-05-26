//! `Source`: the AST leaf that introduces a relation.
//!
//! Two variants per docs/ARCHITECTURE.md §9.3:
//! - `Memory { schema, rows }` carries inline data validated against
//!   the supplied schema.
//! - `Table { schema, name }` is a symbolic reference for backends
//!   that consult an external store.

use alloc::vec::Vec;

use whittle::primitive::{CollectionError, LenItems};
use whittle::Refined;

use crate::identifier::TableName;
use crate::limits::MAX_ROWS_IN_AST;
use crate::row::Row;
use crate::schema::Schema;

// Schema-validated rows are simply a length-bounded `Vec<Row>` for
// now. Per-row value-type checking against the schema lands once
// the schema-aware Source::try_memory constructor is wired in; the
// structural bound is already enforced by whittle.
type SourceRowsRule = LenItems<0, { MAX_ROWS_IN_AST }>;

/// Length-bounded list of `Row`s, the inline data for a memory source.
#[derive(Debug, Clone, PartialEq)]
pub struct Rows(Refined<Vec<Row>, SourceRowsRule>);

/// Constructor error for `Rows`.
pub type RowsError = CollectionError;

impl Rows {
    /// Validate `raw` and wrap.
    ///
    /// # Errors
    ///
    /// Returns `CollectionError::LenOutOfRange` when `raw` exceeds
    /// `MAX_ROWS_IN_AST` rows. (Lower bound is `0` so the empty
    /// relation is admissible.)
    #[inline]
    pub fn try_new(raw: Vec<Row>) -> Result<Self, RowsError> {
        Refined::try_new(raw).map(Self)
    }

    /// Borrow the underlying row list.
    #[must_use]
    #[inline]
    pub const fn as_slice(&self) -> &[Row] {
        self.0.as_inner().as_slice()
    }

    /// Number of rows.
    #[must_use]
    #[inline]
    pub const fn len(&self) -> usize {
        self.0.as_inner().len()
    }

    /// `true` if the source has no rows.
    #[must_use]
    #[inline]
    pub const fn is_empty(&self) -> bool {
        self.0.as_inner().is_empty()
    }
}

/// AST leaf introducing a relation.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum Source {
    /// Inline data: schema plus a row collection.
    Memory {
        /// Header of the inline relation.
        schema: Schema,
        /// Rows of the inline relation.
        rows: Rows,
    },
    /// Symbolic reference to a named external table.
    Table {
        /// Header of the external table.
        schema: Schema,
        /// Identifier the backend uses to look the table up.
        name: TableName,
    },
}

impl Source {
    /// Borrow the header that describes the rows this source
    /// produces. Cheap accessor used by `Op` smart constructors to
    /// drive schema-aware validation.
    #[must_use]
    #[inline]
    pub const fn schema(&self) -> &Schema {
        match self {
            Self::Memory { schema, .. } | Self::Table { schema, .. } => {
                schema
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used,
        reason = "explicit in test code")]
mod tests {
    use alloc::string::ToString;
    use alloc::vec;
    use alloc::vec::Vec;

    use whittle::primitive::CollectionError;

    use super::{Rows, Source};
    use crate::identifier::{AttributeName, TableName};
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
        let too_many: Vec<Row> = (0..=MAX_ROWS_IN_AST)
            .map(|_| row.clone())
            .collect();
        let result = Rows::try_new(too_many);
        assert!(matches!(
            result.unwrap_err(),
            CollectionError::LenOutOfRange { .. },
        ));
    }

    #[test]
    fn memory_source_carries_schema_and_rows() {
        let row = Row::try_new(vec![Value::Int64(1)]).unwrap();
        let src = Source::Memory {
            schema: schema(),
            rows: Rows::try_new(vec![row]).unwrap(),
        };
        let Source::Memory { rows, .. } = src else {
            unreachable!();
        };
        assert_eq!(rows.len(), 1);
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
