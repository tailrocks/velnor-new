# Implemented Velnor work

This directory records implementation that has landed and passed its required
acceptance checks. It is not a proposal or a list of work in progress.

No verified Velnor implementation is recorded yet.

## Recording rule

Every pull request that lands an implementation MUST update this index and add
or update its record in the same pull request. A pull request that changes
implementation without the matching record is incomplete and MUST NOT merge.
Do not record drafts, open branches, prototypes, skipped checks, or failed
gates as implemented.

Each record MUST state:

- implemented capability and the specification section it satisfies;
- merged pull request link and merge date;
- exact files or components delivered;
- required acceptance checks and their passing evidence;
- any approved deviation, with its decision record;
- follow-up work, if the landed scope is intentionally partial.

Use this record shape:

```markdown
# <Capability>

- State: implemented
- Specification: <relative link and section>
- Landed by: <merged pull request link>
- Merge date: YYYY-MM-DD
- Delivered: <files/components and behavior>
- Acceptance evidence: <required check names and run links>
- Deviations: none | <approved decision link and effect>
- Follow-up: none | <remaining scoped work>
```

Create one concise Markdown record per landed capability or implementation
gate. Name it `<gate-or-capability>.md`, link it from this index, and keep it in
this directory. Update the record when later work changes its delivered scope
or acceptance evidence. The proposal and deferred specifications remain the
source of required behavior; this directory records only what code has proved.

## Implementation record requirement

Every merged implementation change MUST update this index and its capability
record in the same pull request. Reviewers verify record fields, PR links, and
index links as part of review. Alint checks repository shape and content; it
does not replace this change-aware review requirement.

A record added in an open implementation PR describes the change being merged;
it becomes an implemented record only when that PR merges with its required
checks passing. The PR reference and date MUST identify that merged change.
