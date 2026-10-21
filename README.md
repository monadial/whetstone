<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="assets/brand/whetstone-square-symbol.svg" width="120">
    <img src="assets/brand/whetstone-logo.svg" alt="whetstone by Monadial" width="480">
  </picture>
</p>

# Whetstone by Monadial

**Understand the code you ship.**

Whetstone helps you examine your understanding of real code through questions and written reasoning. You take a change (its commit message, its diff and the code it touches), answer a question about it and, if you choose, explain why. The feedback looks at your choice and your reasoning separately. Writing reasoning is optional: answer with a choice alone, and the feedback shows how your choice fares and what a full answer covers, without assessing reasoning or asking Jev about it.

## Status

Early. `whetstone` opens a terminal home screen. Started inside a Git repository, it offers Practice on that repository first, then Practice on the showcase project (a small sample codebase that ships with Whetstone), Settings and Quit. Practice starts by choosing the areas questions focus on and one change: a commit from your repository's history, or one of the showcase project's curated changes, which are built into the binary. Each commit shows its short ID, when it was authored ("2 hours ago", or "Oct 14" once a week has passed), its author, its summary and its size: the files it touches and the lines it adds and removes, found in the background as the commit comes into view. A narrow terminal shortens the date, then leaves out the size and then the author, so the summary keeps room. Enter opens the change to read its diff and each file it touches, before and after.

On your repository, **Review what will be sent** opens the outbound review: exactly what a Practice will send and where. It shows the commit message, every touched file with its size and whether it is sent, any possible secrets, each area with its prompt, and the total size against the limit. **Send** is enabled once `OPENROUTER_API_KEY` is set, the evidence can be sent, and connected code is found or given up on. It starts Practice on the change, and every request that follows, a Retry or an Ask anyway included, sends exactly the evidence you approved to [OpenRouter](https://openrouter.ai), paid with your own credits. Nothing from your repository is sent before you press Send, and Cancel or Back returns you to the picker.

On the showcase project, **Generate questions** prepares questions once `OPENROUTER_API_KEY` is set, and is disabled until then. It sends showcase excerpts to OpenRouter, paid with your credits: Jev first decides which chosen areas the change is worth asking about, the generator writes candidate questions about those, and Jev checks each one, so only questions that pass are shown. Each question's rubric has one or two criteria, each the idea a sufficient answer needs: Jev drops a criterion that asks for more, such as naming the code's identifiers, and a question with no criterion left isn't shown. Your reasoning meets a criterion when it gives that idea, in any words. When no chosen area is worth asking about, **Ask anyway** writes about them all the same, and their questions are still checked.

When the question check withholds questions, on **No questions passed checks** or when fewer passed than you asked for, **Review withheld questions** shows each withheld question, one at a time and read-only. You see everything the generator wrote, each check that withheld it with Jev's band and probability, and the evidence it rests on. For each check, you vote **Rightly withheld** or **Should have passed**, with an optional note, or skip it. Voting never changes an outcome, and a withheld question never comes into Practice. Each vote is saved as a labelled example in `labels.toml` in Whetstone's state folder, beside the log (by default `~/.local/state/whetstone/labels.toml`). The file is readable only by you and is never sent anywhere. Votes about your own repository hold private code from it. The labelled-example tool reads it, and skips the votes it can't send to Jev as Practice did, such as those about your own repository:

```sh
cargo run -p whetstone-labels -- check --labels ~/.local/state/whetstone/labels.toml
OPENROUTER_API_KEY=… cargo run -p whetstone-labels -- measure --labels ~/.local/state/whetstone/labels.toml
```

[`showcase/labels/README.md`](showcase/labels/README.md#learner-votes) describes the format. Votes about the showcase project hold no private code, and can be copied into `showcase/labels/` by hand.

A request that fails stops its stage, which says plainly what went wrong and what you can do: no key, a refused key, no credits left, rate limited, OpenRouter or the model's provider unavailable, no zero-retention endpoint for the model, no reply in time (30 seconds for Jev, 120 for the generator), a request OpenRouter refuses as invalid, or a reply that couldn't be read, which is never shown as a judgement. **Retry** asks again only what failed, and nothing is retried until you choose to.

## Run it

Whetstone builds with the Rust toolchain pinned in `rust-toolchain.toml`, on Linux and macOS:

```sh
cargo run
```

It needs an interactive terminal: [Terminal support](#terminal-support) says which ones. `whetstone --help` lists the options. Set `NO_COLOR` to turn colour off: focus and selection stay visible through markers and bold text.

Everything works from the keyboard. `j`/`k` or the arrows move, Tab and Shift+Tab move focus, and Enter or Space activates. `q` goes back, and quits from home. `?` shows the keys for the current screen and `:` lists its actions. Ctrl+C quits from anywhere, and Ctrl+Z suspends Whetstone to your shell until `fg`.

In the picker, Page Down and Page Up move through the changes a screen at a time, Ctrl+D/Ctrl+U half a screen, and `gg`/`G` go to the first or last change: on your repository, the newest commit or the oldest read so far. **Next page** and **Previous page** under the list do the same, and the heading over the list says which changes are in view. `/` filters your repository's commits by message, author or ID as you type, reading older commits in the background for more matches: Enter keeps the filter, and Esc shows every commit again. In a change, `h`/`l` switch between the files and the diff, before and after panes. `gg`/`G` and Ctrl+D/Ctrl+U jump and scroll, and each pane keeps its place. `/` searches the focused pane, and `n`/`N` move to the next and previous match. In the outbound review, Space leaves a file out or brings it back, and Enter reads exactly what is sent for the change, a file or an area.

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

`.whetstone/config.toml` is optional, and so are its two keys. `exclude` lists gitignore-style patterns for files that a Practice on this repository leaves out unless the learner brings them back in the outbound review. `connected_code_limit_kb` lowers the learner's connected code limit for this repository:

```toml
exclude = ["fixtures/", "docs/private/*.md"]
connected_code_limit_kb = 40
```

A `!` pattern is invalid, and a limit larger than the learner's has no effect, so the config can only leave more out. Lockfiles and common secret paths, such as `.env` and `*.pem`, are left out by default too, and can be brought back the same way. Binary files, submodules and symlinks are always left out.

## Configure it

Settings shows what Practice will use, and how much colour Whetstone draws with. It is read-only: the values come from `config.toml` in Whetstone's config folder, `$XDG_CONFIG_HOME/whetstone/` (by default `~/.config/whetstone/`) on Linux and macOS. Every setting is optional, and without a file Whetstone uses these defaults and says so:

```toml
# The service Practice sends its requests to. OpenRouter is the only one so far.
provider = "openrouter"

# How much colour Whetstone draws with: "auto", "24-bit", "256", "16" or "none".
# Auto reads COLORTERM, then TERM. A non-empty NO_COLOR wins over any value.
colours = "auto"

# How much connected code Practice adds to the evidence, from 0 to 400 KB.
# 0 sends none. A repository's .whetstone/config.toml can lower it, never raise it.
connected_code_limit_kb = 100

# How many questions a Practice asks for, from 3 to 5.
questions = 4

# Jev, the evaluator: it returns decisions and probabilities, never prose.
[jev]
model = "typesafe/jev-1.14"

# The generator writes questions, explanations and hints.
[generator]
model = "anthropic/claude-sonnet-5.5"

# The assessment policy's cut-offs for each kind of Jev check, policy 0's by
# default: an answer at or above upper is clear yes, at or below lower is clear
# no, and unclear in between. Each is a number from 0 to 1, with lower below
# upper. Your own make outcomes not comparable with policy 0's, and every
# outcome records the cut-offs that banded it. A repository's config can't set them.
[cut_offs]
relevance_gate = { upper = 0.80, lower = 0.20 }
question_check = { upper = 0.80, lower = 0.20 }
option_rating = { upper = 0.80, lower = 0.20 }
rationale_and_hint_checks = { upper = 0.80, lower = 0.20 }
reasoning_criterion = { upper = 0.80, lower = 0.20 }
explanation_check = { upper = 0.80, lower = 0.20 }
```

Settings also says how far to trust the outcomes: the assessment policy's version, whether its cut-offs have been validated against labelled examples, each job's cut-offs, marked default or from your config, a note when any of them isn't policy 0's, and a note when `[jev] model` isn't `typesafe/jev-1.14`, the model the policy was checked against. A model OpenRouter refuses for having no zero-retention endpoint is marked there until Whetstone quits.

The OpenRouter key never goes in the file. Set `OPENROUTER_API_KEY` in your shell; Settings shows only whether it is set. Whetstone writes nothing to disk except its log and your votes on withheld questions (`labels.toml`), both in `$XDG_STATE_HOME/whetstone/` (by default `~/.local/state/whetstone/`), and the area files `whetstone areas init` copies.

## Terminal support

Whetstone states which terminals it is meant for, in two tiers. **Supported** means tested in CI on every change and checked by hand before each release. CI runs Whetstone in a pseudo-terminal on every supported platform, and before a release each supported terminal goes through the [pre-release terminal checklist](docs/terminal-checklist.md). **Expected to work** means bugs are fixed when they're reported, but nobody checks before a release.

Supported:

- Linux and macOS, on x86_64 and arm64.
- iTerm2, Ghostty and Terminal.app on macOS; GNOME Terminal and Ghostty on Linux.
- Each of those inside tmux, and over SSH.

Expected to work:

- kitty, WezTerm, Alacritty, Konsole, the VS Code terminal and GNU screen, and other terminals compatible with `xterm-256color`.
- WSL, the only way to run Whetstone on Windows. There is no Windows build.

Whetstone needs:

- a UTF-8 locale;
- a font with box-drawing characters, which draw its borders;
- a terminal with bracketed paste, so that a paste arrives as text rather than keys.

It refuses to start, says why and exits with 1 when stdin or stdout isn't a terminal, or when `TERM=dumb` says the terminal can't move the cursor. `--help`, `--version` and `whetstone areas init` work anywhere.

Every screen fits whole at 80×24. Below 50×12, Whetstone shows "Window too small" instead, and growing the window brings back where you were.

A paste goes into a search as text, on one line: a pasted line break never runs the search and a pasted `q` never goes back. Anywhere else a paste does nothing.

Ctrl+Z gives you your shell back, and `fg` brings back the same screen at the window's current size. When Whetstone runs without a shell to return to, such as a tmux pane's own command or `ssh -t host whetstone`, Ctrl+Z has nowhere to go, so Whetstone carries on. Whetstone leaves your terminal as it found it when it ends by `q`, Ctrl+C, a panic, or SIGHUP, SIGINT or SIGTERM from outside, as when the window closes or SSH drops. It exits with 0 after `q` or Ctrl+C, and with 128 plus the signal's number after one of those signals.

SSH doesn't forward `COLORTERM` unless set up to, so over SSH Whetstone goes by `TERM` alone and colours drop to 256 or 16. If your terminal shows 24-bit colour, set `colours = "24-bit"` in the [config](#configure-it).

Known limits:

- Right-to-left text, such as Arabic or Hebrew, is kept as you typed it but shown left to right.
- A rare emoji may sit one cell off, where your terminal and Whetstone disagree on its width.
- There's no mouse. Whetstone never captures it, so your terminal's own selection and copying keep working, and a click does nothing.

In tmux, set `escape-time` low, so Esc acts at once. It is how long tmux waits after an Esc for the rest of a key sequence, and before tmux 3.5 it is 500 ms by default. In `~/.tmux.conf`:

```text
set -sg escape-time 10
```

## License

Apache-2.0. See [LICENSE](LICENSE).
