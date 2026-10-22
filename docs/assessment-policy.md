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
- **Measured under policy 1 ([#159](https://github.com/monadial/whetstone/issues/159), 22 October 2026):**
  - **The same six pairs:** 5 questions shown, on 2 pairs, where policy 0 showed none. Of 30 candidates, the answer probe withheld 10, each clear yes. Its probability on the answer key was clear yes for 10, unclear for 11 and clear no for 9.
  - **The check that now withholds most** is `about_area`: 17 of 30, each unclear.
  - **Three hand-made candidates, three runs each:** one whose wording gives the answer away scored 0.96 to 0.97 on its key; one answerable from general knowledge scored 1.0; one that needs the change scored 0.22 to 0.25.
- **Decision ([#162](https://github.com/monadial/whetstone/issues/162)):** the question check's `about_area` withholds a candidate only when Jev clearly says it is off its area, which is clear no. Clear yes and unclear both pass, as a criterion Jev is unclear about stays in the rubric. No cut-off changes. Policy 1 isn't released yet, so this is part of it, not a new version.
  - **Reason:** whether a question fits its area is a matter of focus, not fairness. A question slightly outside its area can still be answerable from the evidence, need the change and have exactly one right option, and each of those has a check of its own.
  - **Evidence:** the #159 measurement above. `about_area` withheld 17 of the 30 candidates, every one unclear and none clear no.
  - **Measured (22 October 2026), the same six pairs:** 12 questions shown, on all 6 pairs, where #159's run showed 5 on 2. Of 30 candidates, 13 passed and 17 were withheld, and `about_area` withheld none. Each run writes new candidates, so the counts per check move between runs too.
  - **The check that now withholds most** is the option rating, every time unclear: `option_a` and `option_b` withheld 9 candidates each, and `option_c` 7. The answer probe withheld 6, each clear yes.
- **Still open:**
  - #99's labels should check the 0.60 upper cut-off: eight of the ten candidates the probe withheld scored from 0.61 to 0.78 on their key, close to it.
  - The option ratings' unclear answers now withhold most candidates.

## Policy 0 (20 October 2026)

- **Status:** unvalidated.
- **Cut-offs:** every job at 0.80 / 0.20: the relevance gate, the question check (checks 1 to 6, `needs_change` and `no_giveaway` included), option ratings, rationale and hint checks, reasoning criteria and explanation checks.
- **Decision (spec [#81](https://github.com/monadial/whetstone/issues/81)):** start from the example cut-offs in Jev's documentation, until labelled examples can set them.
- **Evidence:** none yet. The labelled-example tool ([#98](https://github.com/monadial/whetstone/issues/98)) was built to measure them.
