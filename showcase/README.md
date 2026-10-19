# Showcase project

The showcase project is a small multi-tenant payments service that ships with
Whetstone, so anyone can try Practice without supplying their own source. Its
curated history is ten commits, and each is a Change that a Practice can be
built from.

| Path | What it is |
| --- | --- |
| `payments/` | The project's code, exactly as the history builds it. It is its own Cargo workspace with its own lockfile and is never a member of the CLI workspace. |
| `payments/.whetstone/config.toml` | Whetstone's settings for the project. It sets nothing, and the project has no area files, so Practice offers Whetstone's default areas. |
| `payments-history/` | The curated history: a `git format-patch` series that builds `payments/` from an empty tree. |
| `check-history.sh` | Replays the series onto an empty tree and checks that it reproduces `payments/` exactly. CI runs it. |

Build and test the project on its own, offline once its dependencies are
fetched:

```sh
cd showcase/payments
cargo test --locked
```

## Areas trailer

Every change names the areas it touches in one trailer at the end of its
commit message:

```text
Areas: scalability, infrastructure
```

The value is a comma-separated list of area IDs: the file names, without
`.md`, of Whetstone's default area files in `crates/whetstone-core/areas/`,
such as `system-design`. Git reads it with
`git log --format='%(trailers:key=Areas,valueonly)'`. The build and
`check-history.sh` both fail when a change has no `Areas:` trailer or names
anything but a default area's ID, and `check-history.sh` also fails when some
default area is covered by no change.

Two changes are deliberately trivial, a rename and a formatting pass, so that
Practice also meets changes with nothing worth asking about.

## Known gaps

The service aims to be realistic, not flawless. These gaps are known, and
Practice may well ask about them:

- Requests for the same payment are not serialised, so the last save wins. A
  retry racing another retry can overwrite a settled payment, and when two
  first requests share an idempotency key, the loser gets a 409.
- The file store writes and syncs to disk under a lock, on the async request
  path.
- The cache holds every tenant behind one lock, and retry backoff has no
  jitter.

## Changing the history

Edit the history with git, never `payments/` or a patch file by hand. From the
repository root:

```sh
showcase/check-history.sh --keep ../payments-replay
# Edit ../payments-replay with git: commit, rebase -i, commit --amend.
# Keep every change building and its tests passing.

rm -rf showcase/payments showcase/payments-history
git -C ../payments-replay format-patch --root --no-signature --zero-commit \
  -o "$PWD/showcase/payments-history" HEAD
mkdir showcase/payments
git -C ../payments-replay archive HEAD | tar -x -C showcase/payments
showcase/check-history.sh
```

`--zero-commit` and `--no-signature` keep the patch files stable across
replays and git versions. The patches carry fixed author dates, so set
`GIT_AUTHOR_DATE` when you add a change.
