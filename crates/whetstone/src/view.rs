//! What every screen provides. A new screen implements [`View`] in its own
//! module and gets a [`Route`], and the app, key help and the palette pick
//! it up from there.

use std::fmt;

use ratatui::Frame;
use ratatui::layout::Rect;
use whetstone_core::ShowcaseProject;

use crate::config::Settings;
use crate::home::Home;
use crate::keymap::{Command, Input, Keymap};
use crate::palette::Action;
use crate::picker::Picker;
use crate::settings::SettingsScreen;
use crate::theme::Theme;
use crate::viewer::Viewer;

/// A screen: its name, its keys and actions, how commands change it, and
/// how it is drawn.
pub(crate) trait View: fmt::Debug {
    /// Where the screen is, for going to it again.
    fn route(&self) -> Route;

    /// The screen's name, as key help and the palette title it.
    fn name(&self) -> &'static str;

    /// The screen's own keys, before the ones that work everywhere.
    fn keymap(&self) -> Keymap;

    /// What key presses are for: commands, or text while a text entry on
    /// the screen has focus.
    fn input(&self) -> Input {
        Input::Commands
    }

    /// The screen's own actions, which the palette lists before the ones
    /// every screen has.
    fn actions(&self) -> &'static [Action] {
        &[]
    }

    /// Applies `command` to the screen drawn in `area`, and returns what it
    /// asks of the app, if anything. Only scrolling needs `area`, to know
    /// how far a page goes; the state never keeps it.
    fn apply(&mut self, command: Command, area: Rect) -> Option<Command>;

    /// Draws the screen in `area`. Drawing never changes state.
    fn render(&self, context: &Context<'_>, frame: &mut Frame<'_>, area: Rect);
}

/// What screens draw with and show.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Context<'a> {
    pub(crate) theme: &'a Theme,
    pub(crate) settings: &'a Settings,
    pub(crate) showcase: &'a ShowcaseProject,
}

/// A screen a learner can go to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Route {
    Home,
    Settings,
    /// The area and change picker.
    Picker,
    /// The change viewer, showing the showcase project's curated change at
    /// this index.
    Viewer {
        change: usize,
    },
}

impl Route {
    /// The screen at this route, as it first opens over `showcase`.
    pub(crate) fn open(self, showcase: &ShowcaseProject) -> Box<dyn View> {
        match self {
            Self::Home => Box::new(Home::default()),
            Self::Settings => Box::new(SettingsScreen::default()),
            Self::Picker => Box::new(Picker::new(showcase)),
            Self::Viewer { change } => Box::new(Viewer::new(
                change,
                showcase
                    .changes()
                    .get(change)
                    .expect("only the picker opens a change, and only one it lists"),
            )),
        }
    }
}
