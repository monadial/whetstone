# Assessment policy versions

The assessment policy's default cut-offs turn Jev's probabilities into clear yes, unclear or clear no for each job. They live in `crates/whetstone-core/src/assessment_policy.rs`, and they change only by a recorded decision that bumps the version ([rule 38](../CODING_STANDARDS.md#ai-and-storage)). This file is that record: one entry per version, newest first, with its date, its cut-offs, the reason and the evidence.

Every released version stays built in (`AssessmentPolicy::released()`), so an outcome recorded under an older one is still identified by its version and the cut-offs that banded it. Nothing rewrites it. A learner's own cut-offs from `[cut_offs]` keep the version of the policy whose defaults they replace, and outcomes banded with them are marked as the learner's.

## Policy 1 (21 October 2026)

- **Status:** unvalidated. [#99](https://github.com/monadial/whetstone/issues/99)'s labels will confirm or change these values.
- **Cut-offs (upper / lower):**

  | Job | Policy 1 | Policy 0 |
  | --- | --- | --- |
  | Giveaway check (`giveaway_check`): `no_giveaway` only | 0.50 / 0.20 | 0.80 / 0.20, as part of the question check |
  | Every other job, the question check's other checks included | 0.80 / 0.20 | 0.80 / 0.20 |

- **Decision (the maintainer, [#124](https://github.com/monadial/whetstone/issues/124)):** `no_giveaway` gets a cut-off pair of its own, defaulting to 0.50 and 0.20. The other checks keep 0.80 and 0.20.
- **Reason:** Jev scores `no_giveaway` on a different scale from the other question checks. At 0.80 it withholds every candidate, sound ones included, so out of the box Practice showed no questions. Lowering the whole question check to 0.50 would also loosen "answerable", "about its area" and "needs this change", which don't need it.
- **Evidence ([#119](https://github.com/monadial/whetstone/issues/119), PR [#121](https://github.com/monadial/whetstone/pull/121)):**
  - **Six showcase pairs (change and area), 30 candidates, under policy 0:** no pair showed a question. `no_giveaway` withheld all 30, unclear on every one after the reworked base prompts (median 0.46, range 0.24 to 0.70). On the other question checks, candidates scored around 0.8 (`needs_change` median 0.81, `about_area` 0.79).
  - **Hand-made variants of one question, through the labelled-example tool, three runs each:** even options with no cue at all topped out around 0.6 on `no_giveaway` (three bare strings: 0.55 to 0.62; one short template: 0.51 to 0.55). #98's seed giveaway, whose stem quotes the answer, stayed clear no. So the check still tells a broken candidate from a sound one, on a lower scale.
  - **The same answers with the whole question check at a lower upper cut-off, computed offline:** at 0.60, 1 of 6 pairs showed a question; at 0.50, 5 of 6. Policy 1 lowers `no_giveaway` alone, so it shows at most that many.
  - **The maintainer's own repository, as #124 reports:** in two Practices, 18 of 18 candidates were withheld, 6 of them by `no_giveaway` alone.
- **Measured under policy 1 ([#124](https://github.com/monadial/whetstone/issues/124), 21 October 2026), with the base prompts of #130 and #135:**
  - **The same six pairs:** 2 of 30 candidates were shown, one each on 2 pairs. `no_giveaway` still withheld 24, each unclear, so below 0.50. Of the 6 candidates at 0.50 or more, the other checks withheld 4.
  - **The labelled examples:** the three seeds labelled yes for `no_giveaway`, still to review, scored 0.16 to 0.46, none of them at 0.50. #98's seed giveaway stayed clear no.
- **Still open:**
  - #98's sound seed (`seed-retry-key`) scored 0.16 to 0.20 on `no_giveaway`, clear no even at 0.50, so #99's labels should check whether that label or Jev is wrong.
  - At 0.50, 24 of the 30 live candidates were still unclear on `no_giveaway`, so #99's labels should also say whether the giveaway check's upper cut-off belongs lower still.

## Policy 0 (20 October 2026)

- **Status:** unvalidated.
- **Cut-offs:** every job at 0.80 / 0.20: the relevance gate, the question check (checks 1 to 6, `no_giveaway` included), option ratings, rationale and hint checks, reasoning criteria and explanation checks.
- **Decision (spec [#81](https://github.com/monadial/whetstone/issues/81)):** start from the example cut-offs in Jev's documentation, until labelled examples can set them.
- **Evidence:** none yet. The labelled-example tool ([#98](https://github.com/monadial/whetstone/issues/98)) was built to measure them.
