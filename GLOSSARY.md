# Whetstone

Whetstone helps learners examine their understanding of supplied work through questions and written reasoning.

## Practice

**Practice**:
A source-grounded learning session in which a learner answers questions and explains their reasoning.

**Showcase project**:
A small sample codebase that ships with Whetstone so anyone can try Practice without supplying their own source.
_Avoid_: demo repo, fixture, sample app.

**Change**:
The commit a Practice is built from: its message, its diff and the files it touches.
_Avoid_: patch, diff (for the whole change).

**Evidence**:
Everything a Practice sends and can cite: the change, the chosen areas with their area prompts, its connected code, and related tests and configuration.
_Avoid_: context, payload, manifest.

**Connected code**:
What the touched files call, are called by, link to or name elsewhere in the repository at the change's commit.
_Avoid_: dependency code, which reads as third-party library source; connections; context.

**Area**:
A lens on a change that questions focus on, chosen by the learner from the areas the repository defines, or from Whetstone's defaults when it defines none: domain, infrastructure, observability, scalability and system design.
_Avoid_: category, topic, tag.

**Area prompt**:
An area's instructions for what to look for in a change; it adds focus to Whetstone's own question instructions and never replaces them.
_Avoid_: master prompt.

**Session record**:
The local record of one Practice: its questions, the evidence that was sent, the learner's responses and every outcome, with the models, versions and assessment policy that produced them.

**Session export**:
A session record written out as one file the learner can keep or share.
_Avoid_: report, which a learner's **Report** already uses; evidence report.

**Outbound review**:
The learner's check of exactly what a Practice on their own repository will send and where, before anything leaves the machine; approving it covers that evidence, those destinations and the learner's later responses in the session.
_Avoid_: consent screen; review alone, which **Needs review** uses.

## Responding

**Response**:
The learner's selected choice and written reasoning for a question.
_Avoid_: treating an answer choice alone as the whole response.

**Attempt**:
A learner's work on a particular question.

**Hint**:
Help that points to relevant evidence without revealing the answer; the attempt still counts as the learner's own.

**Assisted attempt**:
An attempt made after the learner revealed the answer or rubric, so its assessment is not independent.

**Skip**:
A learner's choice to leave a question unattempted for now.

**Cannot answer from this context**:
A learner signal that the supplied material is insufficient to answer the question.
_Avoid_: assuming the learner lacks the relevant skill.

**Report**:
A learner's claim that a question, explanation or assessment may be a hallucination, mistaken or ambiguous.
_Avoid_: confirmed defect before review.

## Outcomes

**Assessment**:
A stated outcome for a selected choice or a reasoning criterion, with its evidence and limitations.
_Avoid_: mastery score, competence certificate.

**Needs review**:
An outcome whose support or reliability has not been established well enough to call it supported or not.
_Avoid_: treating it as incorrect.

**Assessment policy**:
The versioned cut-off values that turn Jev's probabilities into clear yes, unclear or clear no for each kind of check. The learner's own config can override any check's cut-offs, which makes outcomes not comparable with the policy's; every outcome records the cut-offs that produced it.
_Avoid_: threshold alone; confidence.

## Questions

**Question type**:
The form a question takes, such as a single choice from A, B and C with written reasoning; it sets what the generator writes, what the question check asks and how a response becomes outcomes.

**Answer key**:
The option the generator marks as a question's correct answer.

**Rubric**:
The criteria a question's written reasoning is assessed against, written with the question.

**Criterion**:
One point of a rubric, with what meeting it and not meeting it look like.

**Relevance gate**:
Jev's decision, for each chosen area, whether a change is worth asking about.

**Question check**:
Jev's check of a generated question before it is shown, including its option ratings; only questions that pass are shown.
_Avoid_: question review, which clashes with **Needs review** and **Outbound review**.

**Giveaway check**:
The part of a question check that asks whether a question's stem and options give its answer away; the assessment policy gives it cut-offs of its own.

**Option rating**:
Supported, Not supported or Needs review for one option of a question, decided in its question check before the learner sees it.

## AI roles

**Jev**:
The evaluator that returns typed decisions and probabilities about changes, questions and responses; it never writes prose.
_Avoid_: grader, judge (for the generator).

**Generator**:
The model that writes questions, explanations and hints from supplied evidence; it never acts as the sole judge.
