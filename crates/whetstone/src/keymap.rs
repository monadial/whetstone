//! The keymap: which key presses do what on each screen and overlay.
//!
//! Screens and overlays each declare their [`Keymap`], and the app adds the
//! keys that work everywhere. Dispatch and key help read the same bindings,
//! so what `?` shows is what the keys do.
//!
//! A screen adds keys by declaring bindings for the commands it
//! understands. A binding can name a single key, such as `G` or `Ctrl-d`, or
//! a two-key sequence, such as `gg`, whose first key [`Pending`] remembers
//! until the next press.
//!
//! While a text entry such as the search box has focus, the screen's
//! [`Input`] is text: every character a key [types](typed) is text before
//! any binding, and only the screen's own keymap (Enter, Backspace, Esc)
//! edits or leaves the entry. The keys that work everywhere wait, except the
//! [reserved](RESERVED) Ctrl+C and Ctrl+Z.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::view::Route;

/// What a key asks for. Each screen or overlay decides what a command means
/// for it, and ignores the ones it has no use for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Command {
    /// Move down a list, or scroll down.
    Down,
    /// Move up a list, or scroll up.
    Up,
    /// Move to the pane on the left.
    Left,
    /// Move to the pane on the right.
    Right,
    /// Go to the top of a list or pane.
    Top,
    /// Go to the bottom of a list or pane.
    Bottom,
    /// Scroll down half a page.
    HalfPageDown,
    /// Scroll up half a page.
    HalfPageUp,
    /// Move focus to the next control.
    NextControl,
    /// Move focus to the previous control.
    PreviousControl,
    /// Activate the focused control, or run the selected action.
    Activate,
    /// Open the focused item on a screen of its own, such as a change in
    /// the change viewer.
    Open,
    /// Close the overlay, or go back to the previous screen.
    Back,
    /// Leave Whetstone.
    Quit,
    /// Drop to the shell until the learner brings Whetstone back with `fg`.
    Suspend,
    /// Show the keys of the current screen.
    KeyHelp,
    /// List the current screen's actions.
    Palette,
    /// Go to a screen.
    Go(Route),
    /// Start typing a search.
    Search,
    /// Move to the next match of the last search.
    NextMatch,
    /// Move to the previous match of the last search.
    PreviousMatch,
    /// Type a character into the focused text entry.
    Type(char),
    /// Erase the last character typed into the focused text entry.
    Erase,
    /// Stop typing, and close the focused text entry.
    Cancel,
}

/// What key presses are for on a screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Input {
    /// Commands, through the screen's keymap and then the keys that work
    /// everywhere.
    Commands,
    /// Text for the focused text entry.
    Text,
}

/// The character `key` types into a text entry, if it types one: a
/// character with no modifier but Shift.
pub(crate) fn typed(key: &KeyEvent) -> Option<char> {
    match key.code {
        KeyCode::Char(c) if (key.modifiers - KeyModifiers::SHIFT).is_empty() => Some(c),
        _ => None,
    }
}

/// One key, with the modifiers held.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Key {
    code: KeyCode,
    modifiers: KeyModifiers,
}

impl Key {
    /// Whether `event` is this key. Shift is part of a character (`?`, `G`)
    /// and of a back tab, and terminals differ in whether they also report
    /// it as a modifier, so it is ignored there.
    fn is(self, event: &KeyEvent) -> bool {
        let modifiers = match event.code {
            KeyCode::Char(_) | KeyCode::BackTab => event.modifiers - KeyModifiers::SHIFT,
            _ => event.modifiers,
        };
        event.code == self.code && modifiers == self.modifiers
    }

    /// The key as key help names it: `j`, `Up`, `Shift+Tab`, `Ctrl+C`. The
    /// names are Whetstone's own, the same on every platform.
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

/// What a binding names: one key press, or two in a row, such as `gg`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Press {
    first: Key,
    /// The second key of a two-key sequence.
    then: Option<Key>,
}

impl Press {
    pub(crate) const fn key(code: KeyCode) -> Self {
        Self {
            first: Key {
                code,
                modifiers: KeyModifiers::NONE,
            },
            then: None,
        }
    }

    pub(crate) const fn char(c: char) -> Self {
        Self::key(KeyCode::Char(c))
    }

    pub(crate) const fn ctrl(c: char) -> Self {
        Self {
            first: Key {
                code: KeyCode::Char(c),
                modifiers: KeyModifiers::CONTROL,
            },
            then: None,
        }
    }

    /// This press followed by the character `c`: a two-key sequence, such
    /// as `Press::char('g').followed_by('g')` for `gg`.
    pub(crate) const fn followed_by(self, c: char) -> Self {
        Self {
            then: Some(Key {
                code: KeyCode::Char(c),
                modifiers: KeyModifiers::NONE,
            }),
            ..self
        }
    }

    /// Whether `key` on its own is this press.
    fn matches(self, key: &KeyEvent) -> bool {
        self.then.is_none() && self.first.is(key)
    }

    /// Whether `key` starts this two-key sequence.
    fn starts(self, key: &KeyEvent) -> bool {
        self.then.is_some() && self.first.is(key)
    }

    /// Whether `key`, pressed after `first`, completes this two-key
    /// sequence.
    fn completes(self, first: &KeyEvent, key: &KeyEvent) -> bool {
        self.then
            .is_some_and(|then| self.first.is(first) && then.is(key))
    }

    /// The press as key help names it: `j`, `Ctrl+C` or `gg`.
    fn name(self) -> String {
        let mut name = self.first.name();
        if let Some(then) = self.then {
            name.push_str(&then.name());
        }
        name
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

/// The footer reminders of `keymaps`' bindings, in order.
pub(crate) fn hints(keymaps: &[Keymap]) -> String {
    keymaps
        .iter()
        .flat_map(|keymap| keymap.iter())
        .filter_map(|binding| binding.hint)
        .collect::<Vec<_>>()
        .join("  ")
}

/// Turns key presses into commands through a screen's keymaps. The first
/// key of a two-key sequence, such as the first `g` of `gg`, sends nothing
/// and waits here for the next. If the next key does not complete the
/// sequence, the first is dropped and the next acts on its own. So a key
/// that starts a sequence never also acts alone in the same keymaps.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Pending {
    first: Option<KeyEvent>,
}

impl Pending {
    /// The command `key` sends in `keymaps`, the first that binds it, if it
    /// sends one.
    pub(crate) fn command(&mut self, keymaps: &[Keymap], key: &KeyEvent) -> Option<Command> {
        let presses = || {
            keymaps
                .iter()
                .flat_map(|keymap| keymap.iter())
                .flat_map(|binding| binding.keys)
        };
        if let Some(first) = self.first.take()
            && let Some(&(_, command)) = presses().find(|(press, _)| press.completes(&first, key))
        {
            return Some(command);
        }
        if presses().any(|(press, _)| press.starts(key)) {
            self.first = Some(*key);
            return None;
        }
        presses()
            .find(|(press, _)| press.matches(key))
            .map(|&(_, command)| command)
    }
}

/// The command `key` on its own sends in `keymap`, if it sends one.
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

/// Ctrl+Z: in raw mode it arrives as a key, and it suspends from anywhere,
/// before any screen or overlay sees it.
pub(crate) const SUSPEND: Binding = Binding {
    keys: &[(Press::ctrl('z'), Command::Suspend)],
    does: "Suspend to the shell, until fg",
    hint: None,
};

/// The reserved keys: they act from anywhere, even under the "window too
/// small" notice, before any overlay, text entry or screen sees the key.
pub(crate) const RESERVED: Keymap = &[INTERRUPT, SUSPEND];

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

/// `h`/`l` and the arrows switch panes.
pub(crate) const PANES: Binding = Binding {
    keys: &[
        (Press::char('h'), Command::Left),
        (Press::key(KeyCode::Left), Command::Left),
        (Press::char('l'), Command::Right),
        (Press::key(KeyCode::Right), Command::Right),
    ],
    does: "Move to the pane on the left or right",
    hint: Some("h/l panes"),
};

/// `gg` and `G` go to the top or bottom.
pub(crate) const ENDS: Binding = Binding {
    keys: &[
        (Press::char('g').followed_by('g'), Command::Top),
        (Press::char('G'), Command::Bottom),
    ],
    does: "Go to the top or bottom",
    hint: None,
};

/// `Ctrl-d` and `Ctrl-u` scroll half a page.
pub(crate) const HALF_PAGE: Binding = Binding {
    keys: &[
        (Press::ctrl('d'), Command::HalfPageDown),
        (Press::ctrl('u'), Command::HalfPageUp),
    ],
    does: "Scroll down or up half a page",
    hint: None,
};

/// `/` starts a search.
pub(crate) const SEARCH: Binding = Binding {
    keys: &[(Press::char('/'), Command::Search)],
    does: "Search the focused pane",
    hint: Some("/ search"),
};

/// `n` and `N` move between the matches of the last search.
pub(crate) const MATCHES: Binding = Binding {
    keys: &[
        (Press::char('n'), Command::NextMatch),
        (Press::char('N'), Command::PreviousMatch),
    ],
    does: "Next or previous match",
    hint: None,
};

/// The keys of a search box, while it has focus. Every other key but the
/// reserved ones types.
pub(crate) const SEARCH_BOX: Keymap = &[
    Binding {
        keys: &[(Press::key(KeyCode::Enter), Command::Activate)],
        does: "Search for what was typed",
        hint: Some("Enter search"),
    },
    Binding {
        keys: &[(Press::key(KeyCode::Backspace), Command::Erase)],
        does: "Erase the last character",
        hint: None,
    },
    Binding {
        keys: &[(Press::key(KeyCode::Esc), Command::Cancel)],
        does: "Stop searching",
        hint: Some("Esc cancel"),
    },
];

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
