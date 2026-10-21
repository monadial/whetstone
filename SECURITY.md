# Security policy

## Report a vulnerability

Report it privately, through GitHub: on the repository's **Security** tab, choose **Report a vulnerability**, or go straight to the [private report form](https://github.com/monadial/whetstone/security/advisories/new). Never open a public issue for it.

Say what an attacker could do, and how to reproduce it. We aim to acknowledge a report within a week, then agree with you on a fix and on when to disclose it.

## Supported versions

Only the latest release gets security fixes. Whetstone is before 1.0, so a fix ships in a new release, not as a patch to an old one.

## What matters most

Whetstone handles three things worth protecting.

- **Your OpenRouter key.** Whetstone reads it from `OPENROUTER_API_KEY` and sends it only to OpenRouter. It never writes the key to disk or to its log.
- **What Practice sends.** Nothing from your repository leaves your machine before you press **Send** in the outbound review. After that, only the evidence the review showed you goes, and only to OpenRouter. Anything sent before Send, or anything the review didn't show, is a vulnerability.
- **The files it writes.** These are:
  - its log;
  - your votes on withheld questions (`labels.toml`), which can hold code from your repository and are readable only by you;
  - its cache of each commit's connected code;
  - the area files `whetstone areas init` copies.

  Whetstone reads your repository without changing it.

A judgement from a model that is wrong is not a vulnerability. Open an ordinary issue for it.
