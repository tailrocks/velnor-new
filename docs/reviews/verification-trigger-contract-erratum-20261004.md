# Verification Trigger Contract Erratum — 2026-10-04

Status: **ADOPTED CONTRACT CORRECTION** for the V1 workflow contract. This
erratum preserves and does not rewrite the earlier immutable manual-trigger
resolution. It supersedes only that record's permission to accept a branch/ref
as `base_sha` or resolve an omitted base through a default-base/baseline
resolver.

## Source and exact preimage

The edited contract is [workflow-contract.md](../proposed/workflow-contract.md),
owned by the V1 workflow-contract surface. Before this patch the file was clean
and byte-identical to Git base
`01c4c7593abf088e75aae2c6966e1f9d6980a7f9`; its exact preimage blob was
`726f0f01ce655153d27ea09eea7dffb40d2f1828` (24,416 bytes, 369 lines). The
replaced text was the schedule/manual contract in §3 that allowed an optional
`base_sha` without a closed grammar and said affected dispatch could compare its
validated base.

Earlier immutable decision: `/Users/donbeave/.codex-chainargos2/evidence/velnor-pr12/recovery-20261004/trigger-contract-resolution-20261004.md`,
SHA-256 `7c3db0038febb6dfeab0aaeb25febf39fbf71197b348879d554cd96877d9426f`.
That record remains intact. This erratum preserves its same-workflow event and
trust decisions while tightening the base identity rule below.

## Adopted rules

1. **One graph.** Schedule and `workflow_dispatch` remain on `ci.yml` and use its
   existing Plan, obligations, and Required gate. They do not dispatch another
   workflow or construct a second task graph. Schedule is always `Full`. Manual
   UI `scope` is a closed string choice, default `full`, with only `affected` and
   `full` accepted. At the runtime boundary, absent raw `scope` means
   `Affected`, regardless of the YAML/native default. Ordinary events ignore
   dispatch inputs.
2. **Full means complete work.** A `Full` plan enumerates the complete applicable
   obligation inventory before job selection, executes every applicable
   obligation without baseline-driven omission, skips baseline lookup and
   coverage, and rejects reuse decisions and baseline proofs. Manual `Affected`
   may narrow only through the ordinary complete proof rules and an exact usable
   base.
3. **Base is an exact commit identity.** In affected manual scope, an absent
   `base_sha` widens to `Full`. A supplied value must match exactly
   `^[0-9a-f]{40}$`; malformed values fail before any Git process is invoked.
   Never accept or resolve branch names, tags, symbolic refs, abbreviations, or
   a configured/default branch as a substitute. Plan invokes Git directly,
   without a shell, using argv
   `["git", "rev-parse", "--verify", "--end-of-options", "<validated-sha>^{commit}"]`
   in the Plan checkout and requires the resulting commit identity to equal the
   supplied SHA. A missing/unavailable object, non-commit object, or identity
   mismatch widens to `Full` with a recorded reason. Do not fetch a base or
   substitute baseline lookup. Explicit `Full` ignores `base_sha` and performs
   no Git lookup.
4. **Preserve raw input truth.** The request and authenticated plan retain raw
   input presence/values alongside parsed event, scope, head, base identity, and
   simulation state. A missing raw `scope` is `Affected`; it must not be replaced
   by a native `inputs.scope` default. Present scope values outside the closed
   choices, or conflicting raw/native values, reject. `simulate_failure` is a
   native Boolean defaulting false. Missing raw value requires native false. The
   exact raw string `"true"` maps to true and `"false"` maps to false; every
   other present raw value rejects. The converted raw value must equal the
   native Boolean, so a default false cannot mask raw true.
5. **Plan and merge have separate evidence.** Plan resolves the base in its own
   checkout. Merge independently recaptures the event, scope, exact `GITHUB_SHA`,
   raw `base_sha`, and simulation input, then compares them with the
   authenticated plan identity and resolution outcome. Merge performs no Git
   object lookup; it verifies the plan's authenticated base identity against
   fresh event input and makes no claim that the object exists in the merge
   checkout. Disagreement rejects the plan/merge operation.
6. **Failure simulation stays fixed.** Only an explicitly matched native true
   `simulate_failure` runs one generator-owned failing step immediately before
   Plan. Required emits its ordinary `planning_failed` report. No extra
   sentinel job or ordinary-run runner allocation is introduced.

These rules implement the C07/full-work requirements in
[`velnor-actions-ci-performance-goal.md`](../../velnor-actions-ci-performance-goal.md)
and §6 of
[`velnor-actions-ci-performance-spec.md`](../../velnor-actions-ci-performance-spec.md).
They are contract changes, not implementation or hosted-qualification claims.
