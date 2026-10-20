//! The theme layer. Screens take every style from a [`Theme`] and never name
//! a colour themselves, so the palette can be swapped in one place.
//!
//! The palette comes from the mineral-blue brand tokens in
//! `assets/brand/brand-tokens.json`, translated to what the terminal can
//! show. Colour only ever adds to markers and text attributes, which stay
//! the same at every colour level, so meaning never depends on colour.

use std::borrow::Cow;
use std::ffi::OsStr;
use std::fmt;

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use serde::Deserialize;

use crate::environment::Environment;

/// How much colour the terminal can show.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorSupport {
    /// 24-bit colour: the brand tokens exactly.
    TrueColor,
    /// The 256-colour palette: the tokens' indexed approximations.
    Ansi256,
    /// The terminal's own 16 colours, whatever they look like.
    Ansi16,
    /// No colour at all, as `NO_COLOR` or the `colours` setting asks.
    NoColor,
}

impl ColorSupport {
    /// The colour level to draw with: none while `NO_COLOR` is set,
    /// otherwise the level `colours` names, otherwise `detected`, what auto
    /// [detects](Self::detect) from the terminal.
    #[must_use]
    pub(crate) fn choose(no_color: NoColorStatus, colours: Colours, detected: Self) -> Self {
        match (no_color, colours.level()) {
            (NoColorStatus::Set, _) => Self::NoColor,
            (NoColorStatus::NotSet, Some(level)) => level,
            (NoColorStatus::NotSet, None) => detected,
        }
    }

    /// What the terminal says it can show: `COLORTERM` set to `truecolor` or
    /// `24bit` allows 24-bit colour, a `TERM` naming `256color` allows 256
    /// colours, and anything else gets the terminal's own 16.
    #[must_use]
    pub(crate) fn detect(environment: &Environment) -> Self {
        let var = |name| environment.get(name).and_then(OsStr::to_str);
        if var("COLORTERM").is_some_and(|value| matches!(value, "truecolor" | "24bit")) {
            Self::TrueColor
        } else if var("TERM").is_some_and(|term| term.contains("256color")) {
            Self::Ansi256
        } else {
            Self::Ansi16
        }
    }
}

/// The level as the `colours` setting names it.
impl fmt::Display for ColorSupport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::TrueColor => "24-bit",
            Self::Ansi256 => "256",
            Self::Ansi16 => "16",
            Self::NoColor => "none",
        })
    }
}

/// The Colours setting: a colour level, or auto to detect it from the
/// terminal. `NO_COLOR` wins over every value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub(crate) enum Colours {
    #[serde(rename = "auto")]
    Auto,
    #[serde(rename = "24-bit")]
    TrueColor,
    #[serde(rename = "256")]
    Ansi256,
    #[serde(rename = "16")]
    Ansi16,
    #[serde(rename = "none")]
    NoColor,
}

impl Colours {
    /// The level the setting names, or `None` for auto.
    fn level(self) -> Option<ColorSupport> {
        match self {
            Self::Auto => None,
            Self::TrueColor => Some(ColorSupport::TrueColor),
            Self::Ansi256 => Some(ColorSupport::Ansi256),
            Self::Ansi16 => Some(ColorSupport::Ansi16),
            Self::NoColor => Some(ColorSupport::NoColor),
        }
    }
}

/// The value as the config file writes it.
impl fmt::Display for Colours {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.level() {
            None => f.write_str("auto"),
            Some(level) => level.fmt(f),
        }
    }
}

/// Whether the environment asks for no colour with a non-empty `NO_COLOR`
/// (no-color.org).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NoColorStatus {
    Set,
    NotSet,
}

impl NoColorStatus {
    /// Reads `NO_COLOR` from `environment`.
    pub(crate) fn read(environment: &Environment) -> Self {
        if environment
            .get("NO_COLOR")
            .is_some_and(|value| !value.is_empty())
        {
            Self::Set
        } else {
            Self::NotSet
        }
    }
}

/// The styles every screen draws with.
#[derive(Debug, Clone)]
pub struct Theme {
    /// The whole screen: reading text on the canvas.
    pub(crate) canvas: Style,
    /// The product name.
    pub(crate) title: Style,
    /// Secondary reading text.
    pub(crate) muted: Style,
    /// The bands of the flat-stone mark.
    pub(crate) stone: Stone,
    /// The cursor at the end of a text entry: reversed, so it shows without
    /// colour.
    pub(crate) cursor: Style,
    /// Text a search found: underlined, so it shows without colour.
    pub(crate) matched: Style,
    /// The match a search landed on: reversed and bold, so it stands out
    /// from the other matches without colour.
    pub(crate) current_match: Style,
    /// A stage of Practice not reached yet: dim, so it recedes without
    /// colour too, while staying readable as what comes next.
    pub(crate) upcoming_stage: Style,
    /// The control that owns keyboard focus.
    focus: Style,
    /// The row of a selected choice.
    selected: Style,
    /// The label of a control that cannot be used yet.
    disabled: Style,
}

/// What a control shows about itself besides its label.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ControlState<'a> {
    /// It owns keyboard focus.
    pub focused: bool,
    /// Whether it is an action or a choice.
    pub kind: ControlKind,
    /// Why it is disabled, if it is. The reason is shown, so a disabled
    /// control reads differently even where text attributes do not show.
    pub disabled: Option<&'a str>,
}

/// What a control is for.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ControlKind {
    /// It does something when activated.
    #[default]
    Action,
    /// It is part of what the learner chooses, and selected or not.
    Choice { selected: bool },
}

/// What marks the control with focus, in front of its label.
const FOCUSED: &str = "> ";
/// What stands in front of the other controls' labels, as wide.
const UNFOCUSED: &str = "  ";
/// What marks a selected choice, after the focus marker.
const SELECTED: &str = "(x) ";
/// What marks a choice not selected, as wide.
const UNSELECTED: &str = "( ) ";

/// The colours of the flat-stone mark's three bands. Equal colours draw as
/// one solid shape.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Stone {
    /// The 45-degree working edge and the top face.
    pub(crate) edge: Color,
    /// The body of the stone.
    pub(crate) body: Color,
    /// The base it rests on.
    pub(crate) base: Color,
}

/// One colour level's translation of the brand tokens (the dark theme, as
/// the canvas is painted at every level that can show it).
#[derive(Debug)]
struct Palette {
    canvas: Color,
    text: Color,
    muted: Color,
    focus: Color,
    selected: Color,
    stone: Stone,
}

impl Palette {
    /// `themes.dark` in the brand tokens.
    const TRUE_COLOR: Self = Self {
        canvas: Color::Rgb(0x1B, 0x1E, 0x20),
        text: Color::Rgb(0xF5, 0xF3, 0xED),
        muted: Color::Rgb(0xB2, 0xBC, 0xBC),
        focus: Color::Rgb(0x9A, 0xAB, 0xFF),
        selected: Color::Rgb(0x30, 0x38, 0x3C),
        stone: Stone {
            edge: Color::Rgb(0xF5, 0xF3, 0xED),
            body: Color::Rgb(0x9A, 0xAB, 0xFF),
            base: Color::Rgb(0x3B, 0x5C, 0xCC),
        },
    };

    /// `ansi256Dark` in the brand tokens.
    const ANSI_256: Self = Self {
        canvas: Color::Indexed(234),
        text: Color::Indexed(255),
        muted: Color::Indexed(250),
        focus: Color::Indexed(111),
        selected: Color::Indexed(237),
        stone: Stone {
            edge: Color::Indexed(255),
            body: Color::Indexed(111),
            base: Color::Indexed(62),
        },
    };

    /// `ansi16` in the brand tokens: the terminal's own colours, with only
    /// focus picked out. The tokens name no stone colours here, so the edge
    /// takes the terminal's text colour and the blues their nearest ANSI
    /// colours.
    const ANSI_16: Self = Self {
        canvas: Color::Reset,
        text: Color::Reset,
        muted: Color::Reset,
        focus: Color::LightBlue,
        selected: Color::Reset,
        stone: Stone {
            edge: Color::Reset,
            body: Color::LightBlue,
            base: Color::Blue,
        },
    };

    /// `noColor` in the brand tokens: terminal defaults only.
    const NO_COLOR: Self = Self {
        canvas: Color::Reset,
        text: Color::Reset,
        muted: Color::Reset,
        focus: Color::Reset,
        selected: Color::Reset,
        stone: Stone {
            edge: Color::Reset,
            body: Color::Reset,
            base: Color::Reset,
        },
    };
}

impl Theme {
    /// The brand palette as far as `color_support` allows.
    #[must_use]
    pub fn new(color_support: ColorSupport) -> Self {
        let palette = match color_support {
            ColorSupport::TrueColor => Palette::TRUE_COLOR,
            ColorSupport::Ansi256 => Palette::ANSI_256,
            ColorSupport::Ansi16 => Palette::ANSI_16,
            ColorSupport::NoColor => Palette::NO_COLOR,
        };
        Self {
            canvas: Style::new().fg(palette.text).bg(palette.canvas),
            title: Style::new().add_modifier(Modifier::BOLD),
            muted: Style::new().fg(palette.muted),
            stone: palette.stone,
            cursor: Style::new().add_modifier(Modifier::REVERSED),
            matched: Style::new()
                .bg(palette.selected)
                .add_modifier(Modifier::UNDERLINED),
            current_match: Style::new().add_modifier(Modifier::REVERSED | Modifier::BOLD),
            upcoming_stage: Style::new().add_modifier(Modifier::DIM),
            focus: Style::new().fg(palette.focus).add_modifier(Modifier::BOLD),
            selected: Style::new().bg(palette.selected),
            disabled: Style::new()
                .fg(palette.muted)
                .add_modifier(Modifier::DIM | Modifier::ITALIC),
        }
    }

    /// A control as one line: `>` in front while it has focus, then `(x)` or
    /// `( )` for a choice, then its label, then why it is disabled if it is.
    /// Focus is also bold, a disabled label dim and italic, and a selected
    /// choice's row is highlighted where colour allows. The markers and
    /// attributes are the same at every colour level, so no state depends on
    /// colour.
    #[must_use]
    pub fn control<'a>(&self, label: impl Into<Cow<'a, str>>, state: ControlState<'a>) -> Line<'a> {
        let emphasis = if state.focused {
            self.focus
        } else {
            Style::new()
        };
        let mut spans = vec![Span::styled(
            if state.focused { FOCUSED } else { UNFOCUSED },
            emphasis,
        )];
        if let ControlKind::Choice { selected } = state.kind {
            spans.push(Span::styled(
                if selected { SELECTED } else { UNSELECTED },
                emphasis,
            ));
        }
        match state.disabled {
            None => spans.push(Span::styled(label, emphasis)),
            Some(reason) => {
                spans.push(Span::styled(label, self.disabled));
                spans.push(Span::styled(format!(" ({reason})"), self.disabled));
            }
        }
        let line = Line::from(spans);
        if state.kind == (ControlKind::Choice { selected: true }) {
            line.style(self.selected)
        } else {
            line
        }
    }

    /// How many cells [`Theme::control`] takes for a control of `kind`
    /// labelled `label` that is not disabled, focused or not.
    #[must_use]
    pub(crate) fn control_width(label: &str, kind: ControlKind) -> usize {
        let choice = match kind {
            ControlKind::Action => "",
            ControlKind::Choice { .. } => SELECTED,
        };
        Line::raw(format!("{UNFOCUSED}{choice}{label}")).width()
    }

    /// The border of a pane: in the focus colour while the pane has focus.
    /// Colour only adds to the focus marker inside the pane, which shows
    /// focus at every colour level.
    #[must_use]
    pub(crate) fn border(&self, focused: bool) -> Style {
        if focused {
            Style::new().fg(self.focus.fg.unwrap_or_default())
        } else {
            self.muted
        }
    }
}
