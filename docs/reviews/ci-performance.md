# CI performance review

Fast, reliable consumer CI/CD is Velnor Actions' primary requirement. Apply these
four gates to every PR, tracing generator changes through the workflows consumers
actually run. Reject avoidable slowdowns, duplicated work and unsupported claims.

Preserve the [hard decisions](../hard-decisions.md),
[cache decisions](../proposed/cache-decisions.md) and
[cache contract](../proposed/cache-contract.md); review guidance never overrides them.

## 1. Run only PR-related jobs

- Audit every workflow caused by a PR or its branch updates, including validators, declared checks, qualification, images and follow-on runs.
- Require changed direct inputs or transitive dependencies for every workload job; a `required`/`global` label, broad directory match or warm cache never proves relevance.
- Justify minimal planning/final-status work separately; reject setup or producer jobs whose consumers are all skipped.
- Skip unrelated jobs at job level before runner allocation, checkout, setup or cache restore; skipping steps after starting a job is insufficient.
- Preserve the adopted job topology and single planner: no second skip mechanism or GitHub `paths:` filters; default-branch pushes, merge groups and dispatches stay full.
- Keep genuinely unknown inputs fail-closed; reject routine non-input documentation/instruction/template changes left unclassified instead of fixing the input model.
- Minimize time to `Required` and total runner time; reject duplicate triggers/setup, unnecessary matrix legs, needless serialization and superseded work within the adopted concurrency rules.

## 2. Reuse completed work

- When compatible, validated state is available, require subsequent runs to reuse unchanged dependency downloads and compiled outputs; reject repeated fetching, installation or compilation that reuse can eliminate.
- Verify downloaded sources, installed tools/dependencies and compiled state separately; a cache hit, especially a partial match, never proves complete outputs or passing checks.
- Bind each reusable layer to its relevant platform, toolchain, locks, flags, features, inputs and producer recipe; unrelated edits must not invalidate it, and fallback state must be reconciled before use.
- Save complete state after its authorized producer succeeds, even if unrelated checks fail; never save failed/cancelled production, add redundant writers or resave an existing exact entry.

## 3. Keep caching cheaper and bounded

- Reject duplicate payload ownership and unbounded key growth; measure restore, validation, extraction, compression and save costs against the work avoided.
- Budget peak stored bytes across consumer caches, families, roots/platforms, concurrent writes and retained generations; enforce adopted family ceilings, operating budgets and headroom, with safe bounded pruning.
- Verify current [GitHub limits](../proposed/github-actions-reference.md#performance-review-sources-checked-2026-11-06), runner disk/RAM and cache request rates; preserve Velnor's 10 GB quota and stricter admission limits without assuming paid expansion.

## 4. Preserve trustworthy verification

- Missing, evicted, unavailable or rejected cache state must take a correct, bounded cold path; caches never become the sole transport for required outputs between jobs.
- Preserve read-only PR/fork/merge-group caches, authorized writers and provenance checks; never expose secrets or private cached content to unauthorized consumers.
- Require every planned check and its current-source evidence; reject stale outputs, hidden failures, weakened checks and a `Required` verdict that accepts missing, cancelled or unexpectedly skipped work.

## Review outcome

Require the author's [evidence](ci-performance-evidence.md), inspect it independently
and report each gate as pass, fail or inapplicable with a reason. Cite the changed
path, affected consumer scenario and evidence for each blocker. Missing or stale
proof blocks approval; ask for the specific missing measurement or reproduction.
Optimization claims need measured gains; necessary added work needs the cheapest
correct design and quantified drawbacks. Escalate unavoidable regressions to the
owner for explicit acceptance; never waive them as the reviewer.
