---
status: accepted
---

# The session record is the source of truth

Whetstone writes each Practice's session record to disk as the session goes, and treats it as the session's source of truth rather than a log kept beside the screen. No request is sent until the record of what it sends is saved, and an unfinished session is resumed by rebuilding it from its record, reusing every result already received instead of paying for it again. We chose this over keeping unfinished sessions read-only because questions and evaluations cost money, and a learner's reasoning shouldn't be lost to Ctrl+C or a dropped SSH connection. We also chose to build one session state machine, once, for Practice, resume and the commit and push gate ([#132](https://github.com/monadial/whetstone/issues/132)), over a smaller extraction that only resume needs, so that the gate doesn't force a second rewrite.

## Considered options

- Unfinished sessions stay read-only, and resume comes later: a smaller first release, but every quit loses paid questions and drafts.
- Pull out only what resume needs, and extend it for the gate later: less work now, but the gate's states (questions prepared in the background, a check a hook waits on, receipts) would reshape it again.

## Consequences

- The record starts before the relevance gate's first request, so evidence sent while questions are prepared is always recorded, even when none pass or the learner quits.
- A request is recorded when it is sent, not only when it ends. One still waiting when Whetstone quits becomes an interrupted request, sent again only on Retry, because it may already have been billed.
- Closing a session doesn't cancel its requests: they finish and are saved while Whetstone runs.
- Drafts are saved as they are written, so quitting never has to ask before losing one.
- Records only grow. A later attempt, re-evaluation or learner verdict is saved beside what came before, never in its place.
- Records carry a format version. A newer Whetstone always opens and exports older records, and resumes one only when it can rebuild it exactly. A record written by a newer Whetstone isn't opened.
- One Whetstone at a time writes a session's record; another can view it read-only. Session and request IDs must be unique across runs, not only within one.
- Records live in `$XDG_STATE_HOME/whetstone/sessions/`, owner-only, and are kept until the learner deletes them. Deleting a session also deletes the labelled examples it produced, because they hold its code.
- The state machine can't be designed until #132's open questions that shape its states are settled: what a gated change is, how a waiting hook finds the pane that answers, and what a receipt covers. ADR 0004 settles them. Whether the gate ships in the first release stays a release-boundary decision.
