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
    EachChar, FirstChar, IdentChar, IdentStart, LenChars, StringError,
};
use whittle::{And, AndError, Refined};

use crate::limits::{
    MAX_ATTRIBUTE_NAME_LEN, MAX_PATTERN_LEN, MAX_TABLE_NAME_LEN,
};

// ─── Internal rule aliases. ──────────────────────────────────────

// Identifier grammar: leading char alpha/underscore, body
// alnum/underscore. The length bound is composed at the outer
// level so the per-name-uniqueness checks downstream see a
// length-bounded string before walking characters.
type AttributeNameRule = And<
    LenChars<1, { MAX_ATTRIBUTE_NAME_LEN }>,
    And<EachChar<IdentChar>, FirstChar<IdentStart>>,
>;

// LenChars<1, MAX>'s lower bound already excludes the empty
// string, so a separate NonEmpty rule would double-encode the
// empty-state. Keep the single bounded rule.
type TableNameRule = LenChars<1, { MAX_TABLE_NAME_LEN }>;

type PatternRule = LenChars<1, { MAX_PATTERN_LEN }>;

// ─── Public newtypes. Inner field crate-private so the only
//      construction path is the named `try_new` below. ────────────

/// Attribute name in a relation schema: 1..=`MAX_ATTRIBUTE_NAME_LEN`
/// characters; first character ASCII-alphabetic or underscore;
/// remaining characters ASCII-alphanumeric or underscore.
#[derive(Debug, Clone, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct AttributeName(Refined<String, AttributeNameRule>);

impl fmt::Display for AttributeName {
    #[inline]
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Constructor error for `AttributeName`.
///
/// The error layers reflect the rule composition
/// `And<LenChars, And<EachChar, FirstChar>>`:
/// - `Left(_)` — length bound failed.
/// - `Right(Left(_))` — a character somewhere in the body failed
///   `IdentChar`.
/// - `Right(Right(_))` — the first character failed `IdentStart`
///   (e.g. a leading digit).
pub type AttributeNameError =
    AndError<StringError, AndError<StringError, StringError>>;

impl AttributeName {
    /// Validate `raw` against the attribute-name rule and wrap.
    ///
    /// # Errors
    ///
    /// Returns the variants documented on `AttributeNameError`.
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

/// Table name (for SQL backend references):
/// 1..=`MAX_TABLE_NAME_LEN` characters.
#[derive(Debug, Clone, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct TableName(Refined<String, TableNameRule>);

/// Constructor error for `TableName`.
pub type TableNameError = StringError;

impl TableName {
    /// Validate `raw` against the table-name rule and wrap.
    ///
    /// # Errors
    ///
    /// Returns `StringError::CharCountOutOfRange` if `raw` is empty
    /// or exceeds the length cap.
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

/// `LIKE`-style pattern string. Bounded length. The pattern's
/// syntax is not validated here — the backend that consumes it
/// performs that check.
#[derive(Debug, Clone, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct Pattern(Refined<String, PatternRule>);

/// Constructor error for `Pattern`.
pub type PatternError = StringError;

impl Pattern {
    /// Validate `raw` against the pattern rule and wrap.
    ///
    /// # Errors
    ///
    /// Returns `StringError::CharCountOutOfRange` if `raw` is empty
    /// or exceeds the length cap.
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
        // EachChar<IdentChar> rejects on the dash at byte offset 3.
        assert!(matches!(
            result.unwrap_err(),
            AndError::Right(AndError::Left(
                StringError::BadChar { offset: 3 },
            )),
        ));
    }

    #[test]
    fn attribute_name_rejects_leading_digit() {
        let result = AttributeName::try_new("1abc".to_string());
        // FirstChar<IdentStart> rejects the leading digit at offset 0.
        assert!(matches!(
            result.unwrap_err(),
            AndError::Right(AndError::Right(
                StringError::BadChar { offset: 0 },
            )),
        ));
    }

    #[test]
    fn attribute_name_admits_leading_underscore() {
        let n = AttributeName::try_new("_internal".to_string()).unwrap();
        assert_eq!(n.as_str(), "_internal");
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
        assert_eq!(
            result.unwrap_err(),
            StringError::CharCountOutOfRange { actual: 0 },
        );
    }

    #[test]
    fn table_name_rejects_overlength() {
        let raw = "x".repeat(MAX_TABLE_NAME_LEN + 1);
        let result = TableName::try_new(raw);
        assert!(matches!(
            result.unwrap_err(),
            StringError::CharCountOutOfRange { .. },
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
        assert_eq!(
            result.unwrap_err(),
            StringError::CharCountOutOfRange { actual: 0 },
        );
    }

    #[test]
    fn pattern_rejects_overlength() {
        let raw = "x".repeat(MAX_PATTERN_LEN + 1);
        let result = Pattern::try_new(raw);
        assert!(matches!(
            result.unwrap_err(),
            StringError::CharCountOutOfRange { .. },
        ));
    }
}
