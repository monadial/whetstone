//! The keymap outside text entry: which key presses do what on each screen
//! and overlay.
//!
//! Screens and overlays each declare their [`Keymap`], and the app adds the
//! keys that work everywhere. Dispatch and key help read the same bindings,
//! so what `?` shows is what the keys do. A later screen registers its own
//! keys (`h`/`l`, `gg`/`G`, `Ctrl-d`/`Ctrl-u`, `/`, `n`/`N`) by declaring
//! bindings for the commands it understands.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::app::Route;

/// What a key asks for. Each screen or overlay decides what a command means
/// for it, and ignores the ones it has no use for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Command {
    /// Move down a list, or scroll down.
    Down,
    /// Move up a list, or scroll up.
    Up,
    /// Move focus to the next control.
    NextControl,
    /// Move focus to the previous control.
    PreviousControl,
    /// Activate the focused control, or run the selected action.
    Activate,
    /// Close the overlay, or go back to the previous screen.
    Back,
    /// Leave Whetstone.
    Quit,
    /// Show the keys of the current screen.
    KeyHelp,
    /// List the current screen's actions.
    Palette,
    /// Go to a screen.
    Go(Route),
}

/// One key press, as a binding names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Press {
    code: KeyCode,
    modifiers: KeyModifiers,
}

impl Press {
    pub(crate) const fn key(code: KeyCode) -> Self {
        Self {
            code,
            modifiers: KeyModifiers::NONE,
        }
    }

    pub(crate) const fn char(c: char) -> Self {
        Self::key(KeyCode::Char(c))
    }

    pub(crate) const fn ctrl(c: char) -> Self {
        Self {
            code: KeyCode::Char(c),
            modifiers: KeyModifiers::CONTROL,
        }
    }

    /// Whether `key` is this press. Shift is part of a character (`?`, `G`)
    /// and of a back tab, and terminals differ in whether they also report
    /// it as a modifier, so it is ignored there.
    fn matches(self, key: &KeyEvent) -> bool {
        let modifiers = match key.code {
            KeyCode::Char(_) | KeyCode::BackTab => key.modifiers - KeyModifiers::SHIFT,
            _ => key.modifiers,
        };
        key.code == self.code && modifiers == self.modifiers
    }

    /// The press as key help names it: `j`, `Up`, `Shift+Tab`, `Ctrl+C`.
    /// The names are Whetstone's own, the same on every platform.
    fn name(self) -> String {
        let key = match self.code {
            KeyCode::Char(' ') => "Space".to_owned(),
            KeyCode::Char(c) if self.modifiers.contains(KeyModifiers::CONTROL) => {
                c.to_ascii_uppercase().to_string()
            }
            KeyCode::Char(c) => c.to_string(),
            KeyCode::Enter => "Enter".to_owned(),
            KeyCode::Esc => "Esc".to_owned(),
            KeyCode::Tab => "Tab".to_owned(),
            KeyCode::BackTab => "Shift+Tab".to_owned(),
            KeyCode::Up => "Up".to_owned(),
            KeyCode::Down => "Down".to_owned(),
            KeyCode::Left => "Left".to_owned(),
            KeyCode::Right => "Right".to_owned(),
            code => format!("{code:?}"),
        };
        if self.modifiers.contains(KeyModifiers::CONTROL) {
            format!("Ctrl+{key}")
        } else {
            key
        }
    }
}

/// Keys that do one thing: a row of key help.
#[derive(Debug)]
pub(crate) struct Binding {
    /// Each key, and the command it sends.
    pub(crate) keys: &'static [(Press, Command)],
    /// What the keys do, as key help says it.
    pub(crate) does: &'static str,
    /// A reminder for the footer, if the keys are worth one.
    pub(crate) hint: Option<&'static str>,
}

impl Binding {
    /// The keys as key help lists them: `j, Down, k, Up`.
    pub(crate) fn key_names(&self) -> String {
        self.keys
            .iter()
            .map(|(press, _)| press.name())
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// The bindings of one screen or overlay, in key help order.
pub(crate) type Keymap = &'static [Binding];

/// The command `key` sends in `keymap`, if it sends one.
pub(crate) fn command(keymap: Keymap, key: &KeyEvent) -> Option<Command> {
    keymap
        .iter()
        .flat_map(|binding| binding.keys)
        .find(|(press, _)| press.matches(key))
        .map(|&(_, command)| command)
}

/// Ctrl+C: in raw mode it arrives as a key, and it quits from anywhere,
/// before any screen or overlay sees it.
pub(crate) const INTERRUPT: Binding = Binding {
    keys: &[(Press::ctrl('c'), Command::Quit)],
    does: "Quit from anywhere",
    hint: None,
};

/// Tab and Shift+Tab move focus between controls, wrapping around.
pub(crate) const FOCUS: Binding = Binding {
    keys: &[
        (Press::key(KeyCode::Tab), Command::NextControl),
        (Press::key(KeyCode::BackTab), Command::PreviousControl),
    ],
    does: "Next or previous control",
    hint: None,
};

/// The keys that work on every screen, after the screen's own.
pub(crate) const EVERYWHERE: Keymap = &[
    FOCUS,
    Binding {
        keys: &[(Press::char('?'), Command::KeyHelp)],
        does: "Show these keys",
        hint: Some("? keys"),
    },
    Binding {
        keys: &[(Press::char(':'), Command::Palette)],
        does: "List the actions",
        hint: Some(": actions"),
    },
    INTERRUPT,
];

/// `j`/`k` and the arrows move through a list.
pub(crate) const MOVE: Binding = Binding {
    keys: &[
        (Press::char('j'), Command::Down),
        (Press::key(KeyCode::Down), Command::Down),
        (Press::char('k'), Command::Up),
        (Press::key(KeyCode::Up), Command::Up),
    ],
    does: "Move down or up",
    hint: None,
};

/// Enter and Space activate the focused control.
pub(crate) const ACTIVATE: Binding = Binding {
    keys: &[
        (Press::key(KeyCode::Enter), Command::Activate),
        (Press::char(' '), Command::Activate),
    ],
    does: "Activate the focused control",
    hint: None,
};

/// `q` quits from home.
pub(crate) const QUIT: Binding = Binding {
    keys: &[(Press::char('q'), Command::Quit)],
    does: "Quit",
    hint: Some("q quit"),
};

/// `q` and Esc go back from any screen but home.
pub(crate) const BACK: Binding = Binding {
    keys: &[
        (Press::char('q'), Command::Back),
        (Press::key(KeyCode::Esc), Command::Back),
    ],
    does: "Back",
    hint: Some("q back"),
};
