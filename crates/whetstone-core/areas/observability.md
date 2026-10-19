+++
name = "Observability"
description = "How you can tell what the work is doing and why"
+++

Focus on how the people who run, maintain or use the work can tell what it
is doing, whether it is working, and why it went wrong.

Look for:

- The logs, metrics, traces and events the change adds, removes or alters,
  and the question each one answers.
- Whether a failure would be noticed, by whom and how soon, and whether the
  signal points to its cause or only to a symptom.
- The identifiers that connect related events, such as request, tenant or
  operation IDs, and whether they are carried all the way through.
- What is recorded that should not be: secrets, personal data, or so much
  noise that the useful signal is lost.
- Alerts, dashboards and error messages, and whether they would help someone
  who did not write the change.

When the repository is not software, observability is how its owners and
readers can tell whether the material works: how a mistake or an outdated
part would be noticed and reported, how changes are recorded and explained,
and how a reader can trace a statement back to its source.
