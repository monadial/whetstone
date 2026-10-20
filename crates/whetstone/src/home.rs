//! The home screen: the flat-stone mark, the actions a learner starts from,
//! why Practice on their repository is unavailable when it is, and what
//! Practice sends.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{List, Paragraph, Wrap};

use std::error::Error as _;

use crate::columns;
use crate::focus::Position;
use crate::keymap::{self, Command, Keymap};
use crate::mark;
use crate::repository::{self, Repository, Unavailable};
use crate::theme::ControlState;
use crate::view::{Context, Project, Route, View};

/// What a learner can start from home.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    Practice(Project),
    Settings,
    Quit,
}

impl Action {
    /// What activating it asks of the app.
    fn command(self) -> Command {
        match self {
            Self::Practice(project) => Command::Go(Route::Picker(project)),
            Self::Settings => Command::Go(Route::Settings),
            Self::Quit => Command::Quit,
        }
    }
}

/// Said under "Practice on" the learner's repository.
const REPOSITORY_NOTICE: &str = "Nothing leaves this machine until you review what will be sent.";
/// How far the repository notice is indented, past the focus marker.
const NOTICE_INDENT: usize = 4;

/// The brand's primary claim, beside the name.
const CLAIM: &str = "Understand the code you ship.";
/// The cells between the mark and the name beside it.
const NAME_GAP: u16 = 4;

/// Where data goes, said before anything is ever sent.
const OUTBOUND_NOTICE: &str =
    "Practice sends showcase excerpts and your answers to OpenRouter, paid with your credits.";

/// Home's own keys: `q` quits from here.
const KEYMAP: Keymap = &[keymap::MOVE, keymap::ACTIVATE, keymap::QUIT];

/// The home screen's state: its actions, and which has focus.
#[derive(Debug, Clone)]
pub(crate) struct Home {
    actions: Vec<Action>,
    focus: Position,
}

impl Home {
    /// Home with its actions: Practice on the learner's `repository` first,
    /// unless it is unavailable, then on the showcase project, Settings and
    /// Quit.
    pub(crate) fn new(repository: Result<&Repository, &Unavailable>) -> Self {
        let mut actions = Vec::new();
        if repository.is_ok() {
            actions.push(Action::Practice(Project::Repository));
        }
        actions.extend([
            Action::Practice(Project::Showcase),
            Action::Settings,
            Action::Quit,
        ]);
        Self {
            actions,
            focus: Position::default(),
        }
    }

    fn focused(&self) -> Action {
        self.actions[self.focus.index()]
    }

    /// The actions as rows `width` cells wide: each control, and under the
    /// learner's repository the notice that nothing is sent before review.
    fn action_rows<'a>(&self, context: &Context<'a>, width: u16) -> Vec<Line<'a>> {
        let theme = context.theme;
        let mut rows = Vec::new();
        for &action in &self.actions {
            let state = ControlState {
                focused: action == self.focused(),
                ..ControlState::default()
            };
            match action {
                Action::Practice(Project::Repository) => {
                    let name = context
                        .repository
                        .map_or("", |repository| repository.name());
                    rows.push(theme.control(format!("Practice on {name}"), state));
                    let indent = " ".repeat(NOTICE_INDENT);
                    rows.extend(
                        columns::wrap(
                            REPOSITORY_NOTICE,
                            usize::from(width).saturating_sub(NOTICE_INDENT),
                        )
                        .into_iter()
                        .map(|line| Line::styled(format!("{indent}{line}"), theme.muted)),
                    );
                }
                Action::Practice(Project::Showcase) => {
                    rows.push(theme.control("Practice on the showcase project", state));
                }
                Action::Settings => rows.push(theme.control("Settings", state)),
                Action::Quit => rows.push(theme.control("Quit", state)),
            }
        }
        rows
    }
}

/// Why Practice on the learner's repository is unavailable, as home says
/// it.
fn why(unavailable: &Unavailable) -> String {
    match unavailable {
        Unavailable::NotInRepository => {
            "To practice on your own work, start Whetstone inside a Git repository.".to_owned()
        }
        Unavailable::Bare { name } => {
            format!("{name} is a bare repository, so it has no working tree to practice on.")
        }
        Unavailable::NoCommits { name } => {
            format!("{name} has no commits yet, so there is nothing to practice on.")
        }
        Unavailable::InvalidConfig { name, problem } => format!(
            "Practice on {name} is unavailable because {} is invalid: {problem}.",
            repository::CONFIG
        ),
        Unavailable::Unreadable {
            name,
            path,
            problem,
        } => format!(
            "Practice on {name} is unavailable because {} cannot be read: {problem}.",
            path.display()
        ),
        Unavailable::Unusable { name } => {
            format!("{name} has a .git that Whetstone cannot open as a repository.")
        }
        Unavailable::Broken(error) => match error.source() {
            Some(cause) => format!("Whetstone cannot open the repository here: {cause}."),
            None => "Whetstone cannot open the repository here.".to_owned(),
        },
    }
}

impl View for Home {
    fn route(&self) -> Route {
        Route::Home
    }

    fn name(&self) -> &'static str {
        "Home"
    }

    fn keymap(&self) -> Keymap {
        KEYMAP
    }

    fn apply(&mut self, command: Command, _area: Rect) -> Option<Command> {
        self.focus = self.focus.after(command, self.actions.len());
        (command == Command::Activate).then(|| self.focused().command())
    }

    fn render(&self, context: &Context<'_>, frame: &mut Frame<'_>, area: Rect) {
        let theme = context.theme;
        let mut notices = Vec::new();
        if let Err(unavailable) = context.repository {
            notices.push(Line::styled(why(unavailable), theme.muted));
            notices.push(Line::default());
        }
        notices.push(Line::styled(OUTBOUND_NOTICE, theme.muted));
        let outbound = Paragraph::new(notices).wrap(Wrap { trim: true });
        let action_lines = self.action_rows(context, area.width);
        let actions_rows = u16::try_from(action_lines.len()).unwrap_or(u16::MAX);
        let outbound_rows = u16::try_from(outbound.line_count(area.width)).unwrap_or(u16::MAX);
        let name = Line::from(vec![
            Span::styled("whetstone", theme.title),
            Span::styled(" by Monadial", theme.muted),
        ]);
        let name_width =
            u16::try_from(name.width().max(Line::raw(CLAIM).width())).unwrap_or(u16::MAX);
        // The mark needs room beside it for the name, and above the rest.
        let with_mark = area.width >= mark::WIDTH + NAME_GAP + name_width
            && area.height >= mark::HEIGHT + 1 + actions_rows + 1 + outbound_rows;
        let [brand, _, actions, _, outbound_area] = Layout::vertical([
            Constraint::Length(if with_mark { mark::HEIGHT } else { 1 }),
            Constraint::Length(1),
            Constraint::Length(actions_rows),
            Constraint::Length(1),
            // The line wraps in narrower terminals.
            Constraint::Fill(1),
        ])
        .areas(area);

        if with_mark {
            let [stone, _, beside] = Layout::horizontal([
                Constraint::Length(mark::WIDTH),
                Constraint::Length(NAME_GAP),
                Constraint::Fill(1),
            ])
            .areas(brand);
            frame.render_widget(Paragraph::new(mark::text(theme.stone)), stone);
            frame.render_widget(
                Paragraph::new(vec![Line::default(), name, Line::raw(CLAIM)]),
                beside,
            );
        } else {
            // A short or narrow window keeps the name: the brand's text fallback.
            frame.render_widget(Paragraph::new(name), brand);
        }
        frame.render_widget(List::new(action_lines), actions);
        frame.render_widget(outbound, outbound_area);
    }
}
