//! Background jobs: the app starts a job, the workers run it on a blocking
//! thread, and its typed result comes back, cancellation included, to be
//! applied only while the app still waits for that job.

use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::Duration;

use tokio::runtime;
use tokio_util::sync::CancellationToken;
use whetstone::{Cancelled, Finished, JobId, JobOutcome, Jobs, TimeLimit, Workers};

use crate::session::Session;

/// Runs `future` on a current-thread runtime, as the UI loop runs.
pub(crate) fn on_a_runtime<F: Future>(future: F) -> F::Output {
    runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("build a runtime")
        .block_on(future)
}

#[test]
fn a_job_s_result_arrives_with_the_job_s_identity() {
    on_a_runtime(async {
        let mut jobs = Jobs::new();
        let mut workers = Workers::new();

        let id = jobs.start(|_| Ok(42));
        for job in jobs.take_started() {
            workers.run(job);
        }
        let finished = workers.finished().await;

        assert_eq!(finished.id, id);
        assert_eq!(finished.outcome, JobOutcome::Done(42));
    });
}

#[test]
fn the_workers_are_busy_only_while_a_job_runs() {
    on_a_runtime(async {
        let mut jobs = Jobs::new();
        let mut workers = Workers::new();
        let (go_on, may_go_on) = mpsc::channel();
        assert!(workers.is_idle(), "idle before any job");

        jobs.start(move |_| {
            may_go_on.recv().expect("wait for the test");
            Ok(42)
        });
        for job in jobs.take_started() {
            workers.run(job);
        }
        assert!(!workers.is_idle(), "busy while the job runs");
        go_on.send(()).expect("let the job end");
        assert_eq!(workers.finished().await.outcome, JobOutcome::Done(42));

        // Its thread may still be returning once its report is sent.
        while !workers.is_idle() {
            tokio::task::yield_now().await;
        }
    });
}

#[test]
fn a_job_cancelled_before_it_runs_reports_cancellation_and_never_runs_its_body() {
    on_a_runtime(async {
        let mut jobs = Jobs::new();
        let mut workers = Workers::new();
        let ran = Arc::new(AtomicBool::new(false));

        let id = jobs.start({
            let ran = Arc::clone(&ran);
            move |_| {
                ran.store(true, Ordering::SeqCst);
                Ok(42)
            }
        });
        jobs.cancel(id);
        for job in jobs.take_started() {
            workers.run(job);
        }
        let finished = workers.finished().await;

        assert_eq!(finished.id, id);
        assert_eq!(finished.outcome, JobOutcome::Cancelled);
        assert!(!ran.load(Ordering::SeqCst), "its body never ran");
    });
}

/// The report of a job cancelled while `body` runs: once its body has
/// started, the job is cancelled, then `body` goes on.
fn cancelled_while_running(
    body: impl FnOnce(&CancellationToken) -> Result<u32, Cancelled> + Send + 'static,
) -> (JobId, Finished<u32>) {
    on_a_runtime(async {
        let mut jobs = Jobs::new();
        let mut workers = Workers::new();
        let (running, has_started) = mpsc::channel();
        let (cancel, is_cancelled) = mpsc::channel();

        let id = jobs.start(move |token| {
            running.send(()).expect("tell the test the body runs");
            is_cancelled
                .recv()
                .expect("wait for the test to cancel the job");
            body(token)
        });
        for job in jobs.take_started() {
            workers.run(job);
        }
        has_started.recv().expect("the body runs");
        jobs.cancel(id);
        cancel.send(()).expect("let the body go on");
        (id, workers.finished().await)
    })
}

#[test]
fn a_job_cancelled_while_it_runs_reports_cancellation_even_if_its_body_runs_to_its_end() {
    // As a body that cannot be cancelled midway does.
    let (id, finished) = cancelled_while_running(|_| Ok(42));

    assert_eq!(finished.id, id);
    assert_eq!(finished.outcome, JobOutcome::Cancelled);
}

#[test]
fn a_job_s_body_can_see_its_cancellation_and_stop_early() {
    let (id, finished) = cancelled_while_running(|token| {
        assert!(token.is_cancelled(), "the body sees its job cancelled");
        Err(Cancelled)
    });

    assert_eq!(finished.id, id);
    assert_eq!(finished.outcome, JobOutcome::Cancelled);
}

#[test]
fn a_result_is_accepted_only_from_a_job_the_app_still_waits_for() {
    on_a_runtime(async {
        let mut jobs = Jobs::new();
        let mut workers = Workers::new();

        let replaced = jobs.start(|_| Ok("replaced"));
        jobs.cancel(replaced);
        let current = jobs.start(|_| Ok("current"));
        for job in jobs.take_started() {
            workers.run(job);
        }
        let mut accepted = Vec::new();
        for _ in [replaced, current] {
            if let Some(finished) = jobs.accept(workers.finished().await) {
                accepted.push((finished.id, finished.outcome));
            }
        }

        assert_eq!(accepted, [(current, JobOutcome::Done("current"))]);
    });
}

#[test]
fn quitting_cancels_a_running_job_and_discards_its_result_without_waiting() {
    let runtime = runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("build a runtime");
    let (running, has_started) = mpsc::channel();
    let (go_on, may_go_on) = mpsc::channel();
    let (saw_cancelled, cancelled) = mpsc::channel();

    runtime.block_on(async {
        let mut jobs = Jobs::new();
        let workers = Workers::new();
        jobs.start(move |token| {
            running.send(()).expect("tell the test the body runs");
            may_go_on.recv().expect("wait until the app has quit");
            saw_cancelled
                .send(token.is_cancelled())
                .expect("tell the test what the body saw");
            Ok(42)
        });
        for job in jobs.take_started() {
            workers.run(job);
        }
        has_started.recv().expect("the body runs");
        // Quitting drops the app with its jobs, and the UI loop's workers.
        drop(jobs);
        drop(workers);
    });
    // The UI loop's runtime shuts down this way, without waiting for the
    // job, which is still running.
    runtime.shutdown_background();
    go_on.send(()).expect("let the body go on");

    assert!(
        cancelled.recv().expect("the body runs to its end"),
        "the job is cancelled"
    );
}

#[test]
fn with_no_job_running_settling_and_ticking_change_nothing_on_screen() {
    let mut session = Session::plain(80, 24);
    session.press("j");
    let before = session.frame();

    session.settle();
    session.tick();

    assert_eq!(session.frame(), before);
}

/// The identities of `jobs`' jobs started since the last take.
fn started<T>(jobs: &mut Jobs<T>) -> Vec<JobId> {
    jobs.take_started()
        .into_iter()
        .map(|job| job.run().expect("a blocking job needs no runtime").id)
        .collect()
}

#[test]
fn jobs_started_one_at_a_time_take_turns_and_only_the_newest_waits() {
    let mut jobs = Jobs::new();

    let first = jobs.start_one_at_a_time(|_| Ok("first"));
    let first_report = jobs
        .take_started()
        .remove(0)
        .run()
        .expect("a blocking job needs no runtime");
    let replaced = jobs.start_one_at_a_time(|_| Ok("replaced"));
    assert!(started(&mut jobs).is_empty(), "waits for the first");
    let newest = jobs.start_one_at_a_time(|_| Ok("newest"));
    let replaced_report = jobs
        .take_started()
        .remove(0)
        .run()
        .expect("a blocking job needs no runtime");

    assert_eq!(replaced_report.id, replaced);
    assert_eq!(replaced_report.outcome, JobOutcome::Cancelled);
    assert!(jobs.accept(replaced_report).is_none());
    assert!(
        started(&mut jobs).is_empty(),
        "the first still has its turn"
    );
    assert_eq!(first_report.id, first);
    assert!(jobs.accept(first_report).is_some());
    assert_eq!(started(&mut jobs), [newest]);
}

/// What the app did with a job before its report came back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WhileRunning {
    /// Waited for it.
    Waited,
    /// Cancelled it.
    Cancelled,
    /// Stopped waiting for it, as when its time limit runs out.
    Forgot,
}

#[test]
fn a_job_that_ran_one_at_a_time_passes_its_turn_on_however_it_ended() {
    for while_running in [
        WhileRunning::Waited,
        WhileRunning::Cancelled,
        WhileRunning::Forgot,
    ] {
        let mut jobs = Jobs::new();
        let first = jobs.start_one_at_a_time(|_| Ok(1));
        let job = jobs.take_started().remove(0);
        match while_running {
            WhileRunning::Waited => {}
            WhileRunning::Cancelled => jobs.cancel(first),
            WhileRunning::Forgot => assert!(jobs.forget(first)),
        }
        let next = jobs.start_one_at_a_time(|_| Ok(2));

        let accepted = jobs
            .accept(job.run().expect("a blocking job needs no runtime"))
            .is_some();

        assert_eq!(
            accepted,
            while_running == WhileRunning::Waited,
            "{while_running:?}"
        );
        assert_eq!(started(&mut jobs), [next], "{while_running:?}");
    }
}

#[test]
fn jobs_started_as_usual_never_wait_for_one_that_has_its_turn() {
    let mut jobs = Jobs::new();
    let alone = jobs.start_one_at_a_time(|_| Ok(1));
    let usual = jobs.start(|_| Ok(2));

    assert_eq!(started(&mut jobs), [alone, usual]);
}

#[test]
fn a_forgotten_job_runs_to_its_end_uncancelled_and_its_result_is_ignored() {
    let mut jobs = Jobs::new();
    let id = jobs.start(|token| Ok(token.is_cancelled()));

    assert!(jobs.forget(id), "the app waited for it");
    assert!(!jobs.forget(id), "and no longer does");
    let finished = jobs
        .take_started()
        .remove(0)
        .run()
        .expect("a blocking job needs no runtime");

    assert_eq!(finished.outcome, JobOutcome::Done(false), "never cancelled");
    assert!(jobs.accept(finished).is_none());
}

#[test]
fn time_limits_set_on_jobs_are_handed_over_once_for_the_ui_loop_to_time() {
    let mut jobs = Jobs::new();
    let id = jobs.start(|_| Ok(1));

    jobs.limit(id, Duration::from_secs(20));

    assert_eq!(
        jobs.take_time_limits(),
        [TimeLimit {
            job: id,
            duration: Duration::from_secs(20)
        }]
    );
    assert_eq!(jobs.take_time_limits(), []);
}
