# Labelled examples

These are labelled examples about the showcase project's commits. The
`whetstone-labels` tool runs each one through Jev, three times, and reports
how well Jev's answers agree with the labels. The assessment policy's
cut-offs are then set from that report, not guessed. The format and the
tool come from [#98](https://github.com/monadial/whetstone/issues/98), and
the full labelled set from [#99](https://github.com/monadial/whetstone/issues/99).

```sh
cargo run -p whetstone-labels -- check
OPENROUTER_API_KEY=… cargo run -p whetstone-labels -- measure > report.md
```

- **`check`** reads every example and checks it against the showcase
  project. It is offline and free, so run it while you label.
- **`measure`** sends each example's Jev calls three times, and prints the
  report as Markdown, ready to paste into #99.
  - An example is one call, but a candidate example is up to four, as
    Practice checks a question in four calls. A run sends each call three
    times, and `check` prints how many calls that is. In the first run,
    over the seeds, a call cost about 0.0003 USD, when the question check
    was still one larger call.
  - `--model` measures a Jev model other than `typesafe/jev-1.14`.
- **Both** take `--labels DIR` to read another folder, or `--labels FILE`
  to read one file, such as a learner's votes (see
  [Learner votes](#learner-votes)).

Neither command changes the assessment policy. Setting its values is a
recorded decision that bumps its version (rule 38). How closely Jev must
agree before a release is decided in #7.

## The files

Every `.toml` file here is read, in file-name order. A file can hold any
number of examples of any kind. The seeds are split by kind:

| File | Kind of example | Jobs it measures |
| --- | --- | --- |
| `gate.toml` | `[[gate]]` | Relevance gate |
| `candidates.toml` | `[[candidate]]` | Question check, answer probe, option rating, rationale and hint checks |
| `responses.toml` | `[[response]]` | Reasoning criterion |
| `explanations.toml` | `[[explanation]]` | Explanation check |

Add files freely, such as one per commit. Use only the showcase project's
commits: an example names its commit by subject, and its text quotes and
cites only that commit's files. Never paste in other code.

## Every example

```toml
[[candidate]]
id = "retry-key"          # unique across all the files
by = "agent"              # who labelled it
on = 2026-10-21           # when, as a TOML date
note = "Two right options, on purpose."   # optional: why it exists
# … the candidate's own keys

[candidate.labels]
option_a = "yes"
option_b = { label = "yes", by = "Tomas", on = 2026-10-23 }
```

- **A label** says what Jev should answer, `"yes"` or `"no"`. Each one
  records who made it and when. A label takes the example's `by` and `on`,
  unless it names its own, as `option_b` does above.
- **Agent seeds** are labels with `by = "agent"`. The seeds here are all
  agent-made, dated 2026-10-21, and are there to be reviewed. Where you
  agree with a label, set `by` and `on` to yourself. Where you don't, fix
  the label. The report counts the agent labels that are still left.
- **Every label is required.** An example must label everything its calls
  ask, and nothing else. `check` names any label that is missing or extra.

## Gate examples

A gate example covers one commit, and labels whether it is worth asking
about in each area you list. Its keys are area IDs: `domain`,
`infrastructure`, `observability`, `scalability` and `system-design`. Its
areas are asked about in one call, as Practice asks about the areas a
learner chose.

```toml
[[gate]]
id = "gate-retry-identity"
commit = "Preserve payment retry identity"   # the commit's subject
by = "Tomas"
on = 2026-10-22
labels = { domain = "yes", observability = "no" }
```

## Candidate examples

A candidate example is a question as the generator writes it, for one
commit and one area. It has these keys:

- `stem`, `answer_key` and `why_it_matters`;
- `hint`;
- three `[[candidate.options]]`, each with a `letter` (`A`, `B` or `C`), a
  `text` and a `rationale`;
- one or two `[[candidate.rubric]]` criteria, each with a `statement` and
  what `met` and `not_met` look like;
- `[[candidate.citations]]`, each with a `path`, a `first_line` and a
  `last_line`.

The candidate must pass the plain checks, as a generated one must before
Jev sees it. `check` names what fails. The keys that need labels are these:

| Key | Job | "yes" means |
| --- | --- | --- |
| `answerable` | Question check | The evidence holds everything needed to answer |
| `about_area` | Question check | It asks about its area |
| `answer_probe` | Answer probe | From the stem and options alone, without the change, Jev picks the answer key: the question gives itself away or doesn't need the change. A label of "yes" withholds the question |
| `criterion_1` and on | Question check | The criterion follows from the evidence and the answer key, and reasoning can show it |
| `criterion_1_minimum` and on | Question check | The criterion asks only for what a sufficient answer to the stem needs: an idea a learner could give in their own words, without the code's identifiers, quoted conditions or exact values |
| `option_a`, `option_b`, `option_c` | Option rating | The evidence shows this option answers the stem |
| `rationale_a`, `rationale_b`, `rationale_c` | Rationale and hint checks | The evidence supports the rationale |
| `why_it_matters` | Rationale and hint checks | The evidence supports it |
| `hint_points_to_evidence` | Rationale and hint checks | The hint points to specific evidence |
| `hint_hides_answer_key` | Rationale and hint checks | Stem, options and hint together don't reveal the answer |

Depth isn't labelled. It only orders questions, and has no cut-offs.

The tool asks the question check in four calls, as Practice does, and
each call is sent only what its questions are about:

- the option ratings: the evidence, and the stem and the options' text,
  never the answer key;
- `hint_hides_answer_key`: the stem, the options' text and the hint, with
  no evidence;
- `answer_probe`: the stem and the options' text alone, as a choice among
  A, B and C; its answer is Jev's probability on the answer key;
- every other check: the evidence and the whole candidate.

**Retired labels.** Assessment policy 1 replaced `needs_change` and
`no_giveaway` with the answer probe. A file that still labels them, such as
a learner's earlier votes, isn't refused: `check` and `measure` leave those
labels out and say how many. A vote of one of them alone sends nothing.

A criterion labelled `criterion_N_minimum = "no"` asks for more than the
question needs, so the question its labels show leaves it out, as Practice
drops a criterion when Jev's answer is clear no (one Jev is unclear about
stays): the question's response and explanation examples label only the
criteria it keeps, numbered from 1 among them. A candidate whose every
criterion is labelled so is withheld, like any other whose labels fail.

**Generated and hand-made candidates.** #99 asks for both:

- generated candidates, written by the generator and pasted in;
- hand-made broken ones: two right options, none right, a giveaway,
  off-area, an ungrounded criterion, a criterion that asks for more than
  the question needs, and a rationale that contradicts the
  evidence.

Label each one as it is. The tool measures a broken candidate like any
other.

## Learner votes

Whetstone's **Review withheld questions** saves a learner's votes, one
candidate example per vote, in `labels.toml` in Whetstone's state folder
(`$XDG_STATE_HOME/whetstone`, by default `~/.local/state/whetstone`). Each is
a candidate the generator wrote and the question check withheld, with one
label: the check voted on. "Rightly withheld" is the answer that withholds
it, and "Should have passed" the answer the show rule needed. A vote has
two keys of its own:

- `source = "learner vote"`: it labels only the check voted on, not
  everything its calls ask. Jev is asked the one call of the question check
  that holds that check, whole, as Practice asks it, and only that label is
  measured.
- `commit_id`: the full ID of a commit of the learner's own repository,
  when the vote is about one rather than about the showcase project.

```toml
[[candidate]]
id = "learner-vote-20261021T101502-1"
source = "learner vote"
by = "learner"
on = 2026-10-21
note = "Nothing but the change says which option is right."   # the learner's own note, if any
commit = "Preserve payment retry identity"
area = "domain"
# … the candidate as the generator wrote it

[candidate.labels]
answer_probe = "no"
```

`check` and `measure` read the file with `--labels`, and skip two kinds of
vote, saying how many and why:

- **A vote about the learner's own repository.** The tool has only the
  showcase project, and sending that code to Jev again would need a new
  outbound review. Such a vote also holds the learner's private code, so it
  stays on their machine.
- **A vote whose candidate cites lines the tool's evidence doesn't send,**
  such as connected code (see [What Jev sees](#what-jev-sees)).

Votes about the showcase project hold no private code. Copy the ones worth
keeping into a file here by hand, for #99. Review each label as you would
an agent's, and set `by` and `on` to yourself where you agree.

## Response examples

A response example is a learner's reasoning for a candidate example's
question. It has one label per criterion that question keeps (see
[Candidate examples](#candidate-examples)): `"yes"` when the reasoning
meets it, in any words. The question must be one the
candidate's labels would show to a learner. Jev gets the reasoning with the
question's stem and options, never the answer key, as in Practice. The
reasoning can be at most 4,000 characters long, as in Practice's editor.

```toml
[[response]]
id = "retry-key-partial"
candidate = "seed-retry-key"
by = "Tomas"
on = 2026-10-22
reasoning = """
pay no longer generates a key, so the request uses the payment's own.\
"""
labels = { criterion_1 = "yes" }
```

## Explanation examples

An explanation example holds paragraphs that explain what a response to a
candidate example's question missed. Each paragraph explains one criterion
the question keeps, counting from 1, with that criterion's outcome: `"not_met"` or
`"needs_review"`. Each paragraph takes two labels:

- `criterion_N_supported`: the evidence supports everything it says;
- `criterion_N_agrees`: it fits the outcome. For `not_met` it explains
  what was missed. For `needs_review` it describes what meeting the
  criterion looks like, without claiming the learner missed it.

Each paragraph must pass the plain checks, as a generated one must before
Jev sees it: it has text, it cites the evidence, and every citation names
lines the evidence sends. `check` names what fails.

```toml
[[explanation]]
id = "retry-key-explained"
candidate = "seed-retry-key"
by = "Tomas"
on = 2026-10-22

[[explanation.paragraphs]]
criterion = 1
outcome = "not_met"
text = "The request now copies the key the payment was created with, so …"
citations = [{ path = "src/provider.rs", first_line = 26, last_line = 37 }]

[explanation.labels]
criterion_1_supported = "yes"
criterion_1_agrees = "yes"
```

## What Jev sees

Each example's evidence is its commit's change: the message, the diff and
every touched file's full content after the change, numbered from line 1,
as the outbound review sends by default. It has **no connected code**: the
app finds the showcase's connected code internally, and doesn't make it
available to this tool yet. This is what Practice sends when finding a
change's connected code takes too long. So label against the change alone,
and cite only touched files.

To read a file as it is just after a commit, with its line numbers, replay
the history and check the commit out:

```sh
showcase/check-history.sh --keep ../payments-replay
git -C ../payments-replay log --oneline
git -C ../payments-replay show <commit>:src/provider.rs | cat -n
```

## The report

The report has one section per job. Counts are of answers: one per label
per run.

- **Cut-off rows:** each row bands every answer at one pair of cut-offs.
  The pairs run from 0.70/0.30 to 0.90/0.10, and the last row is the
  policy in use. A clear answer disagrees when it is clear yes for a label
  of no, or clear no for a label of yes. The clear-yes columns depend only
  on the upper cut-off, and the clear-no columns only on the lower. So the
  rows also give any mixed pair, such as 0.85/0.20.
- **Unclear:** answers between the cut-offs.
- **Spread:** each label's largest answer minus its smallest, across the
  runs. The report gives the mean, the largest, and how many labels'
  answers change band between runs under the policy in use.
- **Flagged labels:** the labels with an answer that is unclear or that
  disagrees under the policy in use, with their answers. Look at these
  first: either Jev is wrong, or the label is.
- **Failed calls:** each one, by example and run, and for a candidate
  example by which call of its question check failed. Nothing is retried,
  so a failed call's labels have one answer fewer.
