# Implemented Velnor work

This directory records implementation that has landed and passed its required
acceptance checks. It is not a proposal or a list of work in progress.

- [macOS Scale Set ledger](macos-scaleset/checklist.md) — G0–G8 for the Scale Set controller. Status lives in that directory, not in the V1 gate records below.

## Implementation records (Velnor Actions V1, Gates 0–8)

Recorded on unmerged branch `docs/velnor-actions-spec` (suite counts and
CI links below are historical evidence at `bdfffb9` unless a record says
otherwise; behavior descriptions and test citations track current code).
Per the recording rule below, these describe the change being merged and
become implemented records only when that change merges with its required
checks passing; those checks were green at `bdfffb9` (dogfood CI run
`36569723507`, 47/47 —
`https://github.com/tailrocks/velnor-new/actions/runs/36569723507`).

- [Gate 0: repository contract](gate-0-repository-contract.md)
- [Gate 1: repository root, config, inventory, and plan](gate-1-init-inventory-plan.md)
- [Gate 2: deterministic generator](gate-2-deterministic-generator.md)
- [Gate 3: generated execution and visible jobs](gate-3-execution-visible-jobs.md)
- [Gate 4: tool and compilation reuse](gate-4-tool-compilation-reuse.md)
- [Gate 5: trusted baseline coverage](gate-5-trusted-baseline.md)
- [Gate 6: task-result reuse](gate-6-task-result-reuse.md)
- [Gate 7: parallel test fan-out](gate-7-parallel-fanout.md)
- [Gate 8: Velnor dogfooding](gate-8-dogfooding.md)

Renderer run-scalar sharing has a candidate record for unmerged PR #112. Focused
renderer and security checks have passed, while final repository qualification
remains in progress. This candidate is not recorded as an implemented
capability; that requires the PR to merge with required checks passing.

- [Renderer run-scalar sharing](workflow-run-scalar-sharing.md) — bounded
  over-cap workflow compaction and per-use shell validation.

Shared acceptance state: local workspace suite green at `bdfffb9` (1048
pass/0 fail: 978 integration + 70 src-unit, 21 binaries; clippy clean,
fmt clean, deny ok; identical in a clean checkout). Latest measured suite:
1987/1987 Nextest green (2026-10-01, exact commit `4c0edc8`,
0 skipped, `cargo nextest run --workspace --locked --profile ci`).
Seed approvals and
release publication are NEEDS-HUMAN and unproved
([release-gates.md](release-gates.md) BOOT rows). Measurements live in
[performance.md](performance.md) (7 local cases + green-run timings);
clause-by-clause proof lives in
[requirements-evidence.md](requirements-evidence.md).

Current runs (2026-10-01, branch `docs/velnor-actions-spec`): last green
is run `36874320163` at `3a98511`
(`https://github.com/tailrocks/velnor-new/actions/runs/36874320163`,
success: 14 jobs green + `Publish baseline` skipped push-only).
Previous greens: run `36870627159` at `0c9a6a7`, run `36865471829`
at `93dd3d4`, run `36864280056`
at `3054c3e`, run `36862207497` at `34550e8` (each 14 green +
`Publish baseline` skipped), run `36836254328` at `644fdf5`
(`https://github.com/tailrocks/velnor-new/actions/runs/36836254328`,
success 14/14 on the 14-job tree). Run `36860814112` at `395d4bf`
(`https://github.com/tailrocks/velnor-new/actions/runs/36860814112`)
failed exactly two jobs — `Rust / velnor-actions-contract` (rustdoc
private intra-doc link) and dependent `Required`; all other 12 jobs
green, `Publish baseline` skipped (push-only). The fix is `34550e8`
(public `WorkflowIr::validate` link). Per-record `bdfffb9` citations
below are historical evidence and stay tied to that SHA.

## Living process registries

These are maintained process documents, not capability records: they stay
current on every change to the process they describe.

- [File classification registry](classification.md) (RQ-4.2 / RQ-5.2)
- [Recorded deviations](deviations.md) (SHOULD rows)
- [Update and exception procedure](update-procedure.md) (VER §3–§4)
- [Risk-triggered verification](verification-triggers.md) (RQ-9.8)
- [Release gates: NEEDS-HUMAN unblock conditions](release-gates.md)
- [Required-check migration procedure](required-check-migration.md)
  (P05-9 `Velnor / Required` to `Required`)

## Evidence companions

- [Requirements-to-evidence map](requirements-evidence.md) (every
  MUST/MUST NOT + acceptance criterion → owning crate, files, tests, gate)
- [Generator publication qualification](generator-publication-qualification.md)
  (draft release replay, exact-source targeting, and bounded live evidence;
  branch candidate pending independent review)
- [Performance acceptance](performance.md) (measured timings + budget
  verdicts, including the explicitly unpassed small-fixture budget)
- [P08 cache measurements](cache-measurements.md) (R11/R12 sequential-run
  sizes, transfer, hit/miss, eviction, headroom + warm-reuse proof)

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
