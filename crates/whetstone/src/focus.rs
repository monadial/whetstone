//! Where a learner is in a list: which control has focus, or which row is
//! selected. Positions are indexes, never screen rectangles, so a resize
//! cannot move them.

use crate::keymap::Command;

/// A position in a list of items.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Position(usize);

impl Position {
    /// The position of the item at `index`.
    pub(crate) const fn at(index: usize) -> Self {
        Self(index)
    }

    /// Where `command` moves this position in a list of `len` items: `j`/`k`
    /// stop at either end, and Tab and Shift+Tab wrap around. Any other
    /// command leaves it where it is.
    #[must_use]
    pub(crate) fn after(self, command: Command, len: usize) -> Self {
        match command {
            Command::Down => self.down(len),
            Command::Up => self.up(),
            Command::NextControl => self.next(len),
            Command::PreviousControl => self.previous(len),
            _ => self,
        }
    }

    /// Where `command` scrolls this position when it can go from the first
    /// item to `last` and `rows` items show at once: `j`/`k` move it by
    /// one, `Ctrl-d`/`Ctrl-u` by half of `rows`, Page Down and Page Up by
    /// `rows`, and `gg`/`G` to either end.
    /// It stops at either end, and starts from no further than `last`, which
    /// shrinks when a window grows, so the first `k` always moves what shows.
    /// Any other command leaves it where it is, even past `last`, so a window
    /// resized back shows what it showed before.
    #[must_use]
    pub(crate) fn scrolled(self, command: Command, last: usize, rows: usize) -> Self {
        let at = self.0.min(last);
        let half_page = (rows / 2).max(1);
        let page = rows.max(1);
        match command {
            Command::Down => Self((at + 1).min(last)),
            Command::Up => Self(at.saturating_sub(1)),
            Command::HalfPageDown => Self((at + half_page).min(last)),
            Command::HalfPageUp => Self(at.saturating_sub(half_page)),
            Command::PageDown => Self((at + page).min(last)),
            Command::PageUp => Self(at.saturating_sub(page)),
            Command::Top => Self(0),
            Command::Bottom => Self(last),
            _ => self,
        }
    }

    /// The index of the item at this position.
    pub(crate) fn index(self) -> usize {
        self.0
    }

    /// The next of `len` items, stopping at the last.
    #[must_use]
    fn down(self, len: usize) -> Self {
        Self((self.0 + 1).min(len.saturating_sub(1)))
    }

    /// The previous item, stopping at the first.
    #[must_use]
    fn up(self) -> Self {
        Self(self.0.saturating_sub(1))
    }

    /// The next of `len` items, wrapping from the last to the first.
    #[must_use]
    fn next(self, len: usize) -> Self {
        Self(match self.0 + 1 {
            next if next < len => next,
            _ => 0,
        })
    }

    /// The previous of `len` items, wrapping from the first to the last.
    #[must_use]
    fn previous(self, len: usize) -> Self {
        Self(self.0.checked_sub(1).unwrap_or(len.saturating_sub(1)))
    }
}
