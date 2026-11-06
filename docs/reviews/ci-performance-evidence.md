# Required developer evidence

Authors must include this proof in the PR description or a linked durable record.
Reviewers must verify it against the final PR head; assertions and green cache
steps are insufficient. Apply the [four review gates](ci-performance.md).

## Identify and compare

- Record baseline/head SHAs, representative consumer fixtures, generated workflow/configuration, event/ref, runner image/resources and exact tool versions.
- Link reproducible commands, CI runs/attempts and raw logs; compare equivalent workloads and cache scopes, separating queue time from execution time.
- Report time to `Required`, end-to-end completion, summed runner time and relevant transfer/storage costs; include producers outside `Required`, repeat noisy measurements and report their spread.
- State the expected benefit, measured result, regressions and tradeoffs; a faster step alone does not establish faster CI.

## Prove the affected scenarios

| Scenario | Required proof |
|---|---|
| Cold | Empty-cache execution completes all required checks without relying on a previous runner. |
| Warm, unchanged | After an authorized producer saves, the next run on a fresh runner restores/imports valid state and avoids unchanged dependency downloads and compilation. |
| PR source-only change | Only affected work and its dependents run/rebuild; unchanged dependency/tool layers stay reusable. |
| Invalidation | Relevant lockfile, toolchain, platform, feature/flag, input or producer-recipe changes invalidate exactly the incompatible state. |
| Failure and trust | Relevant partial hits, corruption, eviction, service failure, quota refusal and failed/cancelled producers preserve cold correctness, writer boundaries and required-check failure propagation. |

For performance-affecting changes, provide comparable baseline/head cold-to-warm
runs. Seed through an authorized writer; rerunning a read-only PR does not seed a
cache. Distinguish cache transfer from upstream downloads and restored outputs
from compilation using matched keys, restore/import outcomes and tool logs or
counters. Account for every remaining download/rebuild; a hit flag is not proof.

Use existing relevant coverage for unaffected scenarios and identify it explicitly.
New tests still require the reviewed plan and independent red/green proof mandated
by root `AGENTS.md`. Documentation-only changes may explain why runtime evidence
is inapplicable; executable changes cannot claim neutrality without proof.

## Prove the resource budget

- Record stored and extracted bytes, restore/save time, key cardinality, writer ownership and retention/pruning for each changed family.
- Calculate peak aggregate usage including existing consumer caches, platform/root variants, old/new generations and concurrent saves; show admission headroom in bytes, not just nominal quota.
- Check the [adopted cache budgets](../proposed/cache-decisions.md), [compiled-state admission](../proposed/compiled-state-persistence.md#budget) and [current platform limits](../proposed/github-actions-reference.md#performance-review-sources-checked-2026-11-06); reject cache thrashing, duplicate payloads and capacity increases used to hide waste.

## Verify before approving

Independently inspect generated paths, run identities, logs and calculations;
reproduce the critical reuse/invalidation claim when evidence is ambiguous. Link
the [relevant verification](../implemented/ci-maintenance/repo-verification.md)
at the final head. Missing, incomparable or failed proof means request changes,
not a promise to measure after merge.
