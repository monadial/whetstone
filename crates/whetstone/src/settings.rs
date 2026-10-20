//! The Settings screen: what Practice will use and how much colour Whetstone
//! draws with, read-only. Each setting is a control, and the focused one says
//! what it is for and how to change it.

use std::fmt;

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{List, Paragraph, Wrap};
use whetstone_core::{ConnectedCodeLimitKb, QuestionCount};

use crate::columns;
use crate::config::{
    ConfigFile, DEFAULT_GENERATOR_MODEL, DEFAULT_JEV_MODEL, KeyStatus, OPENROUTER_KEY_VARIABLE,
    Origin, Setting, Settings,
};
use crate::focus::Position;
use crate::keymap::{self, Command, Keymap};
use crate::palette::Action;
use crate::theme::{Colours, ControlState, NoColorStatus, Theme};
use crate::view::{Context, Route, View};

/// Settings' own keys: `q` and Esc go back.
const KEYMAP: Keymap = &[keymap::MOVE, keymap::ACTIVATE, keymap::BACK];

/// Settings' own action in the palette.
const ACTIONS: &[Action] = &[Action {
    label: "Back",
    does: "Return to the screen before",
    command: Command::Back,
}];

/// The controls on Settings, in focus order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Control {
    Provider,
    JevModel,
    GeneratorModel,
    OpenRouterKey,
    Colours,
    ConnectedCodeLimit,
    Questions,
    Back,
}

impl Control {
    const ALL: [Self; 8] = [
        Self::Provider,
        Self::JevModel,
        Self::GeneratorModel,
        Self::OpenRouterKey,
        Self::Colours,
        Self::ConnectedCodeLimit,
        Self::Questions,
        Self::Back,
    ];
    const ROWS: [Self; 7] = [
        Self::Provider,
        Self::JevModel,
        Self::GeneratorModel,
        Self::OpenRouterKey,
        Self::Colours,
        Self::ConnectedCodeLimit,
        Self::Questions,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::Provider => "Provider",
            Self::JevModel => "Jev model",
            Self::GeneratorModel => "Generator model",
            Self::OpenRouterKey => "OpenRouter key",
            Self::Colours => "Colours",
            Self::ConnectedCodeLimit => "Connected code limit",
            Self::Questions => "Questions",
            Self::Back => "Back",
        }
    }

    /// The setting's value, and a note if it is the default.
    fn value(self, settings: &Settings) -> (String, Option<&'static str>) {
        match self {
            Self::Provider => shown(&settings.provider),
            Self::JevModel => shown(&settings.jev_model),
            Self::GeneratorModel => shown(&settings.generator_model),
            Self::OpenRouterKey => (
                match settings.openrouter_key {
                    KeyStatus::Set(_) => format!("set ({OPENROUTER_KEY_VARIABLE})"),
                    KeyStatus::NotSet => format!("not set ({OPENROUTER_KEY_VARIABLE})"),
                },
                None,
            ),
            Self::Colours => {
                let (value, default_note) = shown(&settings.colours);
                let value = match settings.colours.value {
                    Colours::Auto => {
                        format!("{value} ({} detected)", settings.detected_color_support)
                    }
                    _ => value,
                };
                (value, default_note)
            }
            Self::ConnectedCodeLimit => shown(&settings.connected_code_limit),
            Self::Questions => shown(&settings.questions),
            Self::Back => (String::new(), None),
        }
    }

    /// What the control is for.
    fn about(self, settings: &Settings) -> String {
        match self {
            Self::Provider => "Where Practice sends its requests. \
                OpenRouter is the only provider so far."
                .to_owned(),
            Self::JevModel => format!(
                "Jev is the evaluator: it returns typed decisions and probabilities, never \
                 prose. The default is {DEFAULT_JEV_MODEL}."
            ),
            Self::GeneratorModel => format!(
                "The generator writes questions, explanations and hints from the evidence it \
                 is given. The default is {DEFAULT_GENERATOR_MODEL}."
            ),
            Self::OpenRouterKey => match settings.openrouter_key {
                KeyStatus::Set(_) => "Read from the environment when Whetstone starts. Its value \
                    is never shown."
                    .to_owned(),
                KeyStatus::NotSet => "Practice needs it to ask Jev and the generator. Its value \
                    is never shown."
                    .to_owned(),
            },
            Self::Colours => {
                let no_color = match settings.no_color {
                    NoColorStatus::Set => {
                        "NO_COLOR is set, so nothing is coloured, whatever this says."
                    }
                    NoColorStatus::NotSet => {
                        "A non-empty NO_COLOR turns colour off, whatever this says."
                    }
                };
                format!(
                    "How much colour Whetstone draws with: auto, 24-bit, 256, 16 or none. Auto \
                     reads COLORTERM, then TERM. Set a level when your terminal shows more than \
                     it reports, as over SSH, or less when colours look wrong. {no_color}"
                )
            }
            Self::ConnectedCodeLimit => format!(
                "How much connected code Practice adds to the evidence: what the change calls, \
                 what calls it, and the files it links to or names. From 0 to 400 KB, and 0 \
                 sends none; related tests and configuration still apply. A repository's \
                 .whetstone/config.toml can lower it, never raise it. The default is {}.",
                ConnectedCodeLimitKb::DEFAULT
            ),
            Self::Questions => format!(
                "How many questions a Practice asks for, from 3 to 5. The default is {}.",
                QuestionCount::DEFAULT
            ),
            Self::Back => "Back to home.".to_owned(),
        }
    }

    /// How to change the setting, if it is one.
    fn how_to_change(self, settings: &Settings) -> Option<String> {
        match self {
            Self::Provider => Some("In config.toml: provider = \"openrouter\"".to_owned()),
            Self::JevModel => Some(format!(
                "In config.toml, under [jev]: model = \"{}\"",
                settings.jev_model.value
            )),
            Self::GeneratorModel => Some(format!(
                "In config.toml, under [generator]: model = \"{}\"",
                settings.generator_model.value
            )),
            Self::OpenRouterKey => Some(format!(
                "Never read from a file: set {OPENROUTER_KEY_VARIABLE} in your shell."
            )),
            Self::Colours => Some(format!(
                "In config.toml: colours = \"{}\"",
                settings.colours.value
            )),
            Self::ConnectedCodeLimit => Some(format!(
                "In config.toml: connected_code_limit_kb = {}",
                settings.connected_code_limit.value.get()
            )),
            Self::Questions => Some(format!(
                "In config.toml: questions = {}",
                settings.questions.value
            )),
            Self::Back => None,
        }
    }
}

/// The Settings screen's state: which control has focus.
#[derive(Debug, Clone, Default)]
pub(crate) struct SettingsScreen {
    focus: Position,
}

impl SettingsScreen {
    fn focused(&self) -> Control {
        Control::ALL[self.focus.index()]
    }

    fn control(&self, theme: &Theme, control: Control) -> Line<'static> {
        theme.control(
            control.label(),
            ControlState {
                focused: control == self.focused(),
                ..ControlState::default()
            },
        )
    }
}

impl View for SettingsScreen {
    fn route(&self) -> Route {
        Route::Settings
    }

    fn name(&self) -> &'static str {
        "Settings"
    }

    fn keymap(&self) -> Keymap {
        KEYMAP
    }

    fn actions(&self) -> &'static [Action] {
        ACTIONS
    }

    fn apply(&mut self, command: Command, _area: Rect) -> Option<Command> {
        self.focus = self.focus.after(command, Control::ALL.len());
        // The settings are read-only; only Back does something.
        (command == Command::Activate && self.focused() == Control::Back).then_some(Command::Back)
    }

    fn render(&self, context: &Context<'_>, frame: &mut Frame<'_>, area: Rect) {
        let Context {
            theme, settings, ..
        } = *context;
        let note = Paragraph::new(Line::styled(file_note(&settings.file), theme.muted))
            .wrap(Wrap { trim: true });
        let [title, note_area, rows, back, about] = layout(area, note.line_count(area.width));
        frame.render_widget(Paragraph::new(Span::styled("Settings", theme.title)), title);
        frame.render_widget(note, note_area);
        // Past the focus marker, and the widest label.
        let value_column = 2 + columns::next_column(Control::ROWS.map(Control::label));
        frame.render_widget(
            List::new(Control::ROWS.map(|control| {
                let mut line = self.control(theme, control);
                let (value, default_note) = control.value(settings);
                columns::pad_to(&mut line, value_column);
                line.push_span(value);
                if let Some(note) = default_note {
                    line.push_span(Span::styled(note, theme.muted));
                }
                line
            })),
            rows,
        );
        frame.render_widget(self.control(theme, Control::Back), back);
        let focused = self.focused();
        let mut explanation = vec![Line::styled(focused.about(settings), theme.muted)];
        explanation.extend(focused.how_to_change(settings).map(Line::raw));
        frame.render_widget(Paragraph::new(explanation).wrap(Wrap { trim: true }), about);
    }
}

/// A setting's value as Settings shows it, and a note if it is the default.
fn shown<T: fmt::Display>(setting: &Setting<T>) -> (String, Option<&'static str>) {
    (
        setting.value.to_string(),
        (setting.origin == Origin::Default).then_some(" (default)"),
    )
}

/// Where the title, the note on the config file, the setting rows, Back and
/// the focused control's explanation go in `area`, when the note takes
/// `note_rows` rows. The controls always get their rows; the note, the gaps
/// and the explanation share what is left, in that order, so no control is
/// ever pushed out of a short terminal.
fn layout(area: Rect, note_rows: usize) -> [Rect; 5] {
    const TITLE: u16 = 1;
    const BACK: u16 = 1;
    let rows = u16::try_from(Control::ROWS.len()).unwrap_or(u16::MAX);
    let mut left = area.height.saturating_sub(TITLE + rows + BACK);
    let mut take = |wanted: u16| {
        let taken = wanted.min(left);
        left -= taken;
        taken
    };
    let note = take(u16::try_from(note_rows).unwrap_or(u16::MAX));
    let gaps = [take(1), take(1), take(1)];
    let [title, note, _, rows, _, back, _, about] = Layout::vertical([
        Constraint::Length(TITLE),
        Constraint::Length(note),
        Constraint::Length(gaps[0]),
        Constraint::Length(rows),
        Constraint::Length(gaps[1]),
        Constraint::Length(BACK),
        Constraint::Length(gaps[2]),
        Constraint::Fill(1),
    ])
    .areas(area);
    [title, note, rows, back, about]
}

/// Where the settings come from.
fn file_note(file: &ConfigFile) -> String {
    match file {
        ConfigFile::Found { shown } => {
            format!("Read from {shown}. Settings is read-only: edit that file to change them.")
        }
        ConfigFile::Missing { shown } => {
            format!("No config found at {shown}, so these are the defaults.")
        }
    }
}
