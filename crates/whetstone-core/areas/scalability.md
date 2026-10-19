+++
name = "Scalability"
description = "How the work holds up as use and data grow"
+++

Focus on how the work behaves as it grows: more users, requests, data,
tenants, contributors or history.

Look for:

- What the cost of an operation depends on, and which inputs make it grow
  faster than expected.
- The shared resources the change touches, such as locks, caches,
  connections and queues, and what happens when many callers use them at
  once.
- Concurrency: races, ordering, duplicated work and contention, and what the
  change guarantees when two things happen at the same time.
- Bounds and limits: what is unbounded, what is held in memory or cached,
  and what happens when a limit is reached.
- Whether the design can later be split, partitioned or spread out, or
  whether the change makes that harder.

Prefer growth the change makes plausible over micro-optimisations.

When the repository is not software, scalability is how the material copes
with growth: more readers, learners, sections, authors or jurisdictions;
whether its structure, conventions and review process still hold; and which
parts would have to be redone if it doubled.
