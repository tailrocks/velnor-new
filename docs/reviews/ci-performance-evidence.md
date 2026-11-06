# Required developer evidence

Authors must publish this proof in the PR description, review discussion or a
linked issue record. Keep change-specific evidence out of repository documentation.
Reviewers must verify it against the final PR head; assertions and green cache
steps are insufficient. Apply the [four review gates](ci-performance.md).

## Prove progress toward the target

- Map each changed behavior to its target rule, baseline deficiency, fix in this PR and final-head proof; identify affected dependencies, consumers and generated paths beyond the edited lines.
- Check [known audit findings](https://github.com/tailrocks/velnor-new/issues/436) and related issues; remediate relevant inherited violations in this PR, including new ones found during review. An issue link alone is not remediation.
- Explain any remaining inherited gap's causal independence from this scope; do not describe an unmet target as passing or hide a regression behind aggregate gains.
- For policy-only changes, demonstrate stronger enforceable requirements, complete instruction links and scope-appropriate validation; retain runtime gaps in the audit without claiming to implement them.

## Identify and compare

- Record baseline/head SHAs, representative consumer fixtures, generated workflow/configuration, event/ref, runner image/resources and exact tool versions.
- Link reproducible commands, CI runs/attempts and raw logs; compare equivalent workloads and cache scopes, separating queue time from execution time.
- Report time to `Required`, end-to-end completion, summed runner time and relevant phase/transfer/storage costs; include producers outside `Required`, repeat noisy measurements and report their spread and tail behavior.
- State the expected benefit, measured result, regressions and tradeoffs against approved budgets; separate observations from hypotheses and unknowns from zero. A faster step alone does not establish faster CI.

## Prove job selection on every PR

- Map changed paths to expected jobs through direct inputs, transitive dependencies or the minimal planning/final-status role; include support jobs only for selected consumers.
- Link the plan and complete observed workflow/job inventory at the reviewed head, including branch-push and follow-on runs, not only checks labelled `pull_request`.
- Reconcile expected, executed and skipped jobs; prove unrelated jobs never executed and required affected work was not skipped. Record every discrepancy and its cause; new, worsened or touched violations block approval, and unrelated inherited gaps remain explicit target failures.
- Explain every broadening reason and prove both affected and unaffected cases; file extensions or missing input declarations do not establish independence from a changed file.

Documentation, instructions and templates can be transitive inputs to generation
or golden checks. Prove their consumers are unaffected before skipping them.

## Prove the affected scenarios

| Scenario | Required proof |
|---|---|
| Documentation/instruction/template only | Only checks with proven changed inputs and minimal planning/status work run; no unrelated Rust, dependency, maintenance, image or qualification jobs start. |
| Cold | Empty-cache execution completes all required checks without relying on a previous runner. |
| Warm, unchanged | After an authorized producer saves, the next run on a fresh runner restores/imports valid state and avoids unchanged dependency downloads and compilation. |
| PR source-only change | Only affected work and its dependents run/rebuild; unchanged dependency/tool layers stay reusable. |
| Invalidation | Relevant lockfile, toolchain, platform, feature/flag, input or producer-recipe changes invalidate exactly the incompatible state. |
| Failure and trust | Relevant partial hits, corruption, eviction, service failure, quota refusal and failed/cancelled producers preserve cold correctness, writer boundaries and required-check failure propagation. |
| Scheduling and coverage | Relevant metadata edits, base retargets, superseding commits, burst arrivals, partial reruns and empty/missing shards preserve progress, cancellation semantics and complete current-source validation. |

For performance-affecting changes, provide comparable baseline/head cold-to-warm
runs. Seed through an authorized writer; rerunning a read-only PR does not seed a
cache. Distinguish cache transfer from upstream downloads and restored outputs
from compilation using matched keys, restore/import outcomes and tool logs or
counters. Account for every remaining download/rebuild; a hit flag is not proof.

Use existing relevant coverage for unaffected scenarios and identify it explicitly.
New tests still require the reviewed plan and independent red/green proof mandated
by root `AGENTS.md`. Documentation-only changes may justify omitting timing
benchmarks, never job-selection evidence; executable changes cannot claim neutrality
without proof.

## Prove the resource budget

- Record stored and extracted bytes, restore/verify/import/save time, peak CPU/RAM/disk, key cardinality, writer ownership and retention/pruning for each changed family.
- Calculate peak aggregate usage including existing consumer caches, platform/root variants, old/new generations and concurrent saves; show admission headroom in bytes, not just nominal quota.
- Check the [adopted cache budgets](../proposed/cache-decisions.md), [compiled-state admission](../proposed/compiled-state-persistence.md#budget) and [current platform limits](../proposed/github-actions-reference.md#performance-review-sources); reject cache thrashing, duplicate payloads and capacity increases used to hide waste.

## Verify before approving

Independently inspect generated paths, run identities, logs and calculations;
reproduce the critical reuse/invalidation claim when evidence is ambiguous. Link
the [relevant verification](../implemented/ci-maintenance/repo-verification.md)
at the final head. Missing, incomparable or failed proof means request changes,
not a promise to measure after merge. Report the concrete improvement, each gate's
verdict, resolved inherited violations and separately justified out-of-scope debt.
