# Coding standards

The rules every Whetstone change follows. Contributors write to them, and reviewers (including `/code-review`) check against them.

The rules come from the research note [*Rust stack and coding standards*](docs/research/rust-stack-and-coding-standards.md), section 10, adjusted where spec [#11](https://github.com/monadial/whetstone/issues/11) decided differently (see [Differences from the research note](#differences-from-the-research-note)). Rules keep the note's numbers, so a review can cite "rule 11". Rules 54 onwards are project rules the note does not have.

Each rule says how it is enforced:

- **Lint/CI**: a lint, test or CI job fails the build. CI denies all warnings, so a lint set to `warn` in the workspace still fails CI.
- **Review**: a reviewer checks it. Nothing mechanical catches it.

Some rules are both. Rules about code that does not exist yet (network code, AI, storage) are marked *from …*, or sit in a section that says when it applies. They apply as soon as that code lands, and their checks arrive with it.

## Toolchain, workspace and dependencies

| # | Rule | Enforced by |
| --- | --- | --- |
| 1 | Every crate uses edition 2024 and inherits `rust-version` from the workspace. The policy is latest stable only: `rust-version` equals the channel in `rust-toolchain.toml` (1.99.1), and the two are bumped together, deliberately. | **Lint/CI**: CI builds with the pinned toolchain, and Cargo refuses a `rust-version` above it. **Review**: the two stay equal. |
| 2 | Commit `Cargo.lock`. Every CI command that resolves dependencies (Clippy, tests, docs, cargo-deny, cargo-shear) runs with `--locked`. | **Lint/CI**: `--locked` fails on a stale lockfile. |
| 3 | Use the stable toolchain only: no nightly features and no unstable rustfmt options. | **Lint/CI**: CI uses the pinned stable toolchain, which rejects `#![feature]`. |
| 4 | Code is formatted with `cargo fmt` in the default style. There is no `rustfmt.toml`: `cargo fmt` takes style edition 2024 from the crates' edition. Bare `rustfmt` defaults to style edition 2015, so use `cargo fmt`. | **Lint/CI**: `cargo fmt --all --check`. |
| 5 | Lint levels are set only in `[workspace.lints]` in the root `Cargo.toml`, and every crate inherits them with `[lints] workspace = true`. `clippy.toml` files hold lint options (test allowances, disallowed paths), never levels. CI denies warnings across all targets. | **Lint/CI**: `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` on every CI platform. **Review**: no lint levels in crate attributes or `clippy.toml`. |
| 6 | Suppress a lint only locally, with `#[expect(lint, reason = "…")]`, never `#[allow]`. | **Lint/CI**: `clippy::allow_attributes`, `clippy::allow_attributes_without_reason`. |
| 7 | No unsafe code: `unsafe_code = "forbid"`. Lifting it needs an ADR, a `// SAFETY:` comment on every unsafe block and `clippy::undocumented_unsafe_blocks`. | **Lint/CI**: rustc's `unsafe_code`. |
| 8 | Every new dependency states its reason in the PR that adds it. No dependency with an active RustSec vulnerability, unmaintained or unsound advisory, no yanked version, and no unused dependency. | **Lint/CI**: `cargo deny check` (advisories), `cargo shear --deny-warnings`. **Review**: the stated reason. |
| 9 | Dependency licenses must be on the allow-list in `deny.toml`. Anything else needs a per-crate `exceptions` entry, with its reason in the PR. Dependencies come from crates.io only (a git source needs a reviewed `allow-git` entry), and no version requirement is `*` (path dependencies between the workspace crates are exempt). | **Lint/CI**: `cargo deny check` (licenses, sources, bans). |
| 10 | Rig crates move together, in a dedicated upgrade PR that re-runs the AI contract tests. | **Lint/CI**: `rig` is the AI crate's only Rig dependency, pinned exactly (`=0.45.0` in `crates/whetstone-ai/Cargo.toml`), and `crates/whetstone-ai/tests/rig_versions.rs` fails unless every `rig-*` crate in `Cargo.lock` is at `rig`'s version; the contract tests in `crates/whetstone-ai` replay recorded `OpenRouter` replies on every PR. **Review**: the upgrade is a PR of its own. |

## Errors and panics

| # | Rule | Enforced by |
| --- | --- | --- |
| 11 | Recoverable failures (I/O, config, source, provider, storage) return `Result`. No `unwrap`, `panic!`, `todo!`, `unimplemented!` or `dbg!` outside tests. `clippy.toml` allows `unwrap`, `expect`, `panic!` and printing in tests. | **Lint/CI**: `clippy::unwrap_used`, `panic`, `todo`, `unimplemented`, `dbg_macro`. |
| 12 | `expect` is used only for invariants, and its message states the invariant: `.expect("signal numbers are positive")`. (`clippy::expect_used` is deliberately off.) | **Review**. |
| 13 | Each module that can fail has its own `thiserror` error type, which is `Error + Send + Sync + 'static`, never `()`. Messages are lowercase, with no trailing period. `anyhow` appears only at the binary edge: the command boundary in `lib.rs` and the CLI output path that reports its error. | **Review**. |
| 14 | No `std::process::exit`. Return an `ExitCode` to `main`, and from it, so that `Drop` guards run. | **Lint/CI**: `clippy::exit`, outside `main`. **Review**: inside `main`, which the lint ignores. |
| 15 | Keep `panic = "unwind"` in every profile. The terminal guard restores the terminal from both `Drop` and its panic hook. So that a test can crash the real binary, a debug build panics right after its first draw when `WHETSTONE_CRASH_AFTER_FIRST_DRAW` is set and not empty. Release builds compile this crash trigger out. | **Review**: profile changes. **Lint/CI**, in debug builds only: the real-panic pseudo-terminal test (`tests/it/pty_terminal.rs`) and the guard's panic test (`tests/panic_restore.rs`). |

## Async work

The UI loop (`tui.rs`) runs on a current-thread Tokio runtime. It waits with `select!` on signals, crossterm's `EventStream`, the bounded channel of background job results and, only while a job runs, a tick, so nothing else wakes it. Background jobs (`job.rs`) are owned by a `TaskTracker`, each with a child `CancellationToken`: blocking ones run on `spawn_blocking`, and async ones, such as network requests, run as tasks on the runtime. Their results enter through `App::handle`, like key presses. Quitting never waits for a job: the runtime shuts down without waiting, and a running job's result is discarded.

| # | Rule | Enforced by |
| --- | --- | --- |
| 16 | Every spawned task has an owner (`JoinSet`, `TaskTracker`, `AbortOnDropHandle`). Dropping a bare `JoinHandle` needs a comment. | **Review**. |
| 17 | Each job derives a child `CancellationToken` and reports cancellation as a typed result, never as silence. | **Review**. **Lint/CI**: the job tests (`tests/it/jobs.rs`, `tests/it/async_jobs.rs`). |
| 18 | `select!` branches inside loops use only cancel-safe operations: `reserve` or `try_send` instead of `send`, and no `write_all` or lock acquisition. | **Review**. |
| 19 | No blocking work on async tasks. Git and parsing go to `spawn_blocking`, storage to the writer thread. No `block_in_place`. | **Review**. **Lint/CI**: `disallowed-methods` for `tokio::task::block_in_place` in `clippy.toml`. Tokio builds it only with `rt-multi-thread`, which is off. |
| 20 | Never hold a std `Mutex` guard or a `RefCell` borrow across `.await`. | **Lint/CI**: `clippy::await_holding_lock`, `await_holding_refcell_ref` (deny). |
| 21 | Channels are bounded, with a documented capacity. An unbounded channel needs a written reason. | **Review**. |
| 22 | *From network code:* every network stage has an explicit timeout. The HTTP client is built with `connect_timeout` and `timeout`. | **Review**. **Lint/CI**: adapter tests. |
| 23 | *From network code:* automatic retries use `backon` with jitter and an explicit retryable predicate. They never replay a paid request the user has not authorised. | **Review**. |
| 24 | A worker result is applied only when its job identity matches the outstanding job, and so do its session, question version and attempt identities once those exist: the key a screen waits for the job with (`Waiting`). | **Lint/CI**: the job tests (`tests/it/jobs.rs`, `tests/it/async_jobs.rs`); reducer tests at the app seam for each kind of job. |

## Terminal UI

| # | Rule | Enforced by |
| --- | --- | --- |
| 25 | Drawing is pure: `render` does no I/O, network, storage or spawning, and changes no state (`App::render` takes `&self`). | **Review**. |
| 26 | One input source. The UI loop in `tui.rs` is the only place that reads terminal events, and only key presses (`KeyEventKind::Press`) and pastes act, filtered once in `App::handle`, where input enters the app. A paste goes where rule 57 says. | **Review**. **Lint/CI**: `disallowed-methods` for crossterm's `read` and `poll` in `clippy.toml`. |
| 27 | One terminal guard enables everything the TUI turns on (raw mode, alternate screen, hidden cursor and bracketed paste) and undoes it in reverse order on exit, on `Drop` and from its panic hook. Mouse capture and keyboard enhancement flags are never enabled or required. | **Lint/CI**: the pseudo-terminal tests (`tests/it/pty.rs`, `tests/it/pty_paste.rs`, and `tests/it/pty_terminal.rs`, which also checks that no mouse mode or keyboard enhancement is ever written) on Linux and macOS, x86_64 and arm64; guard tests (`tests/it/terminal_guard.rs`, `tests/panic_restore.rs`). |
| 28 | In raw mode Ctrl+C and Ctrl+Z are reserved keys: they act from anywhere, even below the 50×12 floor, before overlays, text entry and focus dispatch, and never type. Ctrl+C quits, and SIGINT, SIGTERM and SIGHUP from outside follow the same shutdown path and restore the terminal. Ctrl+Z suspends, and SIGTSTP from outside follows the same path: the guard turns every mode off, then the app's whole process group stops, as Ctrl+Z stops a job outside raw mode. If no job-control shell could continue it, because its process group is its session's own, the app carries on at once instead, as vim does. On SIGCONT the TUI turns every mode off and on again, since the shell may have changed them, reads the window size again, and clears to redraw the same screen in full, with app state untouched. A SIGCONT with no stop before it does no harm, and a stop signal received while suspended ends the app without taking the terminal back. | **Lint/CI**: pseudo-terminal tests for `q`, Ctrl+C and SIGTERM (`tests/it/pty.rs`), for SIGINT and SIGHUP (`tests/it/pty_terminal.rs`), and for Ctrl+Z, SIGTSTP, SIGCONT, a resize while suspended and suspending without job control (`tests/it/pty_suspend.rs`); keystroke tests at the app seam (`tests/it/suspend.rs`). |
| 29 | *From overlays and editors:* key dispatch follows focus: reserved keys, then the top overlay, then the focused control. While an editor has focus, every non-reserved key is text. | **Lint/CI**: keystroke tests at the app seam (`tests/it/answering.rs` for the reasoning editor, `tests/it/help_and_signals.rs` for the help controls and the report's note). |
| 30 | *From submission:* submitting never depends on a modified Enter. Tab → Submit → Enter always works. | **Lint/CI**: a keystroke test (`tests/it/answering.rs`). |
| 31 | Text is measured in cells and edited by grapheme. No byte-length layout and no byte slicing of user text. | **Lint/CI**: `clippy::string_slice`; property tests once the editor lands. **Review**. |
| 32 | Layout is derived from `frame.area()` on every draw. State holds no `Rect`, so a resize never changes selections, scroll or focus. | **Lint/CI**: snapshot and resize tests. **Review**. |
| 33 | Colours come only from theme tokens. The colour level is none under a non-empty `NO_COLOR`, otherwise the Colours setting (`colours` in the global config) when it names a level, otherwise what `COLORTERM` and `TERM` report. `NO_COLOR` wins over every Colours value and removes colour only, never bold, reverse, underline or dim, so meaning never depends on colour alone. | **Lint/CI**: buffer-style tests, from the brand theme; precedence tests at the app seam; the pseudo-terminal tests for `NO_COLOR` and `colours = "none"` (`tests/it/pty.rs`, `tests/it/pty_colours.rs`). **Review**. |
| 34 | Nothing writes to stdout or stderr while the TUI is active. Only the CLI output path (`output.rs`) prints, and its print macros sit under `#[expect(clippy::print_…, reason = "…")]`. Diagnostics go to the log file. | **Lint/CI**: `clippy::print_stdout`, `print_stderr`; the pseudo-terminal test that logs never reach the screen. |
| 35 | Output without a terminal is line-oriented, with no spinner and no cursor control. Without an interactive terminal, or with `TERM=dumb`, `whetstone` refuses before turning on any terminal mode: it prints only the refusal, on stderr, and exits with 1. An unset or empty `TERM` is not refused. Commands that only print lines (`--help`, `--version`, `areas init`) never refuse. | **Lint/CI**: `assert_cmd` and pseudo-terminal tests (`tests/it/pty.rs`, `tests/it/pty_dumb.rs`). |

## AI and storage

These rules apply from the Practice spec, when the AI crate and the session record arrive.

| # | Rule | Enforced by |
| --- | --- | --- |
| 36 | Only the AI crate imports Rig ([ADR 0001](docs/adr/0001-build-agents-with-rig.md)). The rest of the app depends on Whetstone ports and domain types. | **Lint/CI**: `deny.toml`'s `[[bans.deny]]` entries fail `cargo deny check` for any crate but `whetstone-ai` that depends on `rig`, and for any crate outside Rig that depends on one of Rig's own crates (`rig-core`, `rig-typesafeai` and the rest). **Review**: no Rig type in the AI crate's public interface. |
| 37 | LLM output is parsed into typed values and validated, never coerced. A reply that can't be read is a named failure with Retry, never Needs review. An invalid candidate is dropped before Jev checks it. | **Review**. **Lint/CI**: contract tests; the app-seam tests of each named failure (`tests/it/named_failures.rs`). |
| 38 | Jev's distributions are stored as returned. The default cut-offs live in application code and change only by a recorded decision that bumps the policy version, with a dated entry giving the reason and the evidence in [`docs/assessment-policy.md`](docs/assessment-policy.md). A learner may override them in config (`[cut_offs]`, in the global config only), and every outcome records the cut-offs that produced it. | **Review**. **Lint/CI**: the config and Settings tests (`tests/it/cut_offs_setting.rs`) and the app-seam tests of the cut-offs in effect (`tests/it/cut_offs_assessment.rs`, `tests/it/giveaway_cut_off.rs`). |
| 39 | A paid evaluation can only be built from a storage acknowledgement (a `SavedResponse` type) whose reasoning is written. | **Lint/CI**: the compiler, since the AI port's reasoning evaluation takes an acknowledgement (`whetstone_ai::Saved`) and sends the reasoning it holds, and only `SavedResponse::written_reasoning` makes one (`SavedReasoning`); reducer tests (`tests/it/reasoning_evaluation.rs`, and `tests/it/choice_only.rs` for a response with no reasoning written). **Review**: only `SavedReasoning` implements `Saved`. |
| 40 | The session store sets its durability pragmas explicitly when it opens, and tests assert them. "Saved" is shown only after `commit()` returns. | **Lint/CI**: storage tests. |

## Secrets, logging and CI

| # | Rule | Enforced by |
| --- | --- | --- |
| 41 | *From the first code that reads the key:* the API key is read once from `OPENROUTER_API_KEY` into a `SecretString` and exposed only where the auth header is built. It is never a CLI argument, never logged and never serialized. Settings shows only whether it was found. | **Review**. |
| 42 | No `std::env::set_var` or `remove_var`. Tests pass environment variables through `Command::env` or an injected `Environment`. | **Lint/CI**: `unsafe_code = "forbid"` (both are unsafe in edition 2024). |
| 43 | *From the AI crate:* `#[instrument]` on functions that take secrets, source text or learner reasoning uses `skip` or `skip_all`. Rig's content telemetry stays off. | **Review**. |
| 44 | Logging starts once, at launch, and writes to `whetstone.log` in the state directory. If a non-blocking writer (`tracing-appender`) is added, its `WorkerGuard` is bound to a named variable that lives until exit. | **Review**. |
| 45 | Workflows set least-privilege `permissions` (`contents: read` by default), pin actions to a full commit SHA with a version comment, check out with `persist-credentials: false`, and never check out PR code under `pull_request_target`. | **Review**. |

## Structure and tests

| # | Rule | Enforced by |
| --- | --- | --- |
| 46 | `main.rs` stays thin: it passes the process's arguments and environment to `whetstone::run`. Logic lives in the library, where tests can reach it. | **Review**. |
| 47 | TUI snapshots are fixed-size and committed: every screen at 120×32 and 80×24. CI runs with `CI=true`, so insta fails on a changed snapshot instead of writing one. | **Lint/CI**: insta in CI. |
| 48 | Per-PR tests make no network calls. Each HTTP test owns its mock server. The stated exceptions call `OpenRouter` with a real key, and only when asked for: the keyed end-to-end job's live tests, the recorder that re-records the offline replies, and the base-prompt measuring tests in `crates/whetstone/tests/it/measure_questions.rs`, which are ignored and never named `live_…`, so a maintainer runs them by hand and CI never does (see [The keyed end-to-end job](#the-keyed-end-to-end-job)), and the labelled-example tool (`whetstone-labels measure`), which a maintainer runs with their own key and which is never a test; the tool's own tests use mock servers. | **Lint/CI**: `ci.yml` has no API key, and the tests that need one are ignored. **Review**. |
| 49 | CI runs `cargo nextest` and `cargo test --doc` (nextest skips doc tests). `proptest-regressions` files are committed. Time-dependent async tests use a paused clock. | **Lint/CI**: the test job. **Review**: regressions and clocks. |

## Types, naming and docs

| # | Rule | Enforced by |
| --- | --- | --- |
| 50 | Exclusive states are enums and identities are newtypes. No `bool` or `Option` flag stands in for an outcome: there is no `is_correct`. | **Review**. |
| 51 | Names follow the API guidelines' C-CASE, C-CONV and C-GETTER. Domain identifiers use [`GLOSSARY.md`](GLOSSARY.md) terms: Practice, Showcase project, Change, Evidence, Area, Area prompt, Outbound review, Response, Attempt, Assessment, Jev, Generator. | **Lint/CI**: Clippy's style lints (partly). **Review**. |
| 52 | Types derive or implement `Debug`. Types holding secrets redact it. | **Review** (`missing_debug_implementations` is not enabled). |
| 53 | Docs build without warnings, and each module starts with a short `//!` header. | **Lint/CI**: `RUSTDOCFLAGS="-D warnings" cargo doc`. **Review**: module headers. |

## Project rules

| # | Rule | Enforced by |
| --- | --- | --- |
| 54 | `whetstone-core` does no I/O: no files, network, processes, environment or terminal. Because Clippy reads only the nearest `clippy.toml`, the crate's own `clippy.toml` repeats the root file's test allowances; keep the two in sync. | **Lint/CI**: `disallowed-types` and `disallowed-methods` in `crates/whetstone-core/clippy.toml`. **Review**: anything those lists miss. |
| 55 | Only `main.rs` reads the process environment (`Environment::from_process`). Everything else receives an injected `Environment`, so tests never touch the real home directory or environment. | **Review**. |
| 56 | Every crate is unpublishable (`publish = false`, inherited from the workspace) and licensed Apache-2.0. | **Review**. |
| 57 | A paste is text only in a focused text entry, and never keys: it skips the keymap and goes in as one insertion. Before it goes in, `\r\n` and a lone `\r` become line breaks, and control characters other than tab and the line break are dropped. A one-line entry, such as the search box, turns each line break and tab into a space; only a multi-line entry keeps them. Anywhere without a focused text entry, a paste does nothing. Pasted text is never logged. | **Lint/CI**: paste tests at the app seam (`tests/it/paste.rs`, and `tests/it/answering.rs` for the reasoning editor) and in a pseudo-terminal (`tests/it/pty_paste.rs`). **Review**: logging. |
| 58 | *From the reasoning editor:* writing reasoning is optional: Submit needs only a chosen option, and the empty editor says reasoning is optional. Reasoning that is empty or only whitespace is not written, and is never assessed. The reasoning editor is a multi-line text entry. While it has focus, Enter adds a line break and Esc stops editing. Tab moves to **Submit response**, and Enter there submits, so submitting never needs a modified Enter (rule 30). Of the keys that are not reserved, only Esc and Tab leave the editor; every other key edits its text (rule 29). A paste keeps its line breaks (rule 57). The reasoning's limit is 4,000 characters, counted in graphemes. A paste that would take the reasoning past it is cut at the limit, with a visible notice that it was cut. | **Lint/CI**: keystroke and paste tests at the app seam (`tests/it/answering.rs`, and `tests/it/choice_only.rs` for answering with a choice alone), and the editor's unit tests. **Review**. |

## Workspace lints

The lints in the root `Cargo.toml`, and the rules they enforce. CI denies warnings, so `warn` fails CI too.

| Lint | Level | Rule |
| --- | --- | --- |
| rustc `unsafe_code` | forbid | 7, 42 |
| rustc `rust_2018_idioms`, `unreachable_pub`, `unused_qualifications` | warn | 5 |
| rustc `let_underscore_drop` (catches `let _ = guard;`, which drops a guard at once) | warn | 5, 27 |
| Clippy `pedantic`, except `missing_errors_doc`, `missing_panics_doc` and `module_name_repetitions` | warn | 5 |
| Clippy `allow_attributes`, `allow_attributes_without_reason` | warn | 6 |
| Clippy `unwrap_used`, `panic`, `todo`, `unimplemented`, `dbg_macro` | warn | 11 |
| Clippy `exit` | warn | 14 |
| Clippy `mem_forget` (a forgotten guard never restores) | warn | 15, 27 |
| Clippy `await_holding_lock`, `await_holding_refcell_ref` | deny | 20 |
| Clippy `string_slice` | warn | 31 |
| Clippy `print_stdout`, `print_stderr` | warn | 34 |

The root `clippy.toml` allows `unwrap`, `expect`, `panic!` and printing in tests (rule 11). `crates/whetstone-core/clippy.toml` repeats that and adds the no-I/O lists (rule 54). The root file also bans Tokio's `block_in_place` and crossterm's blocking `read` and `poll` (rules 19 and 26).

## CI

`.github/workflows/ci.yml` runs on every pull request and every push to `main`. It has no Windows job.

| Job | Command | Runs on |
| --- | --- | --- |
| Format | `cargo fmt --all --check` | Linux x86_64 |
| Docs | `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --no-deps --locked` | Linux x86_64 |
| cargo-deny | `cargo deny --locked check` | Linux x86_64 (`deny.toml` checks every supported target at once) |
| cargo-shear | `cargo shear --deny-warnings --locked` | Linux x86_64 |
| Clippy and tests | `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`, then `cargo nextest run --workspace --all-features --locked --profile ci`, then `cargo test --workspace --all-features --locked --doc` | Linux and macOS, each on x86_64 and arm64 |

Run the same commands locally before pushing. The tools are `cargo-nextest`, `cargo-deny` and `cargo-shear`, at the versions the workflow pins.

### The keyed end-to-end job

`.github/workflows/live.yml` calls Jev and the generator live, so the recorded replies the per-PR contract tests replay stay true (one of rule 48's exceptions). It runs the ignored tests whose names start with `live_`: one for each kind of request the AI port makes (`crates/whetstone-ai/src/live.rs`, whose docs give the pattern for adding one), and Generate questions on the showcase project at the app seam (`crates/whetstone/tests/it/live_practice.rs`). It runs on Linux x86_64:

- on pull requests that change `crates/whetstone-ai`, the app-seam live test or the workflow, or a Rig crate's version in `Cargo.lock`;
- on manual dispatch;
- as a release gate, when a release workflow calls it (`workflow_call`) with `secrets: inherit` and makes its release job need it. Called that way, a missing key fails it rather than skipping it.

Its key is the `OPENROUTER_API_KEY` repository secret ([#96](https://github.com/monadial/whetstone/issues/96)), and only the step that runs the live tests gets it. Where the secret isn't available, such as on a pull request from a fork, the job is skipped.

With your own key, you can run the same live tests, or re-record the offline replies. Each run costs a few cents:

| What | Command |
| --- | --- |
| Live tests | `OPENROUTER_API_KEY=… cargo nextest run --workspace --all-features --locked --no-fail-fast --run-ignored only -E 'test(/::live_\w+$/)'` |
| Re-record the offline replies | `OPENROUTER_API_KEY=… cargo test -p whetstone-ai --locked record:: -- --ignored --test-threads 1` |
| Measure what Generate questions shows on the showcase, per change and area, before and after a change to the base prompts | `OPENROUTER_API_KEY=… cargo nextest run -p whetstone --locked --no-capture --run-ignored only -E 'test(/measure_questions::/)'` |

The recorder saves each reply's body under `crates/whetstone-ai/fixtures/`, never its headers, and refuses to save one that holds a key or an auth header; a per-PR test checks every fixture for the same. A re-recorded reply changes values the contract tests assert, so update them in the same PR.

## Differences from the research note

Spec #11 and the walking skeleton (#12) decided these differently from the note's proposals:

- **MSRV:** latest stable only. `rust-version` equals the pinned toolchain (1.99.1), not Rig's floor (1.95), so CI has no separate `cargo hack` MSRV job (rule 1).
- **Formatting:** default rustfmt, with no `rustfmt.toml` (rule 4).
- **Layout:** a workspace (`whetstone`, `whetstone-core`) from the start, not one package. The AI crate arrives later, so rule 36 names a crate rather than an `ai` module.
- **License:** Apache-2.0, not `MIT OR Apache-2.0` (rule 56).
- **Platforms:** Linux and macOS, x86_64 and arm64, no native Windows. The terminal support decision ([#4](https://github.com/monadial/whetstone/issues/4)) makes WSL expected to work: bugs there are fixed when reported, but neither CI nor the pre-release terminal checklist covers it. The pseudo-terminal tests use `rexpect`, which is Unix-only.
- **UI loop:** a current-thread Tokio runtime, not `rt-multi-thread`. The loop is its only async task, and blocking work runs on the runtime's blocking threads. Signals from outside still set flags, which decide what happens, and also wake the loop through Tokio's signal streams.
- **Workers:** a `TaskTracker` owns the jobs, not a `JoinSet`. Quitting never waits for a job, where the note proposes a short grace period, because `spawn_blocking` work cannot be stopped once it starts (spec [#56](https://github.com/monadial/whetstone/issues/56)).
- **Logging:** written synchronously through `tracing-subscriber`, without `tracing-appender`, so there is no `WorkerGuard` yet (rule 44).
- **Snapshots:** two sizes, 120×32 and 80×24, not four breakpoints (rule 47).
- **Keyboard flags:** the note's guard turns on keyboard enhancement flags. The terminal support decision ([#4](https://github.com/monadial/whetstone/issues/4)) instead never enables or requires them, so every action works with the keys a terminal sends without special settings (rule 27).
