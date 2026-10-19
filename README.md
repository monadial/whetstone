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

Early. `whetstone` opens a terminal home screen offering Practice on the showcase project (a small sample codebase that ships with Whetstone), Settings and Quit. Questions and feedback need AI, which isn't connected yet. Once it is, Practice will send showcase excerpts and your answers to [OpenRouter](https://openrouter.ai), paid with your own credits. Nothing is sent today.

## Run it

Whetstone builds with the Rust toolchain pinned in `rust-toolchain.toml`, on Linux and macOS:

```sh
cargo run
```

It needs an interactive terminal. `whetstone --help` lists the options. Set `NO_COLOR` to turn colour off: focus and selection stay visible through markers and bold text.

## License

Apache-2.0. See [LICENSE](LICENSE).
