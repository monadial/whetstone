<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="assets/brand/whetstone-square-symbol.svg" width="120">
    <img src="assets/brand/whetstone-logo.svg" alt="whetstone by Monadial" width="480">
  </picture>
</p>

# Whetstone by Monadial

**Understand the code you ship.**

Whetstone helps you examine your understanding of real code through questions and written reasoning. You take a change (its commit message, its diff and the code it touches), answer a question about it and explain why. The feedback looks at your choice and your reasoning separately.

## Status

Early. `whetstone` opens a terminal home screen. Started inside a Git repository, it offers Practice on that repository first, then Practice on the showcase project (a small sample codebase that ships with Whetstone), Settings and Quit. Practice starts by choosing the areas questions focus on and one change: a commit from your repository's history, or one of the showcase project's curated changes, which are built into the binary. Enter opens the change to read its diff and each file it touches, before and after.

On your repository, **Review what will be sent** opens the outbound review: exactly what a Practice would send and where. It shows the commit message, every touched file with its size and whether it is sent, any possible secrets, each area with its prompt, and the total size against the limit. Questions and feedback need AI, which isn't connected yet, so **Send** is disabled. Once it is, Practice will send what you approved and your answers to [OpenRouter](https://openrouter.ai), paid with your own credits. Nothing is sent today.

## Run it

Whetstone builds with the Rust toolchain pinned in `rust-toolchain.toml`, on Linux and macOS:

```sh
cargo run
```

It needs an interactive terminal. `whetstone --help` lists the options. Set `NO_COLOR` to turn colour off: focus and selection stay visible through markers and bold text.

Everything works from the keyboard. `j`/`k` or the arrows move, Tab and Shift+Tab move focus, and Enter or Space activates. `q` goes back, and quits from home. `?` shows the keys for the current screen and `:` lists its actions. Ctrl+C quits from anywhere.

In the picker, `/` searches your repository's commits by message or ID. In a change, `h`/`l` switch between the files and the diff, before and after panes. `gg`/`G` and Ctrl+D/Ctrl+U jump and scroll, and each pane keeps its place. `/` searches the focused pane, and `n`/`N` move to the next and previous match. In the outbound review, Space leaves a file out or brings it back, and Enter reads exactly what is sent for the change, a file or an area.

## Practice on your repository

Start `whetstone` anywhere inside a Git repository, a linked worktree included. It reads the repository without changing it: commits come from Git's object store, never from the working tree, and nothing is written there.

Areas come from Markdown files in `.whetstone/areas/` at the repository's root, read from the working tree when Whetstone starts and listed in file-name order. A repository without any gets Whetstone's five defaults: domain, infrastructure, observability, scalability and system design. A file's name without `.md` is the area's ID, in lowercase letters, digits and hyphens. TOML front matter between two `+++` lines gives its name and a one-line description, and the body is its area prompt:

```markdown
+++
name = "Security"
description = "Who can do what, and what stops everyone else"
+++

Look at how the change decides who may act, and on what.
```

An area file that isn't valid is listed with the reason and can't be chosen. `whetstone areas init` copies the five defaults into `.whetstone/areas/` as a starting point, and refuses if any `.md` file is already there.

`.whetstone/config.toml` is optional. Its only key is `exclude`: gitignore-style patterns for files that a Practice on this repository leaves out unless the learner brings them back in the outbound review:

```toml
exclude = ["fixtures/", "docs/private/*.md"]
```

A `!` pattern is invalid, so the config can only leave more out. Lockfiles and common secret paths, such as `.env` and `*.pem`, are left out by default too, and can be brought back the same way. Binary files, submodules and symlinks are always left out.

## Configure it

Settings shows what Practice will use. It is read-only: the values come from `config.toml` in Whetstone's config folder, `$XDG_CONFIG_HOME/whetstone/` (by default `~/.config/whetstone/`) on Linux and macOS. Every setting is optional, and without a file Whetstone uses these defaults and says so:

```toml
# The service Practice sends its requests to. OpenRouter is the only one so far.
provider = "openrouter"

# Jev, the evaluator: it returns decisions and probabilities, never prose.
[jev]
model = "typesafe/jev-1.14"

# The generator writes questions, explanations and hints.
[generator]
model = "anthropic/claude-sonnet-5.5"
```

The OpenRouter key never goes in the file. Set `OPENROUTER_API_KEY` in your shell; Settings shows only whether it is set. Whetstone writes nothing to disk except its log, in `$XDG_STATE_HOME/whetstone/` (by default `~/.local/state/whetstone/`), and the area files `whetstone areas init` copies.

## License

Apache-2.0. See [LICENSE](LICENSE).
