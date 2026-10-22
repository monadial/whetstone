//! The single UI loop: it owns the terminal while the app runs, and gives it
//! back while the app is suspended.
//!
//! It runs on a current-thread Tokio runtime and waits for terminal input,
//! signals, background job results, the time limits the app set on its jobs
//! and, while a job runs, a tick. Nothing else wakes it.

use std::ffi::c_int;
use std::future::{self, Future};
use std::io;
use std::marker::PhantomData;
#[cfg(test)]
use std::os::unix::thread::{JoinHandleExt as _, RawPthread};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, mpsc};
use std::task::Poll;
use std::thread;
use std::time::Duration;

use futures_util::StreamExt as _;
use nix::sys::signal::{SigSet, SigmaskHow, Signal as NixSignal};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::crossterm::event::{Event, EventStream};
use ratatui::crossterm::style;
use ratatui::layout::{Position, Rect, Size};
use rustix::process::{self, Signal};
use signal_hook::SigId;
use signal_hook::consts::{SIGCONT, SIGHUP, SIGINT, SIGTERM, SIGTSTP};
use tokio::signal::unix::{self as unix_signal, SignalKind};
use tokio::time::{self, Instant, Interval, MissedTickBehavior};

use crate::app::{App, Flow, Message};
use crate::environment::Environment;
use crate::job::{JobId, JobOutput, TimeLimit, Workers};
use crate::terminal::{CrosstermSwitch, TerminalGuard, TerminalMode};

/// How often the screen is drawn while a background job runs, so what shows
/// it running can move.
const TICK: Duration = Duration::from_millis(100);

/// The signals that stop the app, as Ctrl+C does.
const STOP_SIGNALS: [c_int; 3] = [SIGHUP, SIGINT, SIGTERM];

/// The signals that suspend the app, as Ctrl+Z does, and continue it.
const JOB_CONTROL_SIGNALS: [c_int; 2] = [SIGTSTP, SIGCONT];

/// Every signal the loop acts on.
fn the_loops_signals() -> impl Iterator<Item = c_int> {
    STOP_SIGNALS.into_iter().chain(JOB_CONTROL_SIGNALS)
}

/// Why the UI loop ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Ending {
    /// The learner quit.
    Quit,
    /// A signal from outside the terminal stopped the app.
    Stopped { signal: c_int },
}

/// The terminal could not be used.
#[derive(Debug, thiserror::Error)]
pub(crate) enum TuiError {
    #[error("cannot start the async runtime")]
    Runtime(#[source] io::Error),
    #[error("cannot listen for signals")]
    Signals(#[source] io::Error),
    #[error("cannot take over the terminal")]
    Enable(#[source] io::Error),
    #[error("cannot draw to the terminal")]
    Draw(#[source] io::Error),
    #[error("cannot read terminal input")]
    Input(#[source] io::Error),
    #[error("cannot restore the terminal")]
    Restore(#[source] io::Error),
    #[error("cannot suspend")]
    Suspend(#[source] io::Error),
}

/// Takes over the terminal, runs `app` until it quits or a signal stops it,
/// then gives the terminal back exactly as it was. Suspending gives the
/// terminal back until the process is continued. `environment` can set the
/// [`CrashTrigger`].
pub(crate) fn run(app: App, environment: &Environment) -> Result<Ending, TuiError> {
    Runtime::new()
        .map_err(TuiError::Runtime)?
        .block_on(run_loop(app, environment))
}

async fn run_loop(mut app: App, environment: &Environment) -> Result<Ending, TuiError> {
    let crash = CrashTrigger::read(environment);
    // The theme alone honours NO_COLOR. Under NO_COLOR crossterm 0.29 writes
    // each colour, even a reset, as a bare `CSI m`, which also clears the
    // bold and other attributes that mark focus without colour.
    style::force_color_output(true);
    // Listen before taking over the terminal, so no signal can leave it in
    // raw mode.
    let mut signals = Signals::listen().map_err(TuiError::Signals)?;
    let mut guard = TerminalGuard::enable(CrosstermSwitch::new(io::stdout()), &TerminalMode::TUI)
        .map_err(TuiError::Enable)?;
    let mut terminal =
        Terminal::new(CrosstermBackend::new(io::stdout())).map_err(TuiError::Draw)?;
    // The terminal reports later sizes as resize events, but not the first.
    report_size(&terminal, &mut app)?;
    let mut events = EventStream::new();
    let mut workers = Workers::new();
    let mut time_limits = TimeLimits::default();
    let mut ticker = Ticker::default();
    let ending = loop {
        for job in app.take_jobs() {
            workers.run(job);
        }
        // Each counted from now, as the app has just set it.
        time_limits.start(app.take_time_limits());
        ticker.run_while(!workers.is_idle());
        terminal
            .draw(|frame| app.render(frame))
            .map_err(TuiError::Draw)?;
        crash.after_draw();
        let input = next_input(
            &mut signals,
            &mut events,
            &mut workers,
            &mut time_limits,
            &mut ticker,
        );
        let flow = match input.await? {
            Input::Message(message) => app.handle(message),
            Input::Stop { signal } => break Ending::Stopped { signal },
            Input::Suspend => Flow::Suspend,
            // Continued without suspending first, perhaps after a stop the
            // app could not catch. Taking the terminal back again is
            // harmless, but as after a suspend, a stop signal received while
            // stopped ends the app without it, and is recorded only once the
            // signal thread catches up.
            Input::Continue => {
                signals.thread.catch_up().map_err(TuiError::Signals)?;
                if let Some(signal) = signals.stop.last() {
                    break Ending::Stopped { signal };
                }
                resume(&mut guard, &mut terminal, &mut app)?;
                Flow::Continue
            }
        };
        match flow {
            Flow::Continue => {}
            Flow::Quit => break Ending::Quit,
            Flow::Suspend => {
                let suspended = Instant::now();
                guard.suspend().map_err(TuiError::Restore)?;
                // A stop signal received before the stop, or while stopped,
                // ends the app without taking the terminal back.
                if signals.stop.last().is_none() {
                    stop_until_continued().map_err(TuiError::Suspend)?;
                    // Every signal received before the continue is recorded
                    // only once the signal thread catches up.
                    signals.thread.catch_up().map_err(TuiError::Signals)?;
                    // The SIGCONT that continued the app is handled here,
                    // but a SIGTSTP after it is not.
                    signals.job_control.forget(SIGCONT);
                }
                if let Some(signal) = signals.stop.last() {
                    break Ending::Stopped { signal };
                }
                resume(&mut guard, &mut terminal, &mut app)?;
                // A stopped app's jobs are stopped too, so the time it spent
                // suspended never counts towards a time limit.
                time_limits.postpone(suspended.elapsed());
            }
        }
    };
    drop(terminal);
    guard.restore().map_err(TuiError::Restore)?;
    // Quitting never waits for a background job. Dropping the app cancels
    // every job's token, and dropping the workers stops listening for
    // results, so a job still running on its blocking thread has its result
    // discarded when it ends, and one waiting for its turn never runs. The
    // runtime shuts down without waiting for it (see `Runtime`). A running
    // async job, such as a network request, is dropped with the runtime
    // where it waits, so its request is abandoned and its result, which
    // nothing would receive, is discarded.
    drop(app);
    drop(workers);
    Ok(ending)
}

/// The UI loop's current-thread Tokio runtime. It shuts down without
/// waiting for a background job, as the loop ends, fails or panics: a plain
/// Tokio runtime waits for every blocking thread when it is dropped. An
/// async job's task is dropped where it waits.
#[derive(Debug)]
struct Runtime(Option<tokio::runtime::Runtime>);

impl Runtime {
    fn new() -> io::Result<Self> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        Ok(Self(Some(runtime)))
    }

    fn block_on<F: Future>(&self, future: F) -> F::Output {
        self.0
            .as_ref()
            .expect("the runtime is there until it is dropped")
            .block_on(future)
    }
}

impl Drop for Runtime {
    fn drop(&mut self) {
        if let Some(runtime) = self.0.take() {
            runtime.shutdown_background();
        }
    }
}

/// Tells `app` the size `terminal` has now, and returns it.
fn report_size(
    terminal: &Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
) -> Result<Size, TuiError> {
    let size = terminal.size().map_err(TuiError::Draw)?;
    app.handle(Message::Terminal(Event::Resize(size.width, size.height)));
    Ok(size)
}

/// Stops the app's process group, as Ctrl+Z does outside raw mode, and
/// returns once it is continued, usually by the learner's `fg`. If no shell
/// could continue it, it carries on at once instead, as vim does.
fn stop_until_continued() -> io::Result<()> {
    if process::getsid(None)? == process::getpgrp() {
        // The app's process group is its session's own, as when it runs
        // straight under tmux or `ssh -t`, so no job-control shell is there
        // to bring it back.
        tracing::info!("whetstone carries on, since no shell could continue it");
        return Ok(());
    }
    // SIGSTOP, since the app handles SIGTSTP itself. The whole group, so a
    // wrapper waiting for the app, such as `cargo run`, stops too, and the
    // shell takes the terminal back.
    process::kill_current_process_group(Signal::STOP)?;
    Ok(())
}

/// Takes the terminal back once the process is continued: turns every mode
/// off and on again, since the shell may have changed the terminal's
/// settings while the app was stopped, reads the terminal's size again,
/// since the window may have been resized, and clears it, so the next frame
/// draws the same screen in full.
fn resume(
    guard: &mut TerminalGuard,
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
) -> Result<(), TuiError> {
    guard.suspend().map_err(TuiError::Restore)?;
    guard.resume().map_err(TuiError::Enable)?;
    let size = report_size(terminal, app)?;
    // Unlike `Terminal::clear`, resizing clears without asking the terminal
    // where its cursor is, a reply that could be lost or take a while.
    terminal
        .resize(Rect::from((Position::ORIGIN, size)))
        .map_err(TuiError::Draw)
}

#[derive(Debug)]
enum Input {
    /// A terminal event, a background job's result, the end of a job's time
    /// limit or a tick, for the app.
    Message(Message),
    /// SIGHUP, SIGINT or SIGTERM.
    Stop { signal: c_int },
    /// SIGTSTP.
    Suspend,
    /// SIGCONT, other than the one that ends a suspend the loop handled
    /// itself.
    Continue,
}

/// Waits for the next terminal event, the loop's only input source (rule
/// 26), for a signal, for a background job's result, for a job's time limit
/// to run out, or for a tick while a job runs.
async fn next_input(
    signals: &mut Signals,
    events: &mut EventStream,
    workers: &mut Workers<JobOutput>,
    time_limits: &mut TimeLimits,
    ticker: &mut Ticker,
) -> Result<Input, TuiError> {
    loop {
        if let Some(signal) = signals.stop.last() {
            return Ok(Input::Stop { signal });
        }
        match signals.job_control.take() {
            Some(SIGTSTP) => return Ok(Input::Suspend),
            Some(_) => return Ok(Input::Continue),
            None => {}
        }
        // Every branch is cancel-safe (rule 18): one that loses takes
        // nothing.
        tokio::select! {
            // A signal is acted on before anything that arrives with it.
            biased;
            () = signals.arrival() => {}
            event = events.next() => match event {
                Some(Ok(event)) => return Ok(Input::Message(Message::Terminal(event))),
                Some(Err(error)) if error.kind() == io::ErrorKind::Interrupted => {}
                Some(Err(error)) => return Err(TuiError::Input(error)),
                None => return Err(TuiError::Input(io::ErrorKind::UnexpectedEof.into())),
            },
            finished = workers.finished() => {
                return Ok(Input::Message(Message::Finished(finished)));
            }
            // After a result, so one that arrives in time is applied first.
            job = time_limits.run_out() => return Ok(Input::Message(Message::TimeUp(job))),
            () = ticker.tick() => return Ok(Input::Message(Message::Tick)),
        }
    }
}

/// The time limits the app set on its jobs, each with when it runs out.
#[derive(Debug, Default)]
struct TimeLimits(Vec<(Instant, JobId)>);

impl TimeLimits {
    /// Starts counting each of `limits` from now.
    fn start(&mut self, limits: Vec<TimeLimit>) {
        let now = Instant::now();
        self.0.extend(
            limits
                .into_iter()
                .map(|limit| (now + limit.duration, limit.job)),
        );
    }

    /// Moves every time limit's end `by` later.
    fn postpone(&mut self, by: Duration) {
        for (runs_out, _) in &mut self.0 {
            *runs_out += by;
        }
    }

    /// The job whose time limit runs out next, once it has, which never
    /// comes while no time limit is set. It is cancel-safe (rule 18): a
    /// limit is taken only when this returns. A job that has ended still
    /// has its limit run out, which the app ignores.
    async fn run_out(&mut self) -> JobId {
        let Some((at, &(runs_out, _))) = self
            .0
            .iter()
            .enumerate()
            .min_by_key(|(_, (runs_out, _))| *runs_out)
        else {
            return future::pending().await;
        };
        time::sleep_until(runs_out).await;
        self.0.swap_remove(at).1
    }
}

/// Ticks while a background job runs, so the screen is drawn again, and is
/// silent otherwise.
#[derive(Debug, Default)]
struct Ticker(Option<Interval>);

impl Ticker {
    /// Starts ticking while `running`, the first tick [`TICK`] from now, and
    /// stops when not.
    fn run_while(&mut self, running: bool) {
        match (running, &self.0) {
            (true, None) => {
                let mut interval = time::interval_at(Instant::now() + TICK, TICK);
                // After a suspend, one tick, not one for every tick missed.
                interval.set_missed_tick_behavior(MissedTickBehavior::Delay);
                self.0 = Some(interval);
            }
            (false, Some(_)) => self.0 = None,
            (true, Some(_)) | (false, None) => {}
        }
    }

    /// The next tick, which never comes while no job runs.
    async fn tick(&mut self) {
        match &mut self.0 {
            Some(interval) => {
                interval.tick().await;
            }
            None => future::pending().await,
        }
    }
}

/// Signals from outside the terminal, which the loop acts on between
/// events.
#[derive(Debug)]
struct Signals {
    /// SIGHUP, SIGINT and SIGTERM stop the app the way Ctrl+C does. In raw
    /// mode Ctrl+C is a key, so SIGINT can only come from outside.
    stop: LastSignal,
    /// SIGTSTP suspends the app the way Ctrl+Z does, and SIGCONT continues
    /// it. In raw mode Ctrl+Z is a key, so SIGTSTP can only come from
    /// outside. As with the kernel's own stop and continue, only the later
    /// of the two counts.
    job_control: LastSignal,
    /// Wakes the loop when any of those signals arrives. Which one arrived
    /// is read from `stop` and `job_control`, which keep what matters about
    /// their order.
    arrivals: Vec<unix_signal::Signal>,
    /// The one thread whose handlers record those signals.
    thread: SignalThread,
}

impl Signals {
    /// Must be called on the loop's runtime, on the loop's thread, before it
    /// starts any other thread: a thread started before can still take
    /// these signals.
    fn listen() -> io::Result<Self> {
        let thread = SignalThread::start(
            the_loops_signals()
                .map(|signal| NixSignal::try_from(signal).expect("each is a standard signal"))
                .collect(),
        )?;
        let stop = LastSignal::listen(&STOP_SIGNALS)?;
        let job_control = LastSignal::listen(&JOB_CONTROL_SIGNALS)?;
        // Registered after the flags, so a signal's flag is set before it
        // wakes the loop.
        let arrivals = the_loops_signals()
            .map(|signal| unix_signal::signal(SignalKind::from_raw(signal)))
            .collect::<io::Result<_>>()?;
        Ok(Self {
            stop,
            job_control,
            arrivals,
            thread,
        })
    }

    /// Waits until one of the signals arrives. It is cancel-safe (rule 18).
    async fn arrival(&mut self) {
        future::poll_fn(|context| {
            if self
                .arrivals
                .iter_mut()
                .any(|arrival| arrival.poll_recv(context).is_ready())
            {
                Poll::Ready(())
            } else {
                Poll::Pending
            }
        })
        .await;
    }
}

/// The one thread that takes some signals. The thread that starts it blocks
/// them, and so does every thread that one starts afterwards, since a thread
/// starts with the signal mask of the thread that starts it.
///
/// The kernel gives a signal sent to the process to one thread that does
/// not block it, and a thread continued after a stop runs the handler of
/// every signal pending for it before any code of its own. So this thread's
/// handlers record each of these signals, and once the app is continued,
/// this thread replies to [`SignalThread::catch_up`] only after recording
/// every one received before the continue. When any thread could take them,
/// a SIGTERM received while suspended could still be on its way to its
/// flag, on another thread, when the loop's thread looked (#152).
///
/// A child process starts with the signal mask of the thread that starts
/// it, so one started from any other thread would start with these signals
/// blocked. Whetstone starts none: gix opens repositories isolated and cgg
/// analyses without Git, so neither runs `git`.
#[derive(Debug)]
struct SignalThread {
    /// Asks the thread to reply. Dropping it ends the thread.
    requests: mpsc::SyncSender<()>,
    replies: mpsc::Receiver<()>,
    /// The mask the starting thread had before, which it gets back once this
    /// is dropped.
    mask_before: SigSet,
    /// The mask is the starting thread's, so this is dropped there.
    not_send: PhantomData<*const ()>,
    /// The thread, so a test can send it a signal.
    #[cfg(test)]
    pthread: RawPthread,
}

impl SignalThread {
    /// Starts the thread, then blocks `signals` in the calling thread.
    fn start(signals: SigSet) -> io::Result<Self> {
        // One request at a time, each with its reply: [`Self::catch_up`]
        // waits for the reply before it asks again.
        let (requests, asked) = mpsc::sync_channel(1);
        let (reply, replies) = mpsc::sync_channel(1);
        let started = thread::Builder::new()
            .name("signals".to_owned())
            .spawn(move || {
                // Even if the app started with them blocked. It fails only
                // for an invalid way to change the mask.
                let _ = signals.thread_unblock();
                for () in asked {
                    if reply.send(()).is_err() {
                        return;
                    }
                }
            })?;
        #[cfg(test)]
        let pthread = started.as_pthread_t();
        // Detached: it ends once `requests` is dropped.
        drop(started);
        let mask_before = signals.thread_swap_mask(SigmaskHow::SIG_BLOCK)?;
        Ok(Self {
            requests,
            replies,
            mask_before,
            not_send: PhantomData,
            #[cfg(test)]
            pthread,
        })
    }

    /// Returns once the thread has recorded every signal sent to it before:
    /// it runs their handlers before any code of its own, so before it
    /// replies. It blocks the calling thread for that round trip only,
    /// which lasts no longer than those handlers.
    fn catch_up(&self) -> io::Result<()> {
        self.requests.send(()).map_err(io::Error::other)?;
        self.replies.recv().map_err(io::Error::other)
    }
}

impl Drop for SignalThread {
    fn drop(&mut self) {
        // It fails only for an invalid way to change the mask, and setting
        // it is a valid one.
        let _ = self.mask_before.thread_set_mask();
    }
}

/// The last of some signals received, recorded by their handlers.
#[derive(Debug)]
struct LastSignal {
    /// The last signal received, or 0.
    last: Arc<AtomicUsize>,
    registrations: Vec<SigId>,
}

impl LastSignal {
    fn listen(signals: &[c_int]) -> io::Result<Self> {
        let mut received = Self {
            last: Arc::new(AtomicUsize::new(0)),
            registrations: Vec::with_capacity(signals.len()),
        };
        for &signal in signals {
            let value = usize::try_from(signal).expect("signal numbers are positive");
            received
                .registrations
                .push(signal_hook::flag::register_usize(
                    signal,
                    Arc::clone(&received.last),
                    value,
                )?);
        }
        Ok(received)
    }

    /// The last signal received, if any.
    fn last(&self) -> Option<c_int> {
        Self::signal(self.last.load(Ordering::Relaxed))
    }

    /// The last signal received since the last take, if any.
    fn take(&self) -> Option<c_int> {
        Self::signal(self.last.swap(0, Ordering::Relaxed))
    }

    /// Forgets `signal` if it is the last signal received, but not a later
    /// one.
    fn forget(&self, signal: c_int) {
        let value = usize::try_from(signal).expect("signal numbers are positive");
        // The exchange fails when a later signal has replaced it, which
        // stays.
        let _ = self
            .last
            .compare_exchange(value, 0, Ordering::Relaxed, Ordering::Relaxed);
    }

    fn signal(value: usize) -> Option<c_int> {
        match value {
            0 => None,
            signal => c_int::try_from(signal).ok(),
        }
    }
}

impl Drop for LastSignal {
    fn drop(&mut self) {
        for registration in self.registrations.drain(..) {
            signal_hook::low_level::unregister(registration);
        }
    }
}

/// Whether a debug build panics right after its first draw, so a test can
/// watch a real panic restore the terminal. [`CRASH_AFTER_FIRST_DRAW`] sets
/// it. Release builds compile it out and never read the variable.
#[derive(Debug, Clone, Copy)]
enum CrashTrigger {
    /// Draw and carry on, as release builds always do.
    Unset,
    /// Panic right after the first draw.
    #[cfg(debug_assertions)]
    Set,
}

/// The variable that sets the [`CrashTrigger`] when it is not empty.
#[cfg(debug_assertions)]
const CRASH_AFTER_FIRST_DRAW: &str = "WHETSTONE_CRASH_AFTER_FIRST_DRAW";

impl CrashTrigger {
    #[cfg(debug_assertions)]
    fn read(environment: &Environment) -> Self {
        match environment.get(CRASH_AFTER_FIRST_DRAW) {
            Some(value) if !value.is_empty() => Self::Set,
            _ => Self::Unset,
        }
    }

    #[cfg(not(debug_assertions))]
    fn read(_: &Environment) -> Self {
        Self::Unset
    }

    /// Panics if the trigger is set. It runs after every draw, so the first
    /// draw is the one it follows.
    fn after_draw(self) {
        match self {
            Self::Unset => {}
            #[cfg(debug_assertions)]
            Self::Set => crash(),
        }
    }
}

#[cfg(debug_assertions)]
#[expect(clippy::panic, reason = "the crash trigger exists to panic")]
fn crash() -> ! {
    panic!("crashing after the first draw, as {CRASH_AFTER_FIRST_DRAW} asks")
}

#[cfg(test)]
mod tests {
    use nix::sys::pthread::pthread_kill;

    use super::*;
    use crate::job::Jobs;

    /// Longer than any tick.
    const A_MINUTE: Duration = Duration::from_secs(60);

    #[tokio::test(start_paused = true)]
    async fn the_ticker_ticks_every_tick_only_while_a_job_runs() {
        let mut ticker = Ticker::default();
        assert!(
            time::timeout(A_MINUTE, ticker.tick()).await.is_err(),
            "no tick before a job runs"
        );

        ticker.run_while(true);
        let started = Instant::now();
        ticker.tick().await;
        assert_eq!(started.elapsed(), TICK, "the first tick, a tick later");
        ticker.run_while(true);
        ticker.tick().await;
        assert_eq!(started.elapsed(), 2 * TICK, "the next, a tick after it");

        ticker.run_while(false);
        assert!(
            time::timeout(A_MINUTE, ticker.tick()).await.is_err(),
            "no tick once no job runs"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn each_time_limit_runs_out_once_at_its_time_the_earliest_first() {
        let mut jobs = Jobs::<()>::new();
        let [slow, quick] = [jobs.start(|_| Ok(())), jobs.start(|_| Ok(()))];
        let mut time_limits = TimeLimits::default();
        assert!(
            time::timeout(A_MINUTE, time_limits.run_out())
                .await
                .is_err(),
            "nothing runs out before a time limit is set"
        );

        let started = Instant::now();
        jobs.limit(slow, Duration::from_secs(20));
        jobs.limit(quick, Duration::from_secs(5));
        time_limits.start(jobs.take_time_limits());
        assert_eq!(time_limits.run_out().await, quick);
        assert_eq!(started.elapsed(), Duration::from_secs(5));
        // One that loses a race takes nothing.
        assert!(
            time::timeout(Duration::from_secs(1), time_limits.run_out())
                .await
                .is_err()
        );
        assert_eq!(time_limits.run_out().await, slow);
        assert_eq!(started.elapsed(), Duration::from_secs(20));

        assert!(
            time::timeout(A_MINUTE, time_limits.run_out())
                .await
                .is_err(),
            "each runs out once"
        );
    }
    #[tokio::test(start_paused = true)]
    async fn time_spent_suspended_never_counts_towards_a_time_limit() {
        let mut jobs = Jobs::<()>::new();
        let job = jobs.start(|_| Ok(()));
        let mut time_limits = TimeLimits::default();
        let started = Instant::now();
        jobs.limit(job, Duration::from_secs(20));
        time_limits.start(jobs.take_time_limits());

        time::advance(Duration::from_secs(5)).await;
        time_limits.postpone(Duration::from_secs(30));

        assert_eq!(time_limits.run_out().await, job);
        assert_eq!(started.elapsed(), Duration::from_secs(50));
    }

    /// The signals the loop acts on: SIGHUP, SIGINT and SIGTERM stop it,
    /// SIGTSTP suspends it and SIGCONT continues it.
    const THE_LOOPS_SIGNALS: [NixSignal; 5] = [
        NixSignal::SIGHUP,
        NixSignal::SIGINT,
        NixSignal::SIGTERM,
        NixSignal::SIGTSTP,
        NixSignal::SIGCONT,
    ];

    /// The loop's signals that the calling thread blocks.
    fn blocked_here() -> Vec<NixSignal> {
        let mask = SigSet::thread_get_mask().expect("read this thread's signal mask");
        THE_LOOPS_SIGNALS
            .into_iter()
            .filter(|&signal| mask.contains(signal))
            .collect()
    }

    #[tokio::test]
    async fn the_loops_thread_and_every_thread_it_starts_block_the_loops_signals() {
        let before = blocked_here();
        let signals = Signals::listen().expect("listen for signals");

        assert_eq!(blocked_here(), THE_LOOPS_SIGNALS, "the loop's own thread");
        let started = thread::spawn(blocked_here).join().expect("join the thread");
        assert_eq!(started, THE_LOOPS_SIGNALS, "a thread the loop starts");
        let blocking = tokio::task::spawn_blocking(blocked_here)
            .await
            .expect("join the blocking thread");
        assert_eq!(
            blocking, THE_LOOPS_SIGNALS,
            "a blocking thread, where jobs run"
        );

        drop(signals);
        assert_eq!(
            blocked_here(),
            before,
            "the loop's thread takes them again once it stops listening"
        );
    }

    #[tokio::test]
    async fn once_the_signal_thread_catches_up_it_has_recorded_every_signal_sent_before() {
        let signals = Signals::listen().expect("listen for signals");

        // Sent to the signal thread, which the kernel picks for one sent to
        // the process, since every other thread blocks it. That a thread
        // continued after a stop runs their handlers first is the
        // pseudo-terminal tests' to show.
        for signal in [NixSignal::SIGTERM, NixSignal::SIGCONT] {
            pthread_kill(signals.thread.pthread, signal).expect("send the signal");
        }
        signals.thread.catch_up().expect("catch up");

        assert_eq!(signals.stop.last(), Some(SIGTERM));
        assert_eq!(signals.job_control.last(), Some(SIGCONT));
    }
}
