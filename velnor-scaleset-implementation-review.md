# Velnor Scale Set implementation review and recovery order

**Review date:** 4 October 2026, Singapore time.  
**Scope:** Source and PR review, generated consumer configuration, selected recent workflow/job diagnostics, and implementation evidence. No repository edits, builds, live reruns, macOS deployment, or independent host cleanup tests were performed in this review. Some local job-log requests returned 404/BlobNotFound. Moving run/PR states must be refreshed during execution.

## Conclusion

Stabilize and qualify the implementation already on Velnor main. Do not start another Scale Set rewrite. The first fixes are the hosted MBX export failure, local archive/image compatibility, and the gap between source changes and the software actually running on the Mac. Then publish the repaired generator, regenerate the complete ChainArgos output tree, and require full paired and lifecycle qualification before default promotion.

The companion `velnor-scaleset-recovery-goal.md` is self-contained. It contains the detailed workstreams, implementation paths, verification and release gates, source register, Rust pseudocode, and the original user execution directives.

## Current source and consumer state

The reviewed Velnor main was `5a946c33cf005777feab2bc91fa4aa8e01dd58f4`. The runner host already has daemon, listener, registration, capacity, convergence, journal, launch, lifecycle, completion, Docker, worker, service and telemetry modules. This is not an empty architecture scaffold. [1]

ChainArgos PR #2085 uses branch `n1-qualification`, reviewed at `620df66423cdd2403b69ced1724d17fb8afd1d6f`. Its manifest pins generator commit `0561601c1f71a80d16e75983124b4bd69c883bb1`, while its PR description still referred to the earlier `d408681…` product. Use the committed manifest, exact downloaded binary and trusted producer identity as authority. A binary named `velnor-actions-0.1.0` does not by itself identify which generator source it contains. [2,3]

The consumer already has schema-2 `both` execution, hosted `ubuntu-26.04`, local `[velnor, ubuntu-26.04-scale-set]`, and Linux/amd64 profiles. Nextest-result substitution is disabled. Preserve these working intentions rather than reimplementing routing or claiming that the consumer still lacks Scale Set configuration. [4]

The most recent retrieved run snapshot for `37128301624` was attempt 4 and nonterminal (queued). Earlier job inspections found local failures and queued work; one local job-log request could not retrieve its blob. This is not a complete paired pass, and unavailable logs do not establish the failure's root cause. [5]

## Findings ordered by execution priority

| ID | Priority | Finding / confidence | Required resolution |
|---|---|---|---|
| F01 | P0 | Confirmed hosted failure: post-action MBX export exhausts disk after first-party verification succeeds. | Review/adopt #26, repair the generator policy, and prove a real protected-push cache write followed by restore. |
| F02 | P0 | Static archive correctness concern: #25 strips/extracts member paths before relocating them, potentially losing distinct destination identity. Some semantic flags are accepted without equivalent implementation. | Reproduce actual runtime incompatibility; fix the standard-tool/runtime path or fully qualify a deliberately scoped compatibility layer. Add adversarial path/link/flag tests. |
| F03 | P0 | Current local failure diagnostics incomplete; queue progress must be verified against actual daemon/session/capacity state. | Obtain attempt-specific evidence and deploy/verify existing listener, capacity and expired-session fixes. Do not guess every failure is a tar failure. |
| F04 | P0 | Product/deployment identity drift: generator manifest, PR prose and historical daemon evidence refer to different products. | Record generator, daemon, runner image and DinD identities separately; verify running process and actual worker digests. |
| F05 | P0 | Consumer generation must include the new shared local composite actions, not only ci.yml. | Publish the compatible repaired generator and regenerate/check the entire owned tree deterministically. |
| F06 | P1 | N=1 evidence is narrower than a full rollout. | Complete N>1 and N+1 queue pressure, failure recovery, cancellation and independently verified cleanup. |
| F07 | P1 | Capability declarations and two successful summaries do not establish complete parity. | Execute the entire expected paired plan and genuine Docker capability suite; verify terminal jobs and immutable artifacts. |
| F08 | P1 | Published macOS installation and complete cleanup were explicitly unproven in historical evidence. | Publish/install/identify the native product; qualify launchd, Keychain, Docker availability, sleep/wake, upgrade and rollback. |
| F09 | P1 | Rerun/artifact lineage and post-action failures require adversarial verification. This is a required test area, not a proven existing comparator bug. | Fail closed on missing/mismatched evidence; handle legitimate partial-rerun lineage explicitly. |
| F10 | P2 | Documentation, diagnostics, broad generator regression coverage, measured cache performance and broader platform-check work remain. | Finish these after the broken paths, with explicit disposition of #18/#20 and all newly found defects. |

### Hosted cache: the most clearly diagnosed failure

Run `37114238559`, including job `111178048670`, failed at `Post Restore MBX objects`. The inspected log recorded disabled automatic MBX collection and `No space left on device (os error 28)` during directory export. This must not be “fixed” by ignoring post-action errors or weakening Required. [6]

PR #26 upgrades MBX/action versions and enables `MBX_GC_AUTO=1` for hosted object-cache jobs. Its green PR checks are insufficient to demonstrate the protected cache-write path, especially when PR writes remain disabled correctly. Capture peak export/storage/inode usage and prove the next authorized run restores the saved generation. [7]

### Archive layer: correctness before compatibility claims

The PR #25 extraction approach can map `.cache/x` and `../../../.cache/x` to the same stripped staging path even though their requested final destinations differ. This is a source-derived collision concern to reproduce, not a live ChainArgos incident attributed without logs. Review symlinks, hardlinks, member order, repeated entries, PAX paths, permissions, compression and ignored flags as a class of archive semantics. [8]

The source's GNU-tar/qemu/openat2 explanation also needs independent runtime verification. Do not turn a reported explanation into an established upstream defect, or substitute ARM64 silently for the x64 profile.

### Historical deployment evidence must stay historical

PR #18 records a successful N=1 canary, a locally built daemon at `030363d…` rather than a published macOS artifact, and launch row 71 leaving DinD running. These establish missing qualification at that time. They do not establish which binary or leftover container exists now; inspect the host before taking action. [9]

## Recommended integration and completion sequence

Use current main as the integration base. Review #26 and #25 independently, preserve relevant merged fixes such as #24, and treat #20 as a separate broader change requiring deliberate compatibility review. Do not select a branch merely because its diff is large.

Complete upstream regression fixes, verified publication, native/image deployment, full consumer regeneration, real hosted write/restore and local queue recovery. Then complete paired cold/warm execution, strict artifact/job verification, lifecycle/security negatives, final protected-main verification and explicit default promotion. Finish P2 documentation, telemetry, maintenance, performance measurements and outstanding PR dispositions afterward.

A completion claim requires exact source/product identities, successful terminal jobs including post actions, complete expected paired results, real concurrency and cleanup evidence, actual installed published macOS software, and a tested generated hosted-only rollback. A canary, mocks, a green read-only PR, or an unpublished local build cannot substitute for those proofs.

## References

1. https://github.com/tailrocks/velnor-new/blob/5a946c33cf005777feab2bc91fa4aa8e01dd58f4/crates/velnor-runner/crates/velnor-runner-host/src/lib.rs
2. https://github.com/ChainArgos/java-monorepo/pull/2085
3. https://github.com/ChainArgos/java-monorepo/blob/620df66423cdd2403b69ced1724d17fb8afd1d6f/.velnor/release-manifest.json
4. https://github.com/ChainArgos/java-monorepo/blob/620df66423cdd2403b69ced1724d17fb8afd1d6f/.velnor/config.toml
5. https://github.com/ChainArgos/java-monorepo/actions/runs/37128301624
6. https://github.com/ChainArgos/java-monorepo/actions/runs/37114238559/job/111178048670
7. https://github.com/tailrocks/velnor-new/pull/26/files
8. https://github.com/tailrocks/velnor-new/blob/9bf909a19f7c3fc26d0203fd0f28efbdc0eafd41/images/runner/ubuntu-26.04/tar-absolute.sh
9. https://github.com/tailrocks/velnor-new/pull/18
10. https://github.com/tailrocks/velnor-new/pull/24
11. https://github.com/tailrocks/velnor-new/pull/20
