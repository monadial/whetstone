//! The theme layer. Screens take every style from a [`Theme`] and never name
//! a colour themselves, so the palette can be swapped in one place.

use ratatui::style::{Modifier, Style};

/// The styles every screen draws with.
#[derive(Debug, Clone)]
pub struct Theme {
    pub(crate) title: Style,
    pub(crate) text: Style,
    pub(crate) muted: Style,
}

impl Theme {
    /// The plain default palette: the terminal's own colours, with meaning
    /// carried by text and attributes alone.
    #[must_use]
    pub fn plain() -> Self {
        Self {
            title: Style::new().add_modifier(Modifier::BOLD),
            text: Style::new(),
            muted: Style::new(),
        }
    }
}
