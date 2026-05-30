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
use whittle::{And, Refined};

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

// ─── Public newtypes. Hand-written tuple structs wrapping a
//      `Refined` so each `try_new` can map the rule's flat
//      `StringError` onto a domain-shaped enum. `as_str` and
//      `Display` are part of the surface this crate offers beyond
//      whittle's minimal core. ─────────────────────────────────────

/// Attribute name in a relation schema:
/// 1..=`MAX_ATTRIBUTE_NAME_LEN` characters; first character
/// ASCII-alphabetic or underscore; remaining characters
/// ASCII-alphanumeric or underscore.
#[derive(Debug, Clone, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct AttributeName(Refined<String, AttributeNameRule>);

/// Constructor error for `AttributeName`.
///
/// Flat domain-shaped enum. The underlying composition is
/// `And<LenChars, And<EachChar, FirstChar>>` and produces
/// `StringError` directly (both inner rules share that error type).
/// The three branches map to three flat variants so call sites do
/// not see the rule shape at all.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[non_exhaustive]
pub enum AttributeNameError {
    /// Length (in characters) fell outside
    /// `1..=MAX_ATTRIBUTE_NAME_LEN`.
    #[error("attribute-name length out of range (actual: {actual})")]
    Length {
        /// Observed character count.
        actual: usize,
    },

    /// The first character was not an admissible identifier start
    /// (ASCII alphabetic or underscore).
    #[error("attribute-name first character not admissible")]
    FirstChar,

    /// A character in the body was not admissible (ASCII
    /// alphanumeric or underscore).
    #[error("attribute-name body character not admissible (at byte offset {offset})")]
    BodyChar {
        /// UTF-8 byte offset of the first rejected character.
        offset: usize,
    },
}

impl AttributeName {
    /// Validate `raw` against the attribute-name rule and wrap.
    ///
    /// # Errors
    ///
    /// Returns `Length`, `FirstChar`, or `BodyChar` reflecting the
    /// underlying rule branch that rejected the input.
    #[inline]
    pub fn try_new(raw: String) -> Result<Self, AttributeNameError> {
        Refined::try_new(raw).map(Self).map_err(|err| match err {
            StringError::CharCountOutOfRange { actual } => {
                AttributeNameError::Length { actual }
            }
            StringError::BadChar { offset } => {
                AttributeNameError::BodyChar { offset }
            }
            StringError::BadFirstChar => AttributeNameError::FirstChar,
            _ => unreachable!(
                "AttributeNameRule emits only CharCountOutOfRange / BadChar / BadFirstChar"
            ),
        })
    }

    /// Borrow the inner string.
    #[must_use]
    #[inline]
    pub const fn as_str(&self) -> &str {
        self.0.as_inner().as_str()
    }

    /// Consume the wrapper and return the inner string.
    #[must_use]
    #[inline]
    pub fn into_inner(self) -> String {
        self.0.into_inner()
    }
}

impl fmt::Display for AttributeName {
    #[inline]
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Table name (for SQL backend references):
/// 1..=`MAX_TABLE_NAME_LEN` characters.
#[derive(Debug, Clone, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct TableName(Refined<String, TableNameRule>);

/// Constructor error for `TableName`.
///
/// The underlying rule is a single `LenChars`, so the only failure
/// mode is a length-bound violation. The flat variant gives call
/// sites a domain-named match target.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[non_exhaustive]
pub enum TableNameError {
    /// Length (in characters) fell outside `1..=MAX_TABLE_NAME_LEN`.
    #[error("table-name length out of range (actual: {actual})")]
    Length {
        /// Observed character count.
        actual: usize,
    },
}

impl TableName {
    /// Validate `raw` against the table-name rule and wrap.
    ///
    /// # Errors
    ///
    /// Returns `Length` if the character count is outside
    /// `1..=MAX_TABLE_NAME_LEN`.
    #[inline]
    pub fn try_new(raw: String) -> Result<Self, TableNameError> {
        Refined::try_new(raw).map(Self).map_err(|err| match err {
            StringError::CharCountOutOfRange { actual } => {
                TableNameError::Length { actual }
            }
            _ => unreachable!(
                "TableNameRule (LenChars) emits only CharCountOutOfRange"
            ),
        })
    }

    /// Borrow the inner string.
    #[must_use]
    #[inline]
    pub const fn as_str(&self) -> &str {
        self.0.as_inner().as_str()
    }

    /// Consume the wrapper and return the inner string.
    #[must_use]
    #[inline]
    pub fn into_inner(self) -> String {
        self.0.into_inner()
    }
}

/// `LIKE`-style pattern string. Bounded length. The pattern's
/// syntax is not validated here — the backend that consumes it
/// performs that check.
#[derive(Debug, Clone, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct Pattern(Refined<String, PatternRule>);

/// Constructor error for `Pattern`.
///
/// The underlying rule is a single `LenChars`, so the only failure
/// mode is a length-bound violation.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[non_exhaustive]
pub enum PatternError {
    /// Length (in characters) fell outside `1..=MAX_PATTERN_LEN`.
    #[error("pattern length out of range (actual: {actual})")]
    Length {
        /// Observed character count.
        actual: usize,
    },
}

impl Pattern {
    /// Validate `raw` against the pattern rule and wrap.
    ///
    /// # Errors
    ///
    /// Returns `Length` if the character count is outside
    /// `1..=MAX_PATTERN_LEN`.
    #[inline]
    pub fn try_new(raw: String) -> Result<Self, PatternError> {
        Refined::try_new(raw).map(Self).map_err(|err| match err {
            StringError::CharCountOutOfRange { actual } => {
                PatternError::Length { actual }
            }
            _ => unreachable!(
                "PatternRule (LenChars) emits only CharCountOutOfRange"
            ),
        })
    }

    /// Borrow the inner string.
    #[must_use]
    #[inline]
    pub const fn as_str(&self) -> &str {
        self.0.as_inner().as_str()
    }

    /// Consume the wrapper and return the inner string.
    #[must_use]
    #[inline]
    pub fn into_inner(self) -> String {
        self.0.into_inner()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used,
        reason = "explicit in test code")]
mod tests {
    use alloc::string::{String, ToString};

    use super::{
        AttributeName, AttributeNameError, Pattern, PatternError,
        TableName, TableNameError,
    };
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
        assert_eq!(
            result.unwrap_err(),
            AttributeNameError::Length { actual: 0 },
        );
    }

    #[test]
    fn attribute_name_rejects_overlength() {
        let raw = "a".repeat(MAX_ATTRIBUTE_NAME_LEN + 1);
        let result = AttributeName::try_new(raw);
        assert!(matches!(
            result.unwrap_err(),
            AttributeNameError::Length { .. },
        ));
    }

    #[test]
    fn attribute_name_rejects_bad_character() {
        let result = AttributeName::try_new("has-dash".to_string());
        // EachChar<IdentChar> rejects on the dash at byte offset 3.
        assert_eq!(
            result.unwrap_err(),
            AttributeNameError::BodyChar { offset: 3 },
        );
    }

    #[test]
    fn attribute_name_rejects_leading_digit() {
        let result = AttributeName::try_new("1abc".to_string());
        // FirstChar<IdentStart> rejects the leading digit; the rule
        // surfaces `StringError::BadFirstChar` (no offset — head
        // failure is a single position by construction).
        assert_eq!(
            result.unwrap_err(),
            AttributeNameError::FirstChar,
        );
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
            TableNameError::Length { actual: 0 },
        );
    }

    #[test]
    fn table_name_rejects_overlength() {
        let raw = "x".repeat(MAX_TABLE_NAME_LEN + 1);
        let result = TableName::try_new(raw);
        assert!(matches!(
            result.unwrap_err(),
            TableNameError::Length { .. },
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
            PatternError::Length { actual: 0 },
        );
    }

    #[test]
    fn pattern_rejects_overlength() {
        let raw = "x".repeat(MAX_PATTERN_LEN + 1);
        let result = Pattern::try_new(raw);
        assert!(matches!(
            result.unwrap_err(),
            PatternError::Length { .. },
        ));
    }
}
