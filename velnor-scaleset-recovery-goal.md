/goal

# Stabilize, qualify, and complete the Velnor Scale Set rollout to ChainArgos

Implement this goal in `tailrocks/velnor-new` and `ChainArgos/java-monorepo`. Repair and complete the implementation that already exists. Do not restart the runner project, create a competing executor, or treat one successful canary as a completed rollout.

This is an execution goal: investigate, implement, independently review, test, publish verified products, regenerate the consumer, qualify on the actual macOS host, and finish the rollout. P0/P1/P2 define execution order, not permission to leave known defects unfixed. Complete the later work after the immediate pipeline failures are resolved. Use the mandatory execution rules appended below.

## 1. Evidence baseline and required refresh

This review was assembled on 4 October 2026, Singapore time. The following are observed research snapshots, not a promise that branches and runs will remain unchanged. Re-read current repository rules, branches, PRs, releases, generated files, run attempts, job logs, check annotations, and installed host state before changing anything.

| Item | Reviewed snapshot | Interpretation |
|---|---|---|
| Velnor main | `5a946c33cf005777feab2bc91fa4aa8e01dd58f4` | Existing Scale Set implementation plus expired-session fix; start from current main rather than rebuilding the original plan. |
| ChainArgos PR #2085 | `n1-qualification`, head `620df66423cdd2403b69ced1724d17fb8afd1d6f` | Existing paired rollout work to preserve and repair. |
| Consumer generator | `.velnor/release-manifest.json` pins `0561601c1f71a80d16e75983124b4bd69c883bb1` through `generator-0561601…` | The PR description still mentions an older generator. Inspect the manifest and actual binary, not prose or the `0.1.0` filename alone. |
| Paired consumer run | `37128301624`, attempt 4 | Nonterminal in the last API snapshot; earlier inspections found local failures/queued jobs. Not evidence of a completed paired pass. Refresh all attempts and jobs. |
| Hosted main failure | `37114238559` | First-party verification succeeded in affected jobs, but post-action MBX export failed with disk exhaustion. This is not a Scale Set scheduling failure. |
| N=1 canary | `37117721384` | Historical successful canary only. Does not prove concurrency, complete parity, published-binary installation, or cleanup. |
| Open Velnor #26 | `fix(cache): keep MBX GC enabled on hosted jobs` | Relevant hosted-cache fix; needs independent review and actual write/restore qualification. |
| Open Velnor #25 | `fix: honor tar -P so cache paths survive BusyBox`, branch `tar-absolute` | Relevant local-image work; custom tar compatibility needs adversarial tests before acceptance. |
| Open Velnor #18 | `scaleset-evidence` | Historical evidence, including an unpublished local daemon and a leftover DinD container. Refresh rather than promote historical claims to present fact. |
| Open Velnor #20 | Qualified named Mise platform checks | Separate, broader architectural/schema work. Evaluate after restoring the rollout; do not choose it merely because it has more changes. |

The source register at the end provides direct references. Treat a PR body as its author's report; distinguish it from source inspection, retrieved logs, tests independently executed, and current host observations. Where diagnostics were unavailable, investigate rather than inventing a root cause.

### 1.1 Establish one current inventory

Create a concise failure and deployment ledger in the existing `docs/implemented/macos-scaleset/` documentation. Reuse the existing checklist/evidence/decisions/disposition documents where practical. Do not create competing roadmaps or renumber historical gates without a mapping.

Record repository/ref/commit, PR state and ancestry, exact generator source/tag/asset/digest, generated-tree identity, native daemon source and binary digest, runner and DinD image digests/platforms, official runner version, Docker context and engine identity, macOS/guest architecture, Scale Set identity/labels, configured capacity, and actual launchd process/executable identity. A new generator does not automatically update an installed daemon or image.

Inventory relevant workflows on main and PR #2085, plus qualification, generator release, native-binary release, and image publication workflows in Velnor. Paginate jobs and artifacts. Record run ID, source SHA, event, attempt, status, conclusion, job IDs and failed steps. Separate workflow validation rejection, queue starvation, bootstrap/cache restore, execution, artifact upload, post-action cache save, and cleanup failures.

Retain current and prior-attempt diagnostics before destructive cleanup or reruns. Some local job-log downloads returned 404/BlobNotFound during review, including job `111269118868`; this establishes a diagnostic gap, not its execution root cause. Try attempt-specific job APIs, annotations, run-log archives, artifacts, official runner `_diag`, and host logs. Record exact retrieval failures and add durable diagnostic collection where it is missing.

### 1.2 Choose the integration source deliberately

Use current main as the integration base when it contains the Scale Set implementation. Inspect PR #25 and #26 independently; merge/adapt only reviewed relevant changes and their regression tests, respecting repository approvals. Do not blindly merge an entire branch to obtain one fix. Preserve other agents' commits, use one integration branch per repository where safe, and synchronize via merges rather than rewriting shared history.

Create a disposition for every relevant open PR and already-merged fix: adopt with tests, already satisfied, superseded, or separately scheduled with a concrete dependency/correctness reason. Revalidate the daemon-launch, demand/capacity, generated-workflow size, actionlint/local-composite, and expired-session fixes already implemented around PRs #17, #19, #22, #23, and #24. Do not list already-merged code as an unimplemented feature, and do not assume it is deployed.

## 2. P0-A — Repair the hosted MBX post-action failure first

### Observed failure

Hosted run `37114238559`, including job `111178048670`, reached successful first-party build/test/report steps and then failed in `Post Restore MBX objects`. The inspected log showed MBX automatic GC disabled and `mbx cache export --format directory …` ending with `No space left on device (os error 28)`. Other affected jobs in the run included eth-grpc-server, lightdash-csv-delivery-app, eth-processor-app, and tron-processor-app. The final Required failure must not be bypassed merely because tests earlier in those jobs passed.

PR #26 proposes MBX 1.21.1, action v1.6.0, and hosted-job-lifetime `MBX_GC_AUTO=1`. It is the relevant starting point, not proof that the production write path is already fixed.

### Implementation targets and root-cause work

Review `crates/velnor-actions-workflow-renderer/src/constants.rs`, `src/renderer/job_rust.rs`, `src/renderer/mbx_cache_scope.rs`, the corresponding renderer cache tests/fixtures, and the exact upstream MBX/action revisions referenced by PR #26. Update the generator and its canonical contracts, not hand-written consumer YAML.

Verify why the old action disabled GC, when post-action export duplicates objects, whether collection remains enabled during post steps, and how active objects are protected. Test capacity across live objects, export staging, archive compression, temporary files, and inode usage. A free-space check without a correct bounded export/collection design is not sufficient. Do not replace failure with `continue-on-error`, hide it from Required, or indiscriminately purge caches.

Keep trust and write policy explicit: untrusted PR jobs must not acquire privileged cache-write capabilities. Preserve the distinct local cache backend/policy. Where provider behavior is inferred from a job-name suffix, test that invariant and prefer an existing typed provider field rather than making display naming an authority boundary.

### Acceptance

Reproduce the old failure or provide exact existing logs plus a deterministic low-disk regression. Demonstrate a real authorized hosted cache write on a protected push, then a subsequent run restoring the written generation. A green read-only PR does not exercise the failing save path. Capture available bytes/inodes and cache/export sizes before restore, after tests, during export, and after cleanup. Prove active objects remain usable while GC runs. Verify subsequent warm runs avoid unnecessary download/recompilation while tests still execute. Assert terminal GitHub job status including post actions, not only the uploaded test report.

## 3. P0-B — Fix the local archive/image compatibility structurally

### Current review finding

PR #25's `images/runner/ubuntu-26.04/tar-shim.sh` and `tar-absolute.sh` substitute BusyBox/custom archive behavior for GNU tar. The source describes an `openat2`/qemu-user problem; independently reproduce that explanation on the actual provider before treating it as established root cause.

The `-P` extraction path first extracts using BusyBox's rewritten member names, then moves files to reconstructed destinations. This can collapse distinct member paths before relocation. For example, `.cache/x` and `../../../.cache/x` can target different final locations but the same stripped staging path. This is a static semantic finding requiring an executable regression, not a claim that this exact collision was observed in ChainArgos logs. The shim also accepts some flags without implementing their semantics; silently accepting options is not a drop-in compatibility contract.

### Required work

Use independent agents for environment diagnosis and archive correctness/security. Identify the exact failing GNU tar operation, binary, syscall/error, Docker provider/guest, architecture and emulation. Prefer a functioning, qualified standard tool/runtime fix. Do not patch the official Actions runner, falsify architecture/kernel checks, silently substitute ARM64 for the Linux/amd64 profile, or advertise an incomplete custom tar as equivalent.

If a compatibility layer remains necessary, specify its supported contract, fail explicitly on unsupported semantic options, and demonstrate correctness against a reference implementation. Do not construct extraction by losing original path identity and moving files afterward. Preserve archive order, duplicate-entry behavior and link semantics deliberately. Archive paths from untrusted inputs must never cross the intended trust boundary; intentional absolute/parent-relative paths in trusted Actions caches require exact scoped behavior, not blanket suppression.

Test relative, parent-relative and absolute member paths; colliding sanitized paths; spaces, Unicode and newlines; deep and long PAX paths; repeated members; empty directories; hardlinks and symlinks; symlink-following/traversal; modes, executable bits and timestamps; exclusion rules; file lists; stdin/stdout; compression and exit codes. In particular, verify options currently ignored by the shim rather than merely checking that a command exits zero.

Run real pinned `actions/cache` and MBX create/upload/download/restore round trips. Verify content and expected absolute destination, not only archive readability. Exercise empty cache, cache miss, hit, partial archive, corrupt archive, disk full, cancellation, and a repository path with spaces. Prove both correctness and containment for the chosen trust model.

Rebuild and publish a digest-pinned image through the trusted producer. Drain and replace old workers safely, then verify the actual new image ID/digest inside subsequent jobs. A passing test of a local image that is never deployed does not fix the consumer.

## 4. P0-C — Resolve local failures, queue progress, and deployed-state drift

Use the existing Rust host implementation, not a new parallel supervisor. Inspect the actual modules under `crates/velnor-runner/crates/velnor-runner-host/src/`, including `daemon`, `listener`, `registration`, `capacity`, `convergence`, `journal`, `launch`, `lifecycle`, `completion`, `docker`, `worker`, `service`, `storage`, and `telemetry`.

Reconcile source, published binaries, launchd executable, running process, image digests, live containers, durable records, sessions and GitHub jobs. Confirm that the installed daemon includes the actual listener launch, finished-worker redelivery/capacity correction, and expired-session recovery fixes. In particular, PR #24 handles a specifically identified expired-session HTTP 400 when deleting/reopening a session; unrelated 400 responses must remain failures. Do not broadly swallow all 400s or retry the same permanently expired session indefinitely.

For every failed or stuck local job, determine whether the cause is routing, session expiry, acquisition/JIT, pending capacity, provisioning, image bootstrap, archive restore, actual task failure, post actions, or cleanup. Tie each conclusion to the same run attempt and worker lifecycle. Do not relabel all local failures as the tar problem without logs.

Count reserved, uncertain, acquiring, provisioning, running, finishing and cleaning lifecycle states against one global capacity authority. A stale launch row or an exited runner with an uncleaned DinD daemon must not create phantom capacity or permanently occupy a slot without reconciliation. Replayed messages must not launch duplicates or count completed historical workers as newly started workers. Do not release uncertain resources early just to make the queue move.

Test N=1, then N>1 with more queued jobs than slots. Observe every wave to terminal completion. Exercise daemon restart, expired session, network loss/recovery, Docker restart, runner crash, cancellation, and shutdown/drain while jobs and services are active. A retry loop with no durable progress is a failure, not liveness.

The historical evidence in PR #18 says launch row 71 left a DinD container running and used a local daemon build at `030363d…`. Verify present state; do not assume the same leak or binary remains. Investigate any leftover resources and structural ownership/lifecycle cause. Capture diagnostics before cleanup. Match immutable Docker IDs and ownership proof before deletion; never delete unrelated containers, volumes, networks or another daemon's session based on a name prefix alone. Verify complete drain externally, and only then return capacity to the available pool.

Implement useful failure-visible readiness: process alive, Docker connected, credentials usable, session live, capacity available, and last successful progress are separate states. A stalled listener must not advertise readiness. A hosted watchdog must start independently of the local queue and report local unavailability without silently rerouting the missing local lane to hosted execution.

## 5. P0-D — Publish the repaired generator and regenerate ChainArgos completely

The consumer already uses schema 2 and paired routing. Preserve the current intent unless a validated migration is necessary:

```toml
[execution]
mode = "both"
default_profile = "github_ubuntu26"
pair_with = "velnor_ubuntu26"

[profiles.github_ubuntu26]
kind = "github-hosted"
labels = ["ubuntu-26.04"]
platform = "linux/amd64"

[profiles.velnor_ubuntu26]
kind = "scale-set"
labels = ["velnor", "ubuntu-26.04-scale-set"]
platform = "linux/amd64"
```

The reviewed configuration declares Docker actions, services, container jobs, Compose, Buildx and Testcontainers. These are claimed capabilities to qualify, not proof of completed tests. `nextest_result_cache = false` is already configured: preserve real test execution in both lanes.

“Latest” means the newest verified, compatible product containing the required generator changes, not the highest-looking version string, `/releases/latest`, or an arbitrary local binary. Generator assets still named `0.1.0` can have distinct source-bound `generator-<sha>` releases. Compare actual ancestry, generator source closure, manifest, digest and trusted build provenance. A main commit touching only daemon code does not automatically require a different generator product; a renderer/cache fix does.

Integrate and verify the relevant fixes. Publish the generator from the reviewed integration commit through the canonical generated producer. Do not move the original v0.1.0 tag or overwrite old source identities. If the implementation branch is the only correct source, produce a commit-addressed verified prerelease from it; do not make consumer CI routinely compile an unverified branch or accept self-authored checksums as the only provenance.

Update `.velnor/release-manifest.json` with the actual adopted product identity, digest and assets. Use that exact downloaded/verified executable for generation. Confirm current CLI syntax; the reviewed PR documents:

```sh
velnor-actions .github --bless --out-dir .github
velnor-actions .github --check
```

Regenerate the whole owned output set: workflow, local composite actions, validation configuration and other generated support files. Do not repair only `ci.yml`; paired jobs now depend on generated composites. Check deterministic output in a clean directory, ownership/stale-file deletion rules, source-closure coverage, generated annotations and validation. Ensure unrelated hand-maintained files are preserved.

Keep the compact-workflow fix: the earlier paired workflow exceeded GitHub's 500000-byte limit; the later generator extracted shared composites. Test maximum realistic consumer graphs and the complete generated tree, not a small fixture alone. Preserve job identities, dependencies, timeouts, permission boundaries, cache-save placement, outputs and required contexts while sharing bodies. Keep any local-composite actionlint exception narrow and tied to actual generated constructs; no blanket validation ignore.

Verify the resulting PR and protected main after merge under existing approval policy. Do not bypass branch protection or remove Required to obtain green status. Update the PR description and evidence to the manifest actually committed. Record source → producer run → asset → manifest → generated-tree → consumer run identity.

## 6. P1 — Prove real parity, not two incomplete green summaries

Create the expected logical-workload inventory from the single generated plan. For each eligible workload, require both hosted and local execution with the same repository/source SHA, plan/task identity, locked tools, command, feature/configuration and input digest. Report actual host/guest architectures separately. Ubuntu version strings alone do not prove environment equivalence.

Keep planning, independent policy/comparison/Required, watchdog and trusted control jobs hosted where designed. Keep publication, deployments, release signing, baseline mutation and other external side effects single-writer. Define those exclusions before running; do not invent exclusions afterward to excuse a failed local job. Missing optional inputs must follow explicit optional semantics rather than becoming arbitrary blockers, but required inputs and missing lanes must fail closed.

Compare authoritative terminal GitHub jobs with immutable artifact IDs/digests and declared logical result sets. Include post-action and upload failures. Assert all expected API pages were read. Do not search by artifact name and accept whichever result is convenient. Bind repository, source, plan, logical task, provider/profile, run ID and attempt/lineage. Define an explicit partial-rerun policy: GitHub can preserve successful jobs from an earlier attempt; either prove permitted lineage for unchanged jobs or require a fresh full qualification run. Never silently mix unrelated attempts, plans or same-SHA historical reports. This is a required adversarial test, not an assertion that the existing comparator is already known to contain this bug.

Test missing/duplicate reports; wrong artifact ID/digest; stale attempts; wrong source, plan or provider; skipped/cancelled/timed-out jobs; successful task reports followed by failed post steps; missing API pages; and different expected task inventories. Expected-negative workflows must fail in GitHub, and the independent verifier must confirm the intended reason.

Run full paired qualification cold and immediately again warm. Caches may reduce dependency/tool downloads and compilation, but expected tests must still run. Compare expected versus observed test counts and relevant semantic outputs. Do not require byte-identical binaries where build timestamps or paths are intentionally variable; document and test any normalization. Do not allow normalization to erase actual task failures or missing coverage.

Inventory ChainArgos JVM, Rust and any other required work explicitly. The reviewed source closure includes Java/Gradle areas while the check declarations target Rust. Determine current required coverage and preserve it; do not call Rust parity complete Java-platform verification. Address actual coverage omissions with the appropriate existing adapter/contracts, and name any separately scoped qualification honestly.

## 7. P1 — Close host, release, Docker capability and security gates

Run genuine official GitHub Actions on the actual macOS-hosted Docker provider, not an Actions emulator. Qualify pinned JS actions, local/composite actions, Docker actions, `container:` jobs, workflow services, file commands, outputs, masking, post actions, cache restore/save, artifacts, exact-SHA checkout, cancellation and expected failures. Add real Compose, Buildx and Testcontainers probes, including cleanup helpers, networking, bind mounts, concurrent jobs using the same logical port and service readiness.

Keep the official runner unmodified. Match absolute workspace/temp/action/tool paths in the private Docker daemon. The standard Docker socket used by actions must refer to the worker-private daemon, never the outer host engine. Do not mount the user's home, SSH keys or Keychain into jobs. Qualify Linux/amd64 including emulation on Apple Silicon; a faster ARM64 workload is a distinct profile, not equivalent x64 proof.

Complete the native product release path. Verify published macOS binary source/digest/provenance and applicable signing/notarization requirements, install that artifact, and prove launchd executes it. A local `cargo build --release` can aid debugging but cannot satisfy published-product installation. Verify service startup, doctor/status, credentials from the authorized secure store, locked Keychain, offline startup, Docker-unavailable startup, sleep/wake, restart, upgrade, rollback, drain and disconnect. Preserve user-session lifecycle constraints explicitly.

Maintain pinned runner/image identities with an update and requalification process; disabling automatic updates must not strand the runner on a version GitHub stops accepting. Verify exact registration semantics and JIT/AcquireJobs/ACK behavior against the official reference rather than guessing compatibility from ordinary self-hosted runner APIs.

Use canary secrets to test argv, environment metadata, labels, plist, journal, logs and uploaded diagnostics. Do not leak the canary in the evidence itself. Authentication/authorization policy must distinguish trusted repo work from fork or otherwise untrusted PR code. Docker privileges and networking remain within the declared worker trust boundary. Never use `pull_request_target` to execute untrusted checkout code with privileged secrets as a shortcut.

Prove crash recovery across external side-effect boundaries, durable database reopen/migration, failed writes, full disk, uncertain acquisition/start/delete, and ownership conflicts. Preserve capacity until uncertain state is reconciled. Logs and diagnostics must remain recoverable after worker removal without unbounded retention or secret leakage.

## 8. P2 — Later work, still part of the goal

After the pipeline and qualification gates are sound, finish all actionable remaining items. These are sequencing categories, not a “won't fix” list.

**Evidence and documentation.** Refresh PR #18 and PR #2085 descriptions, remove stale product claims, and reconcile the original specification with implemented architecture. Preserve historical evidence with date/source rather than rewriting history. Each gate has a concrete current proof or explicit NOT_RUN/FAILED/BLOCKED status. No checkbox becomes PASS because a PR merged.

**Observability and diagnostics.** Provide concise doctor/status output for deployment identity, engine/session readiness, queue age, capacity by lifecycle state, stale work, last progress, cache disk pressure and cleanup backlog. Add actionable errors, structured redacted logs and bounded retention. Avoid an unrelated dashboard rewrite.

**Generator regression breadth.** Cover hosted/local/both/default/override modes, event/permission matrices, local composites and fresh-directory regeneration, source closure, nested runner workspace CI, workflow-size boundary, exact cache generation and cross-provider noninterference. Verify a deliberately broken test in either Rust workspace makes the appropriate Required check fail.

**Performance and resource control.** Measure queue latency, cold/warm tool and crate downloads, compile reuse, export/storage peaks, local emulation and concurrent memory/CPU demand. Remove redundant work without suppressing verification. Do not invent performance numbers or promise identical hardware behavior; report correctness first and measurements separately.

**PR #20 and other outstanding work.** Independently review qualified named Mise platform checks against current main, schema 2, paired execution, contracts, publication, and consumer needs. Extract a prerequisite only when actually needed. Sequence breaking changes with a deliberate version/configuration migration and compatibility tests; do not blindly merge a conflicting feature branch into emergency fixes. Give every relevant change an explicit disposition, including useful work that is superseded by a better implementation. Do not forget it after the high-priority fixes land.

**Maintenance and rollback.** Finish image/runner/native product update procedures, operator recovery and a generated hosted-only rollback mode. Rollback must be explicit and visible, never disguised as successful local execution. Keep Linux-host support deferred unless it is a small necessary portability boundary, not a new rollout target.

## 9. Rust design and evidence contracts

Keep canonical Rust boundaries and existing package ownership. Do not add another framework or crate merely to hold a small helper. Extend existing validated contracts and error types. Provider, identity and lifecycle states should be typed rather than inferred from strings when they govern behavior. Keep async/blocking boundaries explicit; cancellation and retries must be bounded and observable. Avoid production unwrap/expect on external data, shell interpolation of untrusted values, unbounded polling and silent catch-all errors.

The following is conceptual pseudocode: adapt it to existing types instead of copying parallel definitions.

```rust
enum Provider { GitHubHosted, VelnorScaleSet }
enum Phase { Bootstrap, Restore, Execute, Upload, PostAction, Cleanup }

struct ProductIdentity {
    source: CommitId,
    digest: Sha256,
    producer_run: WorkflowRunId,
}

struct EvidenceKey {
    repository: RepositoryId,
    source: CommitId,
    plan: PlanDigest,
    logical_job: LogicalJobId,
    provider: Provider,
    run: WorkflowRunId,
    lineage: VerifiedAttemptLineage,
}

enum GateOutcome {
    Passed(VerifiedEvidence),
    Failed(FailureEvidence),
    Blocked(AttemptedPrerequisiteEvidence),
    NotRun,
}

fn certify_job(
    expected: &ExpectedJob,
    actual: &TerminalGitHubJob,
    artifact: &VerifiedArtifact,
    policy: &AttemptPolicy,
) -> Result<VerifiedEvidence, VerificationError> {
    actual.require_success_including_post_actions()?;
    policy.verify_lineage(expected, actual, artifact)?;
    artifact.require_matching_identity(expected)?;
    artifact.require_complete_expected_results(expected)?;
    VerifiedEvidence::try_from((expected, actual, artifact))
}
```

For lifecycle cleanup, retain the existing durable-state machinery: record intent, mutate only identity-proven resources, inspect outcomes, persist verified completion, then release capacity. A Docker request succeeding is not by itself proof that all worker-owned resources are gone.

## 10. Parallel execution, integration order, and acceptance gates

Delegate independent owners for (A) live failure/source-product inventory, (B) MBX/export, (C) tar/image correctness, (D) daemon lifecycle and macOS deployment, (E) generator/consumer regeneration, and (F) parity/security verification. Independent reviewers must challenge fixes they did not author. Research/tests can proceed concurrently; serialize shared schema changes, image promotion, host deployment, consumer regeneration and destructive cleanup through clear ownership.

Integrate in this order: exact baseline → upstream P0 fixes and regressions → verified products → deployed daemon/images and regenerated consumer → hosted save/restore and local queue recovery → full paired qualification → release/lifecycle/security negatives → final main verification and promotion → remaining P2 completion. Do not run competing supervisors against one session or let subagents destructively share a Docker environment.

Use these recovery gates without overwriting older G-number meanings:

| Gate | Required evidence |
|---|---|
| R0 | Complete current source/product/run/job/failure inventory and open-PR disposition. |
| R1 | Reproduced hosted cache failure fixed; real authorized write and subsequent restore; healthy bounded disk usage. |
| R2 | Archive semantics/security regressions and genuine Actions cache round trip on the deployed amd64 image. |
| R3 | Actual daemon contains lifecycle/session fixes; N>1 and N+1 pressure, restart/cancellation and verified cleanup pass. |
| R4 | Published verified generator consumed; full generated tree deterministic; source/provenance identities agree. |
| R5 | Complete cold and warm paired consumer runs with strict job/artifact/attempt verification and real test execution. |
| R6 | Genuine Docker capability suite and actual macOS installed-product/service/security/update/rollback qualification. |
| R7 | Required checks pass on final PR and protected main; authorized write path rechecked; no unrelated checks weakened. |
| R8 | Later actionable findings completed, docs current, maintained rollback, and final independent review. |

Promote eligible trusted verification to local-default only after the applicable gates pass. Preserve hosted control and explicit hosted-only recovery. Do not silently convert the user-requested `both` comparison into hosted-only to obtain green status.

## 11. Completion and blocker contract

For every issue, report original evidence, structural root cause, implementation commit, regression test, actual live verification and remaining limitation. Final delivery must identify both repository commits/PRs, generator/native/image source and digest identities, actual host/engine/platform, workflow runs/attempts/jobs/artifacts, expected-versus-observed coverage, cold/warm results, capacity and cleanup proof, publication/installation, and rollback commands validated against the actual CLI.

Use repository-prescribed formatting, linting, tests, security checks and deterministic generation for both workspaces. Commit verified logical units frequently, push regularly, preserve other agents' work and approvals, and finish feasible independent tasks before stopping.

A missing Mac, credential, signing entitlement, authorized approval or unavailable log is not permission to manufacture evidence. Attempt a real supported probe, retain the exact failure, mark only the affected gate blocked, continue independent work, and do not repeat an unchanged failed prerequisite forever. Never claim live qualification, model selection, delegation, commits, publication or deployment that did not occur. Do not call the rollout complete with only a hosted green run, a one-slot canary, mocked Docker tests or a locally built unpublished daemon.

## 12. Source register

References are research entry points; refresh moving PRs before implementation.

- [S1] Velnor reviewed main: https://github.com/tailrocks/velnor-new/tree/5a946c33cf005777feab2bc91fa4aa8e01dd58f4
- [S2] Existing host modules: https://github.com/tailrocks/velnor-new/blob/5a946c33cf005777feab2bc91fa4aa8e01dd58f4/crates/velnor-runner/crates/velnor-runner-host/src/lib.rs
- [S3] Consumer PR: https://github.com/ChainArgos/java-monorepo/pull/2085
- [S4] Consumer config: https://github.com/ChainArgos/java-monorepo/blob/620df66423cdd2403b69ced1724d17fb8afd1d6f/.velnor/config.toml
- [S5] Consumer manifest: https://github.com/ChainArgos/java-monorepo/blob/620df66423cdd2403b69ced1724d17fb8afd1d6f/.velnor/release-manifest.json
- [S6] Current reviewed paired run: https://github.com/ChainArgos/java-monorepo/actions/runs/37128301624
- [S7] Hosted MBX failure: https://github.com/ChainArgos/java-monorepo/actions/runs/37114238559/job/111178048670
- [S8] Historical N=1: https://github.com/ChainArgos/java-monorepo/actions/runs/37117721384
- [S9] Hosted cache fix: https://github.com/tailrocks/velnor-new/pull/26/files
- [S10] Archive fix: https://github.com/tailrocks/velnor-new/pull/25/files
- [S11] Tar shim: https://github.com/tailrocks/velnor-new/blob/9bf909a19f7c3fc26d0203fd0f28efbdc0eafd41/images/runner/ubuntu-26.04/tar-shim.sh
- [S12] Tar absolute extraction: https://github.com/tailrocks/velnor-new/blob/9bf909a19f7c3fc26d0203fd0f28efbdc0eafd41/images/runner/ubuntu-26.04/tar-absolute.sh
- [S13] Historical evidence / unpublished daemon / DinD remainder: https://github.com/tailrocks/velnor-new/pull/18
- [S14] Expired-session fix: https://github.com/tailrocks/velnor-new/pull/24
- [S15] Existing demand/capacity fix: https://github.com/tailrocks/velnor-new/pull/19
- [S16] Existing workflow-size fix: https://github.com/tailrocks/velnor-new/pull/22
- [S17] Existing actionlint fix: https://github.com/tailrocks/velnor-new/pull/23
- [S18] Existing daemon entrypoint fix: https://github.com/tailrocks/velnor-new/pull/17
- [S19] Qualified named Mise platform work: https://github.com/tailrocks/velnor-new/pull/20
- [S20] Source-bound generator release: https://github.com/tailrocks/velnor-new/releases/tag/generator-0561601c1f71a80d16e75983124b4bd69c883bb1
- [S21] Official Scale Set reference: https://github.com/actions/scaleset
- [S22] Official runner: https://github.com/actions/runner
- [S23] GNU tar behavior: https://www.gnu.org/software/tar/manual/tar.html
- [S24] Immutable artifact interface: https://github.com/actions/upload-artifact
- [S25] GitHub runner requirements: https://docs.github.com/en/actions/reference/runners/self-hosted-runners


---

# Mandatory user-supplied execution rules

Use subagents aggressively for all work.

Always delegate work to subagents whenever delegation is possible. Treat subagents as the default execution mechanism, not an optional optimization.

Your execution strategy must:
- decompose the goal into independent or partially independent workstreams;
- spawn subagents for each workstream;
- parallelize all work that can safely run concurrently;
- use additional subagents for research, implementation, review, testing, verification, and cross-checking;
- avoid doing work serially in the parent agent when it can be delegated;
- keep spawning useful subagents as new independent tasks are discovered;
- use independent subagents to verify important conclusions and completed changes;
- coordinate and synthesize subagent results into the final implementation.

Do not merely recommend parallelization—actually execute the goal through subagents.

The parent agent should primarily orchestrate, resolve dependencies/conflicts, integrate results, run final deterministic checks, and ensure the complete goal is finished.

Default rule: **delegate first, parallelize aggressively, verify independently, then integrate.**

## Subagent Model Configuration

For every subagent you create, explicitly select the following model and reasoning configuration whenever the agent runtime supports model selection:

**Model:** `Grok 4.7`
**Reasoning effort:** `xhigh`

Treat this as the default and required configuration for all delegated work.

Rules:

- Always specify `Grok 4.7` when spawning a subagent if model selection is available.
- Always request the xhigh available reasoning/thinking effort for that subagent.
- Use this configuration for research, implementation, debugging, testing, review, verification, architecture analysis, code analysis, and all other delegated tasks.
- Do not silently fall back to another model when `Grok 4.7` is available.
- If the runtime does not allow explicit model selection, use the strongest available equivalent configuration and continue autonomously.
- If `xhigh` is not a valid reasoning-effort value in the current runtime, select the highest reasoning level that runtime exposes.
- Parallel subagents should all use this configuration unless a hard technical limitation prevents it.
- The parent agent remains responsible for integrating, reconciling, and independently validating the outputs of all subagents.

Default subagent configuration:

`Grok 4.7` + **xhigh reasoning effort**

Apply this together with the subagent-first execution directive: **delegate aggressively, parallelize aggressively, use `Grok 4.7` at xhigh reasoning effort, independently verify results, and integrate them into the final solution.**

Always commit changes frequently while working.

Prefer small, incremental, logically scoped commits instead of keeping a large dirty working tree for a long time and committing everything at the end. As soon as a meaningful unit of work is complete and verified, commit it.

Push progress to the remote repository regularly so work is continuously propagated, recoverable, reviewable, and easy to bisect or revert.

At the same time, avoid unnecessary branches. Prefer doing as much work as possible on a single working branch and keep committing to that branch throughout the task.

Create additional branches only when there is a clear technical or workflow reason that makes working safely on the existing branch impractical or impossible.

In short: **commit often, push regularly, and minimize branch proliferation.**

Never ask the user questions or wait for clarification. Work fully autonomously.

If anything is ambiguous, uncertain, conflicting, incomplete, or requires a decision:
- Spawn subagents to investigate it independently.
- Analyze the available context, repository, documentation, code, history, external references, and relevant best practices.
- Research alternative approaches where necessary.
- Compare multiple options and their tradeoffs.
- Verify important assumptions and findings independently.
- Re-verify critical decisions before acting.
- Make the best reasonable decision yourself and continue execution.

Do not stop because information is imperfect. Infer intent from the goal, existing architecture, conventions, documentation, and surrounding context. Prefer making a well-researched, reversible decision over asking the user.

When uncertainty is significant, use multiple independent subagents to challenge the proposed solution and resolve disagreements through evidence.

Your responsibility is to unblock yourself. Questions that would normally be sent to the user should instead become internal research, analysis, verification, or subagent tasks.

Continue working until the goal is fully completed, verified, and no meaningful actionable work remains.

## How to decide what to do

Judge every piece of work by whether it **should** be done — is it correct, is the current state wrong or inconsistent, does it serve the goal — and **never by ROI, cost, effort, or "is it worth it."** Do not label a known-wrong thing "low-value," "marginal," "an edge case," or "not worth it" to justify leaving it unfixed; reasoning by ROI is exactly what keeps work mediocre. ("The reference / competitor also gets it wrong" is a *gap* argument, not a correctness one — it never makes a wrong thing acceptable.)

The only valid reason to stop short of doing the right thing is that it **provably cannot** be done — a demonstrated limit of the model or tools, not an assumed or cost-based one. "Hard," "heavy," "expensive," or "a lot of work" is never a reason to stop; "proven impossible / blocked" is. When unsure which it is, find out — try it, measure it, prove it — before deciding, and never declare a limit you have not proven.

Present choices to me by correctness and feasibility (real impossibilities, real *capability* tradeoffs like portability or expressiveness), not by ROI.

## Fixing bugs

Assume a correct architecture has no bugs — so every bug is evidence that the architecture *permits* it to exist, not merely that one code path is wrong. **Before fixing any bug, first diagnose the root cause: ask why the architecture allowed this bug to exist at all,** and whether it is one instance of a whole *class* of bugs the same structure would keep producing.

Prefer fixes that remove the structural condition that let the bug exist — so this bug and others like it can no longer occur — over patches at the symptom layer (a guard, a special case, or a workaround that leaves the enabling structure in place). Reach for a symptom-layer patch only when the root-cause fix is **provably** infeasible or genuinely belongs in a separate change, never merely because it is larger or harder; when you do, say so and name the root cause you are deferring.

This is a thinking-first rule, not a mandate to refactor on every fix: the root-cause analysis is **always** required; reshaping the architecture to act on it is frequent but conditional on its being the right and feasible move.
