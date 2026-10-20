//! Background jobs: blocking work, such as finding connected code, that runs
//! off the UI loop so the screen never freezes.
//!
//! The app starts a job with [`Jobs::start`]. The UI loop takes it with
//! [`App::take_jobs`](crate::App::take_jobs) and runs it on a blocking
//! thread with [`Workers`]. Its [`Finished`] report comes back through
//! [`App::handle`](crate::App::handle), the way a key press does.

use std::fmt;
use std::mem;

use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;
use whetstone_core::{CommitId, Connections};

/// A job's identity, unique among the jobs one [`Jobs`] starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JobId(u64);

/// What the app's background jobs produce. Each kind of job adds its
/// variant here, and the app applies it in [`App::handle`](crate::App::handle).
#[derive(Debug)]
pub enum JobOutput {
    /// A commit's connected code, found for its outbound review.
    ConnectedCode {
        /// The commit it is for.
        commit: CommitId,
        /// What was found, or why there is none.
        connections: Connections,
    },
}

/// How a job ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JobOutcome<T> {
    /// Its body returned this.
    Done(T),
    /// It was cancelled before it ended (rule 17).
    Cancelled,
}

/// A job's report, sent when it ends.
#[derive(Debug)]
pub struct Finished<T> {
    /// The job that ended.
    pub id: JobId,
    /// How it ended.
    pub outcome: JobOutcome<T>,
}

/// What a job's body returns when it stops early because its job was
/// cancelled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cancelled;

/// A job's blocking work. It can look at its token between steps and stop
/// early with [`Cancelled`].
type Body<T> = Box<dyn FnOnce(&CancellationToken) -> Result<T, Cancelled> + Send>;

/// A started job, waiting to run.
pub struct Job<T> {
    id: JobId,
    /// A child of its [`Jobs`]' token (rule 17).
    token: CancellationToken,
    body: Body<T>,
}

impl<T> Job<T> {
    /// Runs the job's body on this thread, and reports how it ended. A job
    /// cancelled before it runs never runs its body. One cancelled by the
    /// time its body returns reports cancellation, whatever the body
    /// returned.
    #[must_use]
    pub fn run(self) -> Finished<T> {
        let outcome = if self.token.is_cancelled() {
            JobOutcome::Cancelled
        } else {
            match (self.body)(&self.token) {
                Ok(output) if !self.token.is_cancelled() => JobOutcome::Done(output),
                Ok(_) | Err(Cancelled) => JobOutcome::Cancelled,
            }
        };
        Finished {
            id: self.id,
            outcome,
        }
    }
}

impl<T> fmt::Debug for Job<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Job")
            .field("id", &self.id)
            .field("token", &self.token)
            .finish_non_exhaustive()
    }
}

/// The app's side of its background jobs: it starts them, for the UI loop
/// to take and run, and accepts a result only from a job it still waits
/// for (rule 24). Dropping it, as quitting does, cancels every job.
#[derive(Debug)]
pub struct Jobs<T> {
    /// The parent of every job's token.
    root: CancellationToken,
    /// The identity the next job gets.
    next_id: u64,
    /// Jobs started since the UI loop last took them.
    started: Vec<Job<T>>,
    /// The jobs the app waits for, with their tokens: started, and neither
    /// cancelled nor accepted.
    outstanding: Vec<(JobId, CancellationToken)>,
}

impl<T> Jobs<T> {
    /// No jobs yet.
    #[must_use]
    pub fn new() -> Self {
        Self {
            root: CancellationToken::new(),
            next_id: 0,
            started: Vec::new(),
            outstanding: Vec::new(),
        }
    }

    /// Starts a job that runs `body` on a blocking thread, and returns its
    /// identity.
    pub fn start(
        &mut self,
        body: impl FnOnce(&CancellationToken) -> Result<T, Cancelled> + Send + 'static,
    ) -> JobId {
        let id = JobId(self.next_id);
        self.next_id += 1;
        let token = self.root.child_token();
        self.outstanding.push((id, token.clone()));
        self.started.push(Job {
            id,
            token,
            body: Box::new(body),
        });
        id
    }

    /// Cancels job `id`, if the app still waits for it. Its body may still
    /// run to its end, but it reports [`JobOutcome::Cancelled`].
    pub fn cancel(&mut self, id: JobId) {
        if let Some(token) = self.stop_waiting(id) {
            token.cancel();
        }
    }

    /// Rule 24: returns `finished` if the app still waits for its job, which
    /// it then no longer does. The result of a job it cancelled, or already
    /// accepted, is ignored.
    pub fn accept(&mut self, finished: Finished<T>) -> Option<Finished<T>> {
        self.stop_waiting(finished.id).map(|_| finished)
    }

    /// Stops waiting for job `id`, returning its token if the app waited for
    /// it.
    fn stop_waiting(&mut self, id: JobId) -> Option<CancellationToken> {
        let at = self.outstanding.iter().position(|(job, _)| *job == id)?;
        Some(self.outstanding.swap_remove(at).1)
    }

    /// The jobs started since the last take, for the UI loop to run.
    pub fn take_started(&mut self) -> Vec<Job<T>> {
        mem::take(&mut self.started)
    }
}

impl<T> Default for Jobs<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> Drop for Jobs<T> {
    fn drop(&mut self) {
        self.root.cancel();
    }
}

/// How many job results can wait in the channel for the UI loop (rule 21).
/// The loop takes each one as soon as it arrives, so the channel rarely holds
/// more than one. A job that finds it full waits on its own blocking thread
/// until the loop takes one, so the loop itself never waits.
const RESULTS_CAPACITY: usize = 4;

/// Runs jobs on Tokio's blocking threads and hands back their results: the
/// UI loop's side of background jobs.
#[derive(Debug)]
pub struct Workers<T> {
    /// Owns every running job (rule 16).
    tracker: TaskTracker,
    sender: mpsc::Sender<Finished<T>>,
    results: mpsc::Receiver<Finished<T>>,
}

impl<T: Send + 'static> Workers<T> {
    /// No jobs running yet.
    #[must_use]
    pub fn new() -> Self {
        let (sender, results) = mpsc::channel(RESULTS_CAPACITY);
        Self {
            tracker: TaskTracker::new(),
            sender,
            results,
        }
    }

    /// Runs `job` on a blocking thread (rule 19), and sends its report to
    /// [`Workers::finished`]. It must be called on a Tokio runtime.
    pub fn run(&self, job: Job<T>) {
        let sender = self.sender.clone();
        // The tracker owns the task, so its handle can go (rule 16).
        drop(self.tracker.spawn_blocking(move || {
            // Once the workers are gone nothing receives the report, which
            // is discarded.
            drop(sender.blocking_send(job.run()));
        }));
    }

    /// Whether every job these workers ran has ended.
    #[must_use]
    pub fn is_idle(&self) -> bool {
        self.tracker.is_empty()
    }

    /// The next report of a job these workers ran. It is cancel-safe (rule
    /// 18): a report is taken only when this returns.
    pub async fn finished(&mut self) -> Finished<T> {
        self.results
            .recv()
            .await
            .expect("the workers hold a sender, so their channel stays open")
    }
}

impl<T: Send + 'static> Default for Workers<T> {
    fn default() -> Self {
        Self::new()
    }
}
