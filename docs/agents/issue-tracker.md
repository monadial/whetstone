# Issue tracker: GitHub

Issues and specs for this repo live in GitHub Issues at https://github.com/monadial/whetstone/issues. Use the `gh` CLI for tracker operations.

## Repository scope

This tracker covers the CLI and narrowly reviewed public client contracts. Company portal, website, and internal commercial planning belong in their own verified private repositories. A full cross-product map requires a private planning tracker or the user's explicit choice of a public summary.

Before publication, verify the active repository and its current visibility, and fetch any referenced issues. When the local restart kit is present, read `docs/restart-kit/skills/REPOSITORY-ROUTING.md`. Keep the full kit untracked until its file-by-file public subset is reviewed.

## Conventions

Run commands inside this checkout so `gh` infers the repository from the Git remote. Outside the checkout, pass `--repo monadial/whetstone` (or use the explicit repository path for `gh api`).

- **Create an issue**: `gh issue create --title "..." --body-file <body-file>`. Write multiline bodies to a temporary UTF-8 file and pass its path.
- **Read an issue**: `gh issue view <number> --json number,title,body,labels,comments`.
- **List issues**: `gh issue list --state open --json number,title,body,labels,comments --jq '[.[] | {number, title, body, labels: [.labels[].name], comments: [.comments[].body]}]'` with appropriate `--label` and `--state` filters. Choose a sufficient `--limit` or use a paginated API query when completeness matters.
- **Make an issue a sub-issue of a parent**: `gh issue create --parent <parent> --title "..." --body-file <body-file>`, or `gh issue edit <parent> --add-sub-issue <child>`. If the CLI lacks these flags, use `gh api --method POST repos/monadial/whetstone/issues/<parent>/sub_issues -F sub_issue_id=<child-db-id>`. Obtain the database ID with `gh api repos/monadial/whetstone/issues/<child> --jq .id`. Where sub-issues are unavailable, add the child to a task list in the parent body and put `Part of #<parent>` at the top of the child body.
- **Comment on an issue**: `gh issue comment <number> --body-file <body-file>`.
- **Apply / remove labels**: `gh issue edit <number> --add-label "..."` / `--remove-label "..."`. See `docs/agents/triage-labels.md` for triage roles.
- **Close an issue**: `gh issue close <number>`. If a closing explanation is needed, post it first with `gh issue comment <number> --body-file <body-file>`.

## Pull requests as a triage surface

**PRs as a request surface: no.**

Set this to `yes` if the repo later treats external PRs as feature requests; `/triage` reads this flag. When enabled, use the same label mapping and states as issues:

- **Read a PR**: `gh pr view <number> --comments` and `gh pr diff <number>`.
- **List external PRs**: `gh api --paginate 'repos/monadial/whetstone/pulls?state=open' --jq '.[] | select(.author_association | IN("OWNER","MEMBER","COLLABORATOR") | not) | {number, title, author: .user.login, author_association, labels: [.labels[].name]}'`.
- **Comment / label / close**: `gh pr comment <number> --body-file <body-file>`, `gh pr edit <number> --add-label "..."` / `--remove-label "..."`, and `gh pr close <number>`.

GitHub shares one number space across issues and PRs. Resolve ambiguous references with `gh pr view <number>`, falling back to `gh issue view <number>`.

## Skill operations

- **Publish to the issue tracker**: create a GitHub issue.
- **Fetch the relevant ticket**: read the GitHub issue as described above.

## Wayfinding operations

Used by `/wayfinder`. The map is a single issue with child issues as tickets.

- **Map**: an issue labelled `wayfinder:map`, holding the Notes / Decisions-so-far / Fog body. Create it with `gh issue create --label wayfinder:map --title "..." --body-file <body-file>`.
- **Child ticket**: link it to the map as a sub-issue, using the fallback above if needed. Labels are `wayfinder:<type>` (`research`, `prototype`, `grilling`, or `task`). Create any missing wayfinding labels when that workflow is first used. Once claimed, assign the ticket to the driving developer.
- **Blocking**: use GitHub's native issue dependencies. Add an edge with `gh api --method POST repos/monadial/whetstone/issues/<child>/dependencies/blocked_by -F issue_id=<blocker-db-id>`. Obtain the blocker's numeric database ID with `gh api repos/monadial/whetstone/issues/<blocker> --jq .id`; it is neither the issue number nor `node_id`. `issue_dependencies_summary.blocked_by` counts open blockers. Where dependencies are unavailable, put `Blocked by: #<number>, #<number>` at the top of the child body. A ticket is unblocked when every blocker is closed.
- **Frontier query**: list all the map's open children, scoped to its sub-issues or task list. Exclude tickets with an open blocker or an assignee; the first remaining ticket in map order wins. Read each candidate's dependency summary or resolve its fallback blocker references.
- **Claim**: `gh issue edit <number> --add-assignee @me`, the driving session's first write.
- **Resolve**: comment with the answer using `--body-file`, close the ticket, then append a concise answer and link to the map's Decisions-so-far using `gh issue edit <map> --body-file <updated-map-body-file>`. Preserve the rest of the map body.
