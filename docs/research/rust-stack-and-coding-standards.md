# Rust stack and coding standards for the Whetstone CLI

Research note, 19 October 2026. **Status: input for later decision sessions, not a decision.**

**Scope.** This note covers crates, async architecture and coding standards for the free, open-source, BYOK Whetstone CLI/TUI. Three choices are already decided and are not reopened here: ratatui for the TUI, Tokio for async, and Rig for all agent and LLM code ([ADR 0001](../adr/0001-build-agents-with-rig.md)). Five things remain proposals that this note does not settle: help/version, fixture-first sequencing, the fourteen-piece split, the architecture, and release cutoffs.

**Method.** Every fact was checked on 2026-10-19 against a primary source: official docs, docs.rs, crate repositories (README, CHANGELOG, or source at a tag), crates.io API data, rust-lang.org, tokio.rs, ratatui.rs and Rig's repository.
- **Versions and dates** come from the crates.io API: the highest stable version and the date it was published.
- **"Repo active"** means the GitHub API reported a push in October 2026, unless a different date is given.
- **Markers:** *(inference)* means my own reasoning from the sources. *(secondary only)* means no primary source was found.
- **Layout:** each section separates **Facts** from **Proposal**.

## Summary

- **Blocking tension.** `rig-typesafeai` 0.44.0 cannot decode Jev replies from OpenRouter's `/api/v1/systemone`, because `usage.cost` arrives as a number ([rig#2782](https://github.com/0xPlaygrounds/rig/issues/2782)).
  - The fix, [rig#2800](https://github.com/0xPlaygrounds/rig/pull/2800), is marked breaking. It was merged to `main` on 2026-10-15, after 0.44.0 shipped.
  - The open [v0.45.0 release PR](https://github.com/0xPlaygrounds/rig/pull/2753) marks every Rig crate as API-breaking, and `main` already requires Rust 1.97.1 ([Cargo.toml](https://github.com/0xPlaygrounds/rig/blob/main/Cargo.toml)).
  - This confirms ADR 0001's "verify before relying on it": the Jev path does not work on 0.44.0.
- **Async shape.** One UI loop owns all state.
  - It uses `tokio::select!` over crossterm's `EventStream` and a bounded channel of typed worker results.
  - Workers live in a `JoinSet`, each with a child `CancellationToken`.
  - Git and parsing work runs on `spawn_blocking`.
  - The session record has its own writer thread, which acknowledges each commit through a `oneshot`.
- **Storage.** SQLite via `rusqlite` (`bundled`), with `journal_mode=WAL` and `synchronous=FULL` set explicitly. On macOS, durability against the drive cache also needs `fullfsync`, which SQLite calls slow. That trade-off is your call. `redb` is the alternative with simpler durability.
- **AI layer.**
  - Whetstone owns the role ports (generator, Jev), and Rig is confined to one adapter module.
  - One `reqwest` client with explicit timeouts is injected into both Rig clients.
  - Retries live in Whetstone code, because Rig does not retry.
  - Rig upgrades happen in their own PRs.
- **TUI.**
  - crossterm backend, with Elm-style update/view and an explicit focus stack.
  - `ratatui-textarea` sits behind a `ReasoningEditor` boundary that handles paste and grapheme deletion itself.
  - Whetstone owns the terminal guard, because ratatui's `restore` only resets raw mode and the alternate screen.
  - A theme layer implements NO_COLOR.
- **Hygiene.**
  - Edition 2024, with `rust-version` set to Rig's floor (1.95 today).
  - `[workspace.lints]` with `unsafe_code = "forbid"`, Clippy pedantic, and selected restriction lints.
  - `cargo-deny` plus `cargo-shear`, and nextest plus doctests on Linux, macOS and Windows.
  - `dist` for releases, licensed `MIT OR Apache-2.0`.
- **Name clash.** The `whetstone` crate name on crates.io already belongs to another project ([crates.io API](https://crates.io/api/v1/crates/whetstone)).

## Stack at a glance

All rows are proposals except the three marked *decided*.

| Concern | Proposal | Serious alternatives | Version (released) | Why | Source |
| --- | --- | --- | --- | --- | --- |
| Async runtime | `tokio` with explicit features; `tokio-util` for `CancellationToken`, `TaskTracker` (*decided: Tokio*) | none (decided) | tokio 1.53.3 (2026-10-13); tokio-util 0.7.20 (2026-10-09) | Tokio LTS line 1.53.x runs to Sept 2027 | [tokio README](https://github.com/tokio-rs/tokio/blob/tokio-1.53.3/README.md) |
| Retry and backoff | `backon` | tower `retry`; reqwest `retry`; tokio-retry2 | backon 1.6.0 (2025-10-18) | RustSec names it as the replacement for unmaintained `backoff` | [RUSTSEC-2025-0012](https://rustsec.org/advisories/RUSTSEC-2025-0012.html) |
| Terminal backend | crossterm through ratatui's default feature, plus crossterm's `event-stream` | termion (no Windows); termwiz | crossterm 0.29.0 (2025-04-05) | "Choose Crossterm for most tasks"; Windows support | [ratatui backends](https://ratatui.rs/concepts/backends/comparison/) |
| TUI library | `ratatui` (*decided*) | none | 0.30.2 (2026-06-19) | Decided; MSRV 1.88 | [crates.io](https://crates.io/crates/ratatui) |
| App pattern | Elm-style model/update/view, explicit focus stack | component, flux | n/a | Pure view, testable update | [TEA page](https://ratatui.rs/concepts/application-patterns/the-elm-architecture/) |
| Multiline editor | `ratatui-textarea` behind an owned boundary | `edtui` (vim-modal); custom editor; `tui-input` (single line) | 0.9.3 (2026-10-05) | Ratatui-org fork; ratatui 0.30; soft wrap | [repo](https://github.com/ratatui/ratatui-textarea) |
| Unicode | `unicode-segmentation` + `unicode-width` (already pulled in by ratatui) | none | 1.13.3 (2026-06-01); 0.2.2 (2025-10-06) | Ratatui's buffer is grapheme-based | [buffer source](https://docs.rs/ratatui-core/latest/src/ratatui_core/buffer/buffer.rs.html) |
| Color, plain output | Own theme layer; `std::io::IsTerminal`; `anstream` for line output | `supports-color` | anstream 1.0.0 (2026-02-11) | Ratatui leaves NO_COLOR to crossterm | [ratatui#884](https://github.com/ratatui/ratatui/issues/884) |
| AI framework | `rig` with the `typesafeai` feature (*decided*) | none | 0.44.0 (2026-10-07) | Decided; pre-1.0; MSRV 1.95.0 | [ADR 0001](../adr/0001-build-agents-with-rig.md) |
| HTTP | `reqwest` via Rig (`rig_reqwest::reqwest` re-export) | none | 0.13.5 (2026-09-08) | Rig uses it; defaults have no timeouts | [ClientBuilder](https://docs.rs/reqwest/0.13.5/reqwest/struct.ClientBuilder.html) |
| Serialization | `serde`, `serde_json`, `schemars` 1.x (Rig's major) | none | 1.0.229 (2026-07-18); 1.0.154 (2026-10-11); 1.2.3 (2026-10-12) | Rig's typed-output bounds require `JsonSchema` | [rig extractor](https://github.com/0xPlaygrounds/rig/blob/v0.44.0/crates/rig-agent/src/extractor.rs) |
| Session record | SQLite via `rusqlite` (`bundled`) on a dedicated writer thread | `redb`; atomic files (`atomic-write-file`); `sqlx` | 0.40.2 (2026-08-08) | Documented ACID in WAL+FULL; relational history | [SQLite pragma](https://www.sqlite.org/pragma.html#pragma_synchronous) |
| Git reading | Build-time extraction (open question), else `gix` | `git2` (libgit2, C) | gix 0.89.0 (2026-10-08) | Pure Rust; tree and unified diff | [gitoxide README](https://github.com/GitoxideLabs/gitoxide/blob/main/README.md) |
| Dependency graph | Deferred to dedicated ticket: `tree-sitter`+`tree-sitter-rust` or `syn` 3 | `ra_ap_*` | 0.27.1 (2026-10-08); syn 3.0.9 (2026-10-12) | See section 6 | [tree-sitter](https://tree-sitter.github.io/tree-sitter/) |
| Embedding | `include_bytes!` of generated data, or `include_dir` | `rust-embed` with `debug-embed` | include_dir 0.7.4 (2024-06-17) | Always embedded; no runtime file I/O | [include_dir](https://docs.rs/include_dir/latest/include_dir/) |
| CLI parsing | `clap` 4 derive | none | 4.6.7 (2026-09-14) | Built-in help/version and exit codes | [clap docs](https://docs.rs/clap/latest/clap/) |
| Config | `serde` + `toml` 1.x | `figment`; `config` | toml 1.1.8 (2026-10-09) | Single typed struct; no layering needed | [toml docs](https://docs.rs/toml/latest/toml/) |
| Platform dirs | `etcetera` | `directories`; `dirs` | 0.11.0 (2025-10-28) | XDG option on macOS; no MPL dependency | [etcetera docs](https://docs.rs/etcetera/latest/etcetera/) |
| Secrets | `secrecy` (`SecretString`) | `keyring` (later, OS keychain) | 0.10.3 (2024-10-09) | Redacted `Debug`, zeroize on drop | [secrecy source](https://docs.rs/crate/secrecy/latest/source/src/lib.rs) |
| Errors | `thiserror` in modules; `anyhow` at the top | `color-eyre` (ratatui recipes); `miette` | 2.0.21 (2026-09-23); 1.0.104 (2026-07-18) | Library vs application split per READMEs | [anyhow README](https://github.com/dtolnay/anyhow) |
| Logging | `tracing` + `tracing-subscriber` + `tracing-appender` to file | `tui-logger` (in-TUI view) | 0.1.44 (2025-12-18); 0.3.23 (2026-03-13); 0.2.5 (2026-04-17) | TUI owns stdout | [non_blocking](https://docs.rs/tracing-appender/latest/tracing_appender/non_blocking/index.html) |
| Snapshot tests | `insta` + ratatui `TestBackend` | buffer assertions | insta 1.49.0 (2026-10-03) | Ratatui's documented recipe | [snapshots recipe](https://ratatui.rs/recipes/testing/snapshots/) |
| HTTP mocking | `wiremock` | `httpmock` (used by `rig-cassette`); `mockito` | 0.6.5 (2025-08-24) | Async, one server per test, verified on drop | [wiremock README](https://github.com/LukeMathWalker/wiremock-rs) |
| Property tests | `proptest` | `quickcheck` | 1.12.0 (2026-10-10) | Shrinking; committed regressions | [proptest book](https://proptest-rs.github.io/proptest/) |
| PTY tests | `expectrl` (has Windows support) or `termlens` (named by ratatui) | `rexpect` (Unix only); `portable-pty` | 0.9.0 (2026-05-11); 0.11.6 (2026-10-14) | Real input, teardown, exit codes | [snapshots recipe](https://ratatui.rs/recipes/testing/snapshots/) |
| Test runner | `cargo-nextest` + `cargo test --doc` | `cargo test` | 0.9.148 (2026-10-08) | Process per test; no doctests | [nexte.st](https://nexte.st/) |
| Supply chain | `cargo-deny` + `cargo-shear` | `cargo-audit`; `cargo-machete`; `cargo-udeps`; `cargo-vet` | 0.20.2 (2026-07-09); 1.14.0 (2026-09-22) | One tool for advisories, licenses, bans and sources | [cargo-deny](https://github.com/EmbarkStudios/cargo-deny) |
| Release | `dist` (cargo-dist) | release-plz + upload-rust-binary-action; GoReleaser | crates.io 0.32.0 (2026-05-22); GitHub v0.33.0 (2026-09-11) | Installers, GitHub Releases, attestations | [dist repo](https://github.com/axodotdev/cargo-dist) |
| License | `MIT OR Apache-2.0` | none | n/a | C-PERMISSIVE | [API guidelines](https://rust-lang.github.io/api-guidelines/necessities.html#c-permissive) |

## 1. Async architecture with Tokio

### Facts

- **Runtime defaults.**
  - `#[tokio::main]` uses the multi-thread flavor by default, which "requires the `rt-multi-thread` feature flag" ([attr.main](https://docs.rs/tokio/latest/tokio/attr.main.html)).
  - Tokio's default feature set is empty ([Cargo.toml](https://raw.githubusercontent.com/tokio-rs/tokio/tokio-1.53.3/tokio/Cargo.toml)). The docs suggest `full` "if you're unsure" but warn that it "will pull in many extra dependencies" ([feature flags](https://docs.rs/tokio/latest/tokio/index.html#feature-flags)).
  - `AsyncWriteExt` needs `io-util`.
- **LTS lines.** 1.51.x is supported to March 2027 and 1.53.x to September 2027, both at MSRV 1.71. Tokio's MSRV policy is a rolling window of at least 6 months ([README](https://github.com/tokio-rs/tokio/blob/tokio-1.53.3/README.md#bug-patching-policy)).
- **Task ownership.**
  - A dropped `JoinHandle` "detaches the associated task" ([JoinHandle](https://docs.rs/tokio/latest/tokio/task/struct.JoinHandle.html)).
  - A dropped `JoinSet` aborts all of its tasks ([JoinSet](https://docs.rs/tokio/latest/tokio/task/struct.JoinSet.html)).
  - A dropped `TaskTracker` does not abort its tasks ([TaskTracker](https://docs.rs/tokio-util/latest/tokio_util/task/task_tracker/struct.TaskTracker.html)).
  - `AbortOnDropHandle` aborts its task when dropped ([docs](https://docs.rs/tokio-util/latest/tokio_util/task/struct.AbortOnDropHandle.html)).
  - `abort` only schedules cancellation, and "does not guarantee that it fails with a cancelled error" ([task module](https://docs.rs/tokio/latest/tokio/task/index.html#cancellation)).
- **Cancellation.**
  - `CancellationToken::child_token` can be cancelled without cancelling its parent, and `cancelled()` is cancel safe ([CancellationToken](https://docs.rs/tokio-util/latest/tokio_util/sync/struct.CancellationToken.html)).
  - Tokio's shutdown guide pairs `CancellationToken` (to signal) with `TaskTracker` (to wait) ([graceful shutdown](https://tokio.rs/tokio/topics/shutdown)).
  - "Cancellation is performed by dropping a future" ([select tutorial](https://tokio.rs/tokio/tutorial/select)).
- **`select!` cancel safety** ([select! docs](https://docs.rs/tokio/latest/tokio/macro.select.html#cancellation-safety)):
  - Cancel safe: `mpsc::Receiver::recv`, `StreamExt::next`, `watch::changed`, `oneshot` receive.
  - Not cancel safe: `write_all`, `read_exact`, `Mutex::lock`, `Semaphore::acquire`.
  - `mpsc::Sender::send` loses its message if cancelled, so use `reserve` instead ([Sender](https://docs.rs/tokio/latest/tokio/sync/mpsc/struct.Sender.html)).
- **Timeouts.** `time::timeout` cancels the inner future by dropping it ([timeout](https://docs.rs/tokio/latest/tokio/time/fn.timeout.html)).
- **Retries.**
  - `backon`'s `ExponentialBuilder` defaults to "jitter: false" with 3 retries. `when` filters which errors to retry; `adjust` can honour `Retry-After` ([ExponentialBuilder](https://docs.rs/backon/latest/backon/struct.ExponentialBuilder.html), [Retry](https://docs.rs/backon/latest/backon/struct.Retry.html)).
  - `backoff` is unmaintained ([RUSTSEC-2025-0012](https://rustsec.org/advisories/RUSTSEC-2025-0012.html)).
- **Blocking work.**
  - `spawn_blocking` tasks "cannot be aborted once they start running", and runtime shutdown "will wait indefinitely" for them. The docs advise: "Use dedicated threads for long-lived or persistent blocking workloads" ([spawn_blocking](https://docs.rs/tokio/latest/tokio/task/fn.spawn_blocking.html)).
  - CPU-bound work belongs on a separate pool such as rayon ([CPU-bound tasks](https://docs.rs/tokio/latest/tokio/index.html#cpu-bound-tasks-and-blocking-code)).
  - `block_in_place` panics on the current-thread runtime ([block_in_place](https://docs.rs/tokio/latest/tokio/task/fn.block_in_place.html)).
  - Dropping a `Runtime` waits forever for blocking tasks unless you call `shutdown_timeout` ([Runtime](https://docs.rs/tokio/latest/tokio/runtime/struct.Runtime.html)).
- **Channels.**
  - A bounded `mpsc` "provides backpressure". `blocking_send` and `blocking_recv` serve sync threads, and they panic if called inside an async context ([mpsc](https://docs.rs/tokio/latest/tokio/sync/mpsc/index.html)).
  - "Unbounded queues will eventually fill up all available memory" ([channels tutorial](https://tokio.rs/tokio/tutorial/channels)).
  - `watch` keeps only the latest value ([sync](https://docs.rs/tokio/latest/tokio/sync/index.html#message-passing)).
- **Locks.**
  - The std `Mutex` is "ok and often preferred", as long as the guard is not held across `.await` ([tokio Mutex](https://docs.rs/tokio/latest/tokio/sync/struct.Mutex.html), [shared state](https://tokio.rs/tokio/tutorial/shared-state)).
  - Clippy's `await_holding_lock` and `await_holding_refcell_ref` are in the `suspicious` group ([lint list](https://clippy.rust-lang.org/master/index.html#await_holding_lock)).
- **`tokio::fs`.** It "will always use spawn_blocking". Write calls "return before the write has finished", so call `flush`. `sync_all` is needed for data to reach the filesystem ([fs](https://docs.rs/tokio/latest/tokio/fs/index.html), [File](https://docs.rs/tokio/latest/tokio/fs/struct.File.html)).
- **Signals and raw mode.**
  - `signal::ctrl_c` replaces the platform default handler "for the duration of the entire process" ([ctrl_c](https://docs.rs/tokio/latest/tokio/signal/fn.ctrl_c.html)).
  - In raw mode, "CTRL+C will not be processed by terminal driver" ([crossterm raw mode](https://docs.rs/crossterm/latest/crossterm/terminal/index.html#raw-mode)).
  - *(inference from crossterm and rustix source)* Under raw mode, Ctrl+C arrives as a key event. A SIGINT or SIGTERM sent from outside the terminal still arrives as a signal.
- **Terminal input.**
  - `EventStream` needs crossterm's `event-stream` feature. `poll`/`read` must not be combined "with the EventStream" ([crossterm event](https://docs.rs/crossterm/latest/crossterm/event/index.html)).
  - Ratatui's async recipe runs `EventStream` inside `tokio::select!`, together with a token, a tick interval and an unbounded channel ([recipe](https://ratatui.rs/recipes/apps/terminal-and-event-handler/)).
- **Testing.** `#[tokio::test(start_paused = true)]` needs `test-util`, and the paused clock auto-advances to the next timer ([attr.test](https://docs.rs/tokio/latest/tokio/attr.test.html), [pause](https://docs.rs/tokio/latest/tokio/time/fn.pause.html)).

### Proposal

- **Features.** `tokio = { version = "1.53", features = ["rt-multi-thread", "macros", "sync", "time", "signal", "io-util"] }`. Add `fs` only if async file I/O is actually used; the writer thread uses `std::fs`. If you want to stay on the LTS line, the README's tilde form `~1.53` keeps you there.
- **One owner loop.**
  - `main` builds the terminal guard, then runs `loop { select! { biased; ev = events.next() => …, Some(msg) = results.recv() => …, _ = tick.tick(), if animating => …, _ = shutdown.cancelled() => break } ; update(); draw() }`.
  - Every branch uses a cancel-safe operation from the list above.
  - The tick only runs while a spinner needs it, as the kit's design handoff asks.
- **Workers.**
  - The owner holds a `JoinSet` (or a `TaskTracker` plus an `AbortOnDropHandle` per job).
  - Each job gets `root.child_token()` and sends `JobStage { job_id, stage }` and `JobDone { job_id, attempt_id, source_version, result }` through a bounded `mpsc`.
  - `update` discards any result whose identity does not match the outstanding job. This matches the kit's stale-result rule; Tokio's own docs say `abort` may still let a task finish.
- **Cancellation UX.**
  - Cancel means `token.cancel()`, after which the job reports `Cancelled`.
  - Rig documents that "dropping an operation cancels local work, not accepted remote work" ([rig-reqwest](https://github.com/0xPlaygrounds/rig/blob/v0.44.0/crates/rig-reqwest/src/lib.rs)). The UI must therefore not claim that a cancelled paid request was not billed.
- **Timeouts and retries.**
  - One `reqwest::Client` with `connect_timeout` and `timeout`, plus `tokio::time::timeout` per stage.
  - `backon` with `with_jitter()` and `when(is_retryable)` (see section 3).
  - Whether a paid request may be retried automatically is an open question. The kit says "A retry is explicit and traceable".
- **Blocking work.**
  - Git reads and graph building run on `spawn_blocking`; they are short and bounded.
  - The SQLite connection lives on one `std::thread` that takes commands over a bounded channel and replies through `oneshot`. This follows Tokio's "dedicated threads" advice.
  - No `block_in_place`, enforced through `disallowed-methods` (section 9).
- **Shutdown and signals.**
  - Ctrl+C as a key, `signal::ctrl_c()`, and on Unix `tokio::signal::unix` SIGTERM/SIGHUP ([unix signals](https://docs.rs/tokio/latest/tokio/signal/unix/index.html)) all cancel the root token.
  - Shutdown then waits for workers within a short grace period, restores the terminal, and joins the writer thread after its final flush.

## 2. TUI architecture with ratatui

### Facts

- **0.30 release.**
  - Ratatui 0.30 split into `ratatui-core`, `ratatui-widgets`, backend crates and `ratatui-macros`, but "`ratatui` remains the recommended starting point" for apps ([docs.rs](https://docs.rs/ratatui/latest/ratatui/)).
  - MSRV is 1.88 from 0.30.1 ([CHANGELOG](https://raw.githubusercontent.com/ratatui/ratatui/main/CHANGELOG.md)).
  - Changes that affect apps: `Alignment` became `HorizontalAlignment`, `Flex::SpaceAround` changed meaning, and there are now per-version crossterm features ([v0.30 highlights](https://ratatui.rs/highlights/v030/)).
- **Backends.** Crossterm is the default backend, using crossterm 0.29. The docs say "Choose Crossterm for most tasks", and recommend it when Windows matters ([backends](https://ratatui.rs/concepts/backends/), [comparison](https://ratatui.rs/concepts/backends/comparison/)). crossterm's last release was 0.29.0 on 2025-04-05; its repo is active.
- **Patterns.**
  - The Elm Architecture page says `view` "shouldn't modify global state or perform any other actions" ([TEA](https://ratatui.rs/concepts/application-patterns/the-elm-architecture/)).
  - Component Architecture keeps state private to each component ([component](https://ratatui.rs/concepts/application-patterns/component-architecture/)).
  - Flux uses a dispatcher, stores and actions ([flux](https://ratatui.rs/concepts/application-patterns/flux-architecture/)).
  - The event-handling page concludes "the correct way, is the one that works for you" ([event handling](https://ratatui.rs/concepts/event-handling/)).
  - The component template sends every event to every component, so it has no focus model ([app.rs](https://ratatui.rs/templates/component/app-rs/)).
- **Restore.**
  - `ratatui::init` enables raw mode and the alternate screen, and installs a panic hook that "restores the terminal before panicking". It should run "after your app installs any other panic hooks" ([init](https://docs.rs/ratatui/latest/ratatui/fn.init.html)).
  - `restore` undoes only raw mode and the alternate screen, and prints and ignores errors ([restore](https://docs.rs/ratatui/latest/ratatui/fn.restore.html)).
  - `run` is `init; f; restore` with no guard ([init.rs source](https://docs.rs/ratatui/latest/src/ratatui/init.rs.html)).
  - The panic-hook recipe: "avoid panicking while restoring the terminal state" ([recipe](https://ratatui.rs/recipes/apps/panic-hooks/)).
- **Keys and paste.**
  - Windows reports key presses and releases, so filter on `KeyEventKind::Press` ([FAQ](https://ratatui.rs/faq/)).
  - `Event::Paste` is emitted only with bracketed paste enabled, which is unsupported in older Windows terminals ([Event](https://docs.rs/crossterm/0.29.0/crossterm/event/enum.Event.html), [EnableBracketedPaste](https://docs.rs/crossterm/0.29.0/crossterm/event/struct.EnableBracketedPaste.html)).
  - The kitty keyboard protocol is listed as supported only by kitty, foot, WezTerm, alacritty, notcurses, neovim, kakoune and dte ([PushKeyboardEnhancementFlags](https://docs.rs/crossterm/0.29.0/crossterm/event/struct.PushKeyboardEnhancementFlags.html)).
  - Modified Enter is still an open tracking issue ([crossterm#685](https://github.com/crossterm-rs/crossterm/issues/685)).
- **NO_COLOR.**
  - Programs should suppress color when `NO_COLOR` is set and non-empty. Flags and config "should override" it. Bold, underline and italic are not covered ([no-color.org](https://no-color.org/)).
  - Ratatui leaves NO_COLOR to crossterm ([ratatui#884](https://github.com/ratatui/ratatui/issues/884)).
  - crossterm checks `NO_COLOR` ([Colored](https://docs.rs/crossterm/0.29.0/crossterm/style/enum.Colored.html)). Its unreleased CHANGELOG fixes "color commands emitting a bare `CSI m` when colors are disabled via `NO_COLOR`, which reset every attribute" ([CHANGELOG](https://raw.githubusercontent.com/crossterm-rs/crossterm/master/CHANGELOG.md)).
  - `force_color_output` overrides NO_COLOR ([fn](https://docs.rs/crossterm/0.29.0/crossterm/style/fn.force_color_output.html)).
  - Ratatui's `Color::Rgb` displays correctly only on truecolor terminals ([Color](https://docs.rs/ratatui/latest/ratatui/style/enum.Color.html)).
- **Editor widgets.**
  - `ratatui-textarea` is "a Ratatui fork of tui-textarea and maintained independently". The original `tui-textarea` has had no release since 2024-10-22 and its repo was last pushed 2024-12-01 ([repo](https://github.com/ratatui/ratatui-textarea), [discussion](https://github.com/ratatui/ratatui/discussions/2367)).
  - Features: undo/redo, selection, search, soft wrap ("respects Unicode grapheme boundaries", 0.9.0), and `input_without_shortcuts`. Default bindings include Ctrl+C = copy, and Tab inserts spaces ([README](https://raw.githubusercontent.com/ratatui/ratatui-textarea/main/README.md), [CHANGELOG](https://raw.githubusercontent.com/ratatui/ratatui-textarea/main/CHANGELOG.md)).
  - Edits work per `char`, not per grapheme ([source](https://docs.rs/ratatui-textarea/latest/src/ratatui_textarea/textarea.rs.html)). Crossterm `Paste` events are not handled ([input source](https://docs.rs/ratatui-textarea/latest/src/ratatui_textarea/input/crossterm.rs.html)). `insert_str` treats `\n` and `\r\n` as newlines but not a lone `\r` ([TextArea](https://docs.rs/ratatui-textarea/latest/ratatui_textarea/struct.TextArea.html)).
  - `edtui` is "inspired by Vim" and modal ([edtui](https://github.com/preiter93/edtui)). `tui-input` steps by grapheme but has no concept of lines ([Input](https://docs.rs/tui-input/latest/tui_input/struct.Input.html)).
- **Unicode.**
  - Ratatui's buffer splits text into extended graphemes, measures width with `unicode-width`, and skips control and zero-width graphemes ([buffer.rs](https://docs.rs/ratatui-core/latest/src/ratatui_core/buffer/buffer.rs.html)).
  - `unicode-width` gives emoji ZWJ sequences width 2, and its rules "may be tweaked in the future" ([unicode-width](https://docs.rs/unicode-width/0.2.2/unicode_width/)).
  - An emoji rendering fix ([ratatui#2357](https://github.com/ratatui/ratatui/issues/2357)) closed after 0.30.2 was released *(inference: unreleased)*.
  - Narrow-grapheme stale cells are an open bug ([ratatui#2826](https://github.com/ratatui/ratatui/issues/2826)).
- **Layout and resize.**
  - `Length` "is not responsive" ([layout](https://ratatui.rs/concepts/layout/)).
  - Fullscreen viewports resize automatically during `draw`; resizing resets the previous buffer ([Terminal](https://docs.rs/ratatui/latest/ratatui/struct.Terminal.html)).
  - `Layout::areas` panics on a constraint-count mismatch; `try_areas` returns an error instead ([Layout](https://docs.rs/ratatui/latest/ratatui/layout/struct.Layout.html)).
- **Accessibility.** Ratatui has no screen-reader integration, and a maintainer judged the "impedance mismatch is probably too great" ([discussion 2190](https://github.com/ratatui/ratatui/discussions/2190)).

### Proposal

- **Backend.**
  - Use ratatui's default crossterm backend.
  - Add a direct `crossterm = { version = "0.29", features = ["event-stream"] }` that matches ratatui's crossterm version.
  - Read input from one `EventStream` only, enforced by `disallowed-methods` on `crossterm::event::{read, poll}`.
- **Pattern.** Use Elm-style `update(&mut State, Msg) -> Vec<Effect>` and `view(&State, &mut Frame)`. Mutable state is fine; the kit's architecture notes already allow it. Widget-local state, such as the editor and scroll offsets, lives in the model.
- **Key routing.**
  - The pure function `map_key(&Focus, KeyEvent) -> Option<Msg>` checks, in order:
    1. Reserved global keys, which is Ctrl+C only.
    2. The top overlay.
    3. The focused control.
  - When the editor has focus, every key except Tab, Shift+Tab, Esc and Ctrl+C becomes text.
  - The vim-like keys (`j/k/g/G/q/:/?/A/B/C`) are active only while the focus is not an editor.
- **Terminal guard.**
  - Write a `TerminalSession` that enables raw mode, the alternate screen, bracketed paste, cursor hiding, and keyboard flags (only after detection succeeds).
  - It undoes all of these in `Drop` and in its own panic hook, mirroring ratatui's 30-line `init.rs`.
  - Any `color-eyre` or `anyhow` hook must be installed before this guard.
  - Keep `panic = "unwind"` so that `Drop` runs (section 9).
- **Editor.**
  - Use `ratatui-textarea` 0.9 behind a `ReasoningEditor` type, driven through `input_without_shortcuts` and direct method calls.
  - The boundary itself handles four things:
    - Paste: normalise `\r\n` and `\r` to `\n`, then call `insert_str`.
    - Grapheme-aware Backspace and Delete, using `unicode-segmentation`.
    - Tab: leave focus instead of inserting spaces.
    - Ctrl+C: never reaches the widget.
  - Cover the boundary with proptest and keystroke tests. If grapheme fixes grow too large, replace the widget behind the same type.
- **Color.**
  - Theme tokens map to four capability levels: truecolor, 256, ANSI16, and none.
  - Precedence: an explicit flag or config wins; then a non-empty `NO_COLOR` selects none; otherwise use a conservative default. `COLORTERM` is a convention with no primary specification *(secondary only)*.
  - With no color, emit `Color::Reset` and keep the kit's `(x)` and `>` markers.
  - *(inference)* Calling `force_color_output(true)` once, with the theme as the single NO_COLOR authority, avoids crossterm 0.29.0's bare-`CSI m` reset. Verify this with a buffer test plus a PTY test.
- **Plain output.**
  - Use `std::io::IsTerminal` to choose line-oriented output, with no spinner and no cursor control ([IsTerminal](https://doc.rust-lang.org/std/io/trait.IsTerminal.html)). RustSec points users of `atty` to it ([RUSTSEC-2024-0375](https://rustsec.org/advisories/RUSTSEC-2024-0375.html)).
- **Geometry.**
  - Pick the breakpoint from `frame.area()` on every draw, using width and height as the kit's interaction contract specifies.
  - State never stores `Rect`s, so a resize preserves selection, focus, cursor and scroll.

## 3. The AI layer with Rig

### Facts

- **Crate layout.**
  - `rig` is a facade. It "re-exports `rig_core`" and, under the default `agent` feature, `rig_agent`.
  - Its features include `typesafeai`, `cassette`, `test-utils`, `reqwest`, and `rustls`/`native-tls`.
  - 0.44.0 declares `rust-version = "1.95.0"` ([Cargo.toml](https://github.com/0xPlaygrounds/rig/blob/v0.44.0/Cargo.toml)).
- **Churn.**
  - "Rig is pre-1.0. Releases contain breaking changes" ([README](https://github.com/0xPlaygrounds/rig/blob/main/README.md)).
  - Each minor from 0.40 to 0.44 lists 17 to 48 `[breaking]` entries. Releases came 1 to 6 weeks apart ([CHANGELOG](https://github.com/0xPlaygrounds/rig/blob/v0.44.0/CHANGELOG.md)).
- **0.43 rewrote the surface** ([MIGRATING.md](https://github.com/0xPlaygrounds/rig/blob/v0.44.0/MIGRATING.md)):
  - `client.agent(m)` became `AgentBuilder::new(client.completion(m))`.
  - `client.extractor::<T>(m)` became `ExtractorBuilder::<T>::new(…)`.
  - The `Prompt`/`Chat`/`TypedPrompt` traits were removed; their methods are now inherent on `Agent`.
  - `CompletionError` and its siblings became `ProviderError`.
- **Typed output.**
  - The extractor does "Typed extraction through an agent's `submit` output tool, with configurable retries". It requires `T: JsonSchema + DeserializeOwned + Serialize` ([extractor.rs](https://github.com/0xPlaygrounds/rig/blob/v0.44.0/crates/rig-agent/src/extractor.rs)).
  - `prompt_typed::<T>()` returns `TypedPromptResponse { output, usage, … }`.
  - "Tool and prompted modes are best-effort; callers must validate returned values" ([output.rs](https://github.com/0xPlaygrounds/rig/blob/v0.44.0/crates/rig-agent/src/run/output.rs)).
  - rig-core re-exports `schemars`, at `schemars = "1"` ([lib.rs](https://github.com/0xPlaygrounds/rig/blob/v0.44.0/crates/rig-core/src/lib.rs)).
  - On the OpenAI-shaped wire that OpenRouter uses, Rig sends `"strict": true` and rewrites schemas to "Require all properties … `additionalProperties: false`" ([openai/mod.rs](https://github.com/0xPlaygrounds/rig/blob/v0.44.0/crates/rig-core/src/providers/openai/mod.rs)).
- **Providers.**
  - A model is "a Wire … with a Transport" ([driver.rs](https://github.com/0xPlaygrounds/rig/blob/v0.44.0/crates/rig-core/src/driver.rs)).
  - `providers::openrouter` reads `OPENROUTER_API_KEY` in `from_env()` and also offers `new(key)`. `with_base_url` overrides the endpoint, and every client accepts `with_http(...)` ([dialects.rs](https://github.com/0xPlaygrounds/rig/blob/v0.44.0/crates/rig-core/src/providers/openai/wire/dialects.rs), [macros.rs](https://github.com/0xPlaygrounds/rig/blob/v0.44.0/crates/rig-core/src/client/macros.rs)).
  - The shared default HTTP client is `reqwest::Client::builder().build()`. Timeout policy "remains with the host-built client" ([rig-reqwest README](https://github.com/0xPlaygrounds/rig/blob/v0.44.0/crates/rig-reqwest/README.md)).
  - "Rig doesn't retry failed model calls for you". `ProviderError::is_retryable()` covers transport errors, timeouts, 408/429/5xx and truncation ([error handling](https://rig.rs/docs/concepts/error_handling)).
- **rig-typesafeai.**
  - Described as "Experimental typed Jev evaluations alongside Rig agents". Decisions keep their probability distributions; "routing and threshold policy remain in application code"; probabilities "are not silently normalized" ([lib.rs](https://github.com/0xPlaygrounds/rig/blob/v0.44.0/crates/rig-typesafeai/src/lib.rs)).
  - Query types are `Noul`, `Choice`, `Score` and composites. `JevConfig::new(token)` defaults to `jev-latest` at `https://api.typesafe.ai/v1/systemone`. `from_env()` reads `JEV_TOKEN`. `with_endpoint` sets the full URL ([wire.rs](https://github.com/0xPlaygrounds/rig/blob/v0.44.0/crates/rig-typesafeai/src/wire.rs)).
  - The OpenRouter decode bug and its unreleased, breaking fix are covered in the Summary. `main` also removed `Jev::with_http` in favour of `JevConfig::connect`.
- **Jev on OpenRouter.**
  - It is reached with `POST https://openrouter.ai/api/v1/systemone`. Model IDs include `typesafe/jev-1.14`, `typesafe/jev-1.13` and `~typesafe/jev-latest`. Every response includes `usage.cost` in USD ([OpenRouter Jev guide](https://openrouter.ai/docs/guides/community/jev)).
  - Auth uses the OpenRouter key as a Bearer token ([TypeSafe SDK guide](https://openrouter.ai/docs/guides/community/typesafe-sdk)).
  - "Calibration … does not guarantee that an individual answer is correct" ([System One](https://docs.typesafe.ai/concepts/system-one)).
- **Observability and testing.**
  - Rig emits OpenTelemetry GenAI spans; content recording is opt-in ([observability](https://rig.rs/docs/concepts/observability)).
  - `test-utils` provides `MockCompletionModel` ([testing](https://rig.rs/docs/concepts/testing)).
  - `rig-cassette` records and replays HTTP exchanges. Its `http` feature is httpmock-based ([rig-cassette](https://github.com/0xPlaygrounds/rig/blob/v0.44.0/crates/rig-cassette/src/lib.rs)).

### Proposal

- **Ports owned by Whetstone.**
  - Create an `ai` module with the role traits `QuestionGenerator`, `Explainer` (both the generator role) and `Evaluator` (the Jev role), plus domain types such as `Probability(f64)`, `Decision<T> { distribution, selected, confidence, model, cost }` and `AiError`.
  - Only this module imports `rig::*`.
  - If `rig-typesafeai` lives in its own small crate, `cargo-deny` can enforce the boundary with `[bans] deny = [{ name = "rig-typesafeai", wrappers = ["<adapter crate>"] }]` ([bans config](https://embarkstudios.github.io/cargo-deny/checks/bans/cfg.html)).
- **Jev adapter.**
  - Build it with `JevConfig::new(openrouter_key).model("typesafe/jev-1.14").with_endpoint("https://openrouter.ai/api/v1/systemone").connect(http)`.
  - Never use `from_env()`, because it reads `JEV_TOKEN` and targets TypeSafe's own host.
  - Until a release contains #2800, the choices are:
    1. Wait for 0.45, which likely raises the MSRV *(inference)*.
    2. Pin a git revision at or after `5525e29`.
    3. Carry a `[patch.crates-io]` fork of rig-typesafeai 0.44.0 with only that fix ([overriding dependencies](https://doc.rust-lang.org/cargo/reference/overriding-dependencies.html)).
  - Put Jev behind a Cargo feature so the generator path builds without it.
- **Generator adapter.**
  - Build it with `openrouter::new(key)`, the same key, and the same injected HTTP client.
  - Use `ExtractorBuilder` or `prompt_typed` for typed outputs, with schemars derives taken from `rig::schemars` so the major versions cannot skew.
  - Validate every parsed value. Rig calls tool and prompted modes "best-effort".
- **HTTP.**
  - Build one `reqwest::Client` with `connect_timeout` and `timeout` (re-exported as `rig_reqwest::reqwest`), wrap it with `ReqwestClient::from`, and pass it to both adapters.
  - Retries live in Whetstone, using `backon` with `is_retryable()`.
- **Schema design.**
  - Because Rig's strict rewriting makes every property required, `Option` fields must come back as `null`.
  - Use internally tagged enums for exclusive outcomes.
  - Do not combine `deny_unknown_fields` with `flatten`, which serde does not support ([container attrs](https://serde.rs/container-attrs.html)).
- **Paid-request gate.** The evaluation adapter takes a `SavedResponse` value that only the storage acknowledgement can construct. This makes "evaluate before durable save" a compile error *(design proposal)*.
- **Versioning.**
  - `rig = "0.44"` (caret) already excludes 0.45 ([caret semantics](https://doc.rust-lang.org/cargo/reference/specifying-dependencies.html)).
  - Upgrade Rig in a dedicated PR that re-runs the adapter contract tests.
  - Record model IDs, prompt versions and schema versions with each assessment, as the kit draft also asks.
- **Tests.** Use Rig's `MockCompletionModel` for generator logic, and a mock HTTP server (via `with_base_url` and `with_endpoint`) that replays a recorded OpenRouter Jev body, including the numeric `usage.cost`.

## 4. HTTP and serialization beneath Rig

### Facts

- **reqwest through Rig.** The Rig workspace depends on `reqwest = { version = "0.13", default-features = false }`. `rig-reqwest` enables `json`, `stream`, and with `rustls` also `http2` and `system-proxy` ([rig Cargo.toml](https://github.com/0xPlaygrounds/rig/blob/v0.44.0/Cargo.toml)).
- **reqwest 0.13 defaults.**
  - "`rustls` is now the default TLS backend", with the aws-lc crypto provider ([CHANGELOG](https://github.com/seanmonstar/reqwest/blob/master/CHANGELOG.md)).
  - Timeouts: "Default is no timeout". Redirects are followed up to 10 ([ClientBuilder](https://docs.rs/reqwest/0.13.5/reqwest/struct.ClientBuilder.html)).
  - "System proxies are enabled by default" ([crate docs](https://docs.rs/reqwest/0.13.5/reqwest/)).
  - A `retry` module exists; by default it retries protocol NACKs ([retry](https://docs.rs/reqwest/0.13.5/reqwest/retry/index.html)).
- **schemars 1.x.**
  - Targets JSON Schema 2020-12 by default ([SchemaSettings](https://docs.rs/schemars/1.2.3/schemars/generate/struct.SchemaSettings.html)).
  - Honours serde's `rename`, `tag`, `flatten` and `deny_unknown_fields`, and supports `#[schemars(crate = "…")]` ([attributes](https://graham.cool/schemars/deriving/attributes/)).
- **serde enums.** Internally tagged enums reject tuple variants. Untagged enums try the variants in order ([enum representations](https://serde.rs/enum-representations.html)).
- **OpenRouter structured outputs.** Use `response_format` with `json_schema`, `strict: true` and `require_parameters: true`. "Exact compliance is not guaranteed on every endpoint" ([structured outputs](https://openrouter.ai/docs/features/structured-outputs)).
- **Feature unification.** rig-core enables serde_json's `preserve_order`, `float_roundtrip` and `raw_value` for the whole build ([rig Cargo.toml](https://github.com/0xPlaygrounds/rig/blob/v0.44.0/Cargo.toml)).

### Proposal

- Do not add `reqwest` as a direct dependency; use Rig's re-export so there is only one reqwest version.
- *(unverified)* aws-lc may need a C toolchain on some targets. Check it in the first CI run on all three OSes.
- Keep serde types for LLM output in the `ai` module, separate from the storage types; map between them explicitly.

## 5. Local storage for the session record

### Facts

- **SQLite synchronous levels** ([pragma](https://www.sqlite.org/pragma.html#pragma_synchronous)):
  - "FULL is … durable (ACID) in WAL mode", but "not necessarily durable across a power loss in rollback mode". EXTRA fixes that for rollback mode.
  - WAL with NORMAL "might roll back following a power loss or system crash".
  - "Transactions are durable across application crashes regardless of the synchronous setting".
- **SQLite defaults.** `synchronous` is FULL ([compile](https://www.sqlite.org/compile.html#default_synchronous)) and the journal mode is DELETE ([journal_mode](https://www.sqlite.org/pragma.html#pragma_journal_mode)).
- **macOS fullfsync.** It is "off" by default ([fullfsync](https://www.sqlite.org/pragma.html#pragma_fullfsync)). SQLite says it "is profoundly slow … So its use is not recommended" ([atomic commit](https://www.sqlite.org/atomiccommit.html)). Apple's fsync(2) states that drives may hold data in their caches and that F_FULLFSYNC flushes it ([Apple fsync](https://developer.apple.com/library/archive/documentation/System/Conceptual/ManPages_iPhoneOS/man2/fsync.2.html)).
- **Corruption guidance.** "Most consumer-grade mass storage devices lie about syncing". Don't link multiple copies of SQLite into one binary ([howtocorrupt](https://www.sqlite.org/howtocorrupt.html)).
- **rusqlite.**
  - `bundled` ships SQLite 3.53.2 ([README](https://github.com/rusqlite/rusqlite/blob/master/README.md)).
  - `Connection` is `Send` but not `Sync` ([Connection](https://docs.rs/rusqlite/latest/rusqlite/struct.Connection.html)).
  - A `Transaction` rolls back on drop ([Transaction](https://docs.rs/rusqlite/latest/rusqlite/struct.Transaction.html)).
  - `tokio-rusqlite` 0.8.0 spawns "a thread … for each opened connection" ([docs](https://docs.rs/tokio-rusqlite/latest/tokio_rusqlite/)).
- **sqlx 0.9.**
  - "SQLx does not set a journal mode by default"; `synchronous` defaults to FULL ([SqliteConnectOptions](https://docs.rs/sqlx/latest/sqlx/sqlite/struct.SqliteConnectOptions.html)).
  - The compile-time query macros need `DATABASE_URL` or committed `.sqlx` data ([query!](https://docs.rs/sqlx/latest/sqlx/macro.query.html)).
  - MSRV is 1.94.
  - It cannot coexist with rusqlite 0.40: both need `libsqlite3-sys` with `links = "sqlite3"`, in incompatible ranges ([links rule](https://doc.rust-lang.org/cargo/reference/build-scripts.html)).
- **redb 4.4.**
  - Default `Durability::Immediate` is "guaranteed to be persistent as soon as `WriteTransaction::commit` returns" ([Durability](https://docs.rs/redb/latest/redb/enum.Durability.html)).
  - The database file is locked against a second open ([Database](https://docs.rs/redb/latest/redb/struct.Database.html)).
  - 3.0 dropped file format v2, and 2.6.x remains the migration line ([CHANGELOG](https://github.com/cberner/redb/blob/master/CHANGELOG.md)).
- **Plain files.**
  - On Apple targets, std's `sync_all` and `sync_data` call `fcntl(F_FULLFSYNC)` ([std unix fs source](https://github.com/rust-lang/rust/blob/master/library/std/src/sys/fs/unix.rs)).
  - Linux fsync "does not necessarily ensure that the entry in the directory … has also reached disk" ([fsync(2)](https://man7.org/linux/man-pages/man2/fsync.2.html)).
  - `tempfile`'s `persist` does not sync the file or the directory ([NamedTempFile](https://docs.rs/tempfile/latest/tempfile/struct.NamedTempFile.html)).
  - `atomic-write-file` runs `sync_all`, then rename, then a directory fsync, but on Unix only ([source](https://docs.rs/crate/atomic-write-file/0.3.1/source/)).
  - std's Windows rename passes no `MOVEFILE_WRITE_THROUGH` ([std windows fs source](https://github.com/rust-lang/rust/blob/master/library/std/src/sys/fs/windows.rs), [MoveFileExW](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-movefileexw)).
- **Other stores.** `fjall` writes are not durable until `persist(SyncAll)` ([PersistMode](https://docs.rs/fjall/latest/fjall/enum.PersistMode.html)). `sled` says "if reliability is your primary constraint, use SQLite" ([sled README](https://github.com/spacejam/sled/blob/main/README.md)).

| Option | Durable commit | Documented guarantee | Main caveat |
| --- | --- | --- | --- |
| SQLite (rusqlite) | WAL + FULL; on macOS add `fullfsync` | ACID in WAL+FULL | macOS default is plain fsync; sync API |
| redb | default `Immediate` | persistent when `commit` returns | key-value only; format churn across majors |
| atomic files | `atomic-write-file` commit | none stated; code syncs file and dir (Unix) | no directory flush on Windows |
| sqlx (SQLite) | WAL set explicitly, FULL default | same as SQLite | heavy; macros; conflicts with rusqlite |

### Proposal

- **Engine.** Use `rusqlite` with `bundled`. One `std::thread` owns the `Connection`. At open it sets `journal_mode=WAL`, `synchronous=FULL` and `foreign_keys=ON`, and a test asserts them.
- **Saves.**
  - Each save is one transaction.
  - The thread replies `Ok(SaveAck { record_version })` on a `oneshot` only after `commit()` returns.
  - "Saved" and the paid-evaluation effect both depend on that acknowledgement. This matches the kit's acceptance criterion: "Show **Saved** only after an acknowledged durable local write".
- **Durability bar (open question).**
  - Surviving an app crash comes for free in any mode.
  - Surviving an OS crash or power loss needs WAL+FULL.
  - Durability against the drive cache on macOS needs `fullfsync=ON`.
  - *(inference)* At a handful of writes per session, the cost of `fullfsync` is probably acceptable; measure it.
- **Alternatives if SQLite is rejected.**
  - `redb`, if its key-value model is enough. Its default commit is durable, and std makes it F_FULLFSYNC on macOS *(inference)*.
  - One JSON file per session through `atomic-write-file`, if Windows directory durability is acceptable.
- **Not recommended:** `sqlx`, which brings compile-time macro machinery and async pools to a single local file, and blocks rusqlite.

## 6. Git, code analysis and embedding (brief)

### Facts

- **git2.**
  - Binds libgit2 (C). It needs `cc`, can be vendored with `vendored-libgit2`, and its `Repository` is not `Sync` ([git2 README](https://github.com/rust-lang/git2-rs/blob/master/README.md), [Repository](https://docs.rs/git2/latest/git2/struct.Repository.html)).
  - For in-memory use it offers `Odb::new`, `add_new_mempack_backend` and `Repository::from_odb` ([Odb](https://docs.rs/git2/latest/git2/struct.Odb.html)).
- **gix.**
  - Pure Rust. The README rates it "usable … possibly incomplete functionality".
  - Tree diffs, line diffs and rename detection are implemented. `UnifiedDiff` renders git-style output.
  - It has no pure in-memory open; `with_object_memory` layers over an existing repository. Bundles are unimplemented.
  - "Increasing the MSRV is not considered a breaking change".
  - Sources: [README](https://github.com/GitoxideLabs/gitoxide/blob/main/README.md), [crate-status](https://github.com/GitoxideLabs/gitoxide/blob/main/crate-status.md), [STABILITY](https://github.com/GitoxideLabs/gitoxide/blob/main/STABILITY.md), [gix-diff](https://docs.rs/gix-diff/latest/gix_diff/blob/index.html).
- **tree-sitter.**
  - The runtime is "pure C11", built through `cc`. It is "Robust enough … in the presence of syntax errors" ([tree-sitter](https://tree-sitter.github.io/tree-sitter/)).
  - `tree-sitter-rust` ships a `TAGS_QUERY` ([docs](https://docs.rs/tree-sitter-rust/latest/tree_sitter_rust/)).
- **syn.** Version 3.0 (2026-07-18) is a new major ([releases](https://github.com/dtolnay/syn/releases)). It parses syntax only *(inference: no name resolution or macro expansion)*.
- **rust-analyzer crates.** The `ra_ap_*` crates publish weekly and need MSRV 1.98. "The `ide` API of rust-analyzer are explicitly unstable" ([architecture](https://rust-analyzer.github.io/book/contributing/architecture.html)).
- **Embedding.**
  - `include_dir` always embeds, has had no release since 2024-06-17, and keeps `metadata` off for reproducibility ([docs](https://docs.rs/include_dir/latest/include_dir/)).
  - `rust-embed` reads from disk in debug builds unless `debug-embed` is set, and can compress ([README](https://static.crates.io/readmes/rust-embed/rust-embed-8.13.0.html)).

### Proposal

- **Two showcase-project options to decide** (open question):
  - **(a)** A `build.rs` or `xtask` uses `gix` once at build time to extract the curated commits, diffs and file snapshots into versioned data, embedded with `include_bytes!` ([include_bytes](https://doc.rust-lang.org/std/macro.include_bytes.html)). The shipped binary then needs no git crate and no C *(inference)*.
  - **(b)** Embed a bare repository, extract it to a temporary directory, and read it with `gix` at runtime. This reuses the same code path for the later user-supplied repositories.
- **git crate preference.** Prefer `gix` over `git2` either way: no C toolchain, active, diff support.
- **Dependency graph.** Leave it to the dedicated ticket. My early read *(inference)*:
  - `tree-sitter` for error-tolerant, name-based one-hop edges.
  - `syn` 3 if the code is always valid Rust and pure Rust is preferred.
  - `ra_ap_*` is too heavy and unstable for the binary.

## 7. CLI and app plumbing

### Facts

- **clap.**
  - Derive mode gives `-h/--help` and `-V/--version`. `Error::exit` prints to stderr and exits with code 2, or prints help/version to stdout and exits with 0 ([Error](https://docs.rs/clap/latest/clap/error/struct.Error.html)).
  - `Arg::env` needs the `env` feature. `hide_env_values` keeps secret values out of `--help` output ([Arg](https://docs.rs/clap/latest/clap/struct.Arg.html)).
  - clap's docs still state MSRV 1.74, but its `Cargo.toml` declares 1.85 ([Cargo.toml](https://raw.githubusercontent.com/clap-rs/clap/master/Cargo.toml)).
- **Config crates.**
  - `toml` 1.x parses TOML 1.1 ([CHANGELOG](https://raw.githubusercontent.com/toml-rs/toml/main/crates/toml/CHANGELOG.md)).
  - `figment` has had no crates.io release since 2024-05-17, although its master branch is active ([repo](https://github.com/SergioBenitez/Figment)).
  - `config` "can not be used to write changed configuration values back" ([README](https://raw.githubusercontent.com/rust-cli/config-rs/main/README.md)).
- **Directory crates.**
  - `directories` puts config under `~/Library/Application Support` on macOS ([ProjectDirs](https://docs.rs/directories/latest/directories/struct.ProjectDirs.html)).
  - Its GitHub repository is archived. Development moved to Codeberg, whose last commit is dated 2026-03-20 ([Codeberg](https://codeberg.org/dirs/directories-rs)). It pulls in `option-ext` (MPL-2.0) through `dirs-sys` ([crates.io deps](https://crates.io/api/v1/crates/dirs-sys/0.5.0/dependencies)).
  - `etcetera`'s `choose_app_strategy` uses XDG on macOS, "used by most CLI tools". Its docs add that for user-edited config "you should still prefer the `XDG` strategy on macOS" ([etcetera](https://docs.rs/etcetera/latest/etcetera/)).
- **Secrets.**
  - Edition 2024 makes `set_var` and `remove_var` unsafe ([edition guide](https://doc.rust-lang.org/edition-guide/rust-2024/newly-unsafe-functions.html)). In multi-threaded programs, std says the only sound option is not to use them ([set_var](https://doc.rust-lang.org/std/env/fn.set_var.html)).
  - `secrecy`'s `SecretString` redacts `Debug`, zeroizes on drop, and has no `Serialize` by default ([source](https://docs.rs/crate/secrecy/latest/source/src/lib.rs)).
- **Errors.**
  - anyhow: "Use Anyhow if you don't care what error type your functions return … This is common in application code" ([anyhow](https://github.com/dtolnay/anyhow)).
  - thiserror "deliberately does not appear in your public API" ([thiserror](https://github.com/dtolnay/thiserror)).
  - miette: enable `fancy` "only … in your toplevel crate" ([miette](https://github.com/zkat/miette)).
  - Ratatui's recipe uses `color-eyre`, installed before init ([recipe](https://ratatui.rs/recipes/apps/color-eyre/)).
  - The API guidelines (C-GOOD-ERR) want errors that are `Error + Send + Sync`, never `()`, with lowercase messages ([C-GOOD-ERR](https://rust-lang.github.io/api-guidelines/interoperability.html#c-good-err)).
- **Logging.**
  - tracing-appender's `WorkerGuard` "must be assigned to a binding that is not `_`" ([non_blocking](https://docs.rs/tracing-appender/latest/tracing_appender/non_blocking/index.html)). The non-blocking writer is lossy by default ([NonBlockingBuilder](https://docs.rs/tracing-appender/latest/tracing_appender/non_blocking/struct.NonBlockingBuilder.html)).
  - The fmt layer turns ANSI off when `NO_COLOR` is set ([fmt Layer](https://docs.rs/tracing-subscriber/latest/tracing_subscriber/fmt/struct.Layer.html)).
  - `#[instrument]` records all arguments by default ([instrument](https://docs.rs/tracing/latest/tracing/attr.instrument.html)).
  - Ratatui's logging recipe writes to a file under the project data dir ([recipe](https://ratatui.rs/recipes/apps/log-with-tracing/)).

### Proposal

- **CLI.** Use `clap` derive. Keep the key out of argument values. Read it once with `std::env::var_os("OPENROUTER_API_KEY")`, the name Rig's dialect uses, into a `SecretString`. A missing key is a typed, actionable error.
- **Config.** A single `Config` struct read with `toml::from_str`, with serde defaults and a few flags. Skip layering crates.
- **Directories.** `etcetera` with `choose_app_strategy`, which avoids the MPL dependency and the archived repo. Whether macOS uses XDG or `Application Support` is an open question.
- **Errors.**
  - `thiserror` enums for each module: `StorageError`, `AiError`, `SourceError`, `ConfigError`.
  - `anyhow::Result` only in `main` and at the top of the command boundary.
  - Choose between color-eyre and miette later, if their nicer reports are wanted.
- **Logging.**
  - `tracing-subscriber` with `env-filter` and an `fmt` layer, `with_ansi(false)`, writing to `tracing_appender::rolling::never(state_dir, "whetstone.log")` through `non_blocking`, with the guard held in `main`.
  - Off by default, enabled with `RUST_LOG` or a flag.
  - Rig content telemetry stays off.

## 8. Testing

### Facts

- **Binary crates.** If a project has only `main.rs`, "we can't create integration tests … and bring functions defined in the *src/main.rs* file into scope" ([Rust Book ch11-03](https://doc.rust-lang.org/book/ch11-03-test-organization.html)).
- **Integration-test layout.** Each file in `tests/` compiles as its own crate. The Cargo book suggests a single integration test split into modules ([Cargo targets](https://doc.rust-lang.org/cargo/reference/cargo-targets.html)).
- **Ratatui snapshots.**
  - The recipe is `assert_snapshot!(terminal.backend())` with "a consistent terminal size"; "Asserting with color is not supported as of now" ([recipe](https://ratatui.rs/recipes/testing/snapshots/)).
  - The same recipe says backend tests do not cover the event loop or teardown, and suggests PTY tests (`termlens`) for those.
  - `TestBackend` has `assert_buffer_lines` and `assert_cursor_position` ([TestBackend](https://docs.rs/ratatui/latest/ratatui/backend/struct.TestBackend.html)).
- **insta in CI.** It "will not write new snapshot files" when `CI` is set ([quickstart](https://insta.rs/docs/quickstart/)). `INSTA_UPDATE=no` stops all writes ([advanced](https://insta.rs/docs/advanced/)).
- **wiremock.** Expectations "are automatically verified during the shutdown" of each `MockServer`; servers should not be shared ([README](https://github.com/LukeMathWalker/wiremock-rs)). Its last release and last push were both 2025-08-24.
- **proptest.** Commit the `proptest-regressions` files ([failure persistence](https://proptest-rs.github.io/proptest/proptest/failure-persistence.html)).
- **nextest.** Doctests "are currently not supported … run doctests in a separate step with `cargo test --doc`" ([nexte.st](https://nexte.st/)). Process spawning is slow on Windows ([why process-per-test](https://nexte.st/docs/design/why-process-per-test/)).
- **Binary tests.** `assert_cmd` "only works within the context of integration tests" ([assert_cmd](https://docs.rs/assert_cmd/latest/assert_cmd/)).
- **PTY crates.** `expectrl` "works on windows" ([README](https://github.com/zhiburt/expectrl)). `rexpect` depends on `nix` *(inference: Unix only)*. `termlens` 0.11.6 first appeared on 2026-08-09.

### Proposal

- **Layout.** One package with `src/lib.rs` (all logic) and a thin `src/main.rs`. Unit tests sit in `#[cfg(test)]` modules. One `tests/it/main.rs` crate holds the integration modules.
- **Layers.**
  1. **Reducer tests:** `update` transitions, stale results rejected by identity, the save acknowledgement gating evaluation.
  2. **Keystroke tests:** scripted `KeyEvent`s go through `map_key` → `update` → `view` on a `TestBackend`. Cover `q j k : ? A B C` typed inside the editor, the Tab → Submit → Enter path, and Esc keeping the draft.
  3. **Snapshots** at 120x32, 100x28, 80x24 and 50x12, plus a resize round trip. Color and NO_COLOR get cell-style buffer assertions, because snapshots ignore color.
  4. **Adapter contract tests** against wiremock, or httpmock to match `rig-cassette`, with recorded OpenRouter generator and Jev bodies. CI never makes real network calls.
  5. **Storage integration tests** on a temporary database: pragma assertions, acknowledgement after commit, recovery after an abrupt drop.
  6. **PTY smoke tests:** terminal restored after quit, Ctrl+C and a forced panic, plus exit codes. Use `expectrl` if Windows coverage matters, otherwise `termlens`.
- **Tooling.**
  - `proptest` for editor operations (cursor always on a grapheme boundary, no lost text on paste), wrap/layout invariants, and persistence round trips.
  - `#[tokio::test(start_paused = true)]` for timeout and retry logic.
  - `assert_cmd` for `--help`, `--version` and usage errors.

## 9. Project hygiene

### Facts

- **Workspaces.** A virtual manifest must set `resolver` explicitly. Members inherit lints with `[lints] workspace = true`. The book's layout recommendation is `members = ["crates/*"]` ([workspaces](https://doc.rust-lang.org/cargo/reference/workspaces.html)).
- **Edition 2024.**
  - Released with Rust 1.85 on 2025-02-20 ([blog](https://blog.rust-lang.org/2025/02/20/Rust-1.85.0/)).
  - `edition = "2024"` implies `resolver = "3"`, which is MSRV-aware ([edition guide](https://doc.rust-lang.org/edition-guide/rust-2024/cargo-resolver.html)).
  - `unsafe_op_in_unsafe_fn` warns by default, and `static_mut_refs` is denied ([edition guide](https://doc.rust-lang.org/edition-guide/rust-2024/index.html)).
- **Rust versions.**
  - Current stable is 1.99.1, released 2026-10-15 ([blog](https://blog.rust-lang.org/2026/10/15/Rust-1.99.1/)).
  - The Cargo book on MSRV: "The simplest policy to support is to always use the latest Rust version" ([rust-version](https://doc.rust-lang.org/cargo/reference/rust-version.html)).
  - Floors set by dependencies: rig 1.95.0 (rising to 1.97.1 on `main`), ratatui 1.88, gix 1.88, redb 1.90, `ra_ap_*` 1.98 (crates.io data).
- **Cargo.lock.**
  - The Cargo FAQ says tracking it "is dependent on the needs of your package" ([FAQ](https://doc.rust-lang.org/cargo/faq.html)). ANSSI DENV-CARGO-LOCK says Cargo.lock "MUST be tracked" ([ANSSI devenv](https://anssi-fr.github.io/rust-guide/devenv.html)).
  - `cargo install` ignores the lockfile without `--locked` ([cargo install](https://doc.rust-lang.org/cargo/commands/cargo-install.html)).
- **Formatting.**
  - The Style Guide uses 4-space indentation, a 100-character width and trailing commas, and "recommends … the default Rust style" ([style guide](https://doc.rust-lang.org/style-guide/)).
  - rustfmt recommends setting `style_edition` in `rustfmt.toml`. Unstable options require nightly ([rustfmt](https://github.com/rust-lang/rustfmt)).
- **Clippy groups.** `correctness` is deny; `suspicious`, `style`, `complexity` and `perf` warn; `pedantic`, `restriction`, `nursery` and `cargo` are allow ([Clippy book](https://doc.rust-lang.org/clippy/)).
  - "The restriction category should, emphatically, not be enabled as a whole". Pedantic "may have some intentional false positives" ([usage](https://doc.rust-lang.org/clippy/usage.html)).
  - `[lints]` groups need `priority = -1` ([manifest lints](https://doc.rust-lang.org/cargo/reference/manifest.html#the-lints-section)).
  - `lint_groups_priority` does not check inherited workspace lints ([lint](https://clippy.rust-lang.org/master/index.html#lint_groups_priority)).
- **Warnings as errors.** `#[expect]` has been stable since 1.81 ([blog](https://blog.rust-lang.org/2024/09/05/Rust-1.81.0/)). `build.warnings = "deny"` and `CARGO_BUILD_WARNINGS` are respected as of Cargo 1.97 ([config](https://doc.rust-lang.org/cargo/reference/config.html)).
- **ANSSI rules.**
  - LANG-UNSAFE: unless unsafe is justified, "`#![forbid(unsafe_code)]` must appear in the crate root" ([ANSSI unsafe](https://anssi-fr.github.io/rust-guide/unsafe/generalities.html)).
  - LANG-ARITH covers overflow handling ([integer](https://anssi-fr.github.io/rust-guide/integer.html)).
  - LANG-LIMIT-PANIC and LANG-ERRWRAP cover panics and error types ([errors](https://anssi-fr.github.io/rust-guide/errors.html)).
- **Panics.** Panic hooks run under both the abort and unwind strategies ([set_hook](https://doc.rust-lang.org/std/panic/fn.set_hook.html)). Only unwinding runs `Drop` ([reference](https://doc.rust-lang.org/reference/panic.html)).
- **Supply-chain tools.**
  - `cargo-deny` checks advisories, licenses, bans and sources. Its default for unmaintained crates is `"all"`, and its license list is an allow-list ([advisories](https://embarkstudios.github.io/cargo-deny/checks/advisories/cfg.html), [licenses](https://embarkstudios.github.io/cargo-deny/checks/licenses/cfg.html)). It reads the same RustSec database as `cargo-audit`.
  - `cargo-shear` is static, runs on stable and needs one platform ([README](https://github.com/Boshen/cargo-shear)).
  - `cargo-udeps` needs nightly ([README](https://github.com/est31/cargo-udeps)).
- **GitHub Actions.**
  - GitHub recommends pinning actions to full commit SHAs. It now calls workflow lockfiles, in public preview, "the strongest protection available" ([secure use](https://docs.github.com/en/actions/reference/security/secure-use), [lockfiles](https://docs.github.com/en/actions/concepts/security/workflow-dependency-lockfiles)).
  - It advises a read-only default `GITHUB_TOKEN` and avoiding `pull_request_target`.
  - `macos-latest` is macOS 26 on arm64 ([runners](https://docs.github.com/en/actions/reference/runners/github-hosted-runners)).
- **Release tooling.**
  - `dist` generates `release.yml` and shell, powershell, Homebrew, MSI and npm installers. GitHub attestations are "experimental", and macOS notarization is not supported ([repo](https://github.com/axodotdev/cargo-dist), [config](https://axodotdev.github.io/cargo-dist/book/reference/config.html)).
  - dist v0.33.0's crates.io publish job failed ([run](https://github.com/axodotdev/cargo-dist/actions/runs/34565901871)).
  - "release-plz doesn't build and release binaries" ([docs](https://github.com/release-plz/release-plz/blob/main/website/docs/extra/releasing-binaries.md)).
- **License.** C-PERMISSIVE recommends "MIT OR Apache-2.0" with the dual-license contribution clause ([C-PERMISSIVE](https://rust-lang.github.io/api-guidelines/necessities.html#c-permissive)).

### Proposal

- **Layout.**
  - Start with one package (lib + bin), as the kit draft also proposes.
  - Move to a virtual workspace (`crates/*`, `resolver = "3"`) only when a boundary earns it. The first likely candidates are a small Jev adapter crate, which enables the cargo-deny `wrappers` rule, and an `xtask` or build crate for showcase-project extraction.
  - Do not create one crate per kit piece.
- **Toolchain.**
  - Set `edition = "2024"` and `rust-version = "1.95"`, and raise the latter whenever Rig raises its own.
  - A `rust-toolchain.toml` pins the current stable for development, bumped deliberately (ANSSI DENV-STABLE).
  - CI checks the MSRV with `cargo hack check --rust-version` ([Cargo CI guide](https://doc.rust-lang.org/cargo/guide/continuous-integration.html)).
- **Formatting.** `rustfmt.toml` contains only `style_edition = "2024"`.
- **Lints.** Configure them in the manifest (proposal):

```toml
[workspace.lints.rust]   # or [lints.rust] while the project is a single package
unsafe_code = "forbid"                                  # ANSSI LANG-UNSAFE
rust_2018_idioms = { level = "warn", priority = -1 }
let_underscore_drop = "warn"                            # catches `let _ = guard;`
unreachable_pub = "warn"
unused_qualifications = "warn"

[workspace.lints.clippy]
pedantic = { level = "warn", priority = -1 }            # Clippy lints itself with pedantic
missing_errors_doc = "allow"                            # app-internal API
missing_panics_doc = "allow"
module_name_repetitions = "allow"
unwrap_used = "warn"                                    # ANSSI LANG-LIMIT-PANIC-SRC
panic = "warn"
todo = "warn"
unimplemented = "warn"
dbg_macro = "warn"
print_stdout = "warn"                                   # the TUI owns stdout; #[expect] in plain-output module
print_stderr = "warn"
exit = "warn"                                           # exit() skips Drop-based terminal restore
mem_forget = "warn"                                     # ANSSI MEM-FORGET
string_slice = "warn"                                   # byte slicing panics on UTF-8 boundaries
allow_attributes = "warn"                               # prefer #[expect]
allow_attributes_without_reason = "warn"
await_holding_lock = "deny"
await_holding_refcell_ref = "deny"
```

- **`clippy.toml` (proposal):**
  - `allow-unwrap-in-tests`, `allow-expect-in-tests`, `allow-panic-in-tests` and `allow-print-in-tests` are all `true`.
  - `disallowed-methods` covers `tokio::task::block_in_place` (use `spawn_blocking`), and `crossterm::event::read` and `crossterm::event::poll` (single `EventStream`) ([lint configuration](https://doc.rust-lang.org/clippy/lint_configuration.html)).
- **Not enabled:**
  - `expect_used`, because a reasoned `expect` is acceptable.
  - `indexing_slicing`, which is too noisy for layout code.
  - `as_conversions`, because pedantic's `cast_*` lints cover it.
  - The `cargo` group, because Rig makes duplicate dependency versions likely.
  - `nursery`.
- **Profiles.**
  - `[profile.release] overflow-checks = true` (ANSSI LANG-ARITH; cost to be measured).
  - `panic` stays `"unwind"`.
  - `[profile.dev.package] insta.opt-level = 3` and `similar.opt-level = 3`, as insta recommends.
- **Supply chain.**
  - `deny.toml` settings:
    - `unmaintained = "all"` and `yanked = "deny"`.
    - A license allow-list of `MIT`, `Apache-2.0`, `Apache-2.0 WITH LLVM-exception`, `BSD-3-Clause`, `ISC`, `Unicode-3.0` and `Zlib`, with per-crate `exceptions` for anything else.
    - `multiple-versions = "warn"`.
    - `sources` limited to crates.io, plus any pinned Rig git revision while #2800 is unreleased.
  - Use `cargo-shear --deny-warnings`.
  - Dependabot covers `cargo` and `github-actions`.
  - Leave out `cargo-audit` (redundant with cargo-deny) and `cargo-vet` (later).
- **CI jobs (proposal):**

| Job | Command | OS |
| --- | --- | --- |
| fmt | `cargo fmt --all --check` | ubuntu |
| clippy | `CARGO_BUILD_WARNINGS=deny cargo clippy --all-targets --all-features --locked` | ubuntu, macos, windows |
| test | `cargo nextest run --locked --all-features --profile ci` then `cargo test --doc --locked` | ubuntu, macos (arm64), windows |
| docs | `RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features --locked` | ubuntu |
| msrv | `cargo hack check --rust-version --all-targets --locked` | ubuntu |
| deny | `cargo deny check` (Docker action is Linux-only) | ubuntu |
| unused deps | `cargo shear --deny-warnings` | ubuntu |
| release plan | dist `release.yml` in plan mode on PRs | ubuntu |

- Every workflow sets `permissions: contents: read`.
- Actions are pinned by SHA, or by the preview lockfile.
- The workflows use `dtolnay/rust-toolchain`, `Swatinem/rust-cache@v2` and `taiki-e/install-action@v2` ([rust-toolchain](https://github.com/dtolnay/rust-toolchain), [rust-cache](https://github.com/Swatinem/rust-cache), [install-action](https://github.com/taiki-e/install-action)).
- **Release.**
  - `dist` builds on a tag: macOS (arm64, x86_64), Linux (x86_64, aarch64) and Windows x86_64. It produces shell and powershell installers, with Homebrew later, and turns on `github-attestations` and `github-action-commits` pinning.
  - The alternative is release-plz plus `taiki-e/upload-rust-binary-action` and `actions/attest@v4`.
  - Neither notarizes macOS binaries.
- **License.** `license = "MIT OR Apache-2.0"`, with `LICENSE-MIT`, `LICENSE-APACHE` and the C-PERMISSIVE contribution clause in the README. Rig and ratatui are MIT, which is compatible.

## 10. Coding standards (proposed rule list)

"Check" says how each rule is enforced. **M** means mechanical, by a lint, test or CI job. **R** means review judgement. All rules are proposals.

| # | Rule | Check | Primary source |
| --- | --- | --- | --- |
| 1 | Every package uses edition 2024 and declares `rust-version`, never lower than the highest floor among direct dependencies. | M: `cargo hack check --rust-version` | [rust-version](https://doc.rust-lang.org/cargo/reference/rust-version.html) |
| 2 | Commit `Cargo.lock`. CI builds, tests and lints with `--locked`. | M: CI flags | [Cargo FAQ](https://doc.rust-lang.org/cargo/faq.html); [ANSSI DENV-CARGO-LOCK](https://anssi-fr.github.io/rust-guide/devenv.html) |
| 3 | Use the stable toolchain only, with no nightly features or unstable rustfmt options. | M: CI toolchain | [ANSSI DENV-STABLE](https://anssi-fr.github.io/rust-guide/devenv.html); [rustfmt](https://github.com/rust-lang/rustfmt) |
| 4 | Code is `rustfmt`-clean with default style and `style_edition = "2024"`. | M: `cargo fmt --check` | [Style Guide](https://doc.rust-lang.org/style-guide/) |
| 5 | Lints are configured only in the manifest `[lints]` tables. CI denies warnings across all targets. | M: clippy job | [Cargo lints](https://doc.rust-lang.org/cargo/reference/manifest.html#the-lints-section); [Clippy usage](https://doc.rust-lang.org/clippy/usage.html) |
| 6 | Suppress a lint locally only with `#[expect(lint, reason = "…")]`, never a bare `#[allow]`. | M: `allow_attributes`, `allow_attributes_without_reason` | [Rust 1.81](https://blog.rust-lang.org/2024/09/05/Rust-1.81.0/) |
| 7 | No unsafe code: `unsafe_code = "forbid"`. Lifting it needs an ADR, a `// SAFETY:` comment and `undocumented_unsafe_blocks`. | M: rustc lint | [ANSSI LANG-UNSAFE](https://anssi-fr.github.io/rust-guide/unsafe/generalities.html) |
| 8 | Every new dependency states its reason in the PR. No crate with an active RustSec unmaintained or vulnerability advisory. No unused dependencies. | M: `cargo deny check`, `cargo shear`; R: the reason | [cargo-deny](https://embarkstudios.github.io/cargo-deny/checks/advisories/cfg.html); [cargo-shear](https://github.com/Boshen/cargo-shear) |
| 9 | Dependency licenses must be on the allow-list; exceptions are per crate. | M: cargo-deny licenses | [licenses cfg](https://embarkstudios.github.io/cargo-deny/checks/licenses/cfg.html) |
| 10 | Rig crates move together in a dedicated upgrade PR that re-runs the AI contract tests. | R + M: contract tests | [Rig README](https://github.com/0xPlaygrounds/rig/blob/main/README.md) |
| 11 | Recoverable failures (I/O, config, source, provider, storage) return `Result`. No `unwrap`, `panic!`, `todo!` or `unimplemented!` outside tests. | M: Clippy restriction picks | [ANSSI LANG-LIMIT-PANIC](https://anssi-fr.github.io/rust-guide/errors.html) |
| 12 | `expect` is allowed only for invariants, and its message states the invariant. | R | [ANSSI LANG-LIMIT-PANIC-SRC](https://anssi-fr.github.io/rust-guide/errors.html) |
| 13 | Each module has its own `thiserror` error type, which is `Error + Send + Sync + 'static`, never `()`. Messages are lowercase with no trailing period. `anyhow` appears only in `main` and at the command boundary. | R | [C-GOOD-ERR](https://rust-lang.github.io/api-guidelines/interoperability.html#c-good-err); [anyhow](https://github.com/dtolnay/anyhow) |
| 14 | No `process::exit` outside `main`; return to `main` so that `Drop` guards run. | M: `clippy::exit` | [exit lint](https://clippy.rust-lang.org/master/index.html#exit) |
| 15 | Keep `panic = "unwind"`. The terminal guard restores state from both `Drop` and its panic hook. | R: profile review | [set_hook](https://doc.rust-lang.org/std/panic/fn.set_hook.html); [panic](https://doc.rust-lang.org/reference/panic.html) |
| 16 | Every spawned task has an owner (`JoinSet`, `TaskTracker`, `AbortOnDropHandle`). Dropping a bare `JoinHandle` needs a comment. | R | [JoinHandle](https://docs.rs/tokio/latest/tokio/task/struct.JoinHandle.html) |
| 17 | Each job derives a child `CancellationToken` and reports cancellation as a typed result, never as silence. | R | [CancellationToken](https://docs.rs/tokio-util/latest/tokio_util/sync/struct.CancellationToken.html) |
| 18 | `select!` branches inside loops use only cancel-safe operations. Use `reserve` or `try_send` instead of `send`, and no `write_all` or lock acquisition. | R | [select! cancellation safety](https://docs.rs/tokio/latest/tokio/macro.select.html#cancellation-safety) |
| 19 | No blocking work on async tasks. Git and parsing go to `spawn_blocking`, storage to the writer thread. No `block_in_place`. | M: `disallowed-methods`; R | [spawn_blocking](https://docs.rs/tokio/latest/tokio/task/fn.spawn_blocking.html) |
| 20 | Never hold a std `Mutex` guard or `RefCell` borrow across `.await`. | M: `await_holding_lock`, `await_holding_refcell_ref` | [tokio Mutex](https://docs.rs/tokio/latest/tokio/sync/struct.Mutex.html) |
| 21 | Channels are bounded with a documented capacity. An unbounded channel needs a written reason. | R | [channels](https://tokio.rs/tokio/tutorial/channels) |
| 22 | Every network stage has an explicit timeout. The HTTP client is built with `connect_timeout` and `timeout`. | R + M: adapter tests | [reqwest ClientBuilder](https://docs.rs/reqwest/0.13.5/reqwest/struct.ClientBuilder.html); [timeout](https://docs.rs/tokio/latest/tokio/time/fn.timeout.html) |
| 23 | Automatic retries use `backon` with jitter and an explicit retryable predicate. They never replay a paid request the user has not authorised. | R | [backon](https://docs.rs/backon/latest/backon/struct.Retry.html); [Rig errors](https://rig.rs/docs/concepts/error_handling) |
| 24 | A worker result is applied only when its session, question version, attempt and job identities match the outstanding job. | M: reducer tests | [task cancellation](https://docs.rs/tokio/latest/tokio/task/index.html#cancellation) |
| 25 | `view` is pure: no I/O, network, storage, spawning or state mutation inside drawing. | R | [TEA](https://ratatui.rs/concepts/application-patterns/the-elm-architecture/) |
| 26 | One input source (`EventStream`). `KeyEventKind::Press` is filtered once, at the point where input enters. | M: `disallowed-methods` (read/poll); R | [crossterm event](https://docs.rs/crossterm/0.29.0/crossterm/event/index.html); [FAQ](https://ratatui.rs/faq/) |
| 27 | One terminal guard enables, and undoes on `Drop` and on panic, everything it turned on: raw mode, alternate screen, paste, keyboard flags and cursor. | M: PTY smoke test | [init.rs](https://docs.rs/ratatui/latest/src/ratatui/init.rs.html); [panic hooks](https://ratatui.rs/recipes/apps/panic-hooks/) |
| 28 | In raw mode, Ctrl+C is a reserved key that is handled before focus dispatch. SIGINT and SIGTERM from outside follow the same shutdown path. | M: PTY test | [raw mode](https://docs.rs/crossterm/latest/crossterm/terminal/index.html#raw-mode); [ctrl_c](https://docs.rs/tokio/latest/tokio/signal/fn.ctrl_c.html) |
| 29 | Key dispatch follows focus: reserved keys, then the top overlay, then the focused control. While an editor has focus, every non-reserved key is text. | M: keystroke tests | Kit interaction contract; [component template](https://ratatui.rs/templates/component/app-rs/) (counter-example) |
| 30 | Submission never depends on modified Enter; Tab → Submit → Enter always works. | M: keystroke test | [crossterm#685](https://github.com/crossterm-rs/crossterm/issues/685) |
| 31 | Text is measured in cells and edited by grapheme. No byte-length layout and no byte slicing of user text. | M: `string_slice`, proptest; R | [unicode-segmentation](https://docs.rs/unicode-segmentation/1.13.3/unicode_segmentation/trait.UnicodeSegmentation.html); [buffer.rs](https://docs.rs/ratatui-core/latest/src/ratatui_core/buffer/buffer.rs.html) |
| 32 | Layout is derived from `frame.area()` on every draw. State holds no `Rect`, so resize never changes drafts, focus or scroll. | M: snapshot and resize tests | [Terminal](https://docs.rs/ratatui/latest/ratatui/struct.Terminal.html) |
| 33 | Colors come only from theme tokens. A non-empty NO_COLOR disables color, an explicit flag overrides it, and meaning never depends on color alone. | M: buffer-style tests | [no-color.org](https://no-color.org/) |
| 34 | Nothing writes to stdout or stderr while the TUI is active. Diagnostics go to the tracing file. | M: `print_stdout`, `print_stderr` | [print_stdout](https://clippy.rust-lang.org/master/index.html#print_stdout) |
| 35 | Non-TTY output is line-oriented, with no spinner and no cursor control. | M: assert_cmd test | [IsTerminal](https://doc.rust-lang.org/std/io/trait.IsTerminal.html) |
| 36 | Only the `ai` module imports Rig. The rest of the app depends on Whetstone ports and domain types. | R (M via cargo-deny `wrappers` once it is a crate) | [bans cfg](https://embarkstudios.github.io/cargo-deny/checks/bans/cfg.html) |
| 37 | LLM output is parsed into typed values and validated. Invalid output is a typed failure (withheld or Needs review), never coerced. | R + M: contract tests | [Rig output modes](https://github.com/0xPlaygrounds/rig/blob/v0.44.0/crates/rig-agent/src/run/output.rs) |
| 38 | Jev distributions are stored as returned. Thresholds live in application code and need a recorded decision. | R | [rig-typesafeai](https://github.com/0xPlaygrounds/rig/blob/v0.44.0/crates/rig-typesafeai/src/lib.rs); [System One](https://docs.typesafe.ai/concepts/system-one) |
| 39 | A paid evaluation can only be built from a storage acknowledgement (a `SavedResponse` type). | M: compiler; reducer test | Kit interaction contract |
| 40 | The session store sets its durability pragmas explicitly at open, and tests assert them. "Saved" is shown only after `commit()` returns. | M: storage tests | [SQLite pragma](https://www.sqlite.org/pragma.html#pragma_synchronous) |
| 41 | The API key is read once from the environment into a `SecretString` and exposed only where the auth header is built. It is never a CLI argument, never logged, never serialized. | R | [secrecy](https://docs.rs/crate/secrecy/latest/source/src/lib.rs); [clap hide_env_values](https://docs.rs/clap/latest/clap/struct.Arg.html) |
| 42 | No `std::env::set_var` or `remove_var`; tests pass environment through `Command::env`. | M: `unsafe_code = "forbid"` (these are unsafe in 2024) | [set_var](https://doc.rust-lang.org/std/env/fn.set_var.html) |
| 43 | `#[instrument]` on functions that take secrets, source text or learner reasoning uses `skip` or `skip_all`. Rig content telemetry stays off. | R | [instrument](https://docs.rs/tracing/latest/tracing/attr.instrument.html); [Rig observability](https://rig.rs/docs/concepts/observability) |
| 44 | The tracing `WorkerGuard` is bound to a named variable in `main`. | R | [non_blocking](https://docs.rs/tracing-appender/latest/tracing_appender/non_blocking/index.html) |
| 45 | Workflows use least-privilege `permissions`, actions pinned by SHA (or the lockfile), and no `pull_request_target` checkout of PR code. | R | [secure use](https://docs.github.com/en/actions/reference/security/secure-use) |
| 46 | `main.rs` stays thin and logic lives in `lib.rs`. | R | [Rust Book ch11-03](https://doc.rust-lang.org/book/ch11-03-test-organization.html) |
| 47 | TUI snapshots are fixed-size (the four breakpoints) and committed. CI runs with `CI=true`. | M: insta in CI | [snapshots recipe](https://ratatui.rs/recipes/testing/snapshots/); [insta quickstart](https://insta.rs/docs/quickstart/) |
| 48 | Tests make no real network calls. Each HTTP test owns its mock server. | M: CI (no key in CI) | [wiremock](https://github.com/LukeMathWalker/wiremock-rs) |
| 49 | CI runs `cargo nextest` plus `cargo test --doc`. `proptest-regressions` files are committed. Time-dependent async tests use a paused clock. | M: CI | [nexte.st](https://nexte.st/); [proptest](https://proptest-rs.github.io/proptest/proptest/failure-persistence.html); [attr.test](https://docs.rs/tokio/latest/tokio/attr.test.html) |
| 50 | Exclusive states are enums and identities are newtypes. No `bool` or `Option` flag stands in for an outcome (for example, no `is_correct`). | R | [C-CUSTOM-TYPE](https://rust-lang.github.io/api-guidelines/type-safety.html#c-custom-type); [C-NEWTYPE](https://rust-lang.github.io/api-guidelines/type-safety.html#c-newtype) |
| 51 | Names follow C-CASE, C-CONV and C-GETTER, and domain identifiers use `GLOSSARY.md` terms (Practice, Change, Response, Attempt, Assessment, Jev, Generator, Showcase project). | M: Clippy style (partial); R | [naming](https://rust-lang.github.io/api-guidelines/naming.html); [domain docs](../agents/domain.md) |
| 52 | Types derive or implement `Debug`; types holding secrets redact it. | M: `missing_debug_implementations` (optional) | [C-DEBUG](https://rust-lang.github.io/api-guidelines/debuggability.html#c-debug) |
| 53 | Docs build without warnings, and each module has a short header doc. | M: `RUSTDOCFLAGS=-D warnings cargo doc` | [rustdoc lints](https://doc.rust-lang.org/rustdoc/lints.html) |

## Reconciliation with the kit's draft CODING_STANDARDS.md

The draft (`docs/restart-kit/CODING_STANDARDS.md`) is not an approved standard. Its points are compared below with the primary sources.

| Draft statement | Primary sources | Verdict |
| --- | --- | --- |
| "Start with one binary crate and a committed `Cargo.lock`." | Cargo FAQ and ANSSI support committing the lockfile. The Rust Book shows a binary-only crate cannot have `tests/` integration tests. | **Agrees, refined:** one package with lib + bin targets. |
| "Rust edition 2024 is the proposed edition." | Stable since 1.85. It implies resolver 3 (MSRV-aware). | **Agrees.** Rig sets the effective MSRV floor (1.95), which the draft does not mention. |
| Pin toolchain and dependency versions "only after checking official compatibility". | Cargo book: choose an MSRV policy, and "latest" is the simplest. Caret on `0.44` already excludes 0.45. | **Agrees.** Adds a CI MSRV job and lockstep Rig upgrades. |
| "Fourteen initiative briefs … are not fourteen crates". | The Cargo book gives no split criterion. Split when a boundary earns it. | **Agrees.** |
| First executable is logo/name, tagline, help/version. | Not a tooling question. AGENTS.md says logo-first is superseded. | **Superseded** for logo-first. Help/version remain a proposal, and clap provides them. |
| "Introduce asynchronous runtime, storage engine, provider abstractions … when a reviewed slice requires them." | Tokio and Rig are now decided, and Rig is async. | **Compatible.** The timing rule still holds, but the choice of runtime and AI framework is no longer open. |
| Rendering depends only on state and geometry; effects stay outside drawing; one owner applies transitions; results carry identities. | Ratatui's TEA page ("shouldn't … perform any other actions"). Tokio: `abort` doesn't guarantee non-completion. | **Agrees,** with stronger primary backing. |
| "Prefer concrete modules … over a framework of generic traits." | No primary source either way. Rig's churn and the experimental Jev crate are a real integration boundary. | **Agrees in spirit.** The AI ports are the earned seam the draft allows. |
| Enums for exclusive outcomes; no `is_correct` flag. | C-CUSTOM-TYPE, C-NEWTYPE. | **Agrees.** |
| `Result` for recoverable failures; no unchecked unwraps; unsafe "needs a documented invariant and specific review". | ANSSI LANG-LIMIT-PANIC and LANG-UNSAFE (forbid unless justified). | **Stricter:** forbid by default rather than review-only. |
| Provider request construction separate from UI state; direct OpenAI/Anthropic adapters may override generation; evaluation remains Jev via OpenRouter. | ADR 0001 (Rig provider abstraction). Rig docs confirm `with_base_url` and transport injection. | **Agrees.** Adds the finding that rig-typesafeai 0.44.0 fails against OpenRouter. |
| "Use the actual terminal size in cells … handle graphemes, wide glyphs, multiline paste." "A Paragraph is a renderer, not an editor." | Ratatui's buffer is grapheme/width based. `ratatui-textarea` edits by `char` and ignores `Paste`. | **Agrees.** Sources show the chosen widget still needs app-side grapheme and paste handling. |
| Portable submission is Tab → Submit → Enter; modified Enter is optional. | crossterm#685 is open; kitty protocol support is limited to listed terminals. | **Agrees.** |
| "Choose one Crossterm event-consumption strategy." | crossterm: "not allowed to … combine them with the EventStream". | **Agrees exactly.** Can be enforced with `disallowed-methods`. |
| "Verify cleanup after successful exit, interruption and error." | `ratatui::restore` only resets raw mode and the alternate screen. In raw mode Ctrl+C is input. | **Agrees, sharpened:** the app must undo paste and keyboard flags and handle Ctrl+C as a key. |
| "The logo-only path does not need raw mode." | n/a | **Superseded** with logo-first. |
| Pipelines and machine output stay free of spinners and cursor control. | `std::io::IsTerminal`; NO_COLOR spec. | **Agrees.** |
| Run formatting, locked Clippy with denied warnings, tests and a locked build. | Cargo CI guide, nextest docs, cargo-deny. | **Agrees, extended:** add doctests (nextest skips them), docs, MSRV, cargo-deny, cargo-shear, 3-OS matrix. |
| Reducer tests, integration tests, TestBackend snapshots and PTY checks each protect different properties. | Ratatui's snapshot recipe says the same, and that color is not asserted. | **Agrees.** Adds buffer-style tests for color/NO_COLOR. |
| Deterministic authored fixtures for the first slice; human-reviewed evaluation cases; record model, rubric and prompt versions. | Not a tooling question. TypeSafe: calibration doesn't guarantee individual correctness. | **Out of scope.** Fixture-first remains a proposal. Version recording is reflected in rule 37 and section 3. |
| "Mechanical conventions belong in automated checks." | Clippy, Cargo `[lints]`, cargo-deny. | **Agrees.** The rule list marks M and R. |
| References checked 18 Oct 2026: ratatui 0.30.2. | crates.io: 0.30.2 (2026-06-19) is still the latest. | **Confirmed.** |

The draft is silent on lint configuration, `unsafe` forbid, supply-chain checks, secrets handling, logging when the TUI owns stdout, MSRV policy, licensing and release tooling. Sections 7 to 9 propose these.

## Open questions for the user

1. **Jev path while 0.44.0 is broken.** Wait for Rig 0.45, pin a Rig git revision, or carry a one-fix `[patch]`? Each may change the MSRV.
2. **Durability bar for "Saved".** Survive an app crash, an OS crash or power loss, or also the macOS drive cache (`fullfsync`)?
3. **Storage engine.** SQLite (`rusqlite`), `redb`, or atomic JSON files?
4. **Showcase-project history.** Extract at build time into embedded data, or embed a real repository and read it with `gix` at runtime?
5. **Automatic retries of paid requests.** Never automatic, or allowed when the provider cannot have accepted the request (for example, connection refused)?
6. **MSRV policy.** Track Rig's floor (1.95 today), or declare "latest stable only"?
7. **Package and crate name.** `whetstone` is taken on crates.io. Use a different package name (the binary can stay `whetstone`), or skip crates.io and ship binaries only?
8. **macOS directories.** XDG (`~/.config`, used by most CLI tools) or `~/Library/Application Support`?
9. **Editor.** Accept `ratatui-textarea` with app-side grapheme and paste fixes, or plan a custom editor?
10. **Windows support for v1.** It drives the CI matrix, PTY tooling (ConPTY), and the bracketed-paste caveats.
11. **Error reports.** `anyhow` only, or `color-eyre` / `miette` for richer reports?
12. **Workspace timing.** Quarantine the Jev adapter in its own crate from day one, so cargo-deny can enforce the boundary?
13. **Release.** dist installers and GitHub Releases only, or also Homebrew and crates.io? Is unsigned/un-notarized macOS acceptable for v1?

## Sources

All URLs were accessed on 2026-10-19.

- **Project:** [ADR 0001](../adr/0001-build-agents-with-rig.md) · [GLOSSARY.md](../../GLOSSARY.md) · Kit draft `docs/restart-kit/CODING_STANDARDS.md` · Kit `docs/restart-kit/tui/INTERACTION-CONTRACT.md`
- **crates.io API data** (versions, dates, `rust_version`, licenses): `https://crates.io/api/v1/crates/<name>`. Repository activity: GitHub REST API `repos/<owner>/<repo>`.
- **Tokio:** [tokio docs](https://docs.rs/tokio/latest/tokio/) · [README / LTS](https://github.com/tokio-rs/tokio/blob/tokio-1.53.3/README.md) · [graceful shutdown](https://tokio.rs/tokio/topics/shutdown) · [bridging](https://tokio.rs/tokio/topics/bridging) · [select tutorial](https://tokio.rs/tokio/tutorial/select) · [channels](https://tokio.rs/tokio/tutorial/channels) · [shared state](https://tokio.rs/tokio/tutorial/shared-state) · [tokio-util CancellationToken](https://docs.rs/tokio-util/latest/tokio_util/sync/struct.CancellationToken.html) · [TaskTracker](https://docs.rs/tokio-util/latest/tokio_util/task/task_tracker/struct.TaskTracker.html) · [backon](https://docs.rs/backon/latest/backon/) · [tower retry](https://docs.rs/tower/latest/tower/retry/index.html)
- **Ratatui and terminal:** [ratatui docs](https://docs.rs/ratatui/latest/ratatui/) · [v0.30 highlights](https://ratatui.rs/highlights/v030/) · [backends](https://ratatui.rs/concepts/backends/comparison/) · [application patterns](https://ratatui.rs/concepts/application-patterns/) · [event handling](https://ratatui.rs/concepts/event-handling/) · [terminal and event handler recipe](https://ratatui.rs/recipes/apps/terminal-and-event-handler/) · [panic hooks](https://ratatui.rs/recipes/apps/panic-hooks/) · [color-eyre recipe](https://ratatui.rs/recipes/apps/color-eyre/) · [log with tracing](https://ratatui.rs/recipes/apps/log-with-tracing/) · [snapshots](https://ratatui.rs/recipes/testing/snapshots/) · [FAQ](https://ratatui.rs/faq/) · [templates](https://github.com/ratatui/templates) · [crossterm 0.29 docs](https://docs.rs/crossterm/0.29.0/crossterm/) · [crossterm CHANGELOG](https://raw.githubusercontent.com/crossterm-rs/crossterm/master/CHANGELOG.md) · [crossterm#685](https://github.com/crossterm-rs/crossterm/issues/685) · [no-color.org](https://no-color.org/) · [ratatui-textarea](https://github.com/ratatui/ratatui-textarea) · [edtui](https://github.com/preiter93/edtui) · [tui-input](https://docs.rs/tui-input/latest/tui_input/) · [unicode-width](https://docs.rs/unicode-width/0.2.2/unicode_width/) · [unicode-segmentation](https://docs.rs/unicode-segmentation/1.13.3/unicode_segmentation/)
- **Rig and Jev:** [Rig v0.44.0 tree](https://github.com/0xPlaygrounds/rig/tree/v0.44.0) · [MIGRATING.md](https://github.com/0xPlaygrounds/rig/blob/v0.44.0/MIGRATING.md) · [CHANGELOG](https://github.com/0xPlaygrounds/rig/blob/v0.44.0/CHANGELOG.md) · [rig#2782](https://github.com/0xPlaygrounds/rig/issues/2782) · [rig#2800](https://github.com/0xPlaygrounds/rig/pull/2800) · [rig#2753](https://github.com/0xPlaygrounds/rig/pull/2753) · [rig-typesafeai docs](https://docs.rs/rig-typesafeai/0.44.0/rig_typesafeai/) · [Rig docs: errors](https://rig.rs/docs/concepts/error_handling) · [Rig docs: testing](https://rig.rs/docs/concepts/testing) · [Rig docs: observability](https://rig.rs/docs/concepts/observability) · [OpenRouter Jev](https://openrouter.ai/docs/guides/community/jev) · [OpenRouter TypeSafe SDK](https://openrouter.ai/docs/guides/community/typesafe-sdk) · [OpenRouter structured outputs](https://openrouter.ai/docs/features/structured-outputs) · [TypeSafe System One](https://docs.typesafe.ai/concepts/system-one)
- **HTTP and serialization:** [reqwest 0.13.5](https://docs.rs/reqwest/0.13.5/reqwest/) · [reqwest CHANGELOG](https://github.com/seanmonstar/reqwest/blob/master/CHANGELOG.md) · [serde enum representations](https://serde.rs/enum-representations.html) · [serde container attrs](https://serde.rs/container-attrs.html) · [schemars attributes](https://graham.cool/schemars/deriving/attributes/) · [schemars SchemaSettings](https://docs.rs/schemars/1.2.3/schemars/generate/struct.SchemaSettings.html)
- **Storage:** [SQLite pragma](https://www.sqlite.org/pragma.html) · [WAL](https://www.sqlite.org/wal.html) · [atomic commit](https://www.sqlite.org/atomiccommit.html) · [how to corrupt](https://www.sqlite.org/howtocorrupt.html) · [compile options](https://www.sqlite.org/compile.html) · [rusqlite](https://github.com/rusqlite/rusqlite) · [sqlx SqliteConnectOptions](https://docs.rs/sqlx/latest/sqlx/sqlite/struct.SqliteConnectOptions.html) · [redb Durability](https://docs.rs/redb/latest/redb/enum.Durability.html) · [redb CHANGELOG](https://github.com/cberner/redb/blob/master/CHANGELOG.md) · [fjall PersistMode](https://docs.rs/fjall/latest/fjall/enum.PersistMode.html) · [sled README](https://github.com/spacejam/sled/blob/main/README.md) · [std File](https://doc.rust-lang.org/std/fs/struct.File.html) · [std rename](https://doc.rust-lang.org/std/fs/fn.rename.html) · [std unix fs source](https://github.com/rust-lang/rust/blob/master/library/std/src/sys/fs/unix.rs) · [Linux fsync(2)](https://man7.org/linux/man-pages/man2/fsync.2.html) · [Apple fsync(2)](https://developer.apple.com/library/archive/documentation/System/Conceptual/ManPages_iPhoneOS/man2/fsync.2.html) · [MoveFileExW](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-movefileexw) · [tempfile](https://docs.rs/tempfile/latest/tempfile/struct.NamedTempFile.html) · [atomic-write-file source](https://docs.rs/crate/atomic-write-file/0.3.1/source/)
- **Git and code:** [git2-rs](https://github.com/rust-lang/git2-rs) · [gitoxide](https://github.com/GitoxideLabs/gitoxide) · [gix docs](https://docs.rs/gix/latest/gix/) · [tree-sitter](https://tree-sitter.github.io/tree-sitter/) · [syn releases](https://github.com/dtolnay/syn/releases) · [rust-analyzer architecture](https://rust-analyzer.github.io/book/contributing/architecture.html) · [include_dir](https://docs.rs/include_dir/latest/include_dir/) · [rust-embed README](https://static.crates.io/readmes/rust-embed/rust-embed-8.13.0.html)
- **CLI plumbing:** [clap](https://docs.rs/clap/latest/clap/) · [toml](https://docs.rs/toml/latest/toml/) · [config-rs README](https://raw.githubusercontent.com/rust-cli/config-rs/main/README.md) · [etcetera](https://docs.rs/etcetera/latest/etcetera/) · [directories](https://docs.rs/directories/latest/directories/) · [directories-rs on Codeberg](https://codeberg.org/dirs/directories-rs) · [secrecy](https://docs.rs/secrecy/latest/secrecy/) · [anyhow](https://github.com/dtolnay/anyhow) · [thiserror](https://github.com/dtolnay/thiserror) · [miette](https://github.com/zkat/miette) · [eyre / color-eyre](https://github.com/eyre-rs/eyre) · [tracing-appender](https://docs.rs/tracing-appender/latest/tracing_appender/) · [tracing-subscriber EnvFilter](https://docs.rs/tracing-subscriber/latest/tracing_subscriber/filter/struct.EnvFilter.html)
- **Testing:** [Rust Book ch11-03](https://doc.rust-lang.org/book/ch11-03-test-organization.html) · [Cargo targets](https://doc.rust-lang.org/cargo/reference/cargo-targets.html) · [insta](https://insta.rs/docs/) · [wiremock](https://github.com/LukeMathWalker/wiremock-rs) · [httpmock](https://github.com/alexliesenfeld/httpmock) · [proptest book](https://proptest-rs.github.io/proptest/) · [nextest](https://nexte.st/) · [assert_cmd](https://docs.rs/assert_cmd/latest/assert_cmd/) · [expectrl](https://github.com/zhiburt/expectrl) · [portable-pty](https://docs.rs/portable-pty/latest/portable_pty/)
- **Hygiene and standards:** [Cargo workspaces](https://doc.rust-lang.org/cargo/reference/workspaces.html) · [Cargo manifest lints](https://doc.rust-lang.org/cargo/reference/manifest.html#the-lints-section) · [rust-version](https://doc.rust-lang.org/cargo/reference/rust-version.html) · [Cargo CI guide](https://doc.rust-lang.org/cargo/guide/continuous-integration.html) · [Cargo FAQ](https://doc.rust-lang.org/cargo/faq.html) · [Cargo profiles](https://doc.rust-lang.org/cargo/reference/profiles.html) · [edition guide 2024](https://doc.rust-lang.org/edition-guide/rust-2024/index.html) · [Rust Style Guide](https://doc.rust-lang.org/style-guide/) · [rustfmt](https://github.com/rust-lang/rustfmt) · [Clippy book](https://doc.rust-lang.org/clippy/) · [Clippy lint list](https://clippy.rust-lang.org/master/index.html) · [Clippy configuration](https://doc.rust-lang.org/clippy/lint_configuration.html) · [rustc lint listing](https://doc.rust-lang.org/rustc/lints/listing/allowed-by-default.html) · [rustdoc lints](https://doc.rust-lang.org/rustdoc/lints.html) · [Rust API Guidelines checklist](https://rust-lang.github.io/api-guidelines/checklist.html) · [ANSSI Secure Rust Guidelines](https://anssi-fr.github.io/rust-guide/) · [RustSec advisories](https://rustsec.org/) · [Rust 1.99.1](https://blog.rust-lang.org/2026/10/15/Rust-1.99.1/)
- **Supply chain, CI and release:** [cargo-deny book](https://embarkstudios.github.io/cargo-deny/) · [cargo-deny-action](https://github.com/EmbarkStudios/cargo-deny-action) · [cargo-audit](https://github.com/rustsec/rustsec/tree/main/cargo-audit) · [cargo-vet](https://mozilla.github.io/cargo-vet/) · [cargo-shear](https://github.com/Boshen/cargo-shear) · [cargo-machete](https://github.com/bnjbvr/cargo-machete) · [cargo-udeps](https://github.com/est31/cargo-udeps) · [GitHub secure use](https://docs.github.com/en/actions/reference/security/secure-use) · [workflow lockfiles](https://docs.github.com/en/actions/concepts/security/workflow-dependency-lockfiles) · [runner images](https://github.com/actions/runner-images) · [Dependabot options](https://docs.github.com/en/code-security/reference/supply-chain-security/dependabot-options-reference) · [dtolnay/rust-toolchain](https://github.com/dtolnay/rust-toolchain) · [Swatinem/rust-cache](https://github.com/Swatinem/rust-cache) · [taiki-e/install-action](https://github.com/taiki-e/install-action) · [cargo-hack](https://github.com/taiki-e/cargo-hack) · [dist](https://github.com/axodotdev/cargo-dist) · [dist book](https://axodotdev.github.io/cargo-dist/book/) · [release-plz](https://release-plz.dev/docs) · [GoReleaser Rust](https://goreleaser.com/customization/builds/builders/rust/) · [actions/attest](https://github.com/actions/attest) · [cargo-binstall SUPPORT](https://github.com/cargo-bins/cargo-binstall/blob/main/SUPPORT.md) · [C-PERMISSIVE](https://rust-lang.github.io/api-guidelines/necessities.html#c-permissive)

**Could not verify or secondary only:**
- That Ctrl+C reaches the app as a key in raw mode. This is inferred from crossterm and rustix source.
- That NO_COLOR wipes modifiers with crossterm 0.29.0. Inferred from source; not tested.
- `Event::Paste` on Windows consoles, and terminals pasting `\r`. Secondary sources only: [zellij#4885](https://github.com/zellij-org/zellij/issues/4885), [crossterm#780](https://github.com/crossterm-rs/crossterm/issues/780).
- `COLORTERM` truecolor detection. No primary specification exists.
- Whether ratatui#2686 has shipped in a release.
- The release date of ratatui-textarea 0.9.3. crates.io says 2026-10-05; its CHANGELOG says 2026-07-26.
- sqlx 0.9.0's date. crates.io says 2026-05-21; its CHANGELOG says 2026-05-06.
- Whether the #2800 fix cherry-picks onto 0.44.0, and the MSRV 0.45.0 will ship with.
- Whether aws-lc needs a C toolchain on all CI targets.
- Primary ratatui guidance on SIGTERM and SIGHUP. None found.
