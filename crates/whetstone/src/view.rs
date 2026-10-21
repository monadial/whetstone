//! What every screen provides. A new screen implements [`View`] in its own
//! module and gets a [`Route`], and the app, key help and the palette pick
//! it up from there.

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use ratatui::Frame;
use ratatui::layout::Rect;
use whetstone_ai::Ai;
use whetstone_core::{Change, CommitId, Comparison, FileIndex, ShowcaseProject};

use crate::config::Settings;
use crate::connected_code::{AnalysisDirs, GraphCache};
use crate::failures::RefusedModels;
use crate::home::Home;
use crate::job::{JobId, JobOutput, Jobs};
use crate::keymap::{Command, Input, Keymap};
use crate::palette::Action;
use crate::picker::Picker;
use crate::repository::{Repository, Unavailable};
use crate::sent_requests::RequestIds;
use crate::session_record::SessionStore;
use crate::settings::SettingsScreen;
use crate::theme::Theme;
use crate::viewer::{Unreadable, Viewer};

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

    /// The screen `route` names, built from this screen's state, if this
    /// screen builds it: the picker builds the outbound review and Practice
    /// from its choices, the review builds Practice from the evidence it
    /// shows and what it lets the learner read, and Practice's questions
    /// build what they let the learner read. Every other route opens from
    /// the route alone.
    fn open(&self, _route: Route, _context: &Context<'_>) -> Option<Box<dyn View>> {
        None
    }

    /// Starts the background jobs the screen needs now with `jobs`, keeping
    /// their identities to wait for. The app calls it when the screen
    /// opens, and again each time the screen has applied a command, a job's
    /// result or the end of a time limit, so a screen starts a job as soon
    /// as its state needs one, such as a check for each candidate once they
    /// are written, or a Retry. A job is started only once.
    fn start_jobs(&mut self, _jobs: &mut Jobs<JobOutput>) {}

    /// Saves to `store` what the screen's state now holds for the session
    /// record, such as a record to start once questions pass their checks,
    /// or a response the learner submitted. The app calls it each time the
    /// screen has applied a command, a job's result or the end of a time
    /// limit, before it starts the screen's jobs, so a response is saved
    /// before anything else is done with it. Each is saved only once.
    fn keep_record(&mut self, _store: &mut dyn SessionStore) {}

    /// Closes the screen, as the learner leaves it for the screen before it
    /// or for home, with `jobs` to stop the jobs it waits for. By default
    /// they run on and their results are never applied, as the outbound
    /// review lets an analysis run on to be cached for the commit's next
    /// review. A screen whose requests the learner pays for cancels them.
    fn close(&mut self, _jobs: &mut Jobs<JobOutput>) {}

    /// Whether leaving the screen would lose a draft the learner wrote,
    /// such as an unsubmitted response, so the app asks first.
    fn holds_draft(&self) -> bool {
        false
    }

    /// Whether the screen waits for the result of the job `job`.
    fn waits_for(&self, _job: JobId) -> bool {
        false
    }

    /// Applies `output`, the result of the job `job`, which the screen
    /// waits for (rule 24). The app calls it only when
    /// [`View::waits_for`] says so.
    fn finish_job(&mut self, _job: JobId, _output: JobOutput) {}

    /// Carries on without the result of the job `job`, which the screen
    /// waited for until its time limit ran out. The job runs on to its end,
    /// and its result is never applied. The app calls it only when
    /// [`View::waits_for`] says so.
    fn time_up(&mut self, _job: JobId) {}

    /// Inserts pasted `text` into the focused text entry as one insertion.
    /// The app calls it only while [`View::input`] is text, so a screen
    /// with a text entry implements both.
    fn paste(&mut self, _text: &str) {}

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
    /// The learner's repository, or why Practice on it is unavailable.
    pub(crate) repository: Result<&'a Repository, &'a Unavailable>,
    /// Where connected code is analysed.
    pub(crate) analysis: &'a AnalysisDirs,
    /// How many times the UI loop has ticked, for a spinner's frame.
    pub(crate) ticks: usize,
    /// The graphs connected code was found in before.
    pub(crate) cache: &'a Arc<GraphCache>,
    /// How long the outbound review, or Preparing questions on the showcase
    /// project, waits for connected code, from when it opens, before it
    /// carries on without it.
    pub(crate) time_limit: Duration,
    /// Jev and the generator, if Practice can ask them.
    pub(crate) ai: Option<&'a Arc<dyn Ai>>,
    /// The models `OpenRouter` refused during this run for having no
    /// zero-retention endpoint.
    pub(crate) refused_models: &'a Arc<RefusedModels>,
    /// Whetstone's identities for the requests sent during this run.
    pub(crate) request_ids: &'a Arc<RequestIds>,
}

/// What Practice runs on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Project {
    /// The showcase project that ships with Whetstone.
    Showcase,
    /// The learner's repository, opened at startup.
    Repository,
}

/// A screen a learner can go to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Route {
    Home,
    Settings,
    /// The area and change picker over a project.
    Picker(Project),
    /// The change viewer, showing one change.
    Viewer(ChangeRef),
    /// The outbound review of a commit from the learner's repository, which
    /// the picker opens.
    OutboundReview(CommitId),
    /// Something the outbound review lets the learner read whole.
    Read(Reading),
    /// Practice on a change: Preparing questions, then the questions. The
    /// picker opens it on a showcase change, and the outbound review's Send
    /// on a commit from the learner's repository, in the review's place.
    Practice(ChangeRef),
}

/// What the outbound review, or Practice's questions, let the learner read
/// whole.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Reading {
    /// The change: its full commit ID, and its message as it is sent.
    Change,
    /// What the touched file at this index sends.
    File(FileIndex),
    /// The area prompt of the selected area at this index.
    Area(usize),
    /// What the connection at this index in the evidence sends.
    Connection(usize),
    /// Everything Practice sent as the evidence its questions rest on,
    /// which the questions screen and feedback let the learner inspect.
    Evidence,
    /// The answer key, the rationales, "why it matters" and the rubric of
    /// the question Practice shows, which Show answer reveals.
    Answer,
    /// Each of Jev's answers about the question whose feedback shows, with
    /// its number, which Inspect Jev's answers opens.
    JevAnswers,
}

/// A change the viewer can show.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ChangeRef {
    /// One of the showcase project's curated changes, by its place in the
    /// history.
    Showcase(ChangeIndex),
    /// A commit from the learner's repository, by its ID.
    Commit(CommitId),
}

/// One of the showcase project's curated changes, by its place in the
/// history, oldest first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ChangeIndex(usize);

impl ChangeIndex {
    /// The change at `index` in the history, oldest first.
    pub(crate) const fn new(index: usize) -> Self {
        Self(index)
    }

    /// Its place in the history, oldest first.
    pub(crate) const fn get(self) -> usize {
        self.0
    }

    /// The change in `showcase`, if it has one here.
    pub(crate) fn of(self, showcase: &ShowcaseProject) -> Option<&Change> {
        showcase.changes().get(self.0)
    }
}

/// Why Generate questions and the outbound review's Send can't start
/// Practice: no `OpenRouter` key was found, so Whetstone can't ask Jev and
/// the generator.
pub(crate) const NEEDS_KEY: &str = "needs OPENROUTER_API_KEY";

/// What a commit's row in the picker and its title in the viewer say about
/// what it was compared with, unless it was simply its parent.
pub(crate) fn comparison_label(comparison: Comparison) -> Option<&'static str> {
    match comparison {
        Comparison::Parent => None,
        // Its change is the whole merged work.
        Comparison::FirstParent => Some("merge · vs first parent"),
        Comparison::EmptyTree => Some("first commit"),
    }
}

impl Route {
    /// The screen at this route, as it first opens over the showcase project
    /// or the repository in `context`, unless only the screen before it can
    /// build it, as [`View::open`] does.
    pub(crate) fn open(self, context: &Context<'_>) -> Option<Box<dyn View>> {
        let Context {
            showcase,
            repository,
            ..
        } = *context;
        Some(match self {
            Self::Home => Box::new(Home::new(repository)),
            Self::Settings => Box::new(SettingsScreen::default()),
            Self::Picker(Project::Showcase) => {
                Box::new(Picker::new(showcase, context.ai.is_some()))
            }
            Self::Picker(Project::Repository) => Box::new(Picker::for_repository(
                repository.expect("home offers the repository only when it is available"),
            )),
            Self::Viewer(at @ ChangeRef::Showcase(index)) => Box::new(Viewer::new(
                at,
                index
                    .of(showcase)
                    .expect("only the picker opens a change, and only one it lists")
                    .clone(),
            )),
            Self::Viewer(at @ ChangeRef::Commit(id)) => {
                let repository =
                    repository.expect("only the repository's picker opens one of its commits");
                match repository.load(id) {
                    Ok(change) => Box::new(Viewer::new(at, change)),
                    Err(error) => Box::new(Unreadable::new(self, id, &error)),
                }
            }
            Self::OutboundReview(_) | Self::Read(_) | Self::Practice(_) => return None,
        })
    }
}
