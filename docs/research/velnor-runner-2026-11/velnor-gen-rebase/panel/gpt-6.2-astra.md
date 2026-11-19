# velnor-gen rebase by GPT-6.2-Astra

| Field | Value |
|---|---|
| Model | GPT-6.2-Astra (`gpt-6.2-astra`) |
| Client | codex-cli 0.169.0 (codex exec) |
| Reasoning effort | max |
| Date | 2026-11-20 |
| Main tip analysed | `0eb2d76a5c` |
| Brief | [velnor-gen rebase brief v1](../briefs/velnor-gen-brief-v1.md) |

Independent answer from the Velnor runner research panel, written without seeing other members' answers. Unedited except for this header and replacing home-directory paths with `~`.

---

# Velnor after the generator cutover

Research panel · 20 November 2026 · repository fixed at `0eb2d76a5c`

## 1. Verdict in ten lines

1. The design stands. Make the released generator define **what must pass** and the runner prove it passed. (gen/lib.rs:38–52,76–103 · rust/required_job.rs:77–106)
2. First prove one green change through PR, queue and landed audit under enforced protection. On the pinned push both test configurations and lint failed. [Rust job](https://github.com/tailrocks/velnor-new/actions/runs/44584861409/job/135774962993)
3. The current `Required` can pass for a draft or an unlabelled ready PR without test evidence. Binding its present name alone would institutionalise that error. (R:571–625 · rust/verdict.rs:207–315)
4. Ship one persistent `velnor:verify` request, one verdict for the latest evaluated commit, and a separate, mandatory verdict for the actual merge group. (OD:18–20 · OQ:373–385)
5. The engine migration is over. The checkout has one workspace, 18 members and 18 crate-alone steps. Retire estimates built around 26 jobs or 31 steps. (`23bed02e7e`, `57b65af0db` · Cargo.toml:4–25 · R:214–267)
6. The largest remaining opportunities are the Rust critical path and repeated work. The sampled self-hosted queue median was 2–13 seconds. No heavy PR in that sample finished green. (C:52–72,148–157)
7. Reuse each proved obligation separately. A successful merge-group workflow currently says too little to discharge the landed push. (ci/merge_group.rs:45–81 · rust/docs_post_merge.rs:6–17)
8. Keep GitHub's queue, independent witnesses, exact release bytes and the Official reference. Move each job type once its observable behaviour passes conformance. (OD:18–19,31–54 · OQ:371–390)
9. Authorise Native as the default explicitly. The product currently promises the official runner for every job, including local verification. (docs/goals.md:3,16,75)
10. Publish verified merge latency and occupied runner-minutes, including early runs and cold audits. The reported 0.6-minute push-to-merge median measured a period dominated by direct merges. (C:23–46,176–198)

**Evidence notation.** All repository citations refer to `0eb2d76a5c` unless a commit or a GitHub observation is named.
`R`, `CI` and `REL` denote `rust-ci.yml`, `ci.yml` and `release-rust-ci-tools.yml` under `.github/workflows/`.
`model/`, `rust/`, `render/`, `gen/`, `ci/`, `local/`, `cache-rules/` and `runner-{core,github,host}/` expand to `crates/velnor-<name>/src/`. The exception `local/README.md` refers to the repository-level local harness.
`Arch` and `UX` are `docs/proposed/velnor-runner-architecture.md` and `velnor-user-experience.md`.
`Plan`, `OD`, `OQ` and `Evidence` are `delivery-plan.md`, `owner-decisions.md`, `owner-questions.md` and `evidence.md` under `docs/research/velnor-runner-2026-11/`.
`A` through `E` are the supplied `regen/{A-generator-as-built,B-runner-as-built,C-measurements,D-inventory,E-plan-audit}.md`, respectively.
Names introduced as proposals below identify work to build. Current symbols have source locations.

## 2. The design mapped onto velnor-gen

### The compiler and its contract

`velnor_gen::expected_with_notes` already detects repository facts, checks helper capabilities, plans Rust CI, renders it and adds declared workflows and releases.
`velnor_rust::plan` builds a `WorkflowPlan` containing typed jobs, steps, conditions, triggers and permissions. The renderer enforces action pins and command rules.
The single-job plan already embeds planning and exposes a verdict specification. Preserve this production path and its goldens. (gen/lib.rs:38–52,76–103 · gen/helper_gate.rs:8–64 · rust/plan.rs:41–129 · model/plan.rs:22–103 · render/rules.rs:16–40,233 · rust/required_job.rs:77–151)

Add `VerificationContract`, `ObligationId`, `ObligationSpec`, `SubjectSpec` and `ReuseClass` to `velnor-model` in the root workspace.
Generate a versioned deterministic contract alongside the workflows. Include the base policy epoch, stable obligation IDs, variant, executable definition, input rule, expected evidence, event scope, dependencies and trust floor.
Hash canonical bytes with explicit field lengths, schema version and domain. Derive display names and diagnostics from IDs. Renaming a step should not invalidate an unrelated proof.
Extend `velnor_rust::plan` and `required_job::spec` to produce the contract and its `VerdictSpec` projection in one pass. Preserve the actual `docs`, `check_alone` and feature-lane predicates. (model/verdict.rs:65–80 · rust/required_job.rs:77–106 · .velnor/rust-ci.toml:4–32)

Extend declared `WorkflowsPlan` and the release planner to contribute obligations through the same model. The four handwritten policy jobs must enter this inventory before they can be required.
Give every existing check a mapped ID, an explicit advisory disposition or an owner-approved removal. Cover Alint's source limit and sparse checkout, Cargo Deny, both scopes of Cargo Machete and Jactionlint.
The release workflow for these tools is handwritten. Consumer release generation and the local declaration language are distinct compiler entry points. (crates/velnor-workflows/src/lib.rs:22–29 · crates/velnor-workflows/src/helper_call.rs:6–38 · CI:41–156 · R:480–485 · REL:86–151 · render/release.rs:31–42)

`StepKind` additions proposed here are `ResolveContract`, `RunObligation` and `ReportEvidence`. Render each as one released helper command or qualified native operation.
Retain `StepKind::GeneratorCheck` for freshness. Extend `RustCiConfig` in `model/repo_config.rs:11` with a typed request policy and default/per-job-type executor choice in `.velnor/rust-ci.toml`. Reject unknown keys and unsupported helper versions.
Keep deployment topology, credentials and mutable receipt lookup out of generation. Keep workflow logic in Rust, preserve detached generation and check the generated-files set for deletion as well as drift. (model/steps.rs:29,108,159 · rust/config_keys.rs:4–38 · gen/helper_gate.rs:8–64 · gen/io/mod.rs:121–128 · docs/goals.md:69–76)

### Identity, evidence and authority

Generator time can hash the template for a verdict key. `velnor-ci resolve-contract` must materialise the actual subject and execution facts. A generator running on yesterday's checkout cannot know today's test binaries.
Use `ResultKey` for one obligation and `VerdictSubject` for the repository, actual tested SHA/tree, target/ref, approved contract, selected obligation set and epoch. Keep PR head, PR test merge and merge-group HEAD in separate fields.
Record which of them was checked out and verify the tree against the requested subject before execution. If Git metadata, commit stamps or the target ref are visible inputs, include them in the key. (ci/io/git.rs:36–79 · REL:140–151 · ci/io/mod.rs:108 · Arch:54–101)

Default key inputs include the entire materialised tree with modes and symlink targets, locked dependencies, toolchain and helper digests, configuration, feature/profile/target, Cargo and test arguments, executable and fixture digests.
Include the observed executor, image and execution environment. Constrain or bind CPU features, kernel, environment, seeds, locale, network and clock when relevant. Identify submodule or LFS contents if admitted.
A test receipt binds the actual shipped binary or archive member, every invoked auxiliary binary (including `CARGO_BIN_EXE_*`) and the exact filter and discovered tests.
Scheduling priority, the requester and early/late execution may stay out only when invisible to the check. A live service, variable clock or unclassified read puts an obligation in the per-request/fresh class C5. (Arch:54–117,485–556 · OQ:375–390 · docs/goals.md:34–44)

Introduce `velnor-ledger` as a workspace library and service. Use immutable `Observation` and `Receipt` records indexed by `ResultKey`, witness identity, independence domain, generation and policy epoch.
`velnor-ci` collects the execution manifest. A trusted supervisor, outside the checked-out process, observes completion, signs it and persists it. Place the supervisor in the runner service, start the job with an unprivileged identity and keep signing/App credentials outside the job. Treat job-owned outputs as claims to cross-check, even when a released helper is invoked after project code.
Validate inventory equality for selected runnable tests, treating intentional ignores by the declared test policy. Missing, unexpected, duplicate or zero tests in an expected nonempty lane fail closed.
The existing helper wires `velnor_rust::decide(spec, needs, event)` into `velnor-ci gate`. Extend this pure boundary to return missing/failed IDs and verified evidence. Preserve its existing job, package-count and unexpected-skip checks. (rust/verdict.rs:51–68,172–202,207–315 · ci/io/required.rs:18–45 · docs/goals.md:40–43)

### The App and the label

Proposed `velnor-control` owns signed webhook admission, a durable request log, a reconciler and a single fenced status publisher. `RequestState` belongs to (repo, PR, head, base context, generation).
`Velnor / Verified` is one App **commit status**, bound in the ruleset to its App ID. An installation ID, Actions integration ID or release App identifies a different principal. Only `success` after the required evidence allows merging.
Use a nonrequired `Velnor / Details` check for live obligation results, logs, cache observations and cancellation reasons. The generated workflow never holds the App key. (OQ:96–108,371–385 · docs/goals.md:40–43,67)

| Event | Required transition |
|---|---|
| Draft/open or ready without a fresh request | No verification jobs, including the handwritten lints. No new passing status. |
| Add `velnor:verify` on a ready same-repository PR | Arm once, record head, base and epoch, submit the missing obligations. Duplicate deliveries are idempotent. |
| Push on an armed PR | Reconcile the current head, obsolete the prior request immediately, cancel unshared work and schedule the new head. |
| Remove label, convert to draft or close | Disarm and revoke pending execution claims. An already verified commit retains its evidence. A later head needs its own success. |
| Mark ready with a draft-carried label | Require a fresh remove/add or explicit `velnor ship` request. Becoming ready alone starts nothing. Owner choice D16. |
| Edit title/body or another label | No verification. On base retarget, re-evaluate the subject and invalidate incompatible evidence. An armed PR may verify the changed context. |
| Create/update merge group | Always verify the exact GitHub group SHA and base, independently of PR labels. A replaced group cannot publish for its successor. |

Change `PrRun`, `PullRequestType`, `Concurrency`, `quick/full`, draft guards and `label_reset` together. Delete `ci:run`, `ci:auto`, one-shot removal and its write permission at cutover.
Today `unlabeled` is missing, `edited` exists and the generated `Required` skips its gate on drafts. Handwritten `ci.yml` also subscribes independently. (model/plan.rs:32–62,116–145 · rust/plan.rs:330–345 · rust/plan_budget.rs:49–68 · rust/report.rs:94 · R:4–24,571–625 · CI:4–14)

Deduplicate signed deliveries, reconcile authoritative GitHub state and missed transitions every ten seconds, and keep a monotonic request generation. Invalidate on policy reduction or epoch revocation.
Exactly one fenced publisher can call the GitHub status API. A database lease alone cannot retract an in-flight HTTP write. Serialise per SHA, drain an old writer, read back the latest producer/state and hold uncertain publication.
Test lost replies, delayed success after revocation, same-SHA concurrent PRs, base changes and replacement groups. GitHub exposes a shared status per SHA. Restrict this gate to the protected base, or satisfy the union of all live base-policy contexts for that SHA before publishing. Lab-test which head/test-merge SHA GitHub actually requires. The group separately verifies integration against the current base.
Verify label delivery from the actual agent identity. Some events created by a workflow's `GITHUB_TOKEN` do not trigger new workflows. (OQ:373–385 · Arch:103–117 · [GitHub workflow triggers](https://docs.github.com/en/actions/how-tos/write-workflows/choose-when-workflows-run/trigger-a-workflow))

### From an agent's tree to a release

| Boundary | Work and admissible evidence |
|---|---|
| Local edit → `velnor verify` | Canonicalise an explicitly submitted snapshot, including admitted untracked files and deletions. Store an immutable private ref and manifest. A native Mac run is T0 advice. Sealed Linux on a Mac can supply T1 comparison data. |
| Snapshot → armed PR | Two independent T2 cluster witnesses can qualify the identical materialised subject, with separate nodes, private workspaces, order seeds and attempt IDs. Join in-flight equivalents. A changed input schedules the missing work. |
| PR → group | Recompute selection and keys on the actual group checkout. Reuse matching receipts within their trust/freshness limits. New contributors, merge resolution or C5 obligations execute. The group gets its own status. |
| Group → landed `main` | Resolve every landed first-parent change, including intermediate changes in a batch. Run A1 cold and independently select/check its audit obligations plus the push-only remainder. Establish T3 only after A1 passes. |
| `main` → release | Authorise the peeled tag against an exact landed success, push audit and release manifest. Promote the already built immutable bytes. Schedule A2 independently with full fresh coverage and time variation. |

T2 witnesses must differ in failure domain and execution order/generation as prescribed. Keep at least one cold until compile reuse itself qualifies.
A1 is a mandatory independent third observation of the landed subject. Reserve audit capacity per site, alarm at two hours of lag and hold trust advancement, warm compile reuse and releases at six hours. A2 is an additional full, cache-independent audit. It can expose a blind spot shared by planner and ledger.
Qualify each reuse kind with 3,000 clean representative comparisons, then sample at least 2% cold in steady state. A mismatch blocks the kind, revokes its epoch and affected descendants, and re-verifies cold before reissuing statuses.
Cold-run a miss, partial hit or stale observation. PR deltas can accelerate execution within their scope. They never admit a candidate binary into trusted main storage. (Arch:485–556,558–625 · OD:16–20 · docs/goals.md:40–43,72)

Keep `velnor_rust::select` and `select_event` as the authoritative producers of `RustSelection`. They decide which work the change affects. (rust/select.rs:17–43 · rust/select_event.rs:12–38) Define `Disposition::{Excluded(reason), Execute, Reused(receipt)}` on selected obligations. Never coerce a cache or ledger hit to `RustSelection::None`.
`merge-group-passed` records workflow-level success. Keep the repository's shortcut disabled until the migration deletes it and replaces all callers with per-obligation proof. (model/selection.rs:45–100 · ci/merge_group.rs:45–81 · rust/merge_group_skip.rs:9–56 · rust/verdict_plan.rs:94–105)

For releases introduce `ReleaseSubject` and `ReleaseManifest` in `velnor-model`, constructed by `velnor-release` and rendered into a read-only builder and a no-checkout publisher.
Prepare the release profile, target, source/version stamp, dependencies, builder digest and exact artifact IDs at the release-candidate group. The landed audit must bind the same subject. Give `velnor-ci release-gate` distinct typed requirements for the App status and the push-audit producer. The present gate rejects an external integration as a source of push jobs. (ci/gate.rs:141–172,292–301)
The tools embed `VELNOR_COMMIT`. If a squash changes that input, a matching checkout tree alone cannot prove the two archives equal. Prepare under the true release identity before tagging, or hold the release for a new qualified build.
A read-only smoke job executes the exact x64 and arm64 assets and records their digests. The publisher resolves the tag, checks producer/run/attempt and smoke receipts, rehashes the downloaded bytes itself, and makes retries idempotent by verifying existing uploads.
If an artifact is missing, hold promotion. A write-capable job must never rebuild or execute repository code. `runner-v` needs an explicit supported asset roster or an owner-approved retirement. macOS release builds stay paused. (REL:86–105,106–169,187–253 · Cargo.toml:113–118 · release-plz.toml:45–105 · render/release_attest.rs:60–75 · docs/goals.md:76–77)

### Local tools and the two executors

Give `velnor verify`, `velnor ship`, `velnor cargo` and the `velnord` daemon a thin client in the same Cargo workspace.
`verify` resolves the released contract, snapshots the complete tree at an explicit request, submits both remote slots and streams results by obligation. It may prepare the synthetic queue-tip merge in advance, identifying both source trees. When the actual group appears, resolve its true SHA and compare keys again. `ship` binds that evidence to the pushed subject and requests verification on GitHub.
Expose the daemon as `velnord` (the brief's “velnorD”). `velnor cargo` initially counts command shapes, cost and coverage locally. Keep exactly one Cargo wrapper. Only route a recognised invocation to a counting remote run after a replay proves identical command, working directory, feature set and environment.
Keep ordinary Cargo exit status, output, interruption and local files usable. Charge remote work even if the tree is abandoned. Unpushed snapshots have explicit scope, retention and deletion.
`velnord` owns the local socket, snapshot/claim lifecycle, and reconnection. The remote node hashes what arrived. Only a node-observed T2-or-better execution can fill a witness slot. (mise.toml:19–24,47–48 · Arch:966–976 · OQ:377–390 · local/README.md:3–10,91–100,127–139)

`velnor-local` already offers a GitHub stand-in, scenario replay and a cache adapter. Preserve their observable semantics while replacing callers as each route qualifies. Use locally recorded events as replay input. A qualifying witness issues the proof.
Record a real `velnor-new` scenario, normalise only declared nondeterminism and compare job results, outputs, cancellation and post steps against real GitHub. The local harness has known gaps around needs and tokens. (local/README.md:91–100,127–139,230–243 · B:139–239 · E:545–548)

Propose `Executor::{identity,preflight,run}`, `ExecutorId` and `ExecutionRole::{Authoritative,Reference}`. The dispatcher resolves them from trusted job-type configuration and includes the identity in each key.
Velnor Native executes the qualified closed set in Rust. Reject unsupported action revisions, inputs, expressions and services *before* acquisition can count as execution. It needs broker/run-service lifecycle, timeline/Results, artifacts v4 and OIDC in addition to action implementations.
GitHub Official is a deliberate authoritative choice for a configured job type. Official in reference role writes only comparison data, with separate sinks and credentials. An executor failure fails that request. Never silently reroute a failed job.
The Rust runner already implements scale-set polling/acquisition and JIT paths and can launch the Official runner. The historical cluster snapshot shows ARC launching the official C# runner. The Rust host's listener and Docker launcher handle a different part of the lifecycle. Broker/run-service execution and a Velnor-managed production dispatcher remain to build. The repository pin is `2.338.0`. (runner-github/paths.rs:9–19 · runner-github/poll.rs:138,385 · runner-github/acquire.rs:16–41 · .velnor/runner.lock:14–23 · B:61–133 · OD:34–54)

Use the generated action allowlist to enumerate candidates for conformance. Generate a machine-readable manifest of exact action SHA, input variants, pre/main/post hooks, expression contexts and service requirements, across Rust, declared and release workflows.
Compare it to Native's capabilities at generation and again at dispatch. Replace actual `github-script` behaviours with qualified typed Rust helpers before switching a class. Qualify Official against the same observable contract. (render/rules.rs:16–40 · REL:20–45 · .velnor/workflows.toml:1–18 · E:299–309)

### Platform, storage and cache services

| Component | Placement and change |
|---|---|
| Platform | Add `velnor-platform` with `EnvironmentSpec`, `Capabilities`, `provision`, `observe`, `cancel` and `destroy_owned`. Dispatcher schedules by CPU, memory, isolation, mounts, network and lifetime. Docker and Kubernetes implement it. |
| Lifecycle | Preserve `Present / AbsentProven / Unknown`, immutable resource IDs, ownership proofs and intent-before-effect journalling. Deletion needs the same observed UID. Unknown existence holds a slot through reconciliation. |
| Dispatcher | Reuse the scale-set protocol crates and tested capacity invariants. Replace the launch path per qualified job type. Drain owned jobs before deleting their old controller. The Mac Docker local product still needs lifecycle recovery. |
| Storage | New `velnor-storage` with `BlobId`, `put_if_absent`, `get_verified` and publication of an immutable manifest. Specify capabilities and a single authoritative writer for the mutable index. |
| Routing | L0 is node NVMe with read-only bases and private copy-on-write worktrees. L1 is a RustFS installation per site. L2 asynchronously copies immutable data across sites. Local Docker can use disk alone. |
| Failure | A missing or corrupt cache blob is an observable cold miss. Missing release bytes hold publication. Verify length/digest at read, atomically publish only complete sets, protect live manifests from collection, test partition and recovery. |

#1324 defines a proposed platform boundary. Port its observation semantics and review the current Docker client, capacity/ownership logic and journal v58. Migrate the recovery records needed to preserve those proofs and delete drained host-specific state.
Start with a single-writer home site. A stale read from a replica must never authorise trusted writes, witness promotion or the latest status. Test lost writes, split ownership and node replacement.
Evaluate deployment in the operator's infrastructure repository. The ARC/k3s node and volume figures date from 18 November. Reinventory the sites before sizing the production rollout. (docs/proposed/kubernetes-runtime.md:12–18,81–114,181–201 · runner-host/docker_client.rs:98 · runner-core/capacity.rs:45,108 · runner-core/ownership.rs:31 · runner-host/reconcile.rs:138–169 · runner-host/journal/schema.rs:26 · Evidence:533–576)

In operator deployment config propose `StorageConfig { backends, tiers, routes }`, keyed by data class. Support local filesystem, single- or multi-node RustFS and compatible S3 services with verified capabilities. A public hosted consumer can still use GitHub's cache within the existing budget. (OD:70–97 · docs/goals.md:45,49–59)

For native `actions/cache` retain the existing `CacheKey` model and `access_for`, `lookup`, `reserve` and `evict` rules. Move the local v2 adapter onto a shared implementation after parity.
Index by repo, ref, key and cache version, exact then documented prefix/default-branch search within allowed scope. Reservation/finalisation must be atomic across concurrent writers. An abandoned upload is never a hit.
In the new runner-side `velnor-cache` service, implement `reserve_upload`, `finalise` and `resolve_restore` over `velnor-storage` and `velnor-cache-rules`. Preserve `cache-hit` and matched-key outputs, split restore/save semantics and cross-OS compatibility by exact cache version. Implement multipart/block upload, digest verification, limits and eviction against sealed blobs. Partition user cache, verified generations, MBX, artifacts v4 and retained release objects by permissions, lifetime and index.
Native calls the service directly. For Official, lab-test a documented/configurable endpoint with the pinned runner and v2 action, short-lived per-job grants, range and retry behaviour. Verify which endpoints the action actually contacts.
Prefetch a cache candidate only when its key can be evaluated from the exact pre-step workspace. Resolve other keys when the action runs. Prefetch cannot create a visible hit ahead of finalised index state.
If that integration cannot be qualified, keep Official on its supported service for that role. Tie comparative cost claims to the measured route. (model/cache.rs:95–115 · render/cache.rs:20–61 · cache-rules/access.rs:21–37 · cache-rules/lookup.rs:8–41 · cache-rules/reserve.rs:10–22 · local/protocol.rs:22–40 · local/store.rs:104–170 · B:277–302)

In a proposed runner-side `velnor-node` crate put `GitMirror::materialise`, `GenerationStore::clone_verified` and the trusted MBX seed mount. Allow per-site/on-node enablement and capacity. A node Git mirror is new. Fetch/lock by requested object, verify final SHA and hash the **materialised** full tree before running a step. Handle sparse paths, executable modes, symlinks, deletes and stale concurrent mirrors. Fetch a missing object from its authorised origin.
The sparse crates mirror is already configured. Keep lock/checksum validation, scoped credentials for allowed private dependencies, token revocation before project code and Cargo offline after fetch. Extend the existing tool store before adding a second installer.
For warm snapshots propose `GenerationManifest` for toolchain/target/profile/path plus content, dep-info, read closure and output digest. Mount a trusted base read-only at a stable path. Give each witness a separate writable overlay.
Restore mtimes only for content proved identical. Treat deleted inputs, negative lookups, build-script/proc-macro side effects, generated files and absent provenance conservatively. Stage stale-cache mutation tests against a fully cold independent build.
Current PR deltas, target/source/tool caches and restored mtimes already save work. The PR overlay is cumulative and size bounded. Benchmark each separately. Require separate test evidence for a verdict. (ci/io/crates_mirror.rs:90,129 · .velnor/rust-ci.toml:6–14 · docs/rust-ci-cache-keys.md:7–14,38,53–73,133–141 · B:284–310 · docs/goals.md:40–45)

MBX already owns Cargo wrapping. `mbx_shared_store` allows trusted default-branch/scheduled writes and a read-only seed for PR/group use. Preserve that separation when enabling any MBX service.
Measure served units, bytes, miss cause, restore overhead, verifier CPU and critical-path savings against current node storage and generation-only runs. Key the MBX experiment by actual compiler and build settings.
Qualify a new backend against a cold independent build and require measured net benefit. If it wins, migrate users and retire the superseded cache route in the same cutover. (mise.toml:19–24,47–48 · ci/mbx_store.rs:32–38 · generator/CONFIG.md:109–119 · R:91–204,300–403)

## 3. Correctness first

Ranking follows the harm of an incorrect merge or release. “Owner” means a policy, identity, ruleset or infrastructure decision. Repository repairs and read-only measurement can proceed immediately.

**1 · Stop uncontrolled landing and restore a green main.**
The live `protect-main` requires only unbound `Required`, has an admin-role PR bypass, uses HEADGREEN and has auto-merge disabled at the repository.
Of 173 merges, 157 were classified direct and 167 had a failure in the head's `Required` rollup, including mixed attempts. The counts overlap. C records the merging account and infers use of admin bypass. The individual bypass events and final-attempt decisions lack audit-log confirmation.
Operationally freeze ordinary merges on red now. Reproduce each tip failure, keep the checks, repair the causes and get green PR, group, squash-push and policy evidence for the repair. Owner enforces the freeze and rules. Agents fix code.
The pinned Rust job failed lint, tests, local-mode tests and local-mode lint. In the other workflow, Alint and Cargo Deny failed while Cargo Machete and Jactionlint passed. The log identifies unused test helpers/imports. Use independent failing steps to find the remaining causes. (C:23–46,159–174 · crates/velnor-ci/tests/it/prepare_inputs.rs:9 · crates/velnor-ci/tests/it/prepare.rs:67–69 · [tip Rust job](https://github.com/tailrocks/velnor-new/actions/runs/44584861409/job/135774962993) · [tip CI](https://github.com/tailrocks/velnor-new/actions/runs/44584861503) · [ruleset](https://github.com/tailrocks/velnor-new/rules/24396608))

**2 · Make “unrequested” incapable of passing the merge gate.**
Today's `pr_ci = "label"` allows an unlabelled quick pass, and draft handling can complete `Required` with a notice. The one-shot reset has repository write scope. Quick feedback cannot authorise a merge.
Implement the state machine in section 2 across every workflow that starts heavy PR work. A transitional always-evaluated, control-only `Required` must explicitly fail on an unrequested/unproved head. A skipped required job can appear successful.
Move dispatch outside PR Actions when the App is ready so draft/ready alone start zero CI. Test both cancellation and the race with a recently published green. Owner selects the label and draft rule, agents implement and migrate.
A fully proved exact SHA can remain green when the label is removed. Re-evaluate on every new head or invalidated policy epoch. (R:4–24,571–625 · CI:4–14 · .velnor/rust-ci.toml:4–9 · OQ:373–380 · [GitHub job conditions](https://docs.github.com/en/actions/how-tos/write-workflows/choose-when-workflows-run/control-jobs-with-conditions))

**3 · Require an authentic producer and an exercised merge queue.**
Interim owner action, once the repair/control gate is usable, is to bind the *existing single* `Required` to Actions integration `15368`, remove bypass, enable auto-merge and ALLGREEN, depth 2, minimum wait 0, timeout 90 minutes.
Binding to Actions identifies the integration. Multiple workflows share that integration ID. Demonstrate on the lab that the admitted trusted workflow, base policy, exact subject and latest attempt control success. If that cannot be enforced, keep landing frozen and advance the App cutover.
Arm on the PR, enqueue its green latest head, observe GitHub create its real group ref, require group success, then independently audit the landed SHA. Keep group and PR status coverage during the atomic rule change. Exercise queue replacement and failure.
Owner owns settings, the new App and the final swap. Agents supply reproducible lab evidence and before/after settings validation. (ci/gate.rs:15,57–104 · C:159–174 · OQ:20–36,96–108 · [ruleset](https://github.com/tailrocks/velnor-new/rules/24396608) · [GitHub merge queue](https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/configuring-pull-request-merges/managing-a-merge-queue))

**4 · Close the release-authorisation hole.**
The tools gate checks ancestry. Its hosted publish path already rehashes archives against SHA256SUMS. The self-hosted-signer restriction appears in the generated consumer attestation verifier. Preserve the existing archive check and bind its manifest to the authorised build. Require a successful default-branch **push** for the exact peeled tag SHA, expected workflow path and current attempt, accepted proof of each required job, plus the current App verdict once adopted.
Use the released `velnor-ci release-gate` policy, with typed Actions-job and App-status requirements. A skipped job, a success on a PR/other SHA or an older rerun cannot substitute. Bind immutable artifact IDs/manifests to the gated build and verify again in the publishing job.
Finish the surviving #1608 work now. Keep current hosted Linux builders and signing restrictions pending Q-N5, and owner approval for tag authority or `runner-v` changes. (REL:86–105,187–253 · ci/gate.rs:57–104,141–172,292–301,353–392 · render/release_attest.rs:60–75 · generator/CONFIG.md:557–573 · docs/goals.md:77)

**5 · Verify the generator and its policy from a trusted release.**
The `GeneratorCheck` variant and renderer exist, but the planner emits no check. The renderer invokes `velnor-gen check`. The `setup-velnor` action installs only `velnor-ci`. The lock already records the generator artifact.
Add an immutable released generator installer/invocation and emit the check even for eligible non-Rust changes. Cover added/removed workflows, inputs, contract and stale header independently of selection. The rendered digest detects drift. The separate trusted base and release identity establish authorship.
The released planner currently reads checkout configuration. Resolve governing policy from the trusted base and require explicit review for obligation reductions. A candidate generator can test its own goldens, but it cannot define its own passing gate.
Bootstrap new self-hosting behaviour through a separately released candidate, lab proof and lock/regen update. Existing source can be ahead of the tool pin. Owner approves policy reductions. Code/pinning work follows normal release procedure. (model/steps.rs:108 · render/steps.rs:279 · render/helper_steps.rs:74–79 · actions/setup-velnor/action.yml:1–12,97–107 · .velnor/rust-ci.lock:1–14 · render/header.rs:20–45 · ci/io/plan.rs:38–53,80–92 · docs/goals.md:76–78)

**6 · Cover the exact obligation set on each event.**
`decide` checks results and counts. Positive package totals cannot demonstrate that every selected test ID ran exactly once. Capture expected inventory, shard partitions and observed terminal outcomes per variant, including `local-mode`.
Generated policy currently follows the Rust job. Handwritten CI carries independent checks. Establish a union and compare scopes, lockfiles, targets and flags before consolidating anything. Keep advisory `Cache` visible without promoting it to a merge blocker.
`docs = "post_merge"` and `check_alone = "post_merge"` permit group success without those push checks. The contract must list which checks prevent landing. Move those into the group before calling that set fully verified, and run the independent push remainder. The owner decides any change to the project's configured timing. (rust/verdict.rs:172–202,207–315 · rust/docs_post_merge.rs:6–17 · R:205–299,406–505,571–664 · CI:41–156 · docs/goals.md:41,67)

**7 · Preserve conservative selection and make degraded input visible.**
`fetch-depth: 2` can omit a base on a long push or multi-entry group. Distinguish a missing diff base from a checkout of the wrong head. Fetch/deepen the exact permitted base and head with a bound. If the base is still unavailable, select wide with a reason. A wrong head fails.
An unclassified workflow path conservatively selects everything. This can cost time. Track top paths and fallback counts, prove actual readers, and register precise mappings. A repository test reads the tools release workflow.
`non_inputs` and explicit mappings are consulted before some global classifications. Disallow policy/contract/workflow changes being hidden by head-supplied mappings. Owner approves reductions, engineering owns the fetch and reason counters. (rust/plan.rs:37,183–202 · ci/io/git.rs:36–79 · ci/io/plan.rs:166–182 · rust/classify.rs:29–57 · rust/select.rs:204,274 · crates/velnor-gen/tests/it/release_workflow.rs:13–20 · docs/goals.md:65–71)

**8 · Remove implicit policy switches and reject unsupported reuse.**
The surviving `.velnor/config.toml` has Rust input mappings and a `[stacks]` wrapper. Its existence also sets `engine_configured`, which changes drift handling for unmarked workflows. Removing that file changes which workflows the generator judges.
Inventory real callers of the older plan-request commands separately from `velnor-ci plan --explain`. Move useful mappings into typed configuration, explicitly declare workflow ownership, then delete the file, old reader and stale drift exception together.
Leave `skip_push_after_merge_group` off. A completed successful workflow on the SHA can lack documentation, crate-alone or a fresh full selection after cache expiry. Only verified obligation coverage can discharge such work. No owner action to preserve the safe default. (.velnor/config.toml:4–14,43–50 · gen/io/mod.rs:174–194 · gen/drifts.rs:65–75 · ci/plan_request/diff/config.rs:4–14 · ci/io/plan.rs:38–44 · ci/merge_group.rs:45–81 · rust/merge_group_skip.rs:9–56 · R:194–204)

## 4. Speed on the new baseline

### What was measured

C observed 68.5 hours before cutover and 4.3 hours afterwards, from 13:31 to 17:50 UTC on 19 November. The pinned tip came later. Several sampled runs still had separate root jobs.
C's aggregate duration is run start to last update on completed successes/failures. Its heavy-run metric is creation to last job end, including cancelled heavy PRs. Timing a protected green verdict requires the authenticated status event. (C:10–18,52–72,95–111,192–198)

| Population | n | P50 / P90, minutes | Interpretation |
|---|---:|---:|---|
| Post-cutover Rust PR, all runs | 268 | 1.1 / 5.8 | Two workflow successes, 143 failures, 108 cancellations, 15 skips. |
| Post-cutover Rust push, all runs | 172 | 1.2 / 9.7 | Four successes, 166 failures and two incomplete. Successful light changes do not time a full Rust pass. |
| PR where a heavy lane started | 141 | 2.9 / 6.2 | 58 failures, 83 cancellations, zero successful heavy PRs. |
| PR with every heavy lane omitted | 127 | 0.8 / 1.2 | 85 still failed. The advisory cache tail can outlast the required verdict. |
| Push where a heavy lane started | 74 | 7.5 / 11.3 | 72 failures. More push obligations, especially docs and independent crate checking. |
| Last-push proxy → merge, pre/post | 187 / 172 | 121.8 / 700.8 → 0.6 / 3.8 | Different merge regimes. The older measured proxy of 85 / 457 came from a different window. No post-cutover enqueue in C. |

The old hosted queue has already been removed from Rust CI. All 1,584 PR and 1,076 push sampled jobs were self-hosted. Queue medians ranged from 2 to 13 s, with P90 up to 57 s. Historical `rust-root` execution was 67/365 s on PRs and 371/560 s on push.
Rust CI consumed **6.4 summed workflow-wall minutes per merge** in C, 2.5 on PRs plus 3.9 on push and zero on groups. Measuring CPU or occupied runner-minutes requires per-job service records. The previous engine measure was 117 summed minutes with all three contexts.
The 41 merges/hour after cutover and 2.7/hour before describe all merges. Throughput and three-leg latency under enforced healthy protection remain unmeasured. (C:23–46,148–157,176–198)

At the pinned tip the released helper plans within the single `rust` job. Affected-package selection narrows which tests execute through the nextest `-E` filter. Clippy and the test build cover the workspace. Doctests also use workspace scope when emitted. Each feature/target/profile is a distinct compile configuration. (R:205–210,268–299 · docs/hard-decisions.md:486–513 · docs/rust-ci.md:22–43) It restores tools, sources, target and delta, then runs workspace lint, docs, crate-alone checks, build/test and the filtered feature lane.
Generated `policy` waits on outputs from `rust`. `Required` needs both. `rust-trim`, label maintenance and advisory `Cache` are separate. The four handwritten lint jobs run in a parallel workflow with different coverage. (R:34–90,91–204,205–299,300–403,406–505,506–664 · CI:41–156)

One failed push at the tip gives a more recent critical-path sample. The Rust job ran **305 s**, and failed `Required` finished **322 s** after workflow creation. The separate `policy` job was skipped and `Cache` finished green. Sources restore took 5 s and target restore 1 s.
Lint took 31 s, default docs 31 s, four crate-alone steps together 52 s, both failed test steps 12 s each, the feature guard 62 s, feature lint 26 s and feature docs 46 s. Docs plus guard accounted for 139 s.
This single failed push is evidence to instrument discovery and documentation. It does not price all 18 checks or predict a green labelled PR. [Pinned Rust job and step times](https://github.com/tailrocks/velnor-new/actions/runs/44584861409/job/135774962993)

### Five levers ranked by minutes saved per protected merge

Unit for ranking is net **occupied runner-minutes** per successfully landed, fully verified change. Sum all attempts from explicit agent work to group, A1 and the allocated share of A2. Record PR and merge wall latency alongside it.
The numerical examples are marginal planning assumptions for a future protected workload. Charge each avoided attempt once. Compare identical obligations and witness policy and rerank with paired green traces. The queue contribution in C itself is zero.

1. **Join identical claims and reuse proved obligations across stages · illustrative 3.4 min/merge.**
   Assume 0.70 group matches × 3.0 occupied minutes and 0.80 landed overlaps × 2.0 minutes of otherwise repeated *eligible* work, less 0.3 for lookup, transfer and sampling. A1, push-only checks and fresh C5 still execute.
   Instrument template matches, materialised-key matches, denied hits and elapsed service per slot. Replay the same head/group/landed triples with lookup advisory, then compare occupied time with audited hits. Charge any work moved into `verify` once.
   Count a lower bound of zero for a cohort with no equivalent key. Never project the old queue multiplier to today's unprotected merges. (Arch:519–529,546–566,614–624 · C:176–189)

2. **Stop unrequested and superseded work at admission · illustrative 2.0 min/merge.**
   At 0.8 avoided unshared attempts per landing × 2.5 minutes still to run, the marginal saving is 2.0. Shared claims retained by another request are excluded. Some of this saving improves queue latency for others.
   Join webhook event, change-of-head, cancellation signal, last child exit and capacity release. Compare the same push sequence under the old and persistent label policies. Publish P90 stale slot-seconds and per-PR wasted CPU as well as the mean.
   C observed 83 cancelled heavy PR runs. Their remaining unshared service time still needs measuring. (C:56–72,188–190 · R:4–24,613–625)

3. **Make surviving compilations genuinely warm · illustrative 1.2 min/merge.**
   If 1.2 paid compilations per change each avoid a net minute, the saving is 1.2. Include overlay restoration, hashing, storage traffic, twin generation costs and mandatory cold checks.
   Use a randomised paired matrix of current target/PR delta, MBX node seed, verified snapshot and cold. Compare unit counts, first Cargo time, bytes and final binary/test results. Repeat on edits, added/deleted paths, changed toolchains and feature flips.
   Begin with the already deployed delta and mirror controls. Plot expected versus actual compiler work as reported by `Cache`. Native storage earns its place against those current controls. (R:91–204,300–403,627–664 · docs/rust-ci-cache-keys.md:53–73,133–141 · B:284–295)

4. **Avoid repeating discovery and builds inside each surviving configuration · illustrative 0.6 min/merge.**
   At 0.8 affected landed changes × 0.75 net minute saved, discovery reuse or one archive per qualifying configuration buys 0.6. Keep the witness builds independent. Within a witness all test groups can read its same archive.
   The filtered feature guard lists the default and feature inventories and the filtered result. Reuse only exact discovery inputs and compare the full set of IDs, ignore flags, failing and empty lanes. Record the wall time and work of cancelled/failed list commands.
   #1748 overlaps listing with running after a start signal. It is in the pinned source. The installed `tools-v0.1.95` release precedes it. Price the released change before attributing a second saving. (ci/lane_guard.rs:28–58 · ci/io/pump.rs:77 · ci/io/early_list.rs:19–61 · .velnor/rust-ci.lock:1–14 · `3a77d53ce6`, `0eb2d76a5c` · docs/rust-ci.md:22–43)

5. **Consolidate duplicate policy work and shorten its dependency chain · illustrative 0.4 min/merge.**
   If two qualifying policy invocations save 0.20 minute each after scope comparison, net 0.4. Starting an independent policy job from a small trusted plan may shorten the verdict even when total work stays level.
   Inventory Cargo Deny/Machete command, input and target differences first. Preserve Alint and Jactionlint coverage. Compare existing serial jobs against a fused or early policy layout with the same checks. Include startup, queue and extra checkout.
   Record full-select reasons from missing history separately. Improve fetch on measured misses before claiming a speedup. Maintain safe wide selection on unknown paths. (CI:41–156 · R:406–505,571–612 · ci/io/git.rs:36–79 · rust/classify.rs:29–57)

### Latency targets and how to earn them

Measure PR verdict from request/armed push accepted to authentic success or failure on that SHA. Measure gross last head push → GitHub squash merge, plus the segment from label/request when the author deliberately waited.
Also publish group creation → group verdict, green → enqueue, queue dwell, GitHub finalisation, A1 lag, failed/cancelled/never-merged counts and a separate advisory-cache finish time.
The primary population is same-repository changes attempting protected landing. Stratify Rust/default, local-mode, policy/workflow, docs-only, release, full/global and warm/cold. Pair workloads and show P50/P90, sample size and uncertainty.
Size the system with component budgets and calculate realised quantiles on complete traces, preserving dependence between stages. (C:14,23–28,192–198 · Arch:149–176)

**Phase 0 engineering target.** Aim initially for heavy labelled PR verdict **4 / 9 min P50/P90**. This gives the observed 2.9 / 6.2 failed-or-cancelled proxy 1.1 / 2.8 minutes of room for a completing pass and trusted gate.
For an already armed, promptly enqueued PR with two full 4/9-minute verification passes and 1/2 minutes gross queue/control allowance, budget **9 / 20 min push-to-merge**. The current ruleset configures five minutes of minimum batch wait. Measure the actual delay per group before attributing any of it to each PR.
Both targets are trial budgets. If complete heavy green runs exceed them, publish the result, the full obligation set and the measured floor. Recompute after at least 100 protected merges, multiple busy periods and a weekly rolling window. (C:52–72,159–174 · Arch:149–176)

**Qualified reuse target.** Retain **90 s / 4 min PR** and **3 / 8 min gross push-to-merge** as conditional goals for the representative mix.
A median budget is 10 s admission plus up to 75 s for the slower witness and 5 s publication. A 180 s merge budget leaves 90 s for group lookup/fresh exceptions, queue and finalisation. At P90 reserve 4 minutes for the PR and 4 for the remaining segments.
Require measured identity matches, observed C5 fraction, 2% cold sampling, steady-state A1 service, no unexplained divergent verdicts and comparable flake rates. Publish the full-tree/full-rebuild class separately even if it misses.
Separate owner/user label delay, but keep it visible in gross time. Never count pre-push work as free. Revisit depth four only after group P90 ≤5 min and paired false failures ≤3%. Target ≤1% before widening further. (Arch:149–176,519–532,614–624 · OQ:20–36)

The product promises still apply to its supported hosted configuration, including a no-Rust result within a minute, warm reuse, first Cargo within 30 s and ≤8 GB GitHub cache. A self-hosted race should publish hardware, RAM and storage cost separately.
Use `velnor-ci bench collect/compare/budget`, `timings` and `ci-metrics.json`. Add a per-contract clock for the actual green verdict. `shape = auto` without useful timings stays single. Fit grouped/split shapes only when the observed saving exceeds the launch and transfer cost. (docs/goals.md:49–69 · ci/io/timings.rs:16–31 · rust/shape.rs:10–59 · R:32 · A:383–387)

## 5. What to keep, change and drop

E's **174 item statuses** are 86 VALID, 50 CHANGED, 20 CONFLICT, 10 DONE and 8 OBSOLETE.
Its **174 proposed actions** are 93 KEEP, 42 REWRITE, 22 GATE, 9 MARK-DONE, 7 DROP and 1 DEFER. A completed item can legitimately have a drop action when its output was subsequently deleted. These are E's totals before the amendments below. (E:7–20,243–425,426–450)

| Status group | Disposition at this tip |
|---|---|
| VALID | Keep the safety invariants, trust tiers, two witnesses, A1/A2, exact bytes, conformance, platform capability model, immutable stores and strong comparator. Re-derive workloads, deadlines and capacities even when the mechanism survives. |
| CHANGED | Rewrite the 39-job inventory, nested Cargo roots, gate names, workload mix, test floor, caching route, release workflow and critical path. This repository has one Cargo workspace. The product also supports repositories with several roots and grouped roots. |
| CONFLICT | Seek a single owner amendment for executor/local scope and a separate decision for self-hosted release trust. Resolve compiled coverage, cache budgets, ledger evidence, public cache exposure and policy authority explicitly. Lab analysis and read-only work can start before a production decision. |
| DONE | Accept the cutover, off-hosted Rust gates, deleted engine release loops, macOS pause, workspace migration and generated qualification ownership. Check the original observable exit conditions before closing a speed, cancellation or conformance item. |
| OBSOLETE | Retire the engine release batches, old profiles, state seal, image candidate, removed fixture loops, background-step assumptions and migrations already completed. Keep any independent regression as a small case exercised on the new path. |

For the action groups, keep E's seven removals and defer A07 until there is an actual consumer or it gates a supported release template. Take the 93 KEEP and 42 REWRITE entries forward under the new contract.
Separate authority to *deploy/change policy* on the 22 gated entries from work such as corpus preparation, compatibility research and measuring current jobs. Keep nine MARK-DONE entries as history, with explicit follow-up acceptance for B08, A09, G04 and T06/T07.
This gives each old item a disposition without opening 174 new tickets. (E:251–265,291–309,325–337,376–383,426–450)

**Amendments to E that change implementation order or safety**

- E suggests turning on `skip_push_after_merge_group` as the first reuse feature. Reject that activation here. Test an equal SHA whose group legitimately omitted push-only docs/alone work. Only the individual covered obligations may be reused. (E:60–68,551–558 · ci/merge_group.rs:45–81 · rust/docs_post_merge.rs:6–17 · rust/verdict_plan.rs:94–105)
- E would convert selected work to `None` on a hit and make package keys the new default. Preserve selection. Record satisfaction separately. Whole-tree coverage is the default until a reviewed, enforceable input contract covers directory/absent reads and graduates cold. (E:62–68,335–336,539–541 · model/selection.rs:45–100 · Arch:489–517,588–596)
- B08's cancelled totals establish supersession exists. They say nothing about P90 time to reclaim a stale slot, disarming, shared claims or a stale publisher. A09's 16 renderer entries say nothing about Native implementing those operations. (E:258,299–310 · render/rules.rs:16–40 · R:4–24)
- G04 is complete as a generator cutover. The new `qualification.yml` has 64 declared jobs. Its `compare` step and both prerequisite verification jobs each run `echo`. The qualification job needs an actual observer, comparator and failing control before it can establish executor equivalence. (`57b65af0db` · .velnor/workflows.toml:461–492 · .github/workflows/qualification.yml:818–840)
- T06/T07 closed work on named integration harnesses and crate boundaries. Recompute the remaining runners' binary count and long-test floor on this checkout. A large surviving host crate and its `local-mode` lane still matter. B01's inventory and older 31-check counts also need refresh. (E:376–383,399–406 · Cargo.toml:4–25 · R:214–299 · .velnor/rust-ci.toml:26–32)
- #1274 and #1335 are merged. The current workflow emits a feature guard. #1748 changed helper source but the workflow still downloads the earlier release. Distinguish source, emitted YAML and the downloaded executable in every “done” claim. (`61067c17fc`, `47ef96f8ea`, `0eb2d76a5c` · R:67–82,291–293 · .velnor/rust-ci.lock:1–14)
- N14 must remove the drift-mode sentinel along with the remaining configuration. N04 needs a released generator installer. A pinned helper reading candidate policy does not establish a trusted base inventory. (E:79–80,526–539 · gen/io/mod.rs:174–194 · gen/drifts.rs:65–75 · ci/io/plan.rs:38–53 · actions/setup-velnor/action.yml:97–107)
- N13 should correct the metric and treatment of advisory failures. A red `Cache` can make a workflow red while `Required` is green. Keep that signal visible and preserve its nonblocking contract. (E:538 · docs/goals.md:42,67 · R:571–612,627–664)
- N23 requires two trust decisions. The Verify App gets status authority. The Release App/tag rules and an attestation's signer/builder identity govern different actions. Preserve all three boundaries when moving a Linux builder. (E:548–549 · REL:16–45,187–253 · generator/CONFIG.md:557–573)
- Retire the old release goldens, including the obsolete OpenTofu fixtures on #1608. Preserve the exact-push policy, the no-checkout publisher and provenance checks. A release run that happened on a tag does not prove its builder ran on the authorised landed commit. (D:19–39 · `57b65af0db` · REL:86–169,187–253)

**E's additions.** Schedule N01–04, N06, N09–10 and N17–19 in the first work package. N05/07/08/11/12/23 need the owner decisions on scope, deployment or release identity in section 7.
N13–16 become explicit contracts for advisory metrics, workflow ownership, key enrolment and CLI names. N20–22 belong to Native admission, durable evidence and recorded scenarios.
For N24 keep the 500,000-byte render budget and the delta-only relief valve. Compact obligation data into the canonical manifest and render commands over IDs. Never silently prune required work to fit YAML. (E:522–549 · gen/plan_render.rs:15–39 · render/rules.rs:233,378)

### First move in each delivery stream

| Stream | Disposition and replacement first move |
|---|---|
| B · phase 0 | Replace the engine-release pack with the red main/strict gate repair, queue evidence and a dated green benchmark. Coordinate `Required`, the label and the release pin. |
| RV · component reviews | Begin by reviewing `decide`, policy input trust, host ownership/cleanup, the local v2 service and each release provenance reader. Capture evidence for reuse vs replacement. |
| P · platform | Reconcile #1324's observation proofs with the new capability model on the existing Docker path. A Kubernetes backend and site deployment follow the owner-authorised specification. |
| O · Official | Extract a pinned Official execution adapter. Test authoritative and reference credentials and outputs on the same manifest. Only an authoritative execution can satisfy an obligation. |
| X · Native | Start in root-workspace crates with a closed acquired-job schema and the current protocol lifecycle. Introduce the evaluator and step executor once role/scope is approved. Drop X13's unused engine feature. |
| A · native actions | Generate an exact action/input closure for current workflows. Qualify checkout including depth, cache v2, artifacts v4, mise and the setup composite in order of real use. |
| H · conformance | Record the local scenario and replace the echo comparator with a real assertion over normalised outputs, results, logs, post steps and side effects. Pair Native and pinned Official on both backends. |
| L · App and gate | Begin with a lab ruleset, SHA-specific status writer and disarm/revoke races. Compare shadow decisions on 300 real PR heads and 100 actual groups, including reds, before swapping the required context. |
| G · generator/routing | Treat cutover as delivered. Emit the union contract plus semantic digest, then select a single authoritative route per qualified job type. The YAML remains a first-class output. |
| V · ledger | Begin with typed whole-tree keys and read-only would-hit telemetry. Introduce trusted signatures and per-obligation resolution before a hit can discharge work. |
| AN · agent-native | Start with explicit `verify` snapshots and `velnor cargo` counting, paired against the same contract. Preserve MBX as the one wrapper until routing earns its cost. |
| DG · developer interface | Start with a stable obligation-ID vocabulary, reasoned key diffs and count-only telemetry. Link the existing reports to request, subject, attempt and witness IDs. |
| C · generations and mirrors | Benchmark the current node tools, crates mirror, PR delta and `mbx-store`, then add verified full-tree Git checkout and the warm-vs-cold twin trial. |
| ST · storage | Start with immutable blob/manifest types and the current cache-rules contract. Qualify local disk before per-site RustFS and the Official v2 endpoint. |
| T · test floor | Remeasure default and `local-mode` configurations. Price discovery, docs, host long cases and group setup. Retire only duplicated execution proved redundant for the same inputs. |
| S · cluster | Establish actual protected traffic and reserved audit capacity. Reconcile site ownership, outage behaviour and an operator deploy plan before changing nodes. |
| R · release | Finish #1608's landed-push authority. Next bind a release manifest, smoke the exact Linux bytes, and make publication digest-only. Decide the `runner-v` asset roster before deletion. |
| F · final deletion | Inventory consumers per job type and drain enrolled resources. Remove each superseded route, old rules/credentials and config in its cutover. Preserve active product commands and releases. |
| BM · evidence | Restart S1–S6 against a green generated baseline. Check planted failures and publication races before timing. Run conventional tuned and proposed arms on the same workload and capacity. |

These streams replace the old day-by-day dependency calendar. Traffic through a functioning queue controls the gate-parity date. Provenance, cross-backend conformance and warm/cold comparisons control their own promotion dates. (Plan:627–666,668–699 · E:251–425)

## 6. Phase 0 and the first ten work items

Phase 0 requires a replayable chain from a fully verified labelled PR through an *actual* ALLGREEN merge group to an independently green landed SHA. Remove bypass and demonstrate a rejected negative control for each authority.
The owner can stop ordinary landing and provision the lab in parallel with code repair. Keep releases held until card 02 is proven. Before any settings swap, demonstrate recovery on a broken candidate with the same trusted controls.
For the first repair on a red base, use cold, full verification of its proposed tree and require the real GitHub group's exact SHA through the queue. Use an explicitly maintained owner-enforced merge hold until the trusted source and subject checks in card 05 have been demonstrated. Hold the merge if its exact subject cannot be verified.
Cards 01–06 deliver that baseline. Cards 07–10 are the first follow-ons and can prepare in parallel. Source edits that share a planner/lock/generated YAML get one integration owner. Waiting for the owner, GitHub and sufficient traffic is elapsed time, outside the agent-hour estimates.

**Inventory cut-off.** The code analysis is fixed at `0eb2d76a5c` (18:09:51 UTC on 19 November). The later PR inventory closes at **18:43 UTC on 19 November**. The four PRs open at that cut-off were #1608, #1297, #1751 and #1757.

| Work | Disposition |
|---|---|
| #1608 | Open. Rebase the narrow push-gate change and current tests. D lists stale fixtures. The surviving tools release file is handwritten. Use card 02. [#1608](https://github.com/tailrocks/velnor-new/pull/1608) |
| #1320 | Closed **unmerged** at 17:57:28 UTC, before this tip. Its author retained the branch and moved per-root audit work to #1745 after workspace groups landed. Salvage the semantics against shared targets in card 08. [Closure](https://github.com/tailrocks/velnor-new/pull/1320#issuecomment-7017473589) · [#1745](https://github.com/tailrocks/velnor-new/issues/1745) |
| #1200 | Open at the pin, merged at 18:39:35 UTC (`146594fa64`). Card 07 verifies the landed platform-lock change and any residual coverage under #1700. [#1200](https://github.com/tailrocks/velnor-new/pull/1200) |
| #1026 | Open at the pin, merged at 18:33:14 UTC (`17d13fd574`). Use its landed custom target-dir and sibling restore as the starting point for #1745. Verify #1011/#1018 and reconcile #1730 where it overlaps. [#1026](https://github.com/tailrocks/velnor-new/pull/1026) |
| #1297 | Open. Re-resolve the needed dependency changes from the single current `Cargo.lock`, or use a fresh Renovate update. Isolate any security repair that actually fixes the current failure. Retire the obsolete bulk diff only after accounting for each still-needed update. [#1297](https://github.com/tailrocks/velnor-new/pull/1297) |
| #1274 / #1335 / #1747 / #1748 | Already landed. Preserve workspace-group and feature-guard coverage, account for the earlier shipped helper and deleted engine. #1263's daemon lock fix also landed. (`61067c17fc`, `47ef96f8ea`, `57b65af0db`, `0eb2d76a5c`, `a619478fb2`) |
| #1750 / #1751 | Both postdate the pin. #1750 closed unmerged after its OpenTofu fixtures were deleted. #1751 is the open research PR. Coordinate plan tracking there and evaluate delivery through implemented code. [#1750](https://github.com/tailrocks/velnor-new/pull/1750) · [#1751](https://github.com/tailrocks/velnor-new/pull/1751) |
| #1757 | Open. The proposed removal covers the retired source-preview helpers and Mac proof command. Validate all helper-command call sites, generated fixtures and required test setup before deletion. Preserve any still-used attestation/asset checks. [#1757](https://github.com/tailrocks/velnor-new/pull/1757) |

Other post-tip merges affect where the first cards start. #1754 and #1768 address the red-main policy and Linux lint/nextest paths. #1763 moves the tools release workflow into `.velnor/workflows.toml`. Apply #1608 there on a contemporary branch, then regenerate. #1765 bumps tool crate versions to 0.1.96. A version bump alone supplies no proof of installed binaries. (`4f07364608`, `1ffffeb47b`, `cbf55d5df6`, `6f1359196b`)
#1752–53 cover collector/host recovery, #1755–56 declared workflow/local fixes, #1764 deferred docs/alone/input ownership, #1766 release preparation, and #1767 node tools/MBX seed hardening. Replay those landed changes in their respective experiments. Do not rebuild historical fixes from the pinned tree. (`fc43f7c409`, `356c6b1c6c`, `c7a29363e2`, `b21a6080dc`, `e25d599090`, `90dbd4ac1c`, `e3c511f799`)

Use #574 and project 2 for ownership and dependencies. D counted 160 project items and many PR mirror refs. Squash merges make ancestry a poor branch-cleanup test. Inspect surviving value by diff and issue criteria.
Verify apparently completed FAST tickets such as #604/#608 against the delivered #623/#622 work before changing state. Squash-merge receipts, changed paths and surviving tests provide the evidence for each individual disposition. (D:43–89,133–151 · docs/roadmap.md:35–45,71–76)

Effort below includes implementation and independent review. The proposed seats use the roadmap's dispatchable Claude Sonnet/Opus labels. Each review uses a separate agent, including independent authority/security review where relevant. (docs/roadmap.md:35–45,75–76)

### 01 · Restore a reproducible green Rust and policy run

**Goal.** Reproduce the pinned failures, then validate what #1754/#1768 repaired. Their changes to Alint limits, the Cargo Deny manifest set, Linux lint, unused imports and nextest preflight are the first fixes to test cold and warm. Use the protected chain as #1555's acceptance evidence. (`4f07364608`, `1ffffeb47b`)
**Files.** `crates/velnor-ci/tests/it/prepare_inputs.rs`, `crates/velnor-ci/tests/it/prepare.rs`, `.github/workflows/ci.yml`, `deny.toml`, `Cargo.toml` and `Cargo.lock` as warranted by the reproduced failure. `.velnor/rust-ci.toml` defines the exact feature matrix.
**Done when.** Both default and local-mode lint/test, the emitted feature guard, and the four policy jobs pass on the candidate and actual landed commit. Compare expected test inventory against observed counts and preserve regressions for the repaired cause.
Report advisory cache separately. Fix a confirmed Deny issue with the smallest valid dependency repair. Leave unrelated #1297 updates in their own PR. #1746 already fixed some clippy failures. Do not rerun its historical diagnosis as today's result. (`2561e7911e` · R:205–299 · CI:41–156 · D:124–129)
**Agent effort.** 10–18 agent-hours. Implementation Claude Sonnet/high. Review Claude Opus/high.

### 02 · Require exact landed proof before tools publication

**Goal.** Finish #1608's valid check. The gate at the pin lives in handwritten YAML. After #1763, edit its declaration in `.velnor/workflows.toml` and regenerate. Identify the permitted push run, verify its job and bind its artifacts. (`cbf55d5df6`)
**Files.** `.velnor/workflows.toml`, `.github/workflows/release-rust-ci-tools.yml`, `ci/gate.rs`, `ci/cli_release.rs`, `crates/velnor-ci/tests/it/gate/current_run.rs`, `crates/velnor-ci/tests/it/gate/skipped.rs` and `crates/velnor-gen/tests/it/release_workflow.rs`.
**Done when.** Positive lab dry run selects exact repo/default branch, push SHA, trusted workflow and latest attempt. Negative cases include a green PR, skipped job, wrong producer, older attempt, tag target mismatch and missing or substituted artifact.
Publish simulation reads only gated build artifacts, rehashes them and executes no checkout code. Keep group evidence distinct from the landed proof and preserve the correct current release-host restriction. (REL:86–105,187–253 · ci/gate.rs:57–104,141–172 · docs/goals.md:77)
**Agent effort.** 8–14 agent-hours. Implementation Claude Opus/high. Review Claude Opus/high.

### 03 · Make generator freshness a released, executable gate

**Goal.** Wire `GeneratorCheck` to an immutable generator that actually exists on PATH, and protect the bootstrap source of policy.
**Files.** `model/steps.rs`, `rust/plan.rs`, `render/helper_steps.rs`, `gen/helper_gate.rs`, `actions/setup-velnor/action.yml`, `.velnor/rust-ci.lock`, `gen/drifts.rs` and `crates/velnor-gen/tests/it/required_gate.rs`. Regenerate `.github/workflows/rust-ci.yml`.
**Done when.** The lab downloads/hash-checks the base-authorised program. A removed, added, stale or tampered generated file fails on a docs-only eligible change too. A deliberate generator change must pass candidate goldens and the controlled pin/regen rollout.
Record shipped vs source capability, including #1335 and #1748. The pinned generator cannot approve its own replacement from the PR. A clean checkout regenerates byte-identically with the declared release. (model/steps.rs:108 · render/helper_steps.rs:74–79 · gen/io/mod.rs:121–128,174–194 · .velnor/rust-ci.lock:1–14)
**Agent effort.** 12–20 agent-hours. Implementation Claude Sonnet/high. Review Claude Opus/high.

### 04 · Replace both request labels with the persistent owner flow

**Goal.** Implement section 2's transitions on the released planning path. Apply admission to every heavy job, including `ci.yml`, and explicitly control the status for drafts and unproved ready heads.
**Files.** `model/plan.rs`, `rust/plan_budget.rs`, `rust/required_job.rs`, `rust/report.rs`, `render/events.rs`, `.velnor/rust-ci.toml`, `.github/workflows/ci.yml`, `crates/velnor-gen/tests/it/pr_ci_labels.rs`, `generator/CONFIG.md` and generated `R`.
**Done when.** Real lab events cover draft → ready → label → two pushes → unlabel while running → re-add. Test unrelated edits/labels and base retarget separately. No unproved head merges, no one-shot label or job write credential survives, and only valid ongoing work can publish.
Measure actual cancellation-to-capacity-release. Verify correct agent-generated label event delivery and zero heavy work for unrequested heads. In transition retain an explicit blocking control result. Final App admission leaves draft/ready idle. (R:4–24,571–625 · CI:4–14 · OQ:139–151,373–380)
**Agent effort.** 16–26 agent-hours. Implementation Claude Opus/high. Review Claude Opus/high.

### 05 · Prove and apply protected queue admission

**Goal.** Replace the inactive protection with a demonstrated no-bypass path. First test the exact intended rules and actor privileges in `velnor-lab`, with a recovery rehearsal.
**Files and settings.** `rust/required_job.rs`, `ci/io/required.rs`, `crates/velnor-gen/tests/it/required_gate.rs`, `docs/roadmap.md` for the merge procedure, generated `R` and the owner-controlled `protect-main` ruleset 24396608/repository auto-merge setting.
**Done when.** Lab rejects missing, skipped, wrong-origin and duplicate-name evidence, stale heads and replaced groups. Owner readback shows required `Required` bound to Actions 15368 (or the qualified App cutover), empty bypass, ALLGREEN, depth 2, minimum wait 0 and timeout 90.
A genuine enqueued change passes its exact group and landed audit, a red group is ejected, and auto-merge never requires a routine branch catch-up. Capture the actual head/group/landed event series. (C:159–174 · OQ:20–36,64–76,112–124)
**Agent effort.** 6–12 agent-hours plus owner/queue wait. Implementation Claude Sonnet/high. Review Claude Opus/high.

### 06 · Establish the green three-context benchmark and its costs

**Goal.** Instrument a common clock and cost record for real protected traffic, starting with the five levers in section 4.
**Files.** `ci/io/report.rs`, `ci/io/budget.rs`, `ci/io/timings.rs`, `ci/io/plan.rs`, `ci/io/git.rs`, `model/repo_config.rs`, `.velnor/rust-ci.toml` and `docs/rust-ci.md`.
**Done when.** The same request ID joins label, first/last push, head, group, landing and A1. Report eligibility, cancellations, queue dwell and cache/delta restore, transfer, hit/miss reason, expected/actual compiles and warm/cold state.
Publish an initial real full chain, then confidence intervals over ≥100 protected merges, full/failing/censored strata included. Reproduce missing-base behaviour on a long push and multi-entry group and identify real workflow-path readers. (C:95–109,176–198 · ci/io/git.rs:36–79 · ci/io/timings.rs:16–31 · docs/goals.md:44,49–61)
**Agent effort.** 14–22 agent-hours plus traffic collection. Implementation Claude Sonnet/high. Review Claude Opus/high.

### 07 · Preserve platform-specific mise identity in the shared workspace

**Goal.** Validate the landed #1200 against the current Rust and helper boundaries, fixing only demonstrated gaps. Recheck which #1297 updates remain necessary after card 01 and current lock resolution. (`146594fa64`)
**Files.** `rust/mise_lock.rs`, `rust/mise.rs`, `ci/platform.rs`, `ci/io/unit_platforms.rs`, `crates/velnor-ci/tests/it/platform.rs`, `mise.lock` and the single root `Cargo.lock`.
**Done when.** Distinct OS/architecture-qualified tools keep their own URL, version and digest on x64 and arm64. Missing/ambiguous entries fail closed. The helper and generator resolve the same identity for release and cache keys.
Record a verified per-update disposition for #1297 separately from the platform change. No old nested lockfile or removed engine fixture reappears. (D:23–39 · Cargo.toml:4–25 · .velnor/rust-ci.lock:1–14 · [#1700](https://github.com/tailrocks/velnor-new/issues/1700))
**Agent effort.** 10–18 agent-hours. Implementation Claude Sonnet/high. Review Claude Opus/high.

### 08 · Make grouped input audit and restore agree

**Goal.** Build #1745 on the landed #1026 sibling/custom target-dir fixes and #1320's surviving audit idea. Collect once per grouped build, attribute per root/package and reject ambiguous ownership. (`17d13fd574`)
**Files.** `ci/io/inputs.rs`, `ci/io/input_snapshot.rs`, `ci/io/audit/groups.rs`, `rust/groups.rs`, `model/repo_config.rs`, `crates/velnor-ci/tests/it/inputs.rs`, `crates/velnor-gen/tests/it/workspace_groups.rs` and `generator/CONFIG.md`.
**Done when.** Fixtures covering a shared target, a custom target directory, a reachable sibling, a deleted input and concurrent same-group commands either restore a provably correct result or fall back cold. Capture hits/misses by origin. The audit must collect each build once.
Exercise both the single-root repository and the multi-root fixture. Preserve the documented incompatibility of grouped roots with incompatible feature/test-lane configurations until there is a proved design. (model/repo_config.rs:49–58 · rust/groups.rs:93,132,160 · generator/CONFIG.md:91–107 · `61067c17fc`)
**Agent effort.** 16–28 agent-hours. Implementation Claude Sonnet/high. Review Claude Opus/high.

### 09 · Emit the first complete, base-governed verification contract

**Goal.** Extend the existing compiler and pure verdict boundary, then generate a semantic obligation inventory which every execution role can consume.
**Files.** Add `crates/velnor-model/src/verification.rs` and extend `model/plan.rs`, `model/verdict.rs`, `rust/plan.rs`, `rust/verdict.rs`, `gen/lib.rs`, `ci/io/plan.rs`, `ci/io/required.rs`, `crates/velnor-workflows/src/lib.rs` and `render/steps.rs`.
**Done when.** Default, feature, guard, policy, docs, crate-alone, push, release and advisory checks have explicit IDs/conditions. Expanding a grouped/single plan yields the same intended union, and changing protected policy fails without a reviewed reduction.
A planted partial test pass, stale receipt, wrong root and wrong executor all fail the pure verifier. Freshness covers the contract. Golden examples explain explicitly why each obligation was selected, excluded or completely satisfied. (model/verdict.rs:65–80 · rust/required_job.rs:77–151 · rust/verdict.rs:207–315 · CI:41–156)
**Agent effort.** 22–36 agent-hours. Implementation Claude Opus/high. Review Claude Opus/high.

### 10 · Exercise ledger and App authority in the lab

**Goal.** Resolve contracts against signed observations in shadow and compare with fresh independent jobs. Exercise the single status writer and revocation logic with the owner's lab App.
**Files.** New `crates/velnor-ledger/src/lib.rs`, `crates/velnor-control/src/{requests,publisher,reconcile}.rs`, `Cargo.toml`, `ci/io/required.rs`, `.velnor/workflows.toml` and generated `.github/workflows/qualification.yml`.
**Done when.** Plant a failing test, duplicate status source, lost response, revoked epoch, delayed webhook, stale group, different witness/node and missing receipt. The lab never issues an incorrect success, and cold re-execution agrees with the proposed decisions.
Ship replay tooling plus a run/attempt/epoch-indexed parity report. Production keeps one authoritative path. Shadow cannot mint production successes. Production swap waits for 300 head/100 group comparisons, two independent slots and owner readback. (Arch:523–556,598–625 · Plan:651–665 · .velnor/workflows.toml:461–492)
**Agent effort.** 28–48 agent-hours plus evidence collection. Implementation Claude Opus/high. Review Claude Opus/high.

The ten estimates total **142–242 agent-hours**. They include independent review and exclude owner and traffic wait. The production App cutover, Native cohort rollout, release promotion, storage and per-kind reuse qualification continue on evidence gates. The former hour-100/day-10 dates have no basis on current verified traffic. (Plan:648–666,678–699)

## 7. Owner decisions rewritten

A standing decision, an approval to amend the product goals, an external settings action and evidence that the system conforms are four distinct things. Approval of the design does not prove that today's system implements it.
The counts below are evidence thresholds or trial budgets. The owner can approve the words and the settings separately. (docs/goals.md:3,76–78 · OD:29–54 · OQ:12–16,348–390)

### D1 through D26

| ID | Disposition | Recommendation |
|---|---|---|
| D1 | Obsolete | Close the question about dispatching the old engine release. It shipped and its engine was deleted. Build the generator/helper release procedure into cards 02–03. (`053fefa6a3`, `57b65af0db` · E:485–493) |
| D2 | Reword (owner setting) | Enable repository auto-merge and automatic enqueue for a proved latest head. Stop routine catch-up merges and all admin bypass. Measure green-to-enqueue. (C:23–46,159–174 · OQ:64–76) |
| D3 | Reword (owner setting) | Apply the single-context transition from card 05. Start depth 2/ALLGREEN/zero minimum wait/90-minute timeout. First test fail-closed operation and the recovery route. Later bind the App status to its actual App ID. (OQ:20–36 · C:159–174) |
| D4 | Open, after scope approval | Choose the current upstream `Sdk/Expressions` and `WorkflowParser` route. Compare against the pinned Official observable output. Reject unqualified cases at admission. Keep one production evaluator, audit licences for any port. (OQ:153–166 · OD:10–14,43–54 · .velnor/runner.lock:14–23) |
| D5 | Unassigned | Preserve the numbering gap. The actual question sheet explicitly says D5 is absent. (OQ:15–16 · E:499–504) |
| D6 | Answered | Official is the permanent reference in conformance, sampled shadow and explicit comparison. Its separate configured authoritative role has its own result keys. (OD:34–54 · OQ:371–377) |
| D7 | Reword (owner identity) | Create the Verify App with minimal read, statuses-write and optional checks-write for details. Separate the agent identity from ruleset and release administration. Lab first, `Required` holds authority until atomic cutover. (OQ:96–108,373–380) |
| D8 | Reword (owner/operator) | Keep manifests and scoped deployment identity in the operator's `velnor-runners` repository. Stage one site, verify drain/recovery, then consider XFS and dedicated storage volumes. Approve any destructive rebuild and capacity reservations separately. (OQ:78–94 · B:21–29,277–283 · Evidence:533–576) |
| D9 | Obsolete | Retire pre-approval of configuration-only engine releases. Use the generator/helper release and lab rules. Scope any future delegated release authority independently. (E:489–491 · docs/goals.md:76–78) |
| D10 | Answered, unenforced | Two independent witnesses for a new key and ALLGREEN. Require independent builds/orders and read back the setting. Re-run on equal trees if twin failures prove correlated. (Arch:89–101 · OQ:371–376 · C:165–169) |
| D11 | Reword | Set counts per current job type **and backend**. Use 150 paired control runs, 300 build/test, 150 Docker, 30 release plus three dry-run publishes each. Include cancellations and planted reds. Zero unexplained divergence before migration. (OQ:126–137 · Plan:651–665) |
| D12 | Answered, qualify implementation | Prepare release bytes in the candidate group, promote by digest after landed A1. Require matching release subject including embedded commit. Hold or prepare a correct candidate before tagging if the commit identity changes. (OQ:373–382 · REL:140–151 · Arch:575–586) |
| D13 | Re-ask for maintained products | Continue targeted safety/repair work on the in-use host and local product until its replacement qualifies. Assess actual current consumers and open work. Preserve a supported route for each active consumer. (`a619478fb2` · .velnor/rust-ci.toml:26–32 · E:499–504) |
| D14 | Done | Twelve engine crates were removed. Close the engine question and migrate the remaining input mappings and drift sentinel by explicit consumer inventory. (`57b65af0db` · gen/io/mod.rs:174–194 · gen/drifts.rs:65–75) |
| D15 | Open (approve recovery) | Approve redeploy of a last qualified immutable image and a minimal same-authority cold verifier on two independent nodes. Verify the full current policy on the exact SHA, journal and page. An inability to prove the repair holds the merge. (OQ:112–124) |
| D16 | Reword in Q-N2 | Require a fresh explicit request on a ready PR. A labelled draft stays disarmed when marked ready. Reapply the label or explicitly request once ready. Test the carried-label case with reconciler restarts. (OQ:139–151 · E:22–38) |
| D17 | Open (approve signer) | Owner alone signs a versioned, narrowly scoped reduction to an obligation or test. A quarantine requires an equivalent blocking control, expiry and a successful planted regression. (OQ:168–182 · Arch:73–83,103–117) |
| D18 | Reword, defer change | Keep GitHub's queue. Re-evaluate only after 50–200 protected auto-merges quantify GitHub's share. An observational shadow scheduler has no landing authority. (OQ:266–279 · C:37–46) |
| D19 | Answer with no speculative builds | An unarmed head triggers no compile or tests. Explicit `verify` can start paid work before the label and later join it. Count abandoned trees. (OQ:184–195,386–390) |
| D20 | Defer spend | Benchmark existing hardware first. Request a budgeted one-node pilot only if the measured single-thread floor dominates. Require a measured ≥1.15× improvement and independent compatible witness capacity before wider rental. (OQ:199–212) |
| D21 | Answer with independent builds | Do not feed a single compile to both witnesses. Reuse an archive among a single witness's own groups, preserving separate generations/compilers across witnesses. (OQ:240–251 · Arch:525–542) |
| D22 | Conditional owner budget | Authorise a time/cost-capped rented comparator and hosted arm only for an agreed experiment. Publish only comparisons that ran with a matched contract. (OQ:214–225 · Arch:1083–1113) |
| D23 | Answer with explicit requests | Keep automatic unpushed verification off. Revisit after a month of supersession, cost and spare-capacity data from explicit `verify`/`ship`. (OQ:281–295) |
| D24 | Open, default equal | Initially keep remote agent work equal to PR-class work. If measured headroom supports priority, cap at 30 CPU-minutes/hour/identity, protect other tenants, merge, gate and A1 reservations. (OQ:227–238) |
| D25 | Defer (capped pilot) | After `verify` ships, allow two opt-in agents a time-boxed sticky-shell trial with no minting credentials. Rebuild counting runs on sealed inputs. Request a separate decision for dedicated machines. (OQ:297–309) |
| D26 | Reword (premise changed) | Tools already build and smoke on native hosted Linux arm64. Require smoke of the exact promoted arm64 bytes. Qualify a self-hosted Linux arm64 lane if release-host policy changes. Keep macOS releases paused. (REL:106–169 · OQ:311–320 · OD:15–18) |

### Q-N1 through Q-N11

**Q-N1 · Approve the executor scope and local-product amendment.** Recommend approval of the following exact replacement for the paragraph at `docs/goals.md:16`, resolving the superseded “Rust-only” instruction using binding decision 20.

> Velnor generates GitHub Actions workflows for Rust projects. On project-managed machines, Velnor's Rust dispatcher runs each job with its configured executor on Docker or Kubernetes. Velnor Native, written in Rust, is the default. A project may explicitly select GitHub Official for a job type. Only the selected executor in its authoritative role may issue execution evidence, and its identity is part of every result key. Reference executions produce comparison data. An executor failure fails the request. GitHub-hosted workflows use GitHub's official runner.

Replace rule 11 at `docs/goals.md:75` in the same owner-approved change:

> **Local first.** A developer can verify the generated workflow locally through the Velnor runner, a local GitHub stand-in and a cache that enforces GitHub's scope rules. The local runner uses the job's configured executor. GitHub Official provides an independent reference for conformance. Real GitHub confirms the workflow before adoption.

Bring the scope table, vision, runner product responsibilities, same-repo admission, storage/free-tier terms and rules 12–13 into agreement in that change. Preserve the portable hosted profile's 8 GB limit, the named `Cache` check and exact landed/release proof.
Give the macOS spec the same role distinction and reconcile #1324's journal guarantees with the platform trait. Owner approval is required before applying this wording. (docs/goals.md:3,11–24,40–45,49–77 · docs/proposed/macos-scaleset-runner.md:37–39,318–323 · OD:34–54 · E:40–56,506–520)

| ID | Revised question and recommendation |
|---|---|
| Q-N2 | Which request signal? One persistent `velnor:verify`, no automatic quick or heavy verification for drafts/unarmed ready PRs. An unproved head must block. Disarm on removal, retain valid commit-scoped proof, require explicit post-draft arming. |
| Q-N3 | When to remove bypass? Freeze ordinary red merges immediately. Lab-prove the repair and same-authority recovery, then bind trusted `Required` and remove bypass. If the trusted interim source cannot be guaranteed, hold landing for the App. |
| Q-N4 | Which merge authority? Keep GitHub's queue and enable auto-merge. Require the real group's exact SHA on every admitted entry. Rehearse a failing group, replacement and landed audit before raising depth. |
| Q-N5 | Which release trust? Keep existing hosted Linux preparation/signing restrictions while separating builder, signer and publisher in the manifest. Approve a self-hosted builder/signer policy only with reproducible provenance and independently verified equivalence. |
| Q-N6 | Which PRs? Admit same-repository PRs only, reject fork events before credentials or jobs. Retain public-cache data separation, dependency-fetch-only secrets and revocation. Record the product scope in the approved goals. |
| Q-N7 | Which job shape? Keep one job per workspace or supported group by default and workspace-wide compile coverage. Split only on measured net benefit or a distinct machine need. Share an archive within a configuration/witness. |
| Q-N8 | Which semantics for the ledger? Approve a single mechanism that proves satisfaction of the generator's selected obligations, with whole-tree keys, two witnesses and audits. HD3 governed the old engine. Re-state the safety objective in the new product rules. |
| Q-N9 | What runs until a cohort moves? Maintain the current runner-host recovery, local feature and release obligations. Drain and delete the superseded route on migration, preserving qualified Official as a configured executor. |
| Q-N10 | Where does evidence live? Keep evergreen schemas, contracts, decision rules and runbooks in-tree. Store time-stamped raw data, PR outcomes and experiments under versioned research artifacts/issues, with durable links and clear dates. |
| Q-N11 | How do work assignments fit real automation? Use existing `exec:sonnet`/`exec:opus` and supported effort labels now. Add another vendor only by migrating roadmap schema, dispatcher and labels together in a separately approved change. |

Q-N2–N4 apply OQ's same-SHA ruling and the observed single-context ruleset. Q-N5–N9 reconcile product goals and implementation. Q-N10–N11 reconcile the published operating policy. (OQ:348–390 · C:159–174 · docs/goals.md:40–45,65–78 · docs/hard-decisions.md:14–19,223–229,486–513 · AGENTS.md:29–33,42–44 · docs/roadmap.md:35–45,75–76 · E:506–520)

**Three decisions missing even from E's new set**

- **Which post-merge checks must prevent landing?** Recommend moving any check whose failure is unacceptable on main into group obligations, beginning with the measured docs and crate-alone cases. Keep an independent push/A1 proof and measure the extra group time before adopting. (.velnor/rust-ci.toml:6–9 · rust/docs_post_merge.rs:6–17)
- **What release subject can survive squash?** Recommend a signed manifest binding source subject, any embedded commit, profile, architecture, builder, artifact ID and digest. Authorise candidate preparation on a stable source identity, plus a hold and pre-tag preparation when that identity differs. (REL:106–151,187–253 · Arch:575–586)
- **Who can rotate verifier policy or recover split publication?** Recommend one owner-approved home-site authority, an auditable epoch change, retraction of affected live successes and a rehearsed same-authority cold verifier. A partition cannot promote a second status writer. (Arch:598–625 · OQ:112–124)

## 8. What the generator can make possible

### Compile two products from one verification definition

Generate an Actions workflow **and** an executable verification contract with stable semantic identity. Today the project already has typed Rust plans and a renderer, a declared-workflow model, a release generator and a pure verdict function.
Separate semantic fields from scheduling fields in the canonical encoding. For example, moving the feature guard to another worker changes allocation. Removing its default-versus-feature comparison changes what a green means.
Make the compiler produce both a semantic diff and an execution-layout diff. Show the owner the exact test IDs, input rules or proof requirements a policy change would remove, with a small counterexample change where removal affects the outcome.
Verify on a fixed corpus that changing topology alone conserves the multiset of `(obligation, variant, subject, witness count)`. This makes a scheduling optimisation reviewable independently of a coverage reduction. (gen/plan_render.rs:15–39 · model/plan.rs:85–145 · rust/required_job.rs:77–151 · rust/verdict.rs:207–315 · ci/lane_guard.rs:28–58)

Let the same contract drive `velnor explain`, local advisory runs, remote `verify`, group resolution and release eligibility. A developer can inspect precisely why the release docs check ran on push although the PR passed.
The compiler can calculate static key prefixes and expose a budget for resolving pending inputs. At run time the trusted resolver fills actual binary, materialised tree, executor and environment fields. An unresolved field means fresh work.
Give failures a durable, reproducible handle referencing input manifest, approved command, semantic ID and artifact digest. Let a local run replay that operation while the trusted witness and signing boundary remain remote. (Arch:54–101,489–522 · UX:206–239,240–343 · ci/io/plan.rs:38–92)

### Look ahead in the queue without predicting a green

Compile a graph of exact pending obligations over the observed queue prefixes. Predict which immutable data to place locally and which already-requested computations can have another claimant.
When a contributor drops from a group, recompute its real subject and transfer only surviving claims whose keys still agree. Preserve the reserved gate/A1 slots when a low-priority agent and a group share work.
Treat delivery, compilation and evidence as separate state machines. A speculative fetch can succeed on a predicted group and only verification of the group's actual GitHub SHA and current policy can publish its green.
Test the queue scheduler under reordered webhooks, cancellations, force-pushes, stale refs, fleet restarts and site loss. Price unused prefetch and the storage it evicts. Advance to speculative **execution** only with the owner-approved demand policy. (OD:18–20 · Arch:519–556,598–625 · OQ:184–195,266–279)

### Let the generator design the benchmark it must beat

For every new shape, emit the reference shape, common obligation digest, measurement points and a workload-specific decision rule. The reference executes in a non-minting role during the experiment. Graduation replaces the prior production route.
Run the tuned conventional generated workflow on the same hardware, storage warmth, protected queue, witness contract, acceptance corpus and budget as the new system. Then compare Native to configured Official under the same dispatcher to isolate executor overhead.
Preregister S1 planted failures, S2 offer/cancel races, S3 paired end-to-end workload, S4 loaded replay, S5 reuse eligibility/cost and S6 warm/cold correctness. Randomise arm order, publish negative observations and count cancelled attempts.
Bound receipt savings by compatible occupied work, cancellation savings by outstanding unshared work and critical-path savings by measured compiler/discovery time. A feature with high hit count but negative total savings loses.
Compare strict within-workload equivalence before publishing speed. State which hardware/commercial arms actually ran. A fast verdict can be a control-plane win even while Native's raw job execution remains slower. (Arch:23–42,1083–1113 · Plan:651–665 · C:148–157,176–198 · docs/goals.md:44,60,78)

### Put the hardest failure modes in the acceptance contract

| Failure to model | Required demonstration |
|---|---|
| Head policy silently removes a check | The trusted base manifest and candidate projection disagree on an ID. Only a signed, scoped reduction can account for it. A planted failure still blocks after a shape refactor. |
| Equal source tree, different observable commit | Build with the two real commit stamps and compare unsigned artifacts. Hold promotion if the receipt's source differs. Test missing artifact, renamed asset and re-uploaded content. |
| Equal binary, different test data | Change a sibling fixture, an absent path, directory entries, an auxiliary executable or the test's cwd. The key changes or the obligation runs fresh. Full-tree staging stays intact. |
| Two witnesses share one bug or one generation | Track cold/warm, host, ancestry, cache-source and order correlations by class. If measured independence fails, require fresh changed-obligation work at the group and investigate. |
| Cross-site mutable index races | Crash a writer between upload, reserve/finalise and status write. Lab readback must reveal one current generation, no duplicate publication and no trusted receipt from an incomplete object. |
| Latest success outlives its authority | Revoke a signing or policy epoch while multiple PRs share the SHA and a group is about to land. Fence publication, invalidate open successes, reconcile all affected groups and audit commits landed in the window. |
| Time-sensitive work repeatedly appears to hit | Vary clock, seed, network response and feature flags in A2. Assign affected obligations C5 until inputs are explicitly fixed, keyed and reviewed. |
| Snapshot clean in Git, stale on disk | Compare every staged materialised file, mode and symlink. Exercise deletion, submodules, generated outputs and a crash during overlay creation. Reject a mismatched checkout before the first step. |
| Plan is correct but measurement favours short failures | A workload with identical failures, cancellations and queue load must yield the same population accounting. Report time-to-red and verified time-to-green separately. |

When observations disagree, preserve the failing evidence, stop satisfaction from the affected kind, re-run cold, diagnose the first divergent input or execution observation, then requalify. A fast-red lane can shorten the development loop without issuing success.
The first publishable result is a trace that joins a green requested head, the exact passing GitHub group, the audited landed commit and the checksums of the released bytes. That same trace gives the first defensible push-to-merge measurement. (Arch:60–117,543–556,588–625 · docs/goals.md:34–44,73–78)
