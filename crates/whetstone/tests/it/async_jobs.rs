//! Async jobs, such as network requests: the app starts one with a future
//! for its body, the workers run it on the runtime, and its typed result
//! comes back through the same channel as a blocking job's, cancellation
//! included, to be applied only while the app still waits for that job.

use std::future::{self, Future};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use tokio::sync::oneshot;
use tokio::{runtime, time};
use whetstone::{JobOutcome, Jobs, Waiting, Workers};

/// Runs `future` on a current-thread runtime, as the UI loop runs.
fn on_a_runtime<F: Future>(future: F) -> F::Output {
    runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("build a runtime")
        .block_on(future)
}

#[test]
fn an_async_job_s_result_arrives_with_the_job_s_identity() {
    on_a_runtime(async {
        let mut jobs = Jobs::new();
        let mut workers = Workers::new();

        let id = jobs.start_async(async { 42 });
        for job in jobs.take_started() {
            workers.run(job);
        }
        let finished = workers.finished().await;

        assert_eq!(finished.id, id);
        assert_eq!(finished.outcome, JobOutcome::Done(42));
    });
}

#[test]
fn the_workers_are_busy_while_an_async_job_waits_so_the_ui_loop_ticks() {
    on_a_runtime(async {
        let mut jobs = Jobs::new();
        let mut workers = Workers::new();
        let (reply, has_reply) = oneshot::channel();
        assert!(workers.is_idle(), "idle before any job");

        jobs.start_async(async { has_reply.await.expect("the test replies") });
        for job in jobs.take_started() {
            workers.run(job);
        }
        assert!(!workers.is_idle(), "busy while the job waits");
        reply.send(42).expect("the job waits for its reply");
        assert_eq!(workers.finished().await.outcome, JobOutcome::Done(42));

        // Its task may still be returning once its report is sent.
        while !workers.is_idle() {
            tokio::task::yield_now().await;
        }
    });
}

/// Says whether it has been dropped, as an async job's body is when the job
/// stops where it waits.
#[derive(Debug, Default, Clone)]
struct DropFlag(Arc<AtomicBool>);

impl DropFlag {
    fn dropped(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

/// Held by an async job's body: raises its flag when the body is dropped.
#[derive(Debug)]
struct Held(DropFlag);

impl Drop for Held {
    fn drop(&mut self) {
        let Self(DropFlag(dropped)) = self;
        dropped.store(true, Ordering::SeqCst);
    }
}

/// An async job's body that tells the test it runs, then waits for ever,
/// as a request that never gets a reply does, holding `flag`'s guard.
async fn waits_for_ever(running: oneshot::Sender<()>, flag: DropFlag) -> u32 {
    let _held = Held(flag);
    running.send(()).expect("tell the test the body runs");
    future::pending().await
}

#[test]
fn an_async_job_cancelled_while_it_waits_reports_cancellation_and_its_body_stops() {
    on_a_runtime(async {
        let mut jobs = Jobs::new();
        let mut workers = Workers::new();
        let (running, has_started) = oneshot::channel();
        let flag = DropFlag::default();

        let id = jobs.start_async(waits_for_ever(running, flag.clone()));
        for job in jobs.take_started() {
            workers.run(job);
        }
        has_started.await.expect("the body runs");
        jobs.cancel(id);
        let finished = workers.finished().await;

        assert_eq!(finished.id, id);
        assert_eq!(finished.outcome, JobOutcome::Cancelled);
        assert!(flag.dropped(), "its body stopped where it waited");
    });
}

#[test]
fn an_async_job_cancelled_before_it_runs_reports_cancellation_and_never_runs_its_body() {
    on_a_runtime(async {
        let mut jobs = Jobs::new();
        let mut workers = Workers::new();
        let ran = Arc::new(AtomicBool::new(false));

        let id = jobs.start_async({
            let ran = Arc::clone(&ran);
            async move {
                ran.store(true, Ordering::SeqCst);
                42
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

#[test]
fn an_async_job_run_on_the_test_s_thread_runs_to_its_end_as_the_app_seam_harness_runs_it() {
    let mut jobs = Jobs::new();
    let ran = Arc::new(AtomicBool::new(false));

    let done = jobs.start_async(async {
        // Its own runtime is there, as the UI loop's is for a real request.
        tokio::task::yield_now().await;
        42
    });
    let cancelled = jobs.start_async({
        let ran = Arc::clone(&ran);
        async move {
            ran.store(true, Ordering::SeqCst);
            7
        }
    });
    jobs.cancel(cancelled);
    let reports: Vec<_> = jobs
        .take_started()
        .into_iter()
        .map(|job| {
            let finished = job.run();
            (finished.id, finished.outcome)
        })
        .collect();

    assert_eq!(
        reports,
        [
            (done, JobOutcome::Done(42)),
            (cancelled, JobOutcome::Cancelled)
        ]
    );
    assert!(!ran.load(Ordering::SeqCst), "the cancelled one never ran");
}

#[test]
fn a_result_for_an_async_job_the_app_no_longer_waits_for_is_ignored() {
    on_a_runtime(async {
        let mut jobs = Jobs::new();
        let mut workers = Workers::new();

        let cancelled = jobs.start_async(async { "cancelled" });
        jobs.cancel(cancelled);
        // As when a screen stops waiting without cancelling.
        let forgotten = jobs.start_async(async { "forgotten" });
        assert!(jobs.forget(forgotten));
        let current = jobs.start_async(async { "current" });
        for job in jobs.take_started() {
            workers.run(job);
        }
        let mut accepted = Vec::new();
        for _ in [cancelled, forgotten, current] {
            if let Some(finished) = jobs.accept(workers.finished().await) {
                accepted.push((finished.id, finished.outcome));
            }
        }

        assert_eq!(accepted, [(current, JobOutcome::Done("current"))]);
    });
}

#[test]
fn a_screen_applies_a_result_only_from_a_job_it_waits_for_carrying_that_job_s_key() {
    let mut jobs = Jobs::<()>::new();
    let mut waiting = Waiting::new();
    let [area_a, area_b] = [jobs.start_async(async {}), jobs.start_async(async {})];

    waiting.wait_for(area_a, "area a");
    waiting.wait_for(area_b, "area b");

    assert!(waiting.waits_for(area_a) && waiting.waits_for(area_b));
    assert!(
        !waiting.accept(area_b, &"area a"),
        "a result carrying another key"
    );
    assert!(
        !waiting.waits_for(area_b),
        "whose job has ended all the same"
    );
    assert!(waiting.accept(area_a, &"area a"));
    assert!(!waiting.accept(area_a, &"area a"), "applied once");
    assert!(waiting.is_empty());
}

/// Longer than any job in these tests takes.
const PATIENCE: Duration = Duration::from_secs(10);

#[test]
fn several_async_jobs_run_at_once_and_each_result_is_applied_on_its_own() {
    on_a_runtime(async {
        let mut jobs = Jobs::new();
        let mut workers = Workers::new();
        let mut waiting = Waiting::new();
        let (replied, reply) = oneshot::channel();

        // The first can end only once the second has run, so the two run at
        // once and end out of order.
        let first = jobs.start_async(async {
            reply.await.expect("the second job replies");
            ("area a", 1)
        });
        let second = jobs.start_async(async {
            replied.send(()).expect("the first job waits");
            ("area b", 2)
        });
        waiting.wait_for(first, "area a");
        waiting.wait_for(second, "area b");
        for job in jobs.take_started() {
            workers.run(job);
        }
        let mut applied = Vec::new();
        while !waiting.is_empty() {
            let finished = time::timeout(PATIENCE, workers.finished())
                .await
                .expect("both jobs end");
            if let Some(finished) = jobs.accept(finished)
                && let JobOutcome::Done((key, output)) = finished.outcome
                && waiting.accept(finished.id, &key)
            {
                applied.push((key, output));
            }
        }

        assert_eq!(applied, [("area b", 2), ("area a", 1)]);
    });
}

#[test]
fn cancelling_what_a_screen_waits_for_stops_every_job_and_applies_none_of_their_results() {
    on_a_runtime(async {
        let mut jobs = Jobs::new();
        let mut workers = Workers::new();
        let mut waiting = Waiting::new();
        let flags = [DropFlag::default(), DropFlag::default()];
        let mut started = Vec::new();

        for (area, flag) in ["area a", "area b"].into_iter().zip(&flags) {
            let (running, has_started) = oneshot::channel();
            let job = jobs.start_async(waits_for_ever(running, flag.clone()));
            waiting.wait_for(job, area);
            started.push(has_started);
        }
        for job in jobs.take_started() {
            workers.run(job);
        }
        for has_started in started {
            has_started.await.expect("each body runs");
        }
        waiting.cancel(&mut jobs);
        let mut accepted = 0;
        for _ in &flags {
            let finished = workers.finished().await;
            assert_eq!(finished.outcome, JobOutcome::Cancelled);
            accepted += usize::from(jobs.accept(finished).is_some());
        }

        assert_eq!(accepted, 0, "no late result is applied");
        assert!(waiting.is_empty());
        assert!(flags.iter().all(DropFlag::dropped), "every body stopped");
    });
}

#[test]
fn quitting_drops_a_running_async_job_where_it_waits_without_waiting_for_it() {
    let runtime = runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("build a runtime");
    let flag = DropFlag::default();

    runtime.block_on(async {
        let mut jobs = Jobs::new();
        let workers = Workers::new();
        let (running, has_started) = oneshot::channel();
        jobs.start_async(waits_for_ever(running, flag.clone()));
        for job in jobs.take_started() {
            workers.run(job);
        }
        has_started.await.expect("the body runs");
        // Quitting drops the app with its jobs, and the UI loop's workers,
        // so nothing receives the job's result.
        drop(jobs);
        drop(workers);
    });
    // The UI loop's runtime shuts down this way. Were it to wait for the
    // job, which waits for ever, this would never return.
    runtime.shutdown_background();

    assert!(flag.dropped(), "its body was dropped where it waited");
}
