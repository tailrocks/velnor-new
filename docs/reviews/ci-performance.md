# CI performance review

This procedure reviews a change to anything that [the goals](../goals.md)
assign to the existing engine: `crates/`, `.velnor/`, the workflows it
generates, and the scripts, images and fixtures that support it. A change to
the new Rust generator is reviewed with the goals.

Velnor Actions must generate exceptionally fast, efficient, reliable, secure,
production-grade CI/CD. These are acceptance targets, not claims about today's
implementation. Every author and reviewer, including Codex, must apply this
improvement rule and the four gates to every such PR.

Preserve the [hard decisions](../hard-decisions.md),
[cache decisions](../proposed/cache-decisions.md) and
[cache contract](../proposed/cache-contract.md); review guidance never overrides them.

## Improve what the PR touches

- Compare the final head with both the baseline and the target; require a concrete improvement in changed behavior or its enforcement, not merely unchanged performance.
- Trace semantic scope through shared contracts, dependencies, affected consumers and generated workflows; moving work to a helper, producer or another workflow does not remove it from review.
- Fix every established violation in that scope in this PR, including inherited ones discovered during review. Existing design, green CI, small diffs, backlog issues and promises do not excuse them.
- Investigate suspected waste with measurements or a reproduction; distinguish a verified defect from an optimization hypothesis.
- Record inherited debt outside that scope with a causal explanation and tracking issue; never mark it compliant. Instruction-only changes improve policy and enforcement while recording unchanged runtime gaps.
- Reject avoidable regressions in any affected scenario, platform or trust boundary; gains elsewhere cannot offset them. Necessary added validation needs the cheapest correct design, quantified cost and explicit owner acceptance of any budget regression.

## 1. Run only PR-related jobs

- Audit every workflow caused by a PR or its branch updates, including validators, declared checks, qualification, images and follow-on runs.
- Require changed direct inputs or transitive dependencies for every workload job; a `required`/`global` label, broad directory match or warm cache never proves relevance.
- Justify minimal planning/final-status work separately; reject setup or producer jobs whose consumers are all skipped.
- Skip unrelated jobs at job level before runner allocation, checkout, setup or cache restore; skipping steps after starting a job is insufficient.
- Preserve the adopted job topology and single planner: no second skip mechanism or GitHub `paths:` filters; default-branch pushes, merge groups and dispatches stay full.
- Keep genuinely unknown inputs fail-closed; reject routine non-input documentation/instruction/template changes left unclassified instead of fixing the input model.
- Minimize time to `Required` and total runner time; reject duplicate triggers/setup, unnecessary matrix legs and capacity chains mistaken for data dependencies.
- Require successful planning before payload work; reject non-input metadata before validation admission, revalidate base retargets, cancel superseded read-only validation, and preserve bounded finalization and trusted warming/publication progress.

## 2. Reuse completed work

- When compatible, validated state is available, require subsequent runs to reuse unchanged dependency downloads and compiled outputs; reject repeated fetching, installation or compilation that reuse can eliminate.
- Verify downloaded sources, installed tools/dependencies and compiled state separately; a cache hit, especially a partial match, never proves complete outputs or passing checks.
- Bind each reusable layer to its relevant platform, toolchain, locks, flags, features, inputs and producer recipe; unrelated edits must not invalidate it, and fallback state must be reconciled before use.
- Save complete state after its authorized producer succeeds, even if unrelated checks fail; never save failed/cancelled production, add redundant writers or resave an existing exact entry.
- Prove the real producer-to-fresh-consumer path, including installation verification material; test fixtures must not supply state the producer omits.

## 3. Keep caching cheaper and bounded

- Reject duplicate payload ownership and unbounded key growth; measure restore, validation, extraction, compression and save costs against the work avoided.
- Admit every cache writer against peak aggregate bytes across consumer caches, families, roots/platforms, concurrent writes and retained generations; enforce adopted ceilings and headroom, and protect active identities during bounded pruning.
- Verify current [GitHub limits](../proposed/github-actions-reference.md#performance-review-sources), runner disk/RAM and cache request rates; preserve Velnor's 10 GB quota and stricter admission limits without assuming paid expansion.
- Bound CPU/RAM/process concurrency and aggregate phase deadlines; reject optional discovery that consumes validation's deadline, save-only work in read-only jobs, redundant compression and unused materialization.

## 4. Preserve trustworthy verification

- Missing, evicted, unavailable or rejected cache state must take a correct, bounded cold path; caches never become the sole transport for required outputs between jobs.
- Preserve read-only PR/fork/merge-group caches, authorized writers and provenance; verify restored content, expected paths, types, modes, links and completeness before execution, without exposing secrets or private state.
- Require every planned check and its current-source evidence; reject stale outputs, hidden failures, weakened checks and a `Required` verdict that accepts missing, cancelled or unexpectedly skipped work.
- Prove complete, nonempty required-suite coverage, including aggregate coverage across shards; green empty selections, echo-only qualification and unmeasured phases are not evidence.
- Enforce equivalent invariants across every adapter and renderer; require regression coverage and protect acceptance policy from being weakened by the candidate it evaluates.

## Review outcome

Require the author's [evidence](ci-performance-evidence.md), inspect it independently
and report each gate as pass, fail or inapplicable for the PR's semantic scope.
Separately report inherited target gaps outside that scope; inapplicable requires
proof, never missing data. For each blocker, cite the rule, affected behavior,
evidence, required fix and acceptance check. Request changes for a touched violation,
an avoidable regression, an unapproved budget regression or missing proof; approve
only after the final head proves the required improvement. A deferred issue cannot
turn a failed gate into a pass.

