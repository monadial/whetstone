---
status: accepted
---

# The gate answers through the session record

Whetstone's gate is a Git hook that holds a push, or a commit, until the learner passes a Gated Practice on its change. The hook and the Whetstone that answers share nothing but that Practice's session record (ADR 0003): the hook prepares the questions into the record and waits on it, and the learner answers in the same terminal when the hook can open one, or in `whetstone` in another pane when Git runs in a coding agent's shell, which has no terminal. A pass writes a receipt bound to the change's patch ID and the assessment policy version, not to commit SHAs. Whetstone computes the patch ID from its own diff, the way `git patch-id --stable` does. Only the TUI can answer or bypass. We chose this over Sphinx's design, the closest product (a localhost server with a browser questionnaire, receipts tied to exact SHAs for 24 hours, and MCP forms an agent can fill in), because the record already exists and survives a crash, because a receipt that follows the code rather than the commit makes rewording and rebasing free, and because an answer an agent can type isn't the learner's.

## Considered options

- A socket or local server the hook listens on: live, but a second channel to keep in step with the record, and gone when the hook dies.
- A browser questionnaire on localhost, as Sphinx has: works without a terminal, but Whetstone is TUI-only.
- Receipts on exact commit SHAs, with an expiry: simple, but every amend, reword or rebase asks again about the same code.
- One Gated Practice per pushed commit: smaller changes, but questions about intermediate states nobody reviews on their own.
- A command, flag, environment variable or MCP tool that answers or bypasses: lets an agent push unattended, and lets it answer for the learner.

## Consequences

- A push is assessed as one change: only what no remote branch has yet. A merge of already-pushed work counts only its conflict resolutions, a new branch is compared with where it left the remote's default branch, and a force push with the old tip. A push whose commits all hold commit receipts asks nothing, and several gated branches in one push get one Gated Practice each, in turn.
- The session state machine gains the gate's states: prepared by the hook while it holds the session, handed to a pane, waited on, passed, bypassed.
- The hook waits up to 20 minutes by default. A timeout, Ctrl+C or SIGTERM fails the push with the session saved, and once the learner passes, the next push passes on its receipt.
- A receipt doesn't expire; a new assessment policy version asks again.
- Bypass is a key in the TUI, recorded and never a receipt. `whetstone hooks pause` turns the gate off for a while and counts each push it lets through as a bypass. `git --no-verify` skips any hook, so the gate is practice, not a security control, and the docs say so.
- Whetstone reads and writes Git only through gix, the gate included, and never runs the `git` command. So it can't ask Git its version.
- Hooks are installed per repository, as a hook file that runs any existing hook first. That works with every Git version. Only when `core.hooksPath` points elsewhere, where a hook file would never run, does install use Git's config-based hooks instead. Those need Git 2.54 or later, so install warns that older Git skips them, and status shows when each hook last ran. Whetstone doesn't take over a machine-wide `core.hooksPath`.
