# Assessment policy versions

The assessment policy's default cut-offs turn Jev's probabilities into clear yes, unclear or clear no for each job. They live in `crates/whetstone-core/src/assessment_policy.rs`, and they change only by a recorded decision that bumps the version ([rule 38](../CODING_STANDARDS.md#ai-and-storage)). This file is that record: one entry per version, newest first, with its date, its cut-offs, the reason and the evidence.

Every released version stays built in (`AssessmentPolicy::released()`), so an outcome recorded under an older one is still identified by its version and the cut-offs that banded it. Nothing rewrites it. A learner's own cut-offs from `[cut_offs]` keep the version of the policy whose defaults they replace, and outcomes banded with them are marked as the learner's.

## Policy 1 (22 October 2026)

- **Status:** unvalidated. The cut-offs are placeholders: [#99](https://github.com/monadial/whetstone/issues/99)'s labels will confirm or change them.
- **Cut-offs (upper / lower):**

  | Job | Policy 1 | Policy 0 |
  | --- | --- | --- |
  | Answer probe (`answer_probe`) | 0.60 / 0.20 | none: policy 0 had no answer probe. Its built-in record lists 0.80 / 0.20 for the job, which banded no outcome |
  | Every other job | 0.80 / 0.20 | 0.80 / 0.20 |

- **Decision ([#159](https://github.com/monadial/whetstone/issues/159)):** the question check's `no_giveaway` and `needs_change` are replaced by one test, the answer probe. Jev answers the candidate from its stem and its options' text alone, as a choice among A, B and C, and the show rule withholds the candidate when the probe's probability on the answer key is clear yes. A confident pick of a wrong option is a plausible wrong option, not a giveaway, so only the answer key's probability counts. A guess among three options is 0.33, so a key Jev picks at 0.60 or more without reading the change is too easy.
- **Reason:** both checks asked Jev for an opinion about a hypothetical reader, and Jev scored them on a scale of their own. `no_giveaway` withheld every candidate on the showcase, so Practice showed no questions. The probe asks Jev to be that reader instead.
- **Evidence:**
  - **[#154](https://github.com/monadial/whetstone/issues/154), PR [#158](https://github.com/monadial/whetstone/pull/158), under policy 0:** six showcase pairs (change and area), 30 candidates, and no question shown. `no_giveaway` withheld all 30. With the answer key in its state it was unclear on all 30; without it, unclear on 28 and clear no on 2, never clear yes. `needs_change` withheld 10 candidates before that change and 13 after.
  - **[#119](https://github.com/monadial/whetstone/issues/119), PR [#121](https://github.com/monadial/whetstone/pull/121):** even hand-made options with no cue at all scored around 0.6 on `no_giveaway` at most, while sound candidates scored 0.8 or more on the other checks.
  - **[#124](https://github.com/monadial/whetstone/issues/124), PR [#146](https://github.com/monadial/whetstone/pull/146):** lowering `no_giveaway`'s upper cut-off to 0.50 showed 2 of 30 candidates; 24 were still unclear.
- **Measured under policy 1:** see [#159](https://github.com/monadial/whetstone/issues/159)'s pull request.

## Policy 0 (20 October 2026)

- **Status:** unvalidated.
- **Cut-offs:** every job at 0.80 / 0.20: the relevance gate, the question check (checks 1 to 6, `needs_change` and `no_giveaway` included), option ratings, rationale and hint checks, reasoning criteria and explanation checks.
- **Decision (spec [#81](https://github.com/monadial/whetstone/issues/81)):** start from the example cut-offs in Jev's documentation, until labelled examples can set them.
- **Evidence:** none yet. The labelled-example tool ([#98](https://github.com/monadial/whetstone/issues/98)) was built to measure them.
