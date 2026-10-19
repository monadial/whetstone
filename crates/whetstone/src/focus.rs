//! Where a learner is in a list: which control has focus, or which row is
//! selected. Positions are indexes, never screen rectangles, so a resize
//! cannot move them.

/// A position in a list of items.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Position(usize);

impl Position {
    /// The index of the item at this position.
    pub(crate) fn index(self) -> usize {
        self.0
    }

    /// The next of `len` items, stopping at the last.
    #[must_use]
    pub(crate) fn down(self, len: usize) -> Self {
        Self((self.0 + 1).min(len.saturating_sub(1)))
    }

    /// The previous item, stopping at the first.
    #[must_use]
    pub(crate) fn up(self) -> Self {
        Self(self.0.saturating_sub(1))
    }

    /// The next of `len` items, wrapping from the last to the first.
    #[must_use]
    pub(crate) fn next(self, len: usize) -> Self {
        Self(match self.0 + 1 {
            next if next < len => next,
            _ => 0,
        })
    }

    /// The previous of `len` items, wrapping from the first to the last.
    #[must_use]
    pub(crate) fn previous(self, len: usize) -> Self {
        Self(self.0.checked_sub(1).unwrap_or(len.saturating_sub(1)))
    }
}
