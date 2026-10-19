+++
name = "Infrastructure"
description = "Where the work runs and what it depends on"
+++

Focus on what the work depends on to run, and how it is built, configured,
stored, deployed and operated.

Look for:

- The external systems the change relies on or adds, such as storage,
  networks, queues, services and libraries, and what it expects of each.
- How it handles their failure: timeouts, retries, partial writes and
  outages, and the state each one leaves behind.
- Configuration, secrets and environments: what must be set, what differs
  between environments, and what happens when something is missing or wrong.
- Building, packaging, deploying and migrating: what must happen, and in
  what order, to ship the change safely and to roll it back.
- The limits the environment imposes, such as disk, memory, connections and
  quotas, and how close the change comes to them.

Prefer consequences in a real environment over syntax or tool trivia.

When the repository is not software, its infrastructure is what the material
relies on to be produced, published and kept up to date: its build and
publishing steps, the platforms and tools it assumes, the files and sources
it links to, and what breaks when one of them changes or disappears.
