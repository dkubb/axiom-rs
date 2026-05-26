//! Identifier and string-typed domain types.
//!
//! Every refined type here goes through `whittle::Refined` so the
//! invariants are enforced exactly once, at the boundary. The
//! `pub type` shorthand expresses the rule composition; the actual
//! public surface (e.g. `AttributeName`) is a transparent newtype
//! defined below so the rule's vocabulary does not leak into call
//! sites (per docs/ARCHITECTURE.md §9 of axiom-rs and the named-
//! domain-type discipline of whittle's IDEA §4).

use alloc::string::String;
use core::fmt;

use whittle::primitive::{
    EachChar, IdentChar, LenChars, NonEmpty, StringError,
};
use whittle::{And, AndError, Refined};

use crate::limits::{
    MAX_ATTRIBUTE_NAME_LEN, MAX_PATTERN_LEN, MAX_TABLE_NAME_LEN,
};

// ─── Internal rule aliases. ──────────────────────────────────────

type AttributeNameRule =
    And<LenChars<1, { MAX_ATTRIBUTE_NAME_LEN }>, EachChar<IdentChar>>;

type TableNameRule = And<NonEmpty, LenChars<1, { MAX_TABLE_NAME_LEN }>>;

type PatternRule = And<NonEmpty, LenChars<1, { MAX_PATTERN_LEN }>>;

// ─── Public newtypes. Inner field crate-private so the only
//      construction path is the named `try_new` below. ────────────

/// Attribute name in a relation schema: 1..=`MAX_ATTRIBUTE_NAME_LEN`
/// characters, each character ASCII-alphanumeric or underscore.
///
/// First-character-cannot-be-digit is NOT enforced yet — it lands
/// when whittle gains a `FirstChar<P>` primitive or when the
/// `refinement!` macro can express a head/tail split.
#[derive(Debug, Clone, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct AttributeName(Refined<String, AttributeNameRule>);

impl fmt::Display for AttributeName {
    #[inline]
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Constructor error for `AttributeName`.
pub type AttributeNameError = AndError<StringError, StringError>;

impl AttributeName {
    /// Validate `raw` against the attribute-name rule and wrap.
    ///
    /// # Errors
    ///
    /// Returns `AttributeNameError::Left` when the length is out of
    /// range, `AttributeNameError::Right` when a character is
    /// inadmissible (carrying the byte offset of the first
    /// violation).
    #[inline]
    pub fn try_new(raw: String) -> Result<Self, AttributeNameError> {
        Refined::try_new(raw).map(Self)
    }

    /// Borrow the inner string.
    #[must_use]
    #[inline]
    pub const fn as_str(&self) -> &str {
        self.0.as_inner().as_str()
    }
}

/// Table name (for SQL backend references): non-empty,
/// 1..=`MAX_TABLE_NAME_LEN` characters.
#[derive(Debug, Clone, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct TableName(Refined<String, TableNameRule>);

/// Constructor error for `TableName`.
pub type TableNameError = AndError<StringError, StringError>;

impl TableName {
    /// Validate `raw` against the table-name rule and wrap.
    ///
    /// # Errors
    ///
    /// Returns `TableNameError::Left` if `raw` is empty,
    /// `TableNameError::Right` if it exceeds the length cap.
    #[inline]
    pub fn try_new(raw: String) -> Result<Self, TableNameError> {
        Refined::try_new(raw).map(Self)
    }

    /// Borrow the inner string.
    #[must_use]
    #[inline]
    pub const fn as_str(&self) -> &str {
        self.0.as_inner().as_str()
    }
}

/// `LIKE`-style pattern string. Non-empty, bounded length. The
/// pattern's syntax is not validated here — the backend that
/// consumes it performs that check.
#[derive(Debug, Clone, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct Pattern(Refined<String, PatternRule>);

/// Constructor error for `Pattern`.
pub type PatternError = AndError<StringError, StringError>;

impl Pattern {
    /// Validate `raw` against the pattern rule and wrap.
    ///
    /// # Errors
    ///
    /// Returns `PatternError::Left` if `raw` is empty,
    /// `PatternError::Right` if it exceeds the length cap.
    #[inline]
    pub fn try_new(raw: String) -> Result<Self, PatternError> {
        Refined::try_new(raw).map(Self)
    }

    /// Borrow the inner string.
    #[must_use]
    #[inline]
    pub const fn as_str(&self) -> &str {
        self.0.as_inner().as_str()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used,
        reason = "explicit in test code")]
mod tests {
    use alloc::string::{String, ToString};

    use whittle::primitive::StringError;
    use whittle::AndError;

    use super::{AttributeName, Pattern, TableName};
    use crate::limits::{
        MAX_ATTRIBUTE_NAME_LEN, MAX_PATTERN_LEN, MAX_TABLE_NAME_LEN,
    };

    // ─── AttributeName. ──────────────────────────────────────────

    #[test]
    fn attribute_name_accepts_identifier_body() {
        let n = AttributeName::try_new("user_id".to_string()).unwrap();
        assert_eq!(n.as_str(), "user_id");
    }

    #[test]
    fn attribute_name_rejects_empty() {
        let result = AttributeName::try_new(String::new());
        assert!(matches!(
            result.unwrap_err(),
            AndError::Left(StringError::CharCountOutOfRange { actual: 0 }),
        ));
    }

    #[test]
    fn attribute_name_rejects_overlength() {
        let raw = "a".repeat(MAX_ATTRIBUTE_NAME_LEN + 1);
        let result = AttributeName::try_new(raw);
        assert!(matches!(
            result.unwrap_err(),
            AndError::Left(StringError::CharCountOutOfRange { .. }),
        ));
    }

    #[test]
    fn attribute_name_rejects_bad_character() {
        let result = AttributeName::try_new("has-dash".to_string());
        assert!(matches!(
            result.unwrap_err(),
            AndError::Right(StringError::BadChar { offset: 3 }),
        ));
    }

    #[test]
    fn attribute_name_accepts_max_length_inclusive() {
        let raw = "x".repeat(MAX_ATTRIBUTE_NAME_LEN);
        let n = AttributeName::try_new(raw).unwrap();
        assert_eq!(n.as_str().len(), MAX_ATTRIBUTE_NAME_LEN);
    }

    // ─── TableName. ──────────────────────────────────────────────

    #[test]
    fn table_name_accepts_non_empty() {
        let t = TableName::try_new("orders".to_string()).unwrap();
        assert_eq!(t.as_str(), "orders");
    }

    #[test]
    fn table_name_rejects_empty() {
        let result = TableName::try_new(String::new());
        assert!(matches!(
            result.unwrap_err(),
            AndError::Left(StringError::Empty),
        ));
    }

    #[test]
    fn table_name_rejects_overlength() {
        let raw = "x".repeat(MAX_TABLE_NAME_LEN + 1);
        let result = TableName::try_new(raw);
        assert!(matches!(
            result.unwrap_err(),
            AndError::Right(StringError::CharCountOutOfRange { .. }),
        ));
    }

    // ─── Pattern. ────────────────────────────────────────────────

    #[test]
    fn pattern_accepts_non_empty() {
        let p = Pattern::try_new("user_%".to_string()).unwrap();
        assert_eq!(p.as_str(), "user_%");
    }

    #[test]
    fn pattern_rejects_empty() {
        let result = Pattern::try_new(String::new());
        assert!(matches!(
            result.unwrap_err(),
            AndError::Left(StringError::Empty),
        ));
    }

    #[test]
    fn pattern_rejects_overlength() {
        let raw = "x".repeat(MAX_PATTERN_LEN + 1);
        let result = Pattern::try_new(raw);
        assert!(matches!(
            result.unwrap_err(),
            AndError::Right(StringError::CharCountOutOfRange { .. }),
        ));
    }
}
