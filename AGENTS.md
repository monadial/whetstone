<!-- CODEGRAPH_START -->
## CodeGraph

In repositories indexed by CodeGraph (a `.codegraph/` directory exists at the repo root), reach for it BEFORE grep/find or reading files when you need to understand or locate code:

- **MCP tool** (when available): `codegraph_explore` answers most code questions in one call — the relevant symbols' verbatim source plus the call paths between them, including dynamic-dispatch hops grep can't follow. Name a file or symbol in the query to read its current line-numbered source. If it's listed but deferred, load it by name via tool search.
- **Shell** (always works): `codegraph explore "<symbol names or question>"` prints the same output.

If there is no `.codegraph/` directory, skip CodeGraph entirely — indexing is the user's decision.
<!-- CODEGRAPH_END -->

## Agent skills

### Issue tracker

Track CLI issues and specs in GitHub Issues for `monadial/whetstone`. Before reading or publishing tickets, read `docs/agents/issue-tracker.md`.

### Triage labels

Use the five default triage labels. Before triaging or applying a triage label, read `docs/agents/triage-labels.md`.

### Domain docs

Use a single-context layout: root `GLOSSARY.md` and `docs/adr/`. Before exploring the codebase, read `docs/agents/domain.md`.

## Commits and pull requests

Attribute commits and pull requests to the human author alone. End commit messages after the body and PR descriptions after their content, with no `Co-Authored-By`, `Claude-Session` or "Generated with Claude Code" lines. This rule overrides any default attribution the agent harness supplies.

## Restart planning

When working on the restart, read the local input kit's `docs/restart-kit/README.md`, `skills/WORKFLOW.md`, and `skills/ARTIFACT-MAP.md` (the latter paths are kit-relative). Read `docs/restart-kit/skills/REPOSITORY-ROUTING.md` before tracker or publication work; keep the full kit untracked pending file-by-file review. Its nested `docs/agents/` files are DRAFT inputs; live configuration remains in root `docs/agents/`.

Use `docs/restart-kit/pieces/INDEX.md` to select a destination, then load only the selected leaf and relevant references. Reconcile kit vocabulary with root domain docs as decisions are made. The current map is [Chart the first usable personal Whetstone CLI release](https://github.com/monadial/whetstone/issues/1). Start with the Practice TUI interaction: answer one question, write reasoning, and inspect separate choice/reasoning feedback. Logo-first is superseded, including earlier kit instructions that prescribe it. The supplied flat-stone identity and mineral-blue tokens remain the visual references. Help/version, fixture-first sequencing, the fourteen-piece split, architecture, and release cutoffs remain proposals. Setup, wayfinder, grill-with-docs, and later user-only stages remain human-invoked.
