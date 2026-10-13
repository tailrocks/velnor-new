# Implemented Velnor work

This directory records implementation that has landed and passed its required
acceptance checks. It is not a proposal or a list of work in progress.

## Implementation records (Velnor Actions V1, Gates 0–8)

Recorded on unmerged branch `docs/velnor-actions-spec` (HEAD `f725a87`).
Per the recording rule below, these describe the change being merged and
become implemented records only when that change merges with its required
checks passing. Dogfood CI round 5 is pending; run links to be filled then.

- [Gate 0: repository contract](gate-0-repository-contract.md)
- [Gate 1: repository root, config, inventory, and plan](gate-1-init-inventory-plan.md)
- [Gate 2: deterministic generator](gate-2-deterministic-generator.md)
- [Gate 3: generated execution and visible jobs](gate-3-execution-visible-jobs.md)
- [Gate 4: tool and compilation reuse](gate-4-tool-compilation-reuse.md)
- [Gate 5: trusted baseline coverage](gate-5-trusted-baseline.md)
- [Gate 6: task-result reuse](gate-6-task-result-reuse.md)
- [Gate 7: parallel test fan-out](gate-7-parallel-fanout.md)
- [Gate 8: Velnor dogfooding](gate-8-dogfooding.md)

Shared acceptance state: local workspace suite green (933 integration
`#[test]` + 64 src-unit `#[test]` by grep; implementation agent reports
981 green, clippy 0, fmt clean at HEAD). Seed approvals and release
publication are NEEDS-HUMAN and unproved ([release-gates.md](release-gates.md)
BOOT rows). Performance is unmeasured: no benchmark harness or recorded
timings were found in the repo, so no budget claim is made here.

## Living process registries

These are maintained process documents, not capability records: they stay
current on every change to the process they describe.

- [File classification registry](classification.md) (RQ-4.2 / RQ-5.2)
- [Recorded deviations](deviations.md) (SHOULD rows)
- [Update and exception procedure](update-procedure.md) (VER §3–§4)
- [Risk-triggered verification](verification-triggers.md) (RQ-9.8)
- [Release gates: NEEDS-HUMAN unblock conditions](release-gates.md)

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
