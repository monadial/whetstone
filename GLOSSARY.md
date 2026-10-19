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
Everything a Practice sends and can cite: the change, what it connects to elsewhere in the repository at that commit, and related tests and configuration.
_Avoid_: context, payload, manifest.

**Area**:
A lens on a change that questions focus on, chosen by the learner from the areas the repository defines, or from Whetstone's defaults when it defines none: domain, observability, scalability, infrastructure and system design.
_Avoid_: category, topic, tag.

**Area prompt**:
An area's instructions for what to look for in a change; it adds focus to Whetstone's own question instructions and never replaces them.
_Avoid_: master prompt.

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

## AI roles

**Jev**:
The evaluator that returns typed decisions and probabilities about changes, questions and responses; it never writes prose.
_Avoid_: grader, judge (for the generator).

**Generator**:
The model that writes questions, explanations and hints from supplied evidence; it never acts as the sole judge.
