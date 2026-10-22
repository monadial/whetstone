# Contributing to Whetstone

Thank you for helping. Whetstone is early, and Practice still changes from week to week, so the most useful contributions now are bug reports and ideas from using it. Code is welcome too, once an issue has agreed what to build.

## Report a bug or suggest an idea

Open an [issue](https://github.com/monadial/whetstone/issues/new/choose) with the bug or idea form. For a bug, give:

- what `whetstone --version` prints;
- your operating system;
- your terminal, and whether Whetstone ran inside tmux or screen, or over SSH;
- the steps that show the bug.

Never paste your OpenRouter key. Leave out code from a private repository, and describe it instead.

Whetstone's log often says what went wrong. It is `whetstone.log` in Whetstone's state folder, by default `~/.local/state/whetstone/`. Read it before you attach it.

A security problem never goes in a public issue: [SECURITY.md](SECURITY.md) says where to report it.

## Before you write code

Open an issue, or comment on an existing one, and wait for a reply before you start. Whetstone's direction is decided in its issues. A PR without one may not fit where Practice is going, however good it is. One PR addresses one issue.

## Build and check

Whetstone builds on Linux and macOS. You need:

- [rustup](https://rustup.rs), which installs the toolchain pinned in `rust-toolchain.toml` on first use;
- a C compiler, for the grammars the connected-code engine compiles and for the TLS library.

```sh
cargo run
```

Before you push, run what CI runs. [`CODING_STANDARDS.md`](CODING_STANDARDS.md#ci) gives each job's command, and the version of each tool it uses:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo nextest run --workspace --all-features --locked --profile ci
cargo test --workspace --all-features --locked --doc
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --no-deps --locked
cargo deny --locked check
cargo shear --deny-warnings --locked
```

A change to the release setup (`dist-workspace.toml`, `about.toml`, `about.hbs` or `.github/build-setup.yml`) also needs the Release config job's two commands, from the same table, and the dist and cargo-about versions the release pins.

Snapshot tests ([insta](https://insta.rs)) check every screen at 120×32 and 80×24. When a change alters a screen on purpose, review the new snapshots with `cargo insta review`, and commit them with the change.

Tests make no network calls. The live tests call OpenRouter with a real key, so they are ignored, and running them is optional. CI runs them with the project's key. [The keyed end-to-end job](CODING_STANDARDS.md#the-keyed-end-to-end-job) says how to run them with your own key, which costs a few cents.

## Write the change

- [`CODING_STANDARDS.md`](CODING_STANDARDS.md) holds the rules every change follows. They are numbered, so a review can cite one.
- [`GLOSSARY.md`](GLOSSARY.md) holds Whetstone's vocabulary. Name things with its terms: a Practice, a Change, Evidence, an Area.
- [`docs/adr/`](docs/adr/) records the decisions that last.
- A new dependency states its reason in the PR (rule 8). Its license must be on `deny.toml`'s allow-list (rule 9).

Start each commit message with its kind and the issue, then say what the commit does, in plain words:

```text
feat(#123): show the commit's size beside its summary
fix(#123): keep the filter when the window is resized
```

The kinds are `feat`, `fix`, `test`, `docs`, `refactor`, `perf`, `ci` and `chore`. PRs are merged by rebase, so each commit lands on `main` as you wrote it.

## License

Whetstone is licensed under [Apache-2.0](LICENSE). Under section 5 of the license, a contribution you submit is licensed under it too. There is no separate agreement to sign.
