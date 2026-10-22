# Changelog

Every release of Whetstone, newest first. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## Unreleased

The first release.

### Added

- A terminal home screen offering Practice on the Git repository Whetstone starts in, Practice on the showcase project, Settings and Quit.
- Practice on your repository: choose areas and a commit, read its diff and each file it touches, and review exactly what will be sent, and where, before anything leaves your machine.
- Practice on the showcase project, a sample codebase built into the binary.
- Questions the generator writes and Jev checks. You answer with a choice and, if you choose, your reasoning, and the feedback assesses each separately.
- **Review withheld questions**, with a vote on each check that withheld one, saved as a labelled example.
- Areas of your own in `.whetstone/areas/`, and `whetstone areas init` to start from the five defaults.
- Settings read from `config.toml`: provider, colours, connected code limit, number of questions, models and cut-offs.
- Support for Linux and macOS, on x86_64 and arm64, in the terminals the README lists.
- `whetstone --version` names the commit the binary was built from and its date. `-V` prints the version alone.
