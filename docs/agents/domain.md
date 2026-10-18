# Domain Docs

This repo uses a single-context domain documentation layout.

## Before exploring, read these

- **`GLOSSARY.md`** at the repo root: read the domain vocabulary.
- **`docs/adr/`**: read ADRs that touch the area you are about to work in.

If these files do not exist, proceed silently. Do not flag their absence or suggest creating them upfront. The `/domain-modeling` skill, also reached via `/grill-with-docs` and `/improve-codebase-architecture`, creates them lazily when terms or decisions are resolved.

## File structure

```text
/
├── GLOSSARY.md
└── docs/
    └── adr/
        └── NNNN-decision-title.md
```

## Use the glossary's vocabulary

When naming a domain concept in an issue title, refactor proposal, hypothesis, or test, use the term defined in `GLOSSARY.md` rather than synonyms the glossary avoids.

If a concept is missing, reconsider whether the project uses that language. Note a genuine vocabulary gap for `/domain-modeling`.

## Flag ADR conflicts

If a proposal contradicts an existing ADR, identify the ADR and explain why reopening the decision is warranted rather than silently overriding it.
