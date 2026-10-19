//! The Settings screen: what Practice will use, read-only. Each setting is a
//! control, and the focused one says what it is for and how to change it.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{List, Paragraph, Wrap};

use crate::config::{
    ConfigFile, DEFAULT_GENERATOR_MODEL, DEFAULT_JEV_MODEL, KeyStatus, OPENROUTER_KEY_VARIABLE,
    Origin, Settings,
};
use crate::focus::Position;
use crate::keymap::{self, Command, Keymap};
use crate::theme::{ControlState, Theme};

/// Settings' own keys: `q` and Esc go back.
pub(crate) const KEYMAP: Keymap = &[keymap::MOVE, keymap::ACTIVATE, keymap::BACK];

/// The controls on Settings, in focus order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Control {
    Provider,
    JevModel,
    GeneratorModel,
    OpenRouterKey,
    Back,
}

impl Control {
    const ALL: [Self; 5] = [
        Self::Provider,
        Self::JevModel,
        Self::GeneratorModel,
        Self::OpenRouterKey,
        Self::Back,
    ];
    const ROWS: [Self; 4] = [
        Self::Provider,
        Self::JevModel,
        Self::GeneratorModel,
        Self::OpenRouterKey,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::Provider => "Provider",
            Self::JevModel => "Jev model",
            Self::GeneratorModel => "Generator model",
            Self::OpenRouterKey => "OpenRouter key",
            Self::Back => "Back",
        }
    }

    /// The setting's value, and whether it is the default.
    fn value(self, settings: &Settings) -> (String, Option<&'static str>) {
        let shown = |origin| match origin {
            Origin::Default => Some(" (default)"),
            Origin::File => None,
        };
        match self {
            Self::Provider => (
                settings.provider.value.to_string(),
                shown(settings.provider.origin),
            ),
            Self::JevModel => (
                settings.jev_model.value.clone(),
                shown(settings.jev_model.origin),
            ),
            Self::GeneratorModel => (
                settings.generator_model.value.clone(),
                shown(settings.generator_model.origin),
            ),
            Self::OpenRouterKey => (
                match settings.openrouter_key {
                    KeyStatus::Set => format!("set ({OPENROUTER_KEY_VARIABLE})"),
                    KeyStatus::NotSet => format!("not set ({OPENROUTER_KEY_VARIABLE})"),
                },
                None,
            ),
            Self::Back => (String::new(), None),
        }
    }

    /// What the control is for.
    fn about(self, settings: &Settings) -> String {
        match self {
            Self::Provider => "Where Practice will send its requests once AI arrives. \
                OpenRouter is the only provider so far."
                .to_owned(),
            Self::JevModel => format!(
                "Jev assesses your choice and each part of your reasoning; it never writes \
                 prose. The default is {DEFAULT_JEV_MODEL}."
            ),
            Self::GeneratorModel => format!(
                "The generator writes questions, explanations and hints. The default is \
                 {DEFAULT_GENERATOR_MODEL}."
            ),
            Self::OpenRouterKey => match settings.openrouter_key {
                KeyStatus::Set => "Read from the environment when Whetstone starts. Its value \
                    is never shown."
                    .to_owned(),
                KeyStatus::NotSet => "Practice will need it once AI arrives. Its value is \
                    never shown."
                    .to_owned(),
            },
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
    /// Applies `command`, and returns what it asks of the app, if anything.
    pub(crate) fn apply(&mut self, command: Command) -> Option<Command> {
        let count = Control::ALL.len();
        match command {
            Command::Down => self.focus = self.focus.down(count),
            Command::Up => self.focus = self.focus.up(),
            Command::NextControl => self.focus = self.focus.next(count),
            Command::PreviousControl => self.focus = self.focus.previous(count),
            // The settings are read-only; only Back does something.
            Command::Activate if self.focused() == Control::Back => return Some(Command::Back),
            _ => {}
        }
        None
    }

    fn focused(&self) -> Control {
        Control::ALL[self.focus.index()]
    }

    pub(crate) fn render(
        &self,
        theme: &Theme,
        settings: &Settings,
        frame: &mut Frame<'_>,
        area: Rect,
    ) {
        let note = Paragraph::new(Line::styled(file_note(&settings.file), theme.muted))
            .wrap(Wrap { trim: true });
        let [title, note_area, rows, back, about] = layout(area, note.line_count(area.width));
        frame.render_widget(Paragraph::new(Span::styled("Settings", theme.title)), title);
        frame.render_widget(note, note_area);
        let label_width = Control::ROWS
            .iter()
            .map(|control| control.label().len())
            .max()
            .unwrap_or_default()
            + 2;
        frame.render_widget(
            List::new(Control::ROWS.map(|control| {
                let mut line = self.control(theme, control);
                let (value, origin) = control.value(settings);
                line.push_span(" ".repeat(label_width - control.label().len()));
                line.push_span(value);
                if let Some(origin) = origin {
                    line.push_span(Span::styled(origin, theme.muted));
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
