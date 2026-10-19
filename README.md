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

Early. `whetstone` opens a terminal home screen offering Practice on the showcase project (a small sample codebase that ships with Whetstone), Settings and Quit. Practice starts by choosing the areas questions focus on and one change from the showcase project's history, which is built into the binary. Enter opens the change to read its diff and each file it touches, before and after. Questions and feedback need AI, which isn't connected yet. Once it is, Practice will send showcase excerpts and your answers to [OpenRouter](https://openrouter.ai), paid with your own credits. Nothing is sent today.

## Run it

Whetstone builds with the Rust toolchain pinned in `rust-toolchain.toml`, on Linux and macOS:

```sh
cargo run
```

It needs an interactive terminal. `whetstone --help` lists the options. Set `NO_COLOR` to turn colour off: focus and selection stay visible through markers and bold text.

Everything works from the keyboard. `j`/`k` or the arrows move, Tab and Shift+Tab move focus, and Enter or Space activates. `q` goes back, and quits from home. `?` shows the keys for the current screen and `:` lists its actions. Ctrl+C quits from anywhere.

In a change, `h`/`l` switch between the files and the diff, before and after panes. `gg`/`G` and Ctrl+D/Ctrl+U jump and scroll, and each pane keeps its place. `/` searches the focused pane, and `n`/`N` move to the next and previous match.

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

The OpenRouter key never goes in the file. Set `OPENROUTER_API_KEY` in your shell; Settings shows only whether it is set. Whetstone writes nothing to disk except its log, in `$XDG_STATE_HOME/whetstone/` (by default `~/.local/state/whetstone/`).

## License

Apache-2.0. See [LICENSE](LICENSE).
