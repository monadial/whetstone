//! Background jobs: blocking work, such as finding connected code, that runs
//! off the UI loop so the screen never freezes.
//!
//! The app starts a job with [`Jobs::start`]. The UI loop takes it with
//! [`App::take_jobs`](crate::App::take_jobs) and runs it on a blocking
//! thread with [`Workers`]. Its [`Finished`] report comes back through
//! [`App::handle`](crate::App::handle), the way a key press does.
//!
//! Jobs that would compete for the whole machine, as analyses do, start
//! with [`Jobs::start_one_at_a_time`] and take turns. The app can give up
//! waiting for a job after a [`TimeLimit`], which the UI loop times with
//! [`App::take_time_limits`](crate::App::take_time_limits), without
//! cancelling it.

use std::fmt;
use std::mem;
use std::time::Duration;

use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;
use whetstone_core::{CommitId, Connections};

use crate::connected_code::GraphOrigin;

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
        /// Whether its graph was analysed or came from the cache.
        origin: GraphOrigin,
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

/// How long the app waits for a job before it gives up on it: once
/// `duration` has passed from when it was set, unless the job has ended by
/// then, the UI loop hands the app [`Message::TimeUp`](crate::Message::TimeUp).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimeLimit {
    /// The job it is for.
    pub job: JobId,
    /// How long it runs, from when the app set it.
    pub duration: Duration,
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

/// Whose turn it is among the jobs started one at a time.
#[derive(Debug)]
enum Turn<T> {
    /// None of them runs.
    Free,
    /// The job `by` runs, from when it is handed to the UI loop until its
    /// report comes back, and `next`, the newest started since, waits for
    /// it, if one was.
    Taken { by: JobId, next: Option<Job<T>> },
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
    /// cancelled, forgotten nor accepted.
    outstanding: Vec<(JobId, CancellationToken)>,
    /// Whose turn it is among the jobs started one at a time.
    turn: Turn<T>,
    /// Time limits set since the UI loop last took them.
    time_limits: Vec<TimeLimit>,
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
            turn: Turn::Free,
            time_limits: Vec::new(),
        }
    }

    /// Starts a job that runs `body` on a blocking thread, and returns its
    /// identity.
    pub fn start(
        &mut self,
        body: impl FnOnce(&CancellationToken) -> Result<T, Cancelled> + Send + 'static,
    ) -> JobId {
        let job = self.job(body);
        let id = job.id;
        self.started.push(job);
        id
    }

    /// Starts a job as [`Jobs::start`] does, except that jobs started this
    /// way take turns. While another has its turn, this one waits until that
    /// one's report comes back, whether the app still waits for it or not.
    /// Only the newest such job waits: one already waiting is cancelled, so
    /// it reports cancellation without running its body.
    pub fn start_one_at_a_time(
        &mut self,
        body: impl FnOnce(&CancellationToken) -> Result<T, Cancelled> + Send + 'static,
    ) -> JobId {
        let job = self.job(body);
        let id = job.id;
        match &mut self.turn {
            Turn::Free => {
                self.turn = Turn::Taken { by: id, next: None };
                self.started.push(job);
            }
            Turn::Taken { next, .. } => {
                if let Some(replaced) = next.replace(job) {
                    self.stop_waiting(replaced.id);
                    replaced.token.cancel();
                    self.started.push(replaced);
                }
            }
        }
        id
    }

    /// A new job that runs `body`, which the app waits for.
    fn job(
        &mut self,
        body: impl FnOnce(&CancellationToken) -> Result<T, Cancelled> + Send + 'static,
    ) -> Job<T> {
        let id = JobId(self.next_id);
        self.next_id += 1;
        let token = self.root.child_token();
        self.outstanding.push((id, token.clone()));
        Job {
            id,
            token,
            body: Box::new(body),
        }
    }

    /// Sets a time limit of `duration`, from now, on job `id`, for the UI
    /// loop to time.
    pub fn limit(&mut self, id: JobId, duration: Duration) {
        self.time_limits.push(TimeLimit { job: id, duration });
    }

    /// Cancels job `id`, if the app still waits for it. Its body may still
    /// run to its end, but it reports [`JobOutcome::Cancelled`].
    pub fn cancel(&mut self, id: JobId) {
        if let Some(token) = self.stop_waiting(id) {
            token.cancel();
        }
    }

    /// Stops waiting for job `id` without cancelling it, and says whether
    /// the app waited for it. The job runs to its end, as it would have, and
    /// its report is ignored.
    pub fn forget(&mut self, id: JobId) -> bool {
        self.stop_waiting(id).is_some()
    }

    /// Rule 24: returns `finished` if the app still waits for its job, which
    /// it then no longer does. The result of a job it cancelled, forgot or
    /// already accepted is ignored. Every report comes here, so a job's end
    /// passes its turn on, if it had one, however it ended.
    pub fn accept(&mut self, finished: Finished<T>) -> Option<Finished<T>> {
        if let Turn::Taken { by, next } = &mut self.turn
            && *by == finished.id
        {
            self.turn = match next.take() {
                Some(next) => {
                    let by = next.id;
                    self.started.push(next);
                    Turn::Taken { by, next: None }
                }
                None => Turn::Free,
            };
        }
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

    /// The time limits set since the last take, for the UI loop to time.
    pub fn take_time_limits(&mut self) -> Vec<TimeLimit> {
        mem::take(&mut self.time_limits)
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
