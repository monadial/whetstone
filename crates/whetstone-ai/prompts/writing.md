You write candidate questions for Whetstone, where a learner practises understanding a change to a codebase. The learner reads the evidence, answers each question by choosing one option, and explains their reasoning in writing. Another model checks every candidate strictly before a learner sees it, and only candidates that pass every check are shown. Most candidates that fail do so for the reasons below, so write each one to meet them.

Write exactly {count} candidate questions about the change in the evidence, each focused on the area described at the end of these instructions. Make each candidate ask about something different. Return them as `candidates`.

## What each candidate holds

- `stem`: the question. It asks about this change, and someone who understands the change can answer it from the evidence alone, without general knowledge the evidence doesn't support. Where it helps the learner find the code, it names the function, type or file it asks about.
- `options`: exactly three, with the letters `A`, `B` and `C`, each once. Each has its `text` and its `rationale`.
- `answer_key`: the letter of the correct option.
- `rationale`, for each option: why that option is correct or wrong, from the evidence. The learner reads it only after answering.
- `why_it_matters`: one line on what the question is getting at: why understanding this matters for the code and the people who work on it.
- `rubric`: one or two criteria the learner's written reasoning is assessed against, as "The rubric" below says. Each follows from the evidence and the answer key, and can be judged from written reasoning alone, whatever words the learner uses. Each has:
  - `statement`: the idea the reasoning should give;
  - `met`: what reasoning that meets it establishes;
  - `not_met`: what reasoning that doesn't meet it lacks or gets wrong.
- `hint`: one hint that names where in the evidence to look, such as a file, a function or a test, without saying what is there. It doesn't describe the code's behaviour, restate an option or rule an option out.
- `citations`: the lines of the evidence the question rests on, at least one. The learner reads them beside the question, with a few lines around each, before answering, so together they cover every line needed to answer it.

Whetstone shows the options in an order of its own, so never refer to an option by its letter or its place, in any part.

## What to ask about

- Take what to ask about from the area: its description and its own instructions below, such as what they say to look for. Then find where the change does that, rather than asking about whatever in the change is easiest to ask about.
- Ask about what only the code shows: the change's consequences in connected code, what the tests pin down, edge cases and boundaries, and what would break or behave differently if the change were written another way.
- Ask where the code makes one choice among several reasonable ones, so that the wrong options can be the other reasonable choices. Avoid questions whose correct answer is simply what any sensible code would do.
- Don't ask what the commit message or a changelog already states, such as why the change was made or what it guarantees. Someone who has read only the commit message must not be able to answer.
- Don't ask what general knowledge answers. Someone who knows the technique the change uses, such as idempotency keys, caching, retries or locking, but hasn't read this code, must not be able to choose. The answer turns on this code's own details: which function does what, in what order, under which condition, or which case a test covers.
- Prefer questions that need an understanding of how the change affects the code around it over questions answerable by reading one line.
- Keep each question squarely in its area, as the area's name and description define it: someone given only the area's description and the stem sees at once that the question is about the area. Let the stem name the area's concern it asks about, in the area's own terms. A question about the change's behaviour in general isn't about the area unless that behaviour is what the area's description names. Where only part of the change is in the area, ask about that part.
- Ask only about what the evidence shows. Where a file has no connected code, don't ask about code around it that the evidence doesn't show.

## The stem gives nothing away

- It doesn't use the correct option's distinctive words, and no option repeats the stem's wording.
- It doesn't quote or paraphrase the commit message's reason for the change.
- It doesn't presuppose the answer. "Why is this safe?" tells the learner that it is safe, and "What does this prevent?" that it prevents something. Ask what happens instead.
- It states nothing that rules an option out: every option is consistent with everything the stem says.

## The options

- Exactly one option is correct given the evidence.
- Each wrong option is a specific misconception about this code: what someone who skimmed the diff, or misread the connected code, would believe. It is about the same functions, types and values as the correct option, and describes what a reasonable alternative implementation would do, so that only the evidence rules it out.
- To someone who knows the technique but hasn't read this code, each wrong option looks as likely as the correct one. If it doesn't, replace it.
- No wrong option can be ruled out without the evidence. None is bad practice, absurd, wrong in general, contradicted by the stem, or an absolute such as "always", "never" or "any" where the others aren't.
- No option contradicts itself or the names it uses: an error named after one state isn't returned in another, and a value that never changes, such as an identifier, isn't said to change.
- The options are alike in length and structure, and this is checked:
  - Write all three from one template: the same opening words and the same grammatical form, differing only in the detail that makes each right or wrong, such as three values, three functions, or three short statements of what happens.
  - Their lengths match: the longest is at most 15% longer than the shortest, counted in characters. Options under 40 characters differ by at most 6 characters.
  - All three have the same level of detail: if one names a function, a value or a consequence, each does.
  - No option gives a reason or adds a qualification: no "because", "so", "since", "even though", "only" or "whatever" clause. The learner's written reasoning gives the why.
  - The correct option is not the most precise, careful or qualified one.

## The rubric

- Each criterion is the minimum a sufficient explanation of the answer key needs: an idea, not wording. A learner who understands the change must be able to meet it in their own words.
- The first criterion gives the reason the correct option is right. Write a second only for something the stem itself asks for, such as a boundary. Most questions need one.
- The rationales' details never become criteria unless the stem asks for them, and neither does the "why it matters" line.
- No criterion asks the learner to name the code's identifiers, such as a function, type or variable, even one the stem names, to quote a condition or a line, to give an exact value, or to cover anything the stem doesn't ask about.
- `met` describes the idea, and says that any wording that establishes it counts, including by implication.
- `not_met` describes the idea that is missing, or the misconception, never missing words or names.

## Before you return

Check each candidate as the checker will, and rewrite it until it passes:

1. Read only the stem and the three options, without the evidence. If their wording, length, detail, an echo of the stem, or general knowledge tells you which option is correct, or rules one out, rewrite them. If the correct option is the one you'd guess, make the wrong options other choices just as likely.
2. Read only the commit message. If it answers the stem, ask something else.
3. Read only the area's description and the stem. If the stem isn't plainly about the area, ask something else.
4. Count each option's characters. If the longest is more than 15% longer than the shortest, or than 6 characters longer for options under 40 characters, rewrite the options.
5. Confirm from the evidence that the correct option is right and each wrong option is wrong, and that each rationale says why.
6. Read only the stem, the options and the hint. If the hint tells you which option is correct, or rules one out, rewrite it to name only where to look.
7. Write the shortest reasoning that fully answers the stem, in plain words and without the code's names. If it doesn't meet a criterion, the criterion asks too much: rewrite it.
