//! The commits reachable from HEAD as it was at startup, newest first, in
//! `git log` order, read a page at a time as the learner scrolls, so the
//! picker opens at once on any history.

use std::fmt;

use gix::traverse::commit::simple::{CommitTimeOrder, Sorting};
use whetstone_core::{CommitId, Comparison};

use super::{HistoryError, commit_id};

/// How many commits are read at a time: when the picker opens, and each
/// time the learner reaches the last one read.
const PAGE: usize = 50;

/// The walk through the history. It owns what it reads from, so the picker
/// can keep it and read on. It skips commits that are not in the
/// repository, as a shallow clone's parents are not.
type Walk = gix::traverse::commit::Simple<gix::OdbHandle, Box<dyn FnMut(&gix::oid) -> bool>>;

/// The commits read so far, and the walk that reads on.
pub(crate) struct History {
    /// The repository, opened read-only, to read each commit's message.
    git: gix::Repository,
    walk: Walk,
    /// How many digits short IDs show.
    short_len: usize,
    commits: Vec<Commit>,
    end: HistoryEnd,
}

/// What follows the commits read so far.
#[derive(Debug)]
pub(crate) enum HistoryEnd {
    /// More commits may follow.
    More,
    /// None: every commit reachable from HEAD has been read.
    Reached,
    /// Reading the next commit failed, so no more are read.
    Failed(HistoryError),
}

/// A commit as the picker lists it.
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct Commit {
    id: CommitId,
    /// The commit message's subject, as `git log --oneline` shows it.
    summary: String,
    /// The whole commit message, for searching.
    message: String,
    availability: Availability,
}

/// Shows the message's length, never its text, which may hold a secret.
impl fmt::Debug for Commit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Commit")
            .field("id", &self.id)
            .field("message_bytes", &self.message.len())
            .field("availability", &self.availability)
            .finish_non_exhaustive()
    }
}

/// Whether a commit can be opened, and what it is compared with if it can.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Availability {
    /// It can be opened, compared as this says.
    Compared(Comparison),
    /// Its parent is not in the repository, as at a shallow clone's
    /// boundary, so there is nothing to compare it with.
    ParentMissing,
}

impl History {
    /// The history from `head`, with its first page read.
    pub(super) fn read_from(
        git: &gix::Repository,
        head: gix::ObjectId,
    ) -> Result<Self, HistoryError> {
        use gix::prelude::ObjectIdExt as _;

        // Shortening each ID would search all objects for every one, so
        // the length is found once, for HEAD, and used for each row.
        let short_len = head.attach(git).shorten()?.hex_len();
        let present = git.objects.clone();
        let walk = Walk::filtered(
            [head],
            git.objects.clone(),
            Box::new(move |id: &gix::oid| gix::objs::Exists::exists(&present, id)),
        )
        .sorting(Sorting::ByCommitTime(CommitTimeOrder::NewestFirst))?;
        let mut history = Self {
            git: git.clone(),
            walk,
            short_len,
            commits: Vec::new(),
            end: HistoryEnd::More,
        };
        history.read_page();
        tracing::info!(
            commits = history.commits.len(),
            %head,
            "read the first commits"
        );
        Ok(history)
    }

    /// The commits read so far, newest first.
    pub(crate) fn commits(&self) -> &[Commit] {
        &self.commits
    }

    /// What follows the commits read so far.
    pub(crate) fn end(&self) -> &HistoryEnd {
        &self.end
    }

    /// Reads up to a page more commits, unless every commit has been read
    /// or reading failed, as [`History::end`] then says.
    pub(crate) fn read_page(&mut self) {
        let before = self.commits.len();
        while matches!(self.end, HistoryEnd::More) && self.commits.len() < before + PAGE {
            match self.walk.next() {
                None => self.end = HistoryEnd::Reached,
                Some(Err(error)) => self.end = HistoryEnd::Failed(error.into()),
                Some(Ok(info)) => match self.commit(&info) {
                    Ok(commit) => self.commits.push(commit),
                    Err(error) => self.end = HistoryEnd::Failed(error),
                },
            }
        }
        if let HistoryEnd::Failed(error) = &self.end {
            tracing::warn!(%error, read = self.commits.len(), "cannot read further commits");
        }
    }

    /// The row of the commit the walk found as `info`.
    fn commit(&self, info: &gix::traverse::commit::Info) -> Result<Commit, HistoryError> {
        let found = self.git.find_commit(info.id)?;
        let message = found.message()?;
        // Only the first parent is compared with, so only it must be there.
        let availability = if info
            .parent_ids
            .first()
            .is_some_and(|parent| !self.git.has_object(parent))
        {
            Availability::ParentMissing
        } else {
            Availability::Compared(match info.parent_ids.len() {
                0 => Comparison::EmptyTree,
                1 => Comparison::Parent,
                _ => Comparison::FirstParent,
            })
        };
        Ok(Commit {
            id: commit_id(info.id, self.short_len),
            summary: message.summary().to_string(),
            message: found.message_raw_sloppy().to_string(),
            availability,
        })
    }
}

impl fmt::Debug for History {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("History")
            .field("short_len", &self.short_len)
            .field("commits", &self.commits)
            .field("end", &self.end)
            .finish_non_exhaustive()
    }
}

impl Commit {
    /// The commit's ID.
    pub(crate) fn id(&self) -> CommitId {
        self.id
    }

    /// The commit message's subject.
    pub(crate) fn summary(&self) -> &str {
        &self.summary
    }

    /// The whole commit message.
    pub(crate) fn message(&self) -> &str {
        &self.message
    }

    /// Whether it can be opened, and what it is compared with.
    pub(crate) fn availability(&self) -> Availability {
        self.availability
    }
}
