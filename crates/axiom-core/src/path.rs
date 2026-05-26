//! Optic-like paths for nested-data navigation.
//!
//! A `Path<K>` is a bounded sequence of `PathStep`s. `K` is a
//! sealed `Kind` marker: `Lens` (no `Each` step) or `Traversal` (at
//! least one `Each` step). `AnyPath` is the discriminated sum used
//! in operator AST positions where either kind is admissible.
//!
//! The path-step vector length is bounded by `MAX_PATH_STEPS` via
//! whittle's `LenItems`; the lens-vs-traversal partition is enforced
//! by `try_new_*` constructors that scan for the `Each` step.

use alloc::vec::Vec;
use core::marker::PhantomData;

use thiserror::Error;
use whittle::primitive::{CollectionError, LenItems};
use whittle::Refined;

use crate::identifier::AttributeName;
use crate::limit::BoundedIndex;
use crate::limits::MAX_PATH_STEPS;

mod sealed {
    /// Sealed trait so external crates cannot introduce a third
    /// optic kind. Implemented only for `Lens` and `Traversal`
    /// below.
    pub trait Kind {}
}

/// Public re-export of the sealed `Kind` trait. Cannot be
/// implemented downstream.
pub trait Kind: sealed::Kind {}

/// Lens kind: a path containing zero `Each` steps; focuses a single
/// position.
pub enum Lens {}
impl sealed::Kind for Lens {}
impl Kind for Lens {}

/// Traversal kind: a path containing one or more `Each` steps;
/// focuses every element of the addressed collection.
pub enum Traversal {}
impl sealed::Kind for Traversal {}
impl Kind for Traversal {}

/// A single step in a path.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum PathStep {
    /// `.field` — named attribute access.
    Field(AttributeName),
    /// `[i]` — positional index into an ordered collection.
    Index(BoundedIndex),
    /// `[*]` — promotes the path to a traversal over every element.
    Each,
}

/// Kinded path: a bounded vector of `PathStep`s tagged with its
/// optic kind. The `K` parameter is one of `Lens` or `Traversal`;
/// no third option exists.
pub struct Path<K: Kind> {
    steps: Refined<Vec<PathStep>, LenItems<0, { MAX_PATH_STEPS }>>,
    _kind: PhantomData<fn() -> K>,
}

// Hand-written pass-through impls: `K` is a ZST marker that has no
// trait impls of its own, so `#[derive]` would synthesise impossible
// `where K: Debug + Clone + …` bounds.

impl<K: Kind> core::fmt::Debug for Path<K> {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("Path")
            .field("steps", self.steps.as_inner())
            .finish()
    }
}

impl<K: Kind> Clone for Path<K> {
    fn clone(&self) -> Self {
        Self {
            steps: self.steps.clone(),
            _kind: PhantomData,
        }
    }
}

impl<K: Kind> PartialEq for Path<K> {
    fn eq(&self, other: &Self) -> bool {
        self.steps == other.steps
    }
}

impl<K: Kind> Eq for Path<K> {}

/// Path that focuses a single position (no `Each` step).
pub type LensPath = Path<Lens>;

/// Path that focuses every element of a collection (at least one
/// `Each` step).
pub type TraversalPath = Path<Traversal>;

/// AST-side path carrier.
///
/// Discriminated-sum used by operator variants (`Modify`, `Unnest`)
/// that accept either kind. Variants statically distinguish which
/// kind was supplied so the optimizer can match without runtime
/// introspection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnyPath {
    /// A `LensPath`.
    Lens(LensPath),
    /// A `TraversalPath`.
    Traversal(TraversalPath),
}

/// Errors raised by path construction.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum PathError {
    /// Step-count violated `LenItems<0, MAX_PATH_STEPS>`.
    #[error("{0}")]
    Length(#[source] CollectionError),
    /// `LensPath::try_new` saw an `Each` step.
    #[error("lens path contains an Each step at index {index}")]
    LensWithEach {
        /// Position of the first `Each` step.
        index: usize,
    },
    /// `TraversalPath::try_new` saw a path with no `Each` step.
    #[error("traversal path contains no Each step")]
    TraversalWithoutEach,
}

impl From<CollectionError> for PathError {
    fn from(err: CollectionError) -> Self {
        Self::Length(err)
    }
}

impl LensPath {
    /// Validate `steps` as a lens path: no `Each` step admitted.
    ///
    /// # Errors
    ///
    /// Returns `PathError::Length` if `steps` exceeds
    /// `MAX_PATH_STEPS`. Returns `PathError::LensWithEach` with the
    /// offending index if any step is `PathStep::Each`.
    #[inline]
    pub fn try_new(steps: Vec<PathStep>) -> Result<Self, PathError> {
        let refined = Refined::try_new(steps)?;
        for (index, step) in refined.as_inner().iter().enumerate() {
            if matches!(step, PathStep::Each) {
                return Err(PathError::LensWithEach { index });
            }
        }
        Ok(Self { steps: refined, _kind: PhantomData })
    }
}

impl TraversalPath {
    /// Validate `steps` as a traversal path: at least one `Each`
    /// step required.
    ///
    /// # Errors
    ///
    /// Returns `PathError::Length` if `steps` exceeds
    /// `MAX_PATH_STEPS`. Returns `PathError::TraversalWithoutEach`
    /// if no step is `PathStep::Each`.
    #[inline]
    pub fn try_new(steps: Vec<PathStep>) -> Result<Self, PathError> {
        let refined = Refined::try_new(steps)?;
        let has_each = refined
            .as_inner()
            .iter()
            .any(|step| matches!(step, PathStep::Each));
        if !has_each {
            return Err(PathError::TraversalWithoutEach);
        }
        Ok(Self { steps: refined, _kind: PhantomData })
    }
}

impl<K: Kind> Path<K> {
    /// Borrow the underlying step sequence.
    #[must_use]
    #[inline]
    pub const fn steps(&self) -> &[PathStep] {
        self.steps.as_inner().as_slice()
    }

    /// Number of steps in the path.
    #[must_use]
    #[inline]
    pub const fn len(&self) -> usize {
        self.steps.as_inner().len()
    }

    /// `true` if the path has zero steps (the identity path).
    #[must_use]
    #[inline]
    pub const fn is_empty(&self) -> bool {
        self.steps.as_inner().is_empty()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used,
        reason = "explicit in test code")]
mod tests {
    use alloc::string::ToString;
    use alloc::vec;
    use alloc::vec::Vec;

    use super::{
        AnyPath, LensPath, PathError, PathStep, TraversalPath,
    };
    use crate::identifier::AttributeName;
    use crate::limit::BoundedIndex;
    use crate::limits::MAX_PATH_STEPS;

    fn attr(name: &str) -> AttributeName {
        AttributeName::try_new(name.to_string()).unwrap()
    }

    #[test]
    fn lens_accepts_field_only() {
        let p = LensPath::try_new(vec![
            PathStep::Field(attr("address")),
            PathStep::Field(attr("city")),
        ])
        .unwrap();
        assert_eq!(p.len(), 2);
    }

    #[test]
    fn lens_admits_index() {
        let p = LensPath::try_new(vec![
            PathStep::Field(attr("rows")),
            PathStep::Index(BoundedIndex::try_new(3).unwrap()),
        ])
        .unwrap();
        assert_eq!(p.len(), 2);
    }

    #[test]
    fn lens_rejects_each_step() {
        let result = LensPath::try_new(vec![
            PathStep::Field(attr("posts")),
            PathStep::Each,
        ]);
        assert!(matches!(
            result.unwrap_err(),
            PathError::LensWithEach { index: 1 },
        ));
    }

    #[test]
    fn traversal_requires_each_step() {
        let p = TraversalPath::try_new(vec![
            PathStep::Field(attr("posts")),
            PathStep::Each,
        ])
        .unwrap();
        assert_eq!(p.len(), 2);
    }

    #[test]
    fn traversal_rejects_lens_only() {
        let result = TraversalPath::try_new(vec![
            PathStep::Field(attr("posts")),
        ]);
        assert_eq!(result.unwrap_err(), PathError::TraversalWithoutEach);
    }

    #[test]
    fn path_step_count_bounded() {
        let too_many: Vec<PathStep> = (0..=MAX_PATH_STEPS)
            .map(|_| PathStep::Each)
            .collect();
        let result = TraversalPath::try_new(too_many);
        assert!(matches!(result.unwrap_err(), PathError::Length(_)));
    }

    #[test]
    fn lens_identity_path_admissible() {
        let p = LensPath::try_new(Vec::new()).unwrap();
        assert!(p.is_empty());
    }

    #[test]
    fn anypath_carries_kind() {
        let lens = LensPath::try_new(vec![PathStep::Field(attr("a"))]).unwrap();
        let trav = TraversalPath::try_new(vec![
            PathStep::Field(attr("a")),
            PathStep::Each,
        ])
        .unwrap();
        let AnyPath::Lens(_) = AnyPath::Lens(lens) else {
            unreachable!("expected Lens variant");
        };
        let AnyPath::Traversal(_) = AnyPath::Traversal(trav) else {
            unreachable!("expected Traversal variant");
        };
    }
}
