# CI performance independent correctness review

Status: in progress; not qualification or merge approval.

Reviewed original complete goal/specification, including C01–C09, ownership/trust, affected selection, T01–T26 and appendices. Source review starts at HEAD `2eef5063895e6aa57fe5bde0a7108d294ec2106f` plus concurrent working changes. Final committed head, regenerated workflows and raw hosted qualification require another review.

## Findings and traceability

| ID | Requirement | Finding | Disposition |
|---|---|---|---|
| R1 | C07, §6.2, T03/T11/T16 | `baseline_publish.rs:285` drops covered obligations. A successful main run with omitted jobs publishes an incomplete next exact-base baseline, losing usable original proofs and forcing reexecution. Existing publication tests intentionally encode this loss. | Assigned selection/gate. Preserve verified original proof identity with authenticated origin linkage; changing the original run to current would fabricate execution. |
| R2 | C02/C06, T18 | `cache_elect.rs:153` suppresses tool save on any exact hit. Missing restored components can be repaired, but immutable original payload remains incomplete. Repeated fresh runs may repeat repair. | Tools-cache owner investigating; safe repair alone does not prove persistent complete warmth. |
| R3 | C03–C05, T03–T05/T17 | Current `mbx_domain.rs` separates stable writers, but its source-only snapshot suffix and explicit version input retain the pinned action's exact-hit useful-state loss and second installer. File documents these as unresolved. | Upstream MBX owner integration in progress. No performance qualification inferred from domain keys. |
| R4 | §6.1, T06/T10/T11/T12/T24 | Deleted manifests now conservatively broaden. Full task-specific semantic closure and complete base inventory remain necessary for minimal affected work. Whole-checkout closure can establish a conservative content boundary but invalidates unaffected tasks on leaf/docs changes. | Graph/closure owners notified. Distinguish safety floor from minimal-work qualification. |
| R5 | C06, T20 | `source_cache.rs:123` suppresses save on an exact hit even when `source_prep` repairs missing selected dependencies. Conversely, every compatible-prefix hit saves even when offline probes succeed and no payload changes (for example a local package version-only lockfile edit). `source_prep.rs:170` fetches the whole workspace without selected package/feature/target closure. | Independently confirmed by source-cache reviewer; parent/tools-cache owner notified. Completeness/useful-delta strategy and selected fetch remain unresolved. |
| R6 | C06, T08/T09/T20 | Source probe ignores checkout `.cargo/config*`, but obligations honor it. Lockfile-only archive identity excludes source replacement/registry configuration. Default metadata probe also omits selected feature/target configuration; missing optional/target dependencies can escape preparation and fail later offline work. | Independently confirmed by source-cache reviewer; parent notified. Use consistent reviewed source configuration and selected closure or fail closed for unsupported replacement semantics. |

## Reviewed protections

- C01: explicit pinned cache restore/save consume one ordered `TOOL_PAYLOAD`, including Mise, Rustup, Cargo proxies and tool metadata; sources own registry/Git separately. Restore precedes install. Full hidden-version and hosted behavior still need evidence.
- C07: crate job predicate evaluates before runner allocation; explicit `!cancelled()` avoids implicit dependency-success suppression after an intentionally skipped staged predecessor. Plan failure blocks allocation; absent coverage conservatively executes.
- Required accepts skipped jobs only for a nonempty crate obligation group whose every member has baseline coverage. Covered owners must appear in declared Required inventory. Proof validation remains independent and authoritative.
- New `impl_required_skip.rs` cases cover missing owner, missing reports, failed/canceled/neutral/missing jobs, unbound jobs, incomplete/stale/failed baseline and mixed selected/covered groups. Reviewed test source; execution evidence belongs to serialized parent Cargo queue.
- Candidate identity now resolves accepted PR merge checkout to actual tested commit; committed diffs use exact base/candidate trees. Deletion/rename uncertainties broaden instead of silently dropping old owners.
- Rust test preparation uses supported Nextest binaries-only invocation; test execution and doctest obligations remain separate.

## Remaining verification

1. Re-review inherited-proof design, tools repair persistence, supported MBX transport and final task closures after implementation.
2. Inspect regenerated generator/consumer job conditions, Required dependencies, canonical cache payloads, tool homes and release isolation at final head.
3. Run independent focused negative tests through the parent Cargo queue; inspect deterministic gate results.
4. Recompute cold/warm/third-run evidence from raw logs. A passed workflow or local test is not a performance measurement.
5. Review all final-head PR feedback and exact default-branch/artifact identities before merge approval.

No hosted qualification is asserted. Unknown download/compiler/transfer telemetry remains unknown. Scope, waivers and inaccessible measurements must remain distinct from passing CI.
