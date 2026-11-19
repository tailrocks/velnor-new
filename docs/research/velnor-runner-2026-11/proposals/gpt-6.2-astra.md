# Proposal by GPT-6.2-Astra

| Field | Value |
|---|---|
| Model | GPT-6.2-Astra |
| Model id | `gpt-6.2-astra` |
| Client | codex-cli 0.169.0 (codex exec) |
| Reasoning effort | max |
| Date / run | 2026-11-18 17:13–17:53 UTC |
| Brief | v1 (2026-11-18): the goal, problem, material, owner's hypotheses, questions 1–8 and 8a, and [the evidence pack](../evidence.md) |

This is the model's own full answer to the panel brief, unedited except for formatting.

---

## Summary

Move Plan, Ledger CLI, the lints and both required checks onto reserved, clean cluster capacity first.
Keep GitHub’s official worker and change where the work runs, what survives a job, and which work a merge actually requires.
In my replay of 50 successful PRs, changing only the hosted-job wait to 12 seconds moved median time to Required from **27.7 to 11.4 minutes**. This is a counterfactual, before cluster contention. `audit/replay.txt:8`
Maintenance-helpers then finished last in 44 of those runs. Its log shows roughly four minutes preparing fixtures and three and a half minutes executing tests. Both deserve engineering time. `audit/workload.txt:3`
Build each distinct configuration once, keep immutable compiler objects on NVMe, split the genuinely long tests, and carry trusted evidence from the exact merge candidate to main.
My target for a warm ordinary PR is **3–5 minutes to verdict**. With immediate queue admission, budget **4–9 minutes from push to merge** according to what the merge candidate changes. Section 5 gives the arithmetic and the cases outside it.
Build Velnor’s Rust placement, cache and evidence services first. A native Actions worker earns its place only if it beats this same warm system on real PRs with equal results.
The five current workers are enough to run that experiment. Their spare capacity changes by the minute.

## 1. Where the time goes

**Measurement basis.** `M` means I read code, queried an API, ran a command, or recalculated from raw records.
`P` means the supplied pack or a cited owner measurement. `E` means an estimate, experiment setting or acceptance target.
Versions and line numbers identify sources. Times are UTC. Paths without a prefix belong to `tailrocks/velnor-new@95de9b704b`.
`legacy/` means `tailrocks/velnor-legacy@1b9fc038`, `runner/` means `actions/runner@7afce9e`, `arc/` means `actions/actions-runner-controller@89f34c4`, and `hosts/` means the owner’s `velnor-runners` directory.
`audit/` is the measurement directory beside this proposal. `evidence.md` is the supplied pack one directory up.

**Three clocks matter.** Runner-minutes sum active job residence, including I/O. Verdict time follows the longest dependency chain.
Push-to-merge also includes workflow admission, reviews, entry into the merge queue, retries and dispatch to main.
A hosted-job queue starts at the job’s `created_at`. Time before GitHub creates the job has a separate owner.

| Population and clock | Measured result |
|---|---|
| M, Nov 4–18, PR successes and failures, workflow start to update | 1,382 runs, p50 17.47 min, p90 60.04. `audit/replay.txt:1` |
| P, scale-set era, creation to update, successes and failures | 77 sampled PRs, p50 26.7 min, p90 92.3. The older 91.6-minute baseline pooled 11 push and merge-group runs. `evidence.md:75`, `~/.cache/velnor-p3-ci-baseline.md:14` |
| M, same sample, active work | Median 52.92 runner-min/PR, 64.62/group, 61.95/main push. PRs had 39 job records but only **25 executed** at median. Groups and pushes executed 39. `audit/replay.txt:3` |
| M, scale-set-era job creation to start | Hosted p50/p90 4.06/23.56 min across 1,248 active jobs. Cluster 0.22/0.32 across 3,944. `audit/replay.txt:6` |
| M, Nov 18, successful PR subsample | 13 runs, p50 71.77 min to Required. Median summed Plan/verdict/final-gate waits 56.97 min. The ratio of those medians is 79%. Earlier day medians were 12.60 and 5.83 min of gate waits. `audit/replay.txt:9` |

The earlier 60-PR rolling window in issue 574 reported a 61.8-minute median including cancellations. Its dates and inclusion criteria explain another apparent disagreement with the longer baseline. `audit/comment-574.md:3`. [Original PR baseline](https://github.com/tailrocks/velnor-new/issues/574#issuecomment-6969131218)

The pack’s “39 executed” includes zero-duration skipped records. I counted `success` and `failure` job conclusions separately from skips.
For maintenance-helpers, 75 of 77 PRs actually ran it. For runner-host with local-mode, 57 ran it. `audit/active-costs.txt:1`
At the original queue snapshot I recounted 86 hosted jobs, including 61 CI, from the 52 saved job lists.
My fresh GET scan at 17:27 found 21 queued Ubuntu jobs, one macOS release job and zero scale-set jobs. The two run-list queries overlapped by one run. Both observations cover this repository. `evidence.md:387`, `audit/queue.txt:1`

**A real critical path.** Run 44307369619 spent 497 seconds before its first job record.
Plan then waited 1,150 seconds and ran 54. Ledger waited 1,849 and ran 57, in parallel with Plan.
Rust needed both. Required verdict waited 456 seconds and ran 37. Required waited another 791 and ran **two**.
Total creation to Required was 4,306 seconds. A separate successful run, 44344003184, waited 965 seconds for its two-second Required job. I fetched both from GitHub. `audit/jobs1.json:1`, `audit/jobs2.json:1`, `audit/recent1.json:1`. [First run](https://github.com/tailrocks/velnor-new/actions/runs/44307369619) · [Second run](https://github.com/tailrocks/velnor-new/actions/runs/44344003184)

**What moving the labels buys.** I replayed each successful run’s actual dependencies, retaining job duration, pre-job admission, dependency-release gaps and cluster waits.
I replaced each hosted queue interval with 12 seconds, close to the measured cluster median. Replaying the original intervals reconstructed Required completion to the second on all 50 runs.
The changed replay gives p50/p90 **11.37/18.68 min**, versus **27.71/79.15**. On Nov 18 alone it gives **13.93/36.87**, versus **71.77/123.53**.
The bad day’s pre-job admission p90 was 25.45 min. Measuring from pod creation would hide that delay. On a quiet PR the same replacement actually adds 30 seconds. `audit/replay.ts:1`, `audit/replay.txt:8`. [Quiet run](https://github.com/tailrocks/velnor-new/actions/runs/44041557720)

**Breakdown inside the jobs.** M unless marked P. Step medians below cover 112 executed default runner-host jobs on successful PR, group and push runs. Step timestamps have one-second resolution.

| Work | Observed cost and interpretation |
|---|---|
| Starting a job and fetching tools | Runner-host setup 5 s, checkout 3 s, pinned tools 10 s at median. P, across scale-set jobs, mise setup 2 s and mbx setup 1 s. Installing, checking, and launching an action all contribute. `audit/replay.txt:14`, `evidence.md:335` |
| Sources and toolchain | 48/48 Rust logs missed the source restore, 10/10 hosted Plan logs hit, and 58/58 applicable logs reported no restored Rust toolchain. P, typical runner workspace downloaded 435 crates, root 522. In run 44307369619 runner-host’s explicit fetch took 5 s. A download count gives no CPU/network split. `audit/workload.txt:1`, `evidence.md:323` |
| Compilation and linking | Building test executables took 81 s median, 103 s p90. Clippy took 65 s and rustdoc 58 s median, concurrent with tests after the build. The records contain no separate link timer. `audit/replay.txt:17` |
| Test execution | Default runner-host’s unit/integration step took 182 s median, 392 s p90. Its wait-all step covers the same interval. The two host configurations together cost 6.23 + 5.49 = 11.72 runner-min per sampled PR. CLI cost 6.57. `audit/replay.txt:18`, `audit/active-costs.txt:2` |
| Maintenance | 8.41 runner-min per PR. In eight logs with phase markers, median pin-mirror 83.8 s, diagnostics-build 106.6 s, source-fetch 5.4 s and nextest 211.9 s. Native image source inventory took 45.8 s in seven. Phases include child processes and some mixed work. `audit/workload.txt:3`, `mise.toml:97` |
| Cache and artifact transfer | P, hosted Plan restored about 86 MB. Cluster source misses took about 1 s just to discover. All 3,636 sampled state restores skipped. I found zero populated mbx remote settings. In the example host job downloading the plan/CLI took 2 s each and uploading reports 3 s. Transfer of a hypothetical warm build cache is an additional cost. `evidence.md:332`, `evidence.md:348`, `audit/workload.txt:1` |
| Repetition | 120/159 successful group runs had a main push on the same API SHA. E, 120/159 × 61.95 ≈ 46.8 runner-min per successful group at that historical push median, before retaining the push gate. This estimates capacity, not elapsed time saved on the merge already completed. `audit/replay.txt:2` |

The maintenance log for job 134191252252 exposes actual compiler and test time. Cargo reported 44.43 s release, 88 s dev, 36.78 s dev and 2.86 s test builds.
Nextest then reported 245.927 s for 54 tests. Pin-mirror also calls Cargo quietly. This prevents assigning all otherwise unexplained seconds to downloads or all eight minutes to compilation. `mise.toml:103`, `mise.toml:148`. [Job log](https://github.com/tailrocks/velnor-new/actions/runs/44126222992/job/134191252252)

M, the missing primary source key in **48/48** Rust logs exactly matched the paired Plan’s successful key. Check the cache service URL, ref visibility and version computed from paths/compression before changing the key formula. The failed lookup does not identify which of these differs. The owner also found crate downloads inside the maintenance subprocess that the pack’s top-level counter missed. `audit/source-compare.txt:1`. [Maintenance phase investigation](https://github.com/tailrocks/velnor-new/issues/1305#issuecomment-6980497787)

For future attribution, record GitHub creation, offer, assignment, pod Ready and first command, then timestamp action resolution, source bytes, rustc/link spans, nextest start/end and Results acknowledgement. Use overlapping spans for Clippy/docs/tests and one critical path for wall time.

P, seals for generator succeeded in 6/18 recent runs, ci-helper in 4/18, and root, runner and local in none.
The sampled producer failures included both the tip check and verification failures. The workflow’s green status is a poor proxy for publication.
Instrument actual published and consumed object digests. `evidence.md:370`, the State seal workflow. [Owner’s seal analysis](https://github.com/tailrocks/velnor-new/issues/1305#issuecomment-6980735625)

**Time to merge.** I joined PR-head CI creation, queue timeline events and `merged_at`. CI creation on the final head is the observable proxy for push.
The pack’s 45 sampled group records represented 44 PRs. I matched 41 merged final heads by repository branch and SHA. Trigger-to-merge p50/p90 was **48.73/373.40 min**. First documented enqueue after that trigger to merge was **24.00/250.20 min**. This is a group-selected sample and includes waiting for people, other PRs and re-entry. `audit/merges.ts:1`, `audit/merges.txt:1`
For PR 1234 it took 56.1 min from that CI trigger to merge, including 35.7 min from first enqueue and a re-entry. PR 1284 took 373.4 min, including 250.2 min after enqueue.
For PR 1080 the respective times were 109.8 and 27.4 min. These overlapping clocks explain why a faster runner alone cannot guarantee a merge deadline. `audit/merge-times.txt:1`. [PR 1234](https://github.com/tailrocks/velnor-new/pull/1234) · [PR 1284](https://github.com/tailrocks/velnor-new/pull/1284) · [PR 1080](https://github.com/tailrocks/velnor-new/pull/1080)

## 2. Verdict on the owner’s hypotheses

| Hypothesis | Verdict | Expected effect and falsifier |
|---|---|---|
| Replacing JavaScript actions with Rust dramatically accelerates CI | **Fails** for a dramatic whole-pipeline speedup. | M, for runner-host, median checkout plus mise action plus mbx action is about 3 + 2 + 1 = 6 s. Even making all three free saves roughly 1% of an 11.37-minute critical path if those steps occur once on it. Other jobs have their own action budgets. Git transfer, compiler work and tests still run. Time JS CPU separately from children, bytes and waiting before replacing an action. `audit/replay.txt:15`, `evidence.md:335` |
| A Rust-only worker beats the official worker plus ARC | **Unproven.** | M, current cluster dispatch takes 13 s median across the full assignment path. The 11.37-minute routing replay uses the official worker’s recorded execution times. E, removing 5 s on each of three serial stages would save 15 s. Require a further ≥30 s or ≥10% median end-to-end improvement on the matched warm workload, with equal tail and correctness, before adoption. `audit/replay.txt:7` |
| Kubernetes makes the cluster parallel | **Partly holds.** | M, one worker exposes 48 physical cores and 96 logical CPUs. A single pod used 83.7 CPUs in the pack despite a two-CPU request. E, six memory-admitted heavy slots per worker gives up to 30 across five prior to other tenants. Kubernetes places pods. The build graph, jobserver and tenant admission determine useful concurrency. `audit/machine.txt:1`, `evidence.md:518` |
| Node-aware local caches beat remote caches | **Partly holds.** | P, the GitHub blob path delivered 13.6–22.4 MB/s. E, 1,000 MB needs 45–74 s before decompression. A node-local immutable clone can avoid that transfer, and site storage can avoid a cold compile on a different node. A cold node with no useful objects gains nothing from locality alone. Measure bytes and critical-path seconds per tier. `hosts/state/log.md:31` |
| Workflows can select exactly the verification a change needs | **Partly holds.** | M, the median PR already skips 14 of 39 records. In the last 100 first-parent changes, 33 touched only the nested runner, 17 only generator, and 12 changed a workflow, action or declared tooling path. These counts identify where to verify input independence. E, proving maintenance irrelevant saves its 8.41 runner-min on such a PR. Unknown reads, changing external inputs and nondeterministic checks require execution. `audit/workload.txt:24`, `audit/active-costs.txt:1` |

Rust is a good implementation language for the scheduler, verifier and host agent.
The interventions with measured leverage change a dependency, remove repeated work, or avoid moving bytes.

## 3. Runner architecture and GitHub compatibility

**GitHub owns the workflow.** It interprets triggers, filters, reusable workflows, matrices, dependencies, concurrency, environments and merge-queue protection.
The generator writes ordinary workflow YAML with ordinary job names, permissions and `runs-on` labels.
GitHub admits and assigns the job. The runner receives a materialized job message with steps, contexts, endpoint capabilities and a lease.
Re-reading YAML from the PR as scheduling or permission authority would let a checkout disagree with the service. Local mode can still exercise the same workflow. `runner/src/Runner.Listener/Runner.cs:753`, `docs/goals.md:75`

**Use two interfaces.** The GitHub-facing interface implements scale-set capacity and job lifecycle. The machine-facing interface admits an isolated execution with a CPU, memory, storage, trust and locality budget.
The released generator places the checks inside GitHub’s DAG. The machine scheduler places the jobs it is offered.
A node agent owns its immutable stores, scratch allocation, cleanup and physical resource accounting. That is enough to optimize the official worker and, later, benchmark a native one.

| Component | Adopt | Redesign or leave out |
|---|---|---|
| Current Velnor | Scale-set wire types, checks on group and image identity, write-ahead launch intent, ownership proofs, capacity reservations, reconnect and local qualification. The new runner is a separate workspace with eight members. P, the pack’s macOS run passed 430 tests. `Cargo.toml:4`, `crates/velnor-runner-host/src/journal_schema.rs:23`, `evidence.md:889` | Translate Docker object IDs into backend-specific proofs. Keep a single capacity authority. Give the cache its own lifetime and keep job data transient. |
| Kubernetes runtime proposal | Use its reusable runtime boundary, cluster/namespace identity, Pod and PVC UIDs, owner object and guarded cleanup. M, PR 1324 was open at `7e8b0805` and the document is absent at the studied main revision. `audit/kubernetes-runtime.md:124`, `audit/kubernetes-runtime.md:195`. [Proposal](https://github.com/tailrocks/velnor-new/pull/1324) | Qualify Linux storage, admission, socket semantics and reconnect independently. Preserve the Docker local-mode backend. |
| Legacy Velnor | Port expression fixtures, composite context tests, file commands, cache-scoping tests, broker/Results fixtures and isolated backend contracts where current upstream agrees. Its expression code names upstream v2.337.0. `legacy/crates/velnor-runner/src/expression/mod.rs:13`, `legacy/crates/velnor-runner/src/execution/composite_scopes.rs:1` | Its 26 action identities are an allowlist. Its checkout lacks sparse checkout used by this repo. Its scale-set worker itself launches the official runner. Do not carry over a repository-name-only adapter dispatch, daemon per slot or an old protocol as the compatibility contract. `legacy/crates/velnor-runner/src/action.rs:378`, `legacy/content/docs/guides/execution.mdx:139`, `legacy/crates/velnor-runner/src/scaleset/worker/runner.rs:1` |
| Official runner | Use the shipped worker as production executor and semantic oracle. Keep resolution by immutable action SHA, Node, shell, composites, containers/services, credentials, outputs, logs, post actions, cancellation and background joins. `runner/src/Runner.Worker/JobExtension.cs:374`, `runner/src/Runner.Worker/Handlers/HandlerFactory.cs:58` | The process boundary and action downloads can be timed. Avoid copying its internal .NET process layout into a native implementation without a measured reason. |
| ARC | Keep the working listener, capacity protocol, JIT and disposable runners for the first rollout. ARC already supports idle runners. `arc/cmd/ghalistener/scaler/scaler.go:135`, `arc/cmd/ghalistener/scaler/scaler.go:339` | Replica count lacks node cache inventory, trust or thread budgets. Reconciler, runner object, JIT and pod setup need not all occur after demand arrives. Replace this control loop only when its measured tail or placement prevents the target. |

Legacy’s Docker and Firecracker backends expose different capabilities. Its microVM profile excludes the Docker mbx integration and limits action/cache transport. Reuse isolation and teardown fixtures, then qualify the exact feature set on Linux/KVM with the same worker image. `legacy/content/docs/guides/execution.mdx:148`, `legacy/content/docs/guides/execution.mdx:378`

The official source identifies itself as 2.338.0 and forces JavaScript actions to Node 24. The runner resolves action packages, but cache v2, artifact transport and OIDC minting are service interfaces used by action code. A native adapter must preserve their externally visible behavior. `runner/src/runnerversion:1`, `runner/src/Runner.Worker/Handlers/HandlerFactory.cs:58`, `evidence.md:1315`

**Protocol boundaries a replacement must pass.**

1. The org App belongs to the controller. Resolve the permitted group and scale set, open and refresh the message session, advertise real free capacity, long-poll, acquire offered request IDs, journal the response, then acknowledge replay-safe messages. Respect throttling and distinguish an expired session from lost authorization. `docs/proposed/macos-scaleset-runner.md:106`, `arc/docs/gha-runner-scale-set-controller/README.md:14`
2. Provision a one-use runner through scale-set `generatejitconfig`. Its broker listener receives a reference and its run-service `acquirejob` receives the job body. Those are different acquisitions. Scale-set offers contain limited metadata. GitHub may assign another eligible job to an idle runner in the same set. Placement hints never authorize a tenant, cache, or privileged job. `crates/velnor-runner-github/src/paths.rs:20`, `runner/src/Runner.Listener/Runner.cs:753`
3. Bind the actual job ID, attempt, plan, worker and sandbox before enabling capabilities. Start after the first successful renewal. M, the official renewal loop uses 60 seconds and honors the server’s lock expiry. On loss of ownership, fence execution and publication and reconcile. `runner/src/Runner.Listener/JobDispatcher.cs:395`, `runner/src/Runner.Listener/JobDispatcher.cs:775`
4. Evaluate step conditions, expressions and command files with the delivered contexts. Run post actions in the correct order even on failure, implement background/wait-all, service readiness, working directories, masks, outputs and annotations. Preserve distinctions among failure, timeout, cancellation, skip and success. `runner/src/Runner.Worker/JobExtension.cs:374`, `runner/src/Runner.Worker/FileCommandManager.cs:181`
5. Stream masked logs, timeline and Results updates under the correct attempt. Flush durable completion, outputs and annotations, then complete the service job. Artifact, cache and OIDC operations use the scoped service endpoints. Preserve artifact IDs and attestations. A local CAS hit cannot fulfill an upload-artifact step unless the artifact the workflow promised is actually available. `runner/src/Runner.Common/JobServerQueue.cs:23`, `runner/src/Runner.Worker/JobRunner.cs:357`, `evidence.md:1295`
6. Key recovery on immutable identities. Acknowledgement loss may redeliver a message. An ambiguous acquisition, completion or delete gets a lookup and reconciliation. Expiry or a new attempt fences a late writer. Retain quarantined resources until absence of the recorded UID is proven. Retries of publish/deploy actions require their own external idempotency evidence. `audit/kubernetes-runtime.md:172`

**A native worker is a separate qualification decision.** Use a dedicated experimental label and paired, side-effect-free workflow runs. Keep the official lane authoritative.
For an optimized action, dispatch on resolved SHA, subpath, full supported input/semantic profile and runtime version. Fall back to executing the pinned action itself for an unsupported combination.
Route a known unsupported job to an official set before acquisition. If a native worker discovers an unsupported job shape only after acquiring it, fail the experimental attempt explicitly and let the separately scheduled official run qualify it. Do not transfer a consumed lease or repeat side effects.
Test expression coercion, post/save order, cancellation mid-upload, sparse checkout, fork scopes, artifacts, background failure and services against GitHub. Compare reports, log redaction, outputs, status and artifact hashes.
Follow upstream runner releases and run a real canary after each one. GitHub documents an update window of 63 days and earlier blocking for critical security updates. Never rely on advertising a version whose semantics have not passed. [GitHub self-hosted runner reference](https://docs.github.com/en/actions/reference/runners/self-hosted-runners)

**What the archive establishes.** M, I built the whole locked legacy workspace from its archived tree on macOS arm64 using Rust 1.98.1, a scratch target, five build jobs and an offline registry.
The clean dev build took 212.35 s. The isolated nextest expression selection passed 41/41.
Full-workspace nextest ran 8,222 tests across 87 binaries. **7,685 passed, 537 failed, and five additional tests were skipped.** The scratch-only sandbox rejected tests that create hard-coded global `/tmp` paths. One case waited in socket accept until I terminated its individual PID. It passed in 0.210 s when I allowed Unix sockets inside scratch.
A separate runner-package pass with that permission ran 3,268 tests, with 3,198 passes and 70 failures. P, the pack’s less restricted macOS run had 8,172 passes and 50 failures, including unresolved hash mismatches. Neither run qualifies a production Linux executor. `audit/legacy-build.clean.log:376`, `audit/legacy-nextest.log:582`, `audit/legacy-one-failure.log:25`, `audit/legacy-ipc.log:7`, `audit/legacy-runner-bounded.log:228`, `evidence.md:1058`

M, the repository API marks legacy archived and identifies velnor-new as its maintained successor.
The final commits `459159ea` and `1b9fc038` upgraded its external generator after consolidation commits `84d41850` and `e24509dc`.
I found no repository-level history entry attributing archival to performance, language or a protocol incident. The campaign itself recorded no accepted speedup.
Recover tested ideas with their original licence and notices. Treat operational incidents and compatibility gaps as qualification cases. `legacy/.github/AGENTS.md:4`, `legacy/plans/ci-performance/README.md:3`, `legacy/LICENSE:1`. [Archived repository](https://github.com/tailrocks/velnor-legacy)

Production native execution would change the explicit official-runner rule and local verification contract. The historical V2-native document is marked superseded.
The recommended first phases fit the official-runner rule. `docs/goals.md:16`, `docs/goals.md:75`, `docs/deferred/self-hosted-runner.md:3`

## 4. Cache layout, placement and trust

Make persistence a property of the **node service**. Each job still receives a fresh workspace, writable overlay, home, tool configuration and Docker daemon if needed.
A verified read-only object may outlive the job. The job has no writable host cache directory and cannot select its own trust domain.

| Data | Identity and storage | On a miss |
|---|---|---|
| Toolchains, tools, helper binaries, actions, images | Immutable node NVMe lower layers. Include actual rustc build, host/targets, component manifest, mise tool checksums, resolved action SHA and image/platform digest. Stage the next approved pin separately. | Fetch from signed/pinned origin or site mirror, verify, unpack into staging and atomically publish. Point the job’s actual `RUSTUP_HOME` and tool paths at a fresh view. |
| Cargo sources and Git | Content-checked crates and sparse index at a site mirror, node-local immutable pack by registry and checksum, Git objects by permitted repo and commit. Private namespaces enforce read ACLs on every materialization. | An isolated fetcher obtains scoped private sources, verifies lockfile/checksums, revokes access and hands an offline job the allowed files. Dependency or PR code never sees its credential. `docs/goals.md:43` |
| Compiled work | mbx objects and trusted manifests on local reflink-capable NVMe. Distinguish source closure, compiler/sysroot, target and CPU features, profile, flags, features, build-script/proc-macro inputs, native compiler/libs, image, tools and path mapping. Private Cargo targets per job and concurrent lane. | Probe node, then same-site object service. If rebuilding is cheaper, compile. Verify checksum before exposing a hit. Untracked ambient inputs or an unsupported invocation force execution. `docs/hard-decisions.md:57` |
| Test archives and fixtures | Content-addressed nextest archives or binaries plus dependency/runtime manifest, workspace layout, fixtures, dynamic libs and helper executables. Separate default/local-mode and incompatible configurations. | Build once for the configuration. Clone locally or stream only the required archive to a same-site shard. Verify manifest and binary digests before listing tests. |
| Test and check evidence | A small signed record from the trusted verifier binding policy, subject, input closure, command, suite inventory, attempts, environment and result. Logs/artifacts retain their GitHub identities. | Execute every required case. A compiler-object hit never implies a passing test. Disable verdict reuse for timing, stress, external services, stateful and nondeterministic tests under a policy that relies on independent trials. |

**Tier order.** A node-local object hit avoids the network. A same-site service deduplicates immutable chunks, distributes verified manifests and refills a new or busy node.
Publish into existing site storage asynchronously. Validation required for an external artifact stays on the job’s critical path.
Use GitHub for portable hosted-client caches and prescribed artifacts. Retain the hosted generator’s budget of 8 GB out of the project’s 10 GB GitHub-cache allowance and its correct cold path without a Velnor service. Make inter-site replication asynchronous, bounded and recoverable. `docs/goals.md:45`, `docs/goals.md:59` A site outage must permit cold rebuilds with the pinned inputs.
For scale, P saw a 1,457 MB root state bundle. E, 1,457/22.4 to 1,457/13.6 ≈ 65–107 s just to receive it from GitHub. Rebuilding a 30 s task can be cheaper. `evidence.md:363`, `hosts/state/log.md:31`

**Warm placement.** Advertise verified object families and estimated bytes by node, site, tenant and image generation.
First filter for trust, resource fit, toolchain and capability. Among allowed slots minimize the sum of queue delay, materialization, compile/test and reporting time.
Use recent measurements for the same configuration. Break near ties by stable hashing on repo/workspace/target/toolchain and spread concurrent PRs.
For example, E, a warm node free in 35 s with 4 s setup loses to an idle same-site node that can restore in 12 s. A label can choose a pool. A cache miss must never strand an accepted job.
If offers cannot identify the configuration, choose a conservatively scoped warm pool, bind actual identity after assignment, then fetch. Price mispredictions and queue tails in the replay.

**Publication and trust.** Give trusted main, merge validation, same-repo PRs, forks, organizations and release signers independent capabilities.
Under the current engine PRs restore read-only. Under the new generator’s contract a same-repo PR can write into a private PR namespace. Future pushes to that PR may read it. Forks get read-only **public-safe** data.
Neither trusted workers nor another PR read PR-produced mutable objects. Never place private sources, compiled derivatives or cached Git history within reach of a public fork.
A reviewed trusted publisher can publish verified results for its own input closure. A job-controlled “passed” file or a cache key with a trusted-looking prefix supplies no authority. `.velnor/config.toml:14`, `docs/goals.md:43`, `docs/goals.md:72`. [GitHub cache scopes](https://docs.github.com/en/actions/reference/workflows-and-actions/dependency-caching)

Stop racing main on every object write. Publish immutable, versioned manifests for a verified commit that met the protected-branch authority policy.
Only a fenced promotion can move a “latest” pointer. An older approved ancestor can seed compiler objects after exact key validation. It cannot attest a newer tree.
If ancestry is rewritten, policy changes, a manifest is revoked or the publishing run fails, discard its eligibility and rebuild.
Use temp-write, digest verification and atomic rename for CAS insertion, single-flight for identical misses, per-tenant byte/inode quotas and leases on live objects during GC.
Reflink or copy into writable targets. Cross-job writable hard links allow modifications or timestamps to propagate. Pinned seeds and protected rollback generations get separate budgets.
Retain the visible nonblocking `Cache` check for rejected restores and failed intended saves while obtaining the correct cold verdict. `docs/goals.md:42`, `docs/goals.md:67`

**Expected hits, with denominators.** E, after normal warm-up and before a fleet-wide pin change:

| Measure | Initial target and calculation |
|---|---|
| Needed approved toolchain/tool layers already on node | ≥95%. If 19 of 20 placements use a preseeded generation, 19/20 hit. Report pin rotations separately. |
| Locked public source bytes served by node/site | ≥95%. In a 100 MB fetch with ≤5 MB of new or evicted crates, ≥95 MB are served locally. Report private fetch and exact complete-source-bundle hits separately. |
| Admitted mbx compiler requests, local | E, 75% placed on a warm node × 90% exact match there + 25% × 20% elsewhere = **72.5%**. |
| Admitted compiler requests, local or site | E, if the site covers 65% of local misses, 72.5% + 27.5% × 65% = **90.4%**. At two-thirds eligibility this is only about **60% of all invocations**. Record bypass and bytes as well as saved rustc time. |
| Test verdict reuse | Start at **0%** for cross-event test reuse. Measure eligible signed proofs divided by requested checks. The observed 120/159 same-SHA pushes is an upper opportunity for that cohort, not a PR cache-hit target. |

Measure hits by tenant, target, tier and cold/warm generation. A useful hit improves finish time after lookup, decompression and materialization.
Storage failure becomes a visible miss. When proof is unavailable, the job runs the required check.

## 5. Workflow design and the time to verdict

**Change the generator that actually owns each workflow.** The main checkout says “Generated by Velnor Actions 0.1.80”. Its engine reads `.velnor/config.toml`.
It emits one job per crate/configuration, separate test/Clippy/doc target lanes and parallel check steps. Its policy makes merge_group and main full runs.
M, the pinned tree contains 16 workflow files. There is no generated `.github/workflows/rust-ci.yml` at the root. Its golden sample is in `generator/samples/velnor-new/expected/.github/workflows/rust-ci.yml:1`. The new `velnor-gen` and `velnor-ci` implement a different model. The new selector already handles affected-mode push/group events and selects everything if the base diff is unavailable.
Use the v0.1.81 routing release independently. Make the larger change in the new generator and qualify the dogfood cutover across the existing workspaces and the planned root workspace. `.github/workflows/ci.yml:1`, `.velnor/generator.lock:3`, `docs/hard-decisions.md:21`, `docs/hard-decisions.md:101`, `crates/velnor-rust/src/select_event.rs:12`

**Plan from trusted code.** A released planner loads the candidate tree as data and checks the workflow and check inventory against a protected policy.
It emits a versioned manifest of required units, dependencies, configurations, inputs, reasons for exclusions, and the expected check/test inventory.
A PR can propose a change to selection policy. That change itself receives full verification under the previous authority.
GitHub still schedules the CI workflow on PR and merge_group. Always create both required contexts, including for an empty Rust selection. `docs/goals.md:40`, `docs/goals.md:76`, `.github/workflows/ci.yml:514`

- Diff the actual tested PR merge tree and relevant base, and the actual synthetic group tree against its qualified base. Resolve changes on both sides of renames/deletions, including the previous graph. A missing base, incomplete checkout, ambiguous lock change or unplaceable path expands selection.
- Traverse Cargo reverse dependencies for normal, build, dev and target-specific edges in each relevant feature/profile. Include workspace inheritance, feature unification, manifests, proc macros, build scripts, native includes, `include_*`, configuration, examples, doctests and directly executed fixtures. The merged workspace can change feature resolution. Recompute and compare the old and new plans at cutover. `crates/velnor-rust/src/select.rs:17`, `crates/velnor-rust/src/lock_affected.rs:132`
- Maintain positive input declarations for non-Rust work. The maintenance program launches the CLI, a Linux image inventory, Python/Bun tests, examples, Cargo fetches across roots and archive-guard tests. Give each a check identity and real fixture closure. Split only after confirming that every old assertion and platform condition belongs somewhere. `mise.toml:97`, `mise.toml:189`
- For opaque scripts, enforce a hermetic declared input set or conservatively select the whole scope. A successful file-read trace on one branch cannot prove what an unexecuted branch might read. Account for Git metadata, environment, symlinks, external data and generated files. Keep formatting, Clippy, docs, tests, deny, machete, workflow lint and release qualifications wherever their inputs can change.
- Time-dependent inputs need a policy clock. Pin the advisory database snapshot or refresh it on a defined schedule with a freshness check. An external-service, timing or isolation test remains live where its purpose requires a fresh observation. Verify input independence before skipping a check. Use the scheduled full run to audit that decision.

**Build once, then balance actual cases.** Compute build units per workspace, target, profile and effective feature set. Default and local-mode are distinct.
Create a single test build and a signed inventory per such unit. Keep metadata-only checks separate where their output differs. Use Cargo/mbx reuse between compatible units.
List the tests from the built binaries, then partition them with measured timing or nextest’s stable hash scheme. Pin the runner image, cwd, dynamic libraries, fixture tree, services and environment when moving an archive.
For each shard require its exact expected case set. Reject missing, duplicated, unexplained skipped, zero-when-nonempty, failed and cancelled cases. Preserve setup scripts, retries policy, serial groups, Docker-network limits and direct harnesses/doctests outside nextest.
Do not pay an additional GitHub round trip to schedule a 3 s shard. Use a small number of generated jobs or a bounded worker group inside one job, with one declared report per shard. [Nextest partitioning](https://nexte.st/docs/ci-features/partitioning/) · [Reusing test builds](https://nexte.st/docs/ci-features/archiving/)

**The sharding limit is visible.** M, median CLI nextest wall 260.8 s, median longest single case 260.7 s across ten logs.
For default runner-host the figures were 233.6 and 233.6 s. For maintenance they were 211.9 and 55.2 s across eight logs. `audit/workload.txt:3`, `audit/workload.txt:16`
In job 134191420345 the slow CLI test exercises 40 authority rejection cases serially. The slow runner-host test in job 134191420609 loops over three abort states × two container states.
Expose those cases as independently named tests with the same setup isolation, inputs and assertions. Preserve any ordering or shared-state test as a separate case. Profile journal creation, fsync, Docker fixtures and nested runtime thread count first. `crates/velnor-actions-cli/tests/image_publication_public.rs:86`, `crates/velnor-runner-host/src/launch/bootstrap/unbound_retirement/tests/shape.rs:97`. [CLI job](https://github.com/tailrocks/velnor-new/actions/runs/44126222992/job/134191420345) · [Host job](https://github.com/tailrocks/velnor-new/actions/runs/44126222992/job/134191420609)
E, if the CLI cases cost equally, four independent lanes reduce 261 s of serial cases toward 261/4 ≈ 65 s plus fixture and transfer costs. Until cases can run independently, 261 s remains the observed single-test floor for this cohort.

**Eliminate repeat execution only on equivalent evidence.**

| Event | What runs |
|---|---|
| PR | Cancel superseded PR attempts, execute affected units and publish results for the tested subject. Only the released verifier can sign a trusted result. A fork cannot manufacture evidence for a protected event. |
| Merge group | Keep the real `merge_group` run and both required contexts. Build a fresh plan for its SHA/base, ordered group, policy and trust. Reuse only equivalent, admissible unit receipts. Run every changed, missing or non-reusable unit. Group recreation invalidates its old gate. |
| Push to main | On the exact validated candidate, run a lightweight **push** verification that checks the qualified merge proof, finishes both required checks for the pushed SHA and runs push-specific work. A bypass/direct push, changed context, missing proof or new SHA takes the applicable full/affected verification path. |
| Release, schedule, publication | Retain release-plz tag eligibility, per-target archives, SHA256SUMS and attestations. Publishers use the protected main push evidence and separate credentials. Keep clean full/scheduled and platform-specific checks. `crates/velnor-ci/src/gate.rs:50`, `docs/goals.md:77` |

P, only 7/45 sampled PR **head trees** equaled the group tree, and 34/45 heads were behind the group base. The PR head was fetched after the event and can differ from the PR test merge ref. This is not a measurement of reusable test inputs.
Store the actual checkout commit/tree, relevant parents/diff, event and policy at execution. SHA equality alone cannot cover an event-sensitive check. If repeated execution is how an existing flaky or concurrent suite detects failures, preserve its independent trials and their failure policy. `evidence.md:206`. [GitHub event semantics](https://docs.github.com/en/actions/reference/workflows-and-actions/events-that-trigger-workflows)

**Minimum honest budget.** M, the successful replay reaches 11.37 min before any cache or suite redesign. The following are E targets for a warm, ordinary source PR with no long external wait.

| Regime | Critical-path budget |
|---|---|
| A proved empty Rust selection | E, 10 s dispatch + 20 s protected plan and remaining lightweight checks + 15 s final evidence ≈ **45 s**, if the non-Rust checks also fit. The project sets a one-minute target. `docs/goals.md:53` |
| Warm selected leaf or small affected set | E, 15 s admission + 25 s plan/materialization + 35–75 s build + 50–130 s longest required checks + 15 s verdict = **140–260 s**. Allow ordinary scheduling variation and target **3–5 min p50**, conditional on this mix. |
| Warm broad change, unsplit long fixture | M/E, 234–261 s for one observed test alone, plus 40–100 s for planning, compilation and reporting = **4.6–6.0 min** before contention or another slower check. Some local-mode runs had a single test longer than 15 min. No universal five-minute promise. `audit/workload.txt:18` |
| Immediate merge admission | E, add 10–30 s dispatch and 15–45 s verification when a group can prove equivalence. A changed group needing 1–4 min of new work gives roughly **4–9 min** for an ordinary PR and group together. Measure requeues, human waiting and backlog separately, and publish their inclusive clock. |

M, the active ruleset requires `Required verdict` and `Required`, permits two groups to build and uses HEADGREEN, minimum merge size one and a five-minute minimum-group timeout setting.
That timeout applies to reaching a minimum group size. It does not impose a fixed five-minute floor when one PR qualifies.
Model queue throughput before changing concurrency. E, two 5-minute groups provide at most 24 group builds/hour under continuous demand, versus 10/hour at 12 minutes. Rebuilds and queue grouping change the PR rate. `audit/rules.json:1`. [GitHub merge-queue limits](https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/configuring-pull-request-merges/managing-a-merge-queue)

## 6. Kubernetes lifecycle, isolation and placement

Keep k3s. It already schedules work quickly when jobs target its labels. Its single control plane and cross-site network are genuine failure domains.
Give the controller durable placement and admission information. Advertise to GitHub only slots that are healthy and available after reservations for all tenants.

**Warm infrastructure, fresh job.** Pre-pull approved runner, sandbox and toolchain layers. Maintain a small number of prestarted, never-used runners or clean sandbox slots for control jobs.
Register once per ephemeral job or idle one-use runner, validate the assigned envelope, attach private scratch and the read-only cache view, execute, report, wipe, then retire the JIT identity.
Prewarm private DinD only for the pool that needs it. A completed PR’s processes, writable home, worktree, environment, Docker graph, endpoints and sockets never become the next warm slot. `arc/docs/gha-runner-scale-set-controller/README.md:14`, `audit/kubernetes-runtime.md:242`

**Trust is a scheduling constraint.** Separate org groups, credentials, namespaces, quotas, service accounts and cache read/write domains.
Enforce allowed repositories/workflows and trusted refs at the controller and server group. Anyone who can alter a PR’s YAML can request a label.
Use a clean, nonprivileged control pool for the released planner, required gates and evidence signing. Keep privileged release/publish and macOS platform lanes in their approved environments.
Run arbitrary PR code without the cluster service account, host paths, outer Docker socket, App credential, signing ability or control-plane network access. Give only the declared GitHub job permissions.
For external PRs use a verified VM/Kata/Firecracker boundary or a separate disposable host/pool with public-safe inputs. Continue using an isolated hosted runner if that facility is unavailable.
The owner-approved privileged DinD transition needs dedicated tainted nodes and a private daemon per job. A taint keeps workloads apart for scheduling. Separate hosts/VMs and enforced admission supply the security boundary between mutually untrusted tenants. `docs/goals.md:43`, `audit/kubernetes-runtime.md:331`

**Use both sites without a cross-site build filesystem.** Verify physical locations, then label the DE/FI zones, trust pools, CPU class and cache generations.
Reserve gate capacity in both sites. Prefer same-node, then same-site input data. Spread replicas for availability, but keep shards near their archive while capacity permits.
On five machines, reserve clean control slots on trusted workers, isolate the agreed privileged pool, and assign ordinary and external work only to a compatible boundary. These allocations are shared across tailrocks, ChainArgos and jackin.
If partitioning leaves too little capacity for another trust class, route it to an already isolated pool. Do not weaken its boundary to use an idle CPU. `hosts/README.md:12`, `hosts/state/RESULT.md:82`

**Admission and failure.** Use measured CPU, RSS, disk and Docker needs by class, plus weighted fair shares. Give short gates and admitted merge validation priority over superseded PRs and background cache warming.
Cap queue reservations by tenant. Avoid preempting a nearly complete compile to run a two-second gate when a reserved slot is available.
On node loss, fence the old lease, discard unpublished scratch, let GitHub reassign where permitted and run cold if needed. Persist results outside the failed node before promising reuse.
On site or control-plane loss, stop new unprovable admissions. Allow already-authorized work to finish only while its GitHub lease remains valid. Exercise partition and recovery before claiming site failover.

## 7. mbx, sccache and blaze

**Use mbx as the compiler cache.** M, this checkout pins mbx 1.29.2, nextest 0.9.151, mise 2026.11.1 and Rust 1.99.1. It configures mbx, local backend, per-job temporary storage and no test sharding.
The upstream action supports local and remote backends at the pinned action revision. Use the documented mbx protocol or a verified snapshot/import path. Pointing `actions/cache` at `target/` would restore a different contract. `.velnor/version-policy.toml:23`, `.velnor/config.toml:25`, `evidence.md:702`. [Pinned mbx action](https://github.com/jdx/mr-boxington-action/blob/88acf2bb5055/action.yml) · [mbx storage backends](https://mr-boxington.jdx.dev/github-action)

Keep Cargo responsible for ordering and freshness and preserve separate writable target lanes. Reuse immutable objects across jobs and worktrees.
The action restricts remote publishing to protected-branch pushes. Give consumers read-only access and a verified main publisher the write capability. Implement any permitted same-PR reuse through a separate private layer. Enforce writes server-side regardless of mbx environment variables. [Upstream write policy](https://mr-boxington.jdx.dev/github-action) · [Repository cache investigation](https://github.com/tailrocks/velnor-new/issues/574#issuecomment-6969256836)
Build scripts, proc macros, native outputs and path remapping need cold-versus-restored correctness fixtures. Preserve the existing build-script execution setting until its exact guarantees are qualified. `.github/workflows/ci.yml:634`, `docs/hard-decisions.md:57`

M, across 154 object-summary lines I counted 18,818 hits, 272 misses, 26,808 not looked up and 306 bypassed.
That is 98.6% of hit-or-miss lookups, but only 40.7% of all four categories. Much of it is within-job reuse.
The pack’s 40–74% per selected pass uses a different denominator and omits some first passes. Track eligible lookups, all compiler invocations, restored bytes and wall saved separately. `audit/workload.txt:2`, `evidence.md:324`

Use sccache as a controlled alternative for workloads where mbx cannot cache useful work, including a separately keyed native compiler lane if justified.
Compare Cargo alone, mbx local, mbx local plus site storage, and sccache alone on identical inputs, flags and nodes. Avoid two competing Rust compiler wrappers and double transfer. sccache’s Rust support also constrains incremental builds. [sccache Rust support](https://github.com/mozilla/sccache/blob/main/docs/Rust.md)

**Blaze does not fit the required compiler.** The checked upstream implementation describes its own `blaze-2026-10-27` toolchain and ignores the project’s Rust override.
The project requires the exact Rust 1.99.1 toolchain, components, targets and project-defined settings. An upstream incremental edit benchmark using a resident compiler answers a different question from a fresh, pinned CI build with Clippy, docs and tests.
Use blaze for optional developer feedback only, with separate outputs and no authoritative receipt. Revisit if it supports the exact pinned compiler and passes the complete toolchain/feature/profile parity suite. E, credited saving on required CI today **0 seconds**. `rust-toolchain.toml:5`, `docs/hard-decisions.md:232`. [Blaze toolchain design at the inspected commit](https://github.com/indexable-inc/blaze/blob/6e3d4da7482130bfa5f1903d89042c6b17e0d8ef/docs/toolchain.md)

## 8a. Servers and cluster tuning, ranked by gain per effort

**Observed before making recommendations.** M, I ran read-only `kubectl describe nodes`, `top nodes` and selected `get` queries for runner sets, pods, StorageClasses, claims, kubelet config, cAdvisor and filesystem stats. I made no host or cluster changes.

| Surface | Baseline |
|---|---|
| CPU and RAM | M, five workers advertise 96 CPUs and about 125.5 GiB each. cAdvisor on velnor-241 reports one socket, 48 physical cores and 96 logical CPUs. P, four workers are 9454P and one 9454. Aggregate 480 hardware threads, approximately 240 physical cores by this topology, shared among three organizations. `audit/nodes.txt:1`, `audit/machine.txt:1`, `hosts/README.md:25` |
| Utilization | M, at 17:15:47 workers reported 6%, 92%, 51%, 94%, 53%. Aggregate 287.3/480 logical CPUs, about 60%. At 17:24:28 they used 1.70/480, about 0.35%. P, individual workers reached 95% earlier. Both the owner’s low-load observation and the pack’s saturated samples fit. `audit/nodes-top.txt:1`, `audit/nodes-top-2.txt:1`, `evidence.md:502` |
| OS and runtime | M, Debian 13, kernel `6.12.94+deb13-amd64`, k3s/kubelet `v1.37.2+k3s1`, containerd `2.3.7-k3s1`. The control-plane node is tainted. All five workers had no taint or zone label. `audit/nodes.txt:14`, `audit/zones.tsv:1` |
| Kubelet and pods | M, velnor-241 reports no system/kube CPU or memory reservations, CPU/topology policy `none`, memory manager `None`, disk-only hard eviction at 5%, image GC high/low 85/80, `failSwapOn=false`. This flag does not reveal active host swap. Runner requests 2 CPU/4 GiB, memory limit 32 GiB, no CPU limit. Its privileged DinD sidecar has no resource specification. `audit/configz.json:2`, `audit/sets.txt:1`, `audit/pod-shape.json:1` |
| Pool capacity | M, min/max tailrocks 4/60, ChainArgos 4/60 and jackin 2/40. The sum of maximums is 160 runners. The scheduler can see only 4 GiB requested per runner while P observed up to 16.8 GiB in a busy runner. A replica ceiling is a poor model for simultaneous Rust builds. `audit/sets.txt:1`, `evidence.md:518` |
| Storage and images | P, OS on a small NVMe, remaining devices in LVM/XFS at `/var/lib/rancher`, `noatime`. Workers have two to five disks. M, velnor-240 reports 3.78 TB nodefs and 49.93 TB imagefs. There were zero PVCs, one local-path StorageClass and the runner used emptyDir. Live runner image was `2026-11-16.2`. `hosts/README.md:25`, `hosts/README.md:35`, `audit/storage.txt:1`, `audit/pod-shape.json:1` |
| Network | M, Flannel WireGuard across both sites. P, a three-packet FI-to-DE probe measured 35 ms, pods to US GitHub blobs about 120 ms. The owner had already reverted a BBR/buffer experiment with no gain. `audit/zones.tsv:1`, `hosts/state/log.md:9`, `hosts/state/log.md:31` |

**Host measurements still needed.** The read-only surface did not expose firmware NPS/NUMA layout, governor/EPP and effective frequency, THP, active swap/overcommit, dirty limits, inotify/FD limits, physical RAID/striping, cache filesystem reflink/ftype, active snapshotter or path MTU.
Hugepage resource capacity does not report THP. LVM aggregate capacity does not prove striping. A 96-thread count alone does not establish the right cpuset.
Record these through owner-operated host inventory on the canary before changing them. No SSH or pod exec was used.

The following order ranks expected return per engineering effort for this workload. **All gains below are E unless explicitly marked M or P.** Where the data contain no time attributable to a setting, the credited gain is zero pending a canary.

**1. Reserve slots for the short critical path.** Configure clean gate and merge-validation PriorityClasses below system-critical, tenant quotas, node labels/taints and a small warm idle pool spread across sites.
Reserve enough slots for measured burst concurrency. Size with the arrival rate × slot residence, plus a burst margin. Use weighted admission for background and releases.
The historical replay yields 27.71 − 11.37 = **16.34 min** p50 opportunity for routing plus capacity, with no second credit for priority alone.
Risk is starvation or moving the queue to an occupied cluster. Roll back the pool routing and quotas to the working ARC profile and preserve protected check names. `audit/replay.txt:8`

**2. Align compiler and test parallelism with what the pod can actually use.**
Give separate light, compile and Docker classes. For a heavy slot, trial twelve logical CPUs, 18 GiB total including sidecars, and a measured temporary-disk allowance.
Start `CARGO_BUILD_JOBS` at the pod’s useful cores and sweep 4, 8 and 12 under concurrent load. Coordinate Cargo jobservers, parallel Clippy/docs, build-script processes, nextest and per-test Tokio workers under one node budget.
Keep the explicit Docker test group limit and tests requiring exclusive threads. Instrument CPU throttling, runnable threads, pressure, RSS and completed PRs/hour.
M, tests use `num-cpus` and the runner Docker group caps at four. One packed pod consumed 83.7 CPU. E, reducing contention by just 10% on an observed 234 s test saves about 23 s. An increase in parallel jobs can instead lengthen it.
Risk is excess RSS, context switching or an idle CPU quota. Roll back class sizing and thread settings together. `.config/nextest.toml:7`, `.config/nextest.toml:15`, `evidence.md:518`

**3. Put tool and build data on an explicitly owned local volume.**
Keep the existing XFS/noatime base. Verify `reflink=1`, `ftype=1`, inode availability, quota enforcement and real clone semantics before depending on them.
Use dedicated directories and quotas or separate LVs/filesystems for containerd, immutable caches and job targets. Put compiler outputs on a directly mounted private view to avoid overlay copy-up.
Use a local PV or CSI node service with node affinity. Generic dynamic local-path PVCs alone cannot express trust, object identity or safe reuse across pods.
P, pre-seeding the older image reduced mean pinned-tool setup from 28.4 to 10.6 s across 30 lanes. E, at 20 active Rust lanes, saving another 8 s each would save 2.7 runner-min. A local build hit additionally avoids the transfers estimated in section 4.
Risk is inode/disk exhaustion or shared writes. Enforce byte/inode limits, test reflink fallback to copy, and roll back by switching the mount generation to disposable emptyDir. `hosts/state/RESULT.md:124`, `audit/storage.txt:1`

**4. Make cache and registry traffic same-site.**
Use the existing crates.io mirror with an explicit per-job Cargo config honored from `/tmp` and custom `CARGO_HOME`. Check the sparse index, crate checksums, CA, and offline fetch in the same test.
Pre-stage verified rustup distributions and OCI digests in each site. Configure k3s/containerd registry mirrors and the isolated DinD daemon separately. Check origin fallback deliberately.
P, for a 59 MB fetch the owner measured 3.5–6.1 s direct and 2.7–3.3 s mirrored. P, all 1,424 image “Pulled” events in the pack already had the image on-node, so additional steady-state runner image pull benefit in that window was **zero**.
E, 100 serialized cross-site misses × 35 ms = 3.5 s before bytes. Batch requests and localize large archives. Measure path MTU and loss in both directions before changing an overlay setting.
Retain Flannel WireGuard while the measured path meets the workload. Prove that the installed network-policy controller actually enforces the isolation rules before sharing a pool. Risk includes a stale mirror, a CA or registry namespace mismatch, or MTU black holes. Bypass the mirror to the pinned origin, restore the previous Cargo config and restore measured-good MTU. Keep WireGuard encryption. `hosts/state/RESULT.md:130`, `evidence.md:601`. [k3s registry configuration](https://docs.k3s.io/installation/private-registry)

**5. Reserve host capacity and keep eviction ahead of failure.**
Canary a total reserve of eight logical CPUs and about 13.5 GiB including system, kubelet and memory safety margin, leaving about 88 CPUs/112 GiB per worker for this capacity example.
That admits min(⌊88/12⌋, ⌊112/18⌋) = **six heavy jobs per worker** before tenant and disk limits. A separate gate slot comes from this budget or an explicitly larger reserve.
Set memory and inode eviction thresholds as well as nodefs/imagefs thresholds. When changing kubelet hard thresholds, explicitly preserve each intended default.
Ensure cache GC starts before image GC and eviction. On a 50 TB imagefs a 5% floor is 2.5 TB, so choose watermarks with the actual mount and working set in mind.
Keep image GC at the observed 85/80 until re-pulls or disk pressure justify a change. Pin the approved warm generation through rollout and evict old cache objects independently.
Expected steady-state gain on an idle node is zero. Avoiding one eviction of an eight-minute job saves up to eight minutes of repeat work. Risk is unusable reserved capacity or premature eviction. Roll back one node’s kubelet configuration and admission class. `audit/configz.json:2`. [Node reservation](https://kubernetes.io/docs/tasks/administer-cluster/reserve-compute-resources/) · [Eviction signals](https://kubernetes.io/docs/concepts/scheduling-eviction/node-pressure-eviction/)

**6. Qualify the linker and CI profiles.**
No project linker override appears in the checkout. Rust documents bundled LLD as the native `x86_64-unknown-linux-gnu` default since 1.90. The pack’s inference of a system linker needs a production command trace.
Capture the real Linux runner link command/map. Compare its baseline to mold on the same binaries, with the same symbols and tests. A macOS cross-target probe is not a native-link measurement.
Retain the project’s dev/test assertions, overflow checks, panic behavior, feature set, release profiles and deliberate package optimizations.
Try reduced debug info, incremental off for portable shared objects, and measured codegen-unit counts in a distinct CI profile only if diagnostics and checks remain equivalent. Rust codegen settings are cache identity. Keep full diagnostics where a check consumes them.
E, if link spans occupy 20 s of the 81 s build, cutting them by 25% saves **5 s**. The build record gives an 81 s ceiling for that lane, not a measured link share.
Risk is link/ABI differences, lost diagnostics or worse cache reuse. Revert to the pinned driver and original profiles, then repopulate their cache namespace. `Cargo.toml:69`, `Cargo.toml:74`. [Rust linker change](https://blog.rust-lang.org/2025/09/18/Rust-1.90.0/) · [Cargo profiles](https://doc.rust-lang.org/cargo/reference/profiles.html)

**7. Test CPU topology, SMT and frequency policy on one worker.**
Start with SMT enabled. Compare complete sibling-pair allocations and one thread per physical core at matched fleet concurrency. Record throughput as well as per-job p95.
Under CPU Manager `static`, use integer CPU requests equal to limits in a Guaranteed pod, including correctly specified sidecars. Test `full-pcpus-only` and reserve system CPUs. For shared/bursty jobs, use an honest CPU request with measured burst headroom and a memory bound.
Inspect NUMA domains and memory bandwidth first. Use Topology Manager `restricted` or `single-numa-node` only for a class that fits. If memory locality matters, configure and qualify Memory Manager and per-NUMA reservations too.
Inspect the active amd-pstate/ACPI driver and EPP. Compare current policy to a performance hint under sustained load, observing effective clock, power and thermal throttling.
E, a genuine 5% frequency improvement throughout 81 CPU-bound seconds saves 81 − 81/1.05 ≈ **3.9 s**. No observed NUMA or SMT speedup is claimed.
Risk is fragmented admission, thermals and lower fleet throughput. Canary new pools, keep existing scheduling available, and revert kubelet CPU policy/checkpoint through a controlled node recycle and the prior firmware/governor settings. [Kubernetes CPU Manager](https://kubernetes.io/docs/tasks/administer-cluster/cpu-management-policies/) · [Topology Manager](https://kubernetes.io/docs/tasks/administer-cluster/topology-manager/) · [AMD P-State](https://docs.kernel.org/admin-guide/pm/amd-pstate.html)

**8. Storage layout, RAID, snapshotter and tmpfs.**
Keep the OS on its current volume. Use the existing XFS for the first experiment. ext4 is a reasonable baseline for metadata work with copies. XFS reflinks suit private target clones. btrfs and ZFS add different CoW, metadata, fragmentation and memory costs. Measure the repository’s unpack/list/link/clone workload before choosing either.
LVM over several devices does not imply RAID0 or redundancy. Striping is an option only for rebuildable scratch after measuring device saturation. Use replicated manifests/objects for durability and test node loss. On two-disk workers, leave room for the OS and cache isolation without assuming extra spindles.
Keep the installed containerd snapshotter until identified. Verify overlay backing filesystem support and watch copy-up, layer unpack latency and garbage collection. Use bounded tmpfs for small temp files only, with tmpfs charged to the memory limit. Large Cargo targets and Docker graphs go to NVMe.
E, if materializing 2 GB takes 10 s on a pressured volume and 4 s on a dedicated one, the maximum direct saving is 6 s per such restore. A cold repull or lost slot can cost more.
Risk is losing disposable stripes, memory OOM or starving containerd. Test using a new LV/mount and a new snapshotter node pool. Roll back by pointing pods to the old volume/pool, with trusted objects available from the site tier. `hosts/README.md:35`, `audit/storage.txt:1`. [Overlay backing filesystem requirements](https://docs.docker.com/engine/storage/drivers/overlayfs-driver/) · [Kubernetes volumes and memory-backed emptyDir](https://kubernetes.io/docs/concepts/storage/volumes/)

**9. Tune Linux VM and scheduler settings only against observed stalls.**

| Knob | Canary, expected gain and evidence | Risk and rollback |
|---|---|---|
| THP | Compare current policy with `madvise` and deferred compaction, measuring page faults, compaction and p99 linker stalls. Credit 0 s until a stall is seen. | RSS growth or extra TLB misses. Restore the recorded THP/defrag policy. [Linux THP](https://docs.kernel.org/admin-guide/mm/transhuge.html) |
| Swap and overcommit | Inventory swap, cgroup swap accounting, `vm.overcommit_memory` and committed memory. Prefer bounded RAM and no build-pod swapping on latency-sensitive nodes. Preserve host recovery headroom. Credit 0 s absent paging. | Strict commit limits can cause allocation failures, permissive overcommit can end in OOM. Restore host and kubelet swap/VM policy and reduce admitted jobs. |
| Writeback | Measure dirty pages, IO PSI, fsync and p99 writeback. Derive byte thresholds from measured throughput. At 1 GiB/s, 256 MiB background and 1 GiB hard represent roughly 0.25 and 1 s of writing. These are E examples. | Too-small thresholds throttle builds, too-large cause long stalls. Restore the recorded byte **or** ratio pair and the original expiration/writeback intervals. [Linux VM sysctls](https://docs.kernel.org/admin-guide/sysctl/vm.html) |
| File descriptors, PIDs and inotify | Measure peak per-job open FDs, inotify watches/instances, PIDs and actual EMFILE/ENOSPC. Set per-class limits above observed peaks with margin and enforce per-tenant quotas. E, 0 s gain for a successful run below limits. | Raising global ceilings can exhaust kernel memory. Restore service limits/sysctls and lower concurrency. [Linux filesystem sysctls](https://docs.kernel.org/admin-guide/sysctl/fs.html) |
| Kernel and scheduling | Stay on the observed Debian kernel, distro scheduler and security mitigations. Compare a supported kernel only for a reproduced scheduler, storage or driver issue. Monitor run queue, steal, PSI, migrations and time throttled. E, 0 s assumed. | Kernel regressions or worse fairness. Boot the prior pinned kernel on the canary and revert scheduling parameters through host configuration. `audit/nodes.txt:14` |

**Experiment design.** Freeze commit, tool/image digests, cold/warm state, tenant concurrency and exact test inventory.
Alternate treatments on a comparable worker at quiet and burst load. E, start with five paired runs per setting as a noise screen, then validate over real PRs before promotion.
Measure p50/p90/p95 push-to-Required, CPU-seconds, runner-minutes, peak RSS, bytes read/written, disk/inode high water, cache outcomes and other tenants’ queue time.
Run mbx where compiler objects persist, and put any sccache experiment in its own pool. Blaze receives no required-CI capacity.
A configuration change earns credit only when the same assertions run and the bottleneck moves in the measured end-to-end trace.

## 8. Phased rollout, acceptance and failure containment

Use the same test inventory, trusted verifier and branch protection in each comparison. Start in velnor-lab as the project requires, then measure velnor-new.
Match commits or changed closures, target, features, cache state, event and arrival-hour load. Publish successful and failed verdict times separately.
Include cancelled attempts and post-merge work in total compute per merged PR. Also collect CPU-seconds, GiB-minutes and transferred bytes so larger pods cannot manufacture an efficiency win.

**Phase gates.** All thresholds below are E acceptance criteria. “Object hit” means an admitted compiler request satisfied by node or site. “PR minutes” sum active job seconds/60.
Use at least the original successful-PR sample size of 50 for rollout comparisons, spanning peak and quiet periods. Report raw distributions and per-cohort counts. Passing a wall-time gate requires matching verification evidence.

| Phase and change on velnor-new | Required measured outcome before expanding |
|---|---|
| **Route and reserve.** Deliver the independent v0.1.81 routing, with read-only, signed control jobs separated from arbitrary code. Move eligible Linux lint and Ledger jobs too. Keep approved release platforms and existing Required contexts. | **PR p50 ≤12 min, p90 ≤20 min** on a cohort matched to the 50-run replay, PR minutes ≤55, paid-by-time queue measured per gate. p90 eligible job queue ≤30 s. Object hit may remain at the current zero cross-job baseline. Compare against the replay’s 11.37/18.68 before blaming the worker. |
| **Persist verified inputs and accelerate maintenance.** Deliver toolchain layers, source mirrors and local/site mbx. Put maintenance fixtures on the same cache path and split their preparation from execution. Fix protected producer publication and expose restore/save outcomes. | **PR ≤9.5 min p50, ≤17 min p90**, ≤42 PR minutes, ≥95% warm tool layers, ≥70% object hits. E, saving 8 s × 20 tool setups, 35 s × 12 builds and 2.5 min of maintenance takes 52.9 to about 40.7 runner-min. Count overlap before crediting. A replay halving maintenance wall alone gives 9.47/16.69 min. `audit/replay.txt:23` |
| **Reduce critical tests and repeat work.** Isolate serial fixture cases, build test binaries once, balance shards and enable verified group-to-main evidence. Keep a main push gate for release consumers. | **PR ≤7 min p50, ≤15 min p90**, ≤32 PR minutes, ≥80% object hits. A replay halving maintenance, CLI and both runner-host job durations gives 6.97/14.44 min. Target ≤90 s and ≤2 runner-min for an eligible main push proof. Group/new-context tests still execute. `audit/replay.txt:24` |
| **Cut over to proved affected selection.** Integrate the new generator, verify reverse closure and declared inputs on the root workspace, revalidate queue candidates, then enable reuse per eligible check class. | **Warm ordinary PR 3–5 min p50, ≤10 min p90, ≤25 PR minutes, ≥85% object hits**. Scenario arithmetic is in sections 4–5. For automatically enqueued PRs with immediate admission, target median push-to-merge ≤9 min. Publish the inclusive final-head-to-merge p50/p90 alongside it, including queue retries and human delay. |
| **Optional native worker.** Only after an owner decision on the official-runner rule, compare stock ARC, locality-aware Velnor with official execution, and Velnor native with identical warmed placement and caches. Record paired cold paths too. | Adopt only for **≥30 s or ≥10% additional p50 saving**, no worse p90, PR minutes ≤25 and no reduction from the ≥85% eligible hit target. Require zero unexplained semantic differences. Otherwise keep the faster verified production arrangement and retire the experiment. |

**Confidence gate.** On rollout, send the same tree and plan to the full baseline and to the candidate in isolated, nonpublishing jobs.
Compare selected/skipped reasons, enumerated tests, features, outputs, task exits and final verdicts. Inject controlled changes into build scripts, fixtures, transitive dependencies, workflow policy, lockfiles and removed files in the lab.
A sampled green comparison can miss a rare false green. Enforce input closure, trusted evidence and fail-closed joins on every production run. Keep scheduled cold/full runs and sample unoptimized executions after cutover.
A mismatch blocks promotion, quarantines the affected cache/proof generation and reruns the full official lane on that candidate.

| Top failure | Detect | Contain |
|---|---|---|
| False skip or stale group proof | Changed input outside plan, different test inventory, base advance, audit disagreement | Replan on the actual SHA, invalidate the receipt, run whole affected scope or full suite. |
| Poisoned cache or private material visible to forks | Trust-boundary fixtures, unauthorized-read/write counters, checksum or cold-build discrepancy | Revoke namespace/read capability, quarantine producer generation, rebuild from verified upstream. |
| GitHub protocol/worker drift | Live heartbeat/canary, version deprecation, lease/completion errors, missing logs/artifacts or status differences | Stop experimental admission, preserve logs, return new jobs to the qualified official set. |
| Saturation, tenant starvation or warm-node stampede | Queue age by pool, node PSI, steal/throttle, disk latency/evictions, site transfer and other orgs’ p90 | Tighten fair shares, spread load, release stale reservations, bypass locality or disable expensive restores. |
| Lost node, site, or uncertain completion | Fencing epoch/UID mismatch, renewal expiry, duplicate report or lost trusted manifest | Stop writes, reconcile durable job state, cold-start a valid retry. Release operations require proof of external state before retry. |
| Faster PR paired with worse delivery | Inclusive final-head-to-merge, queue re-entry, cancelled runner-min and direct-push failures | Keep the last good generator/runner generation, preserve both gate names, fix the actual limiting stage. |

**Buying hardware.** Per section 8a, one comparable worker adds at most six example heavy slots, or 6/30 ≈ **20%** ideal heavy-slot capacity before trust partitions and tenant shares.
It buys isolation headroom and throughput when admitted work waits for cores or memory. It saves zero time on the original two-second gate sitting in GitHub’s hosted queue.
Buy only if post-routing peak queue persists while the suitable workers are demonstrably full. If disk IO, GitHub admission, a single long test or the two-group queue cap dominates, capacity of that specific stage determines the return.

## Owner questions

1. Should `docs/goals.md:16` and its local official-runner rule ever permit a native production worker that clears the paired gate? My default is official execution with Rust placement, cache and verification. `docs/goals.md:75`
2. May an equivalent protected receipt discharge a **current-run** check, and which checks require fresh independent trials at PR, merge-group or push time? My default is group-to-identical-main reuse only after that policy is explicit. Release still receives verified push evidence. `docs/goals.md:41`, `crates/velnor-ci/src/gate.rs:50`
3. Should the maintained engine retain its one-job-per-crate/full-group rules until replacement by the new generator? I recommend that cutover boundary. Changing the old engine’s selection or compiled-state trust policy earlier requires an explicit decision. `docs/hard-decisions.md:16`, `docs/hard-decisions.md:101`, `docs/goals.md:86`
4. How should the three organizations divide protected gate capacity, privileged DinD hosts and isolated external-PR capacity at peak? My default is reserved gate capacity in each site and a separate trust boundary before admitting an external workload. A sustained shortage in a specific pool is the case for another server. `hosts/state/RESULT.md:82`
