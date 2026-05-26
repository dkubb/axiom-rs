//! Schema-cardinality refinement — the first axiom-rs dogfood of
//! Whittle.
//!
//! The `Schema` type itself lands in a later commit; what's here is
//! the constrained representation of an attribute count, which every
//! operator's header rule will key off of. The point of building it
//! first is to validate that Whittle's `Refined<T, R>` carries
//! cleanly across a real consumer's crate boundary.

use whittle::primitive::{NumericError, Within};
use whittle::Refined;

use crate::limits::MAX_SCHEMA_ATTRIBUTES;

/// Number of attributes in a `Schema`: at least one (the empty
/// schema is not admissible), at most `MAX_SCHEMA_ATTRIBUTES`.
pub type SchemaCardinality =
    Refined<usize, Within<1, { MAX_SCHEMA_ATTRIBUTES as i128 }>>;

/// Construction error for `SchemaCardinality`. Re-exports Whittle's
/// `NumericError` under the axiom-rs vocabulary so callers do not
/// need to name the rule type at every error-handling site.
pub type SchemaCardinalityError = NumericError;

/// Placeholder so `Schema` has somewhere to live until its full
/// shape lands; the `pub` re-export in `lib.rs` references this.
pub struct Schema {
    /// Attribute count, refined to lie in `1..=MAX_SCHEMA_ATTRIBUTES`.
    pub cardinality: SchemaCardinality,
    // Header, attribute types, etc. land in a later commit.
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used,
        reason = "explicit in test code")]
mod tests {
    use super::{SchemaCardinality, SchemaCardinalityError, MAX_SCHEMA_ATTRIBUTES};

    #[test]
    fn empty_schema_rejected() {
        let result = SchemaCardinality::try_new(0_usize);
        assert!(matches!(
            result.unwrap_err(),
            SchemaCardinalityError::OutOfRange { value: 0 },
        ));
    }

    #[test]
    fn single_attribute_accepted() {
        let count = SchemaCardinality::try_new(1_usize).unwrap();
        assert_eq!(*count.as_inner(), 1_usize);
    }

    #[test]
    fn max_attributes_accepted_inclusive() {
        let count = SchemaCardinality::try_new(MAX_SCHEMA_ATTRIBUTES).unwrap();
        assert_eq!(*count.as_inner(), MAX_SCHEMA_ATTRIBUTES);
    }

    #[test]
    fn over_max_rejected() {
        let result = SchemaCardinality::try_new(MAX_SCHEMA_ATTRIBUTES + 1);
        assert!(matches!(
            result.unwrap_err(),
            SchemaCardinalityError::OutOfRange { .. },
        ));
    }

    proptest::proptest! {
        #[test]
        fn admissible_inputs_round_trip(
            count in 1_usize..=MAX_SCHEMA_ATTRIBUTES
        ) {
            let r = SchemaCardinality::try_new(count).unwrap();
            proptest::prop_assert_eq!(*r.as_inner(), count);
        }
    }
}
