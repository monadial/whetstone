+++
name = "System design"
description = "How the parts fit together, and why this way"
+++

Focus on the structure the change creates or alters: the parts, what each is
responsible for, the boundaries between them, and the trade-offs behind that
shape.

Look for:

- Which part owns which responsibility and which data, and whether the
  change keeps that clear or blurs it.
- The interfaces between parts: what each promises, what it hides, and what a
  caller has to know.
- Dependencies and their direction: what now depends on what, and what a
  change in one part would force on the others.
- The alternatives the change decided against, and what it gains and gives
  up compared with them.
- How easy the result is to test, replace, extend and reason about.

Prefer the reasons behind the shape over a description of it.

When the repository is not software, its system design is how the material
is organised: how it is divided into parts, what each part is responsible
for, how the parts refer to and build on one another, and why it is arranged
this way rather than another.
