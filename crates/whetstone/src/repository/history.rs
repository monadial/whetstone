//! The commits reachable from HEAD as it was at startup, newest first, in
//! `git log` order, read a page at a time as the learner scrolls, so the
//! picker opens at once on any history. A background job can take the
//! reader to read on, as the picker's filter does, and give back what it
//! read.

use std::fmt;

use gix::traverse::commit::simple::{CommitTimeOrder, Sorting};
use tokio_util::sync::CancellationToken;
use whetstone_core::{CommitId, Comparison};

use super::{HistoryError, commit_id};
use crate::job::Cancelled;

/// How many commits are read at a time: when the picker opens, and each
/// time the learner reaches the last one read.
const PAGE: usize = 50;

/// The walk through the history. It owns what it reads from, so the picker
/// can keep it and read on, on its own thread or a job's. It skips commits
/// that are not in the repository, as a shallow clone's parents are not.
type Walk = gix::traverse::commit::Simple<gix::OdbHandle, Box<dyn FnMut(&gix::oid) -> bool + Send>>;

/// The commits read so far, and what reads on.
pub(crate) struct History {
    /// What reads on, unless a background read has it.
    reader: Option<Reader>,
    commits: Vec<Commit>,
    end: HistoryEnd,
}

/// What reads the history on: the walk, and the repository it reads each
/// commit's message from.
pub(crate) struct Reader {
    /// The repository, opened read-only, to read each commit's message.
    git: gix::Repository,
    walk: Walk,
    /// How many digits short IDs show.
    short_len: usize,
}

/// What a background read found: the commits it read, in order, what
/// follows them, and the reader to give back.
pub struct Read {
    reader: Reader,
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
            reader: Some(Reader {
                git: git.clone(),
                walk,
                short_len,
            }),
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

    /// Reads up to a page more commits, unless every commit has been read,
    /// reading failed, as [`History::end`] then says, or a background read
    /// has the reader.
    pub(crate) fn read_page(&mut self) {
        if let Some(reader) = &mut self.reader {
            reader.read_into(&mut self.commits, &mut self.end, PAGE, |_| false);
        }
    }

    /// Reads pages until `count` commits have been read, every commit
    /// has, or reading failed, as [`History::end`] then says, unless a
    /// background read has the reader.
    pub(crate) fn read_until(&mut self, count: usize) {
        while self.commits.len() < count
            && matches!(self.end, HistoryEnd::More)
            && self.reader.is_some()
        {
            self.read_page();
        }
    }

    /// Takes the reader for a background read, if more commits may follow
    /// and no background read has it already.
    pub(crate) fn take_reader(&mut self) -> Option<Reader> {
        match self.end {
            HistoryEnd::More => self.reader.take(),
            HistoryEnd::Reached | HistoryEnd::Failed(_) => None,
        }
    }

    /// Adds what a background read found after the commits read so far,
    /// and takes its reader back.
    pub(crate) fn add(&mut self, read: Read) {
        self.commits.extend(read.commits);
        self.end = read.end;
        self.reader = Some(read.reader);
    }
}

impl Reader {
    /// Reads on, a page at a time, until `enough` says a commit read is
    /// the last needed, `limit` commits are read, every commit is, or
    /// reading fails. Between pages it stops early, with [`Cancelled`],
    /// once `token` is cancelled, as the app quits.
    pub(crate) fn read(
        mut self,
        token: &CancellationToken,
        limit: usize,
        mut enough: impl FnMut(&Commit) -> bool,
    ) -> Result<Read, Cancelled> {
        let mut commits = Vec::new();
        let mut end = HistoryEnd::More;
        let mut done = false;
        while !done && matches!(end, HistoryEnd::More) && commits.len() < limit {
            if token.is_cancelled() {
                return Err(Cancelled);
            }
            let page = PAGE.min(limit - commits.len());
            done = self.read_into(&mut commits, &mut end, page, &mut enough);
        }
        Ok(Read {
            reader: self,
            commits,
            end,
        })
    }

    /// Reads up to `count` more commits into `commits`, setting `end` when
    /// none follow or reading fails, and stops after a commit `enough` says
    /// is the last needed. Returns whether it stopped there.
    fn read_into(
        &mut self,
        commits: &mut Vec<Commit>,
        end: &mut HistoryEnd,
        count: usize,
        mut enough: impl FnMut(&Commit) -> bool,
    ) -> bool {
        let before = commits.len();
        while matches!(end, HistoryEnd::More) && commits.len() < before + count {
            match self.walk.next() {
                None => *end = HistoryEnd::Reached,
                Some(Err(error)) => *end = HistoryEnd::Failed(error.into()),
                Some(Ok(info)) => match self.commit(&info) {
                    Ok(commit) => {
                        let last = enough(&commit);
                        commits.push(commit);
                        if last {
                            return true;
                        }
                    }
                    Err(error) => *end = HistoryEnd::Failed(error),
                },
            }
        }
        if let HistoryEnd::Failed(error) = end {
            tracing::warn!(%error, read = commits.len(), "cannot read further commits");
        }
        false
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
            .field("reading_in_the_background", &self.reader.is_none())
            .field("commits", &self.commits)
            .field("end", &self.end)
            .finish()
    }
}

impl fmt::Debug for Reader {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Reader")
            .field("short_len", &self.short_len)
            .finish_non_exhaustive()
    }
}

impl fmt::Debug for Read {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Read")
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
