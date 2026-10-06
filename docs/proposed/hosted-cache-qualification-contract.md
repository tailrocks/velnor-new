# Hosted cache qualification contract

**Status:** Proposed contract for the Velnor-only hosted qualification path.
This is not a cache-performance result. The synthetic round-trip in
[`implemented/cache-measurements.md`](../implemented/cache-measurements.md)
does not satisfy this contract or complete any hosted evidence row.

## Dispatch boundary

Only the generated Velnor repository workflow accepts `workflow_dispatch` for
this protocol. Its closed inputs are a non-empty `campaign`, one `phase` from
`cold`, `warm`, `third`, `useful_delta`, or `control`, and the paired optional
coordinates `predecessor_run_id` and `predecessor_run_attempt`. Both
coordinates are absent for `cold` and `control`; both are positive for
`warm`, `third`, and `useful_delta`. A coordinate selects a run attempt for
lookup. It grants no cache access by itself.

Every run must come from the exact repository's protected default branch, the
generated `.github/workflows/ci.yml`, a successful `workflow_dispatch` attempt,
and the recorded source/workflow SHA. The runner's repository, ref,
`GITHUB_REF_PROTECTED`, workflow ref/SHA, source SHA, run ID, and attempt are
captured with the receipt. The resolver also checks the current repository's
default branch and branch-protection status through the read-only Actions API.
Missing or conflicting authority data fails closed. Generated repositories
without Velnor repository policy do not get this trigger or resolver.

## Phase sequence

| Phase | Source and runner | Restore | Save |
| --- | --- | --- | --- |
| `cold` | Initial source on a fresh hosted runner | Every enabled layer must prove absence; an unexpected prefix match is discarded and fails the cold claim | Save useful new state as K1 |
| `warm` | Same source and workload on a fresh runner; predecessor is `cold` | Admit the exact K1 cache object before import | Save K2 only when that layer's useful state changed |
| `third` | Same source and workload on another fresh runner; predecessor is `warm` | Admit the exact K2 cache object before import | Read-only; no cache rewrite |
| `useful_delta` | A documented source B after source A, on a fresh runner; predecessor is `third` | Admit the exact compatible K2 object before import | Save K3 only for observed useful state created by the source delta |
| `control` | Same workload/configuration and a comparable hosted runner | Disable every cache and task-result reuse layer | No cache save |

The Cold, Warm, and Third source SHA and configuration identity are equal.
`useful_delta` keeps the same full task set and cache namespace. Its source
delta is derived from the checked-out Git history: source A must be an
ancestor of source B, the changed paths are captured in sorted repository-
relative form, and the canonical path list is digested. Caller-supplied delta
claims are not accepted. The control run uses the source/configuration chosen
for its paired comparison and changes cache access only.

## Cache identity and pre-use admission

Each directive binds campaign, repository, workspace, task set, configuration,
profile, toolchain, runner image/version/ABI, target, cache format, layer, and
lane. The typed plan output `qualification_cache_directives` is derived only
after plan validation and predecessor receipt admission; the exact serialized
value is included in the same promoted-output map and UTF-16 budget used by
workflow rendering. Renderer consumers must reconstruct authorization from
the validated plan and admitted lineage before binding runtime observations
or deriving a backend key. An informational campaign or phase output is never
an authorization token.

GitHub's dependency-cache restore may return and extract a prefix match for
the primary key even when `restore-keys` is empty. A full key or
`cache-matched-key` check after extraction is not sufficient to authorize
consumption. The producer must bind the selected backend cache ID, key, ref,
version/format, and stored size to the admitted receipt before bytes are
imported or used. Unexpected Cold bytes are discarded; a mismatched Warm,
Third, or UsefulDelta candidate is never moved into a live tool/cache path.
Control omits or disables restores and saves for MBX objects, the MBX bundle,
Cargo sources, Mise tools, and OpenTofu providers. Task-result reuse stays
disabled in every phase.

## Immutable receipt and collection

Each successful producer run publishes one artifact named
`velnor-qualification-cache-receipt-v1` containing exactly
`qualification-cache-receipt.json`. The schema-1 document binds the producer's
runner context to the validated plan and contains every lane's completed task
IDs, closure and useful-state digests, and one record per cache layer. Each
layer records the requested primary key, restore result and exact matched
backend object (or an observed miss), post-job save result, and before/after
state digest. Save eligibility is not evidence that a cache was created.

A collector runs after cache-producing jobs finish so action post-job saves
have completed. It records the actual backend listing and producer logs needed
to distinguish a cache created, skipped, or failed. UsefulDelta additionally
requires changed source paths, a fresh late-closure observation, and a useful
state change for the specific layer being saved. The collector uploads the
receipt only after its contract validator accepts the whole plan/run/phase and
all lane/layer observations.

The next dispatch resolves an immutable run ID and attempt, verifies that
attempt through repository-scoped Actions API metadata, requires the unique
receipt artifact, fetches it by immutable artifact ID, checks API size and
SHA-256, and accepts a bounded ZIP with exactly one regular receipt file.
Receipt links include predecessor run/attempt, receipt digest, artifact ID,
and artifact digest. The resolver follows and verifies the complete
Cold-to-Warm-to-Third chain with a fixed maximum depth; run coordinates and
artifact names alone do not establish lineage. API documents may contain
unrelated fields, but every authority field in this contract is mandatory.

All 47 qualification rows remain incomplete until source-bound hosted
receipts and required workload measurements exist. Missing measurements stay
null; a passing dispatch, cache hit, synthetic probe, or eligible-save output
does not establish a performance result.
