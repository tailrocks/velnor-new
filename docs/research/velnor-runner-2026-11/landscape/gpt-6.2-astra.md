# What to borrow from other runners: GPT-6.2-Astra

| Field | Value |
|---|---|
| Model | GPT-6.2-Astra |
| Model id | `gpt-6.2-astra` |
| Client | codex-cli 0.169.0 (codex exec) |
| Reasoning effort | high |
| Date / run | 2026-11-19 |
| Brief | landscape brief v1 (2026-11-19): the runner landscape dossier, the final design and the owner decisions |

This is the model's own analysis of what Velnor should borrow from, or avoid in, other well-known CI runners, based on the public landscape dossier, unedited except for formatting.

---

# What Velnor should borrow from other CI runners

## Verdict

- **Hosted and self-hosted Actions fleets.** Against GitHub-hosted runners, ARC, Depot, Namespace, Blacksmith, WarpBuild, RunsOn, Actuated and Ubicloud, Velnor's design can avoid an entire repeated verification request. Those fleets already deliver isolation, standby capacity or local storage that Velnor has yet to qualify. The dossier has too little on BuildJet for a ranking. Cirrus's Apple VM expertise addresses a platform Velnor has paused.
- **General CI agents.** Buildkite, GitLab Runner and CircleCI offer working models for disposable capacity, durable local state or timing-based splitting. Velnor ties a passing test to the exact executable and merge subject. A tuned persistent agent on the same EPYCs could match its Cargo execution time.
- **Remote execution.** BuildBuddy, EngFlow and the Bazel/Buck2 class can distribute work at action granularity. Velnor can carry evidence from an ordinary Cargo build through GitHub to release, but its whole-tree default and cold second witness make wide edits expensive.
- **Large presubmit systems.** Velnor requires every obligation before landing. Google's TAP and Meta's Sandcastle offer better-developed models for prioritising failures and matching work to workers. Their reported selection gains buy a different coverage contract.
- **Merge queues.** GitHub, Mergify, Trunk, Aviator, Graphite and Uber's SubmitQueue supply techniques for overlapping checks and recovering from ejections. Velnor adds per-obligation evidence and cold audit, but has yet to show how often a real GitHub group can use that evidence under load. None of these comparisons establishes a speed ranking.

## Ideas to borrow

| ID | Technique | Who does it | What changes in Velnor | Expected gain and basis | Correctness argument | Owner fit | Agent-hours |
|---|---|---|---|---|---|---|---:|
| B01 | Standby pool with webhook-driven demand | Depot, GitLab Runner, Buildkite Elastic | §§5.4–5.5, 8.2, 9. Size empty environments by class and eligible arrivals | Closing the gap between the ~6 s historical ARC pod start and the 0.3 s sandbox-start target would save ~5.7 s per start | Single-use lease, current request generation and proven teardown | fits 12, 13, 20–21 | 12–20 |
| B02 | Fork a sealed volume per job | Namespace, Buildkite hosted | §§5.4, 7.2–7.3. Give each job a private writable fork of an immutable base | One runner target took 8.6 s to copy on ext4 and 0.25 s to clone on XFS, an 8.35 s setup difference | Content manifest and cold witness validate compile inputs and outputs | fits 8, 10–11, 21–23 | 8–16 |
| B03 | Sticky disk as a placement preference | Blacksmith, RunsOn, Depot builders | §§7.3, 7.5, 9. Advertise resident generation digests and bound the wait for them | On the measured unchanged root, 39.9 s remote mbx build versus 0.56 s local clone plus build allows at most ~39 s extra wait before other costs | Only eligible, verified digests count. A job writes its own clone | fits 8, 10–11, 23 | 8–14 |
| B04 | Restore a clean whole sandbox | BuildBuddy, WarpBuild | §§5.4, 5.8–5.9. Specify snapshot contents, identity and restore qualification | A 50 s restore replacing 30 s of setup loses 20 s even before snapshot creation. WarpBuild reports 45–60 s snapshot boots | Only pristine images may be captured. Unproved isolation selects fresh provisioning | fits 13, 21 for the design | 6–12 |
| B05 | Proxy stock cache actions on GitHub Official | Depot, Ubicloud | §§5.3, 5.7, 7.2. Qualify the cache-v2 wire path separately from blob storage | Depot's historical 1 GB at 100–150 MB/s versus 1 GB/s implies ~5.7–9 s saved on transfer alone | Authorised immutable index, digest check, no cache input to receipt minting | fits 3, 20, 22–23 | 24–48 |
| B06 | Co-locate extracted caches and batch site reads | Namespace, Blacksmith, Depot, EngFlow | §§7.1–7.2, 9. Resolve names once, then schedule near verified bytes | On the design's loaded Mac, 1.2 s GET + 17 s extract versus 5 s local clone saves ~13 s on that registry fixture | Location cannot change the resolved manifest, scope or digest | fits 8, 22–24 | 8–16 |
| B07 | High-clock workers for the serial floor | Blacksmith, Ubicloud Premium | §§8.5, 9. Identify surviving serial spans now, evaluate hardware later | At the design's estimated 1.3–1.5× serial rate, a CPU-bound 60 s span saves 14–20 s | Key machine features and qualify independent witnesses on the new class | needs an owner question for procurement | 4–8 |
| B08 | Action cache, CAS and batched FindMissingBlobs | REAPI, BuildBuddy, EngFlow | §§6.1–6.5, 7.1–7.3. Add a missing-digest query behind BlobStore and keep proof in the ledger | One measured cache edit introduced 4 of 419 chunks. A batched presence check can avoid 415 redundant chunk PUTs versus blind upload | Blob presence supplies bytes only. The ledger alone can satisfy witness slots | fits 10–11, 20, 23 | 12–24 |
| B09 | Split tests by historical duration | Buildkite Test Engine, CircleCI | §§6.2, 8.5. Partition exact test IDs within each witness | For 240 s of independent test work, four slots and an 80 s longest test, the lower bound is 80 s before setup | Each witness covers every expected ID exactly once, with order-sensitive suites kept intact | fits 10–11 | 12–24 |
| B10 | Flake quarantine with equivalent blocking coverage | Buildkite Test Engine, Google TAP, Trunk | §§1.3, 6.2, 8.5. Track discordant attempts and isolate the failing property | 10 of 124 push runs had a one-direction false failure. Preventing 3 such incidents in a comparable cohort would lower the rate by 3/124 ≈ 2.4 percentage points | A failed required witness blocks. Replacement coverage must already gate the same property | fits 11 with equivalent coverage | 6–12 |
| B11 | Test impact analysis with a conservative dependency closure | Google TAP, Trunk, Aviator | §§6.1, 6.3–6.4. Separate impact predictions from the proof needed to reuse a result | Six valid unchanged records out of ten otherwise-new two-witness obligations avoid 12 witness executions | Derive the full inventory from the base. Reuse only complete keys enforced for every possible observation | fits 10–11 | 16–32 |
| B12 | Predict likely failures and run them first | Meta Sandcastle, Google Transition Prediction | §§1.1, 6.3, 9. Let scores order eligible work | If a 5 s failing test moves ahead of 60 s of work on the same lane, actionable red moves from 65 s to 5 s | A score can change priority only. Green still needs the whole vector | fits 11–12 | 4–10 |
| B13 | Share work across queue prefixes, bisect failures, fast-forward proof lookup | GitHub MQ, Mergify, Trunk, Aviator, Graphite, Uber SubmitQueue | §§6.2–6.3, 8.2–8.3. Bound speculative prefixes and join actual groups by exact key | Four groups needing the same new comparable key can share 2 witnesses instead of 8. All four still require a verdict | ALLGREEN, separate subject vectors, fresh request-dependent checks, only GitHub lands | fits 7, 10–12 | 16–32 |
| B14 | Move corroborating tests off the merge path | TAP asynchronous testing, Trunk optimistic merge | §§6.5, 8.3–8.4. Keep only the asynchronous audit half of the technique | §4 budgets ~10 runner-min per landed commit for A1. Asynchrony moves that work off the gate, saving zero executions | Every required result must already pass before merge. A1 gates trust promotion and release | fits 7, 10–11 for audit only | 4–8 |
| B15 | Lock mirror updates and pin readers across garbage collection | Buildkite agent | §7.4. Coalesce fetches per node/repository and protect live object closures | Eight same-SHA cold misses on one node need one successful upstream fetch instead of eight, saving seven transfer attempts | Verify the requested commit and materialised tree before the first step | fits 11, 24 | 6–12 |

Section numbers refer to the [architecture][A]. Owner numbers refer to the [binding decisions][O].
Vendor behaviour comes from the [landscape dossier][L] unless marked **from my knowledge**.
Velnor measurements above are observations cited in the architecture. Worked examples are conditional estimates.
Hours estimate agent implementation, conformance and review **beyond work already specified in v3**.
They exclude traffic collection, shipping hardware and waiting for the merge queue. The gains overlap.
Implement the proposed runner components in Rust. Velnor Native remains the default. A deliberate configuration choice selects GitHub Official, and both use the same dispatcher, platform and storage contracts.

**B01 — Warm the environment, then admit the subject.** Depot separates standby provisioning from assigning an ephemeral instance.
Keep clean, credential-free slots by site and resource class on the existing five nodes. Refill from eligible demand and count idle memory against §9.
A webhook wakes the dispatcher. A released controller checks arming, current head and generation before claiming a slot.
Draft pushes and ready-for-review do no work. Applying the label on a ready PR starts the current head, a labelled push replaces it immediately, and removal cancels it.
An actual GitHub group is an independent request. Acquired Actions jobs keep the broker/JIT path. A pool miss provisions a fresh environment.
Time delivery, queue wait, claim, mounts and first process separately. The measured 0.25 s clone covers only one of those operations. [Depot][depot], [GitLab Runner][gitlab]

**B02 — Give forks distinct commit rules.** Namespace and Buildkite expose private writable forks and publish a successful job's cache generation.
Keep Velnor's XFS reflink for the fork, or make a private copy when unavailable. A stock cache save may publish an immutable name in the job's own ref.
A successful PR exit cannot publish a trusted Cargo generation. Seal the input and dep-info manifests, compare changed paths against that generation,
preserve modes and freshness rules, and require the §7.3 cold digest comparison and §6.5 A1 before promotion.
If a build script or proc macro can read inputs outside the proven closure, build its affected units cold. Inject an older-mtime failing edit and a new undeclared file.
Retire a disagreeing generation. The 0.31 s no-op build after cloning is evidence for unchanged inputs only. [Buildkite volumes][bkvol], [Namespace volumes][nsvol]

**B03 — Make disks sticky to content.** Record which eligible nodes have a sealed generation, an extracted cache manifest or verified image layers.
Rank candidates by predicted completion time including queue wait, setup and execution. Keep affinity soft when the second witness needs a distinct node and generation.
A lost disk becomes a miss. Retain only approved generations under the per-node budget and give every job a fresh writable upper layer or clone.
A raw reattached workspace, a branch name, an old daemon and a mutable Docker tag supply no provenance.
The ~39 s crossover is an idle-node illustration. Under contention, calculate it for the slower witness and preserve reserved merge and A1 capacity. [RunsOn][runson], [Depot builders][builders]

**B04 — Define what a whole-sandbox snapshot may contain.** BuildBuddy shows why chunked disk and lazily restored memory can cut setup. WarpBuild shows why booting a snapshot can also cost more.
For any backend advertising restore, require a sealed template made before assignment, keyed by image, platform, kernel, CPU features, tools and isolation policy.
Start with disk-only templates. Never capture a previous job's memory, credentials, entropy state, leases, network connections or process tree.
Supply fresh identity and private writable storage at restore. Exercise cancellation, crash recovery and cross-job state detection.
Choose restore only when its loaded cost plus amortised capture beats fresh provision plus setup. Today this is a contract and conformance design for Docker and Kubernetes.
A backend without a proved implementation keeps the warm single-use path. No third runtime or VM snapshot service is a dependency. [BuildBuddy][buddy], [WarpBuild][warp]

**B05 — Qualify the stock action end to end.** Depot's proxy returned S3 URLs. Ubicloud used a runner fork.
Neither establishes that Velnor can redirect its pinned Official Worker and action unchanged.
**From my knowledge**, upstream toolkit cache v2 reads the Results URL and its save path selects an Azure Blob SDK client.
Inspect the exact bundled action revision before implementing anything. If needed, serve the client's signed blob operations, including block upload/finalize and ranged restore, over the selected store.
Route cache RPCs only, preserve GitHub logs, artifacts v4 and run completion. Derive permissions from the authenticated job and commit a manifest once all chunks verify.
Check own/base/default ref ordering, immutable concurrent saves, prefix outputs, cross-OS versions, interrupted uploads and Native↔Official archive round trips on both backends.
Exchange cache bytes across executors only under those semantics. Executor-specific verdicts and reference-role writes remain isolated.
Until the pinned path passes, acquired Official jobs use GitHub's cache. The stock action still pays archive creation/extraction on a hit. [Depot cache][dcache], [toolkit URL selection][toolurl], [toolkit v2 client][toolcache]

**B06 — Put extraction next to execution.** Keep extracted immutable trees on L0 NVMe and compressed content-addressed blobs in the site tier.
Resolve all candidate names at the home index before checking local contents. A stale local prefix must never outrank an exact entry elsewhere.
Prefetch a known lockfile-derived key after admission, then resolve again at the action step. Batch independent metadata queries across the 35 ms site link.
Docker defaults to local-only. Kubernetes defaults to local plus qualified per-site RustFS. Keep optional cross-site replication asynchronous and routes independently configurable for each data class.
A missing or corrupt cache object is a miss. Missing release bytes stop publication. A1 and the cache index must never infer durability from a nearby copy.
Track extraction CPU, bytes from each tier and L0 evictions beside locality hits. Four fully read reflinks can occupy ~57 GB of page cache at the observed 14.29 GB apiece. [Namespace volumes][nsvol], [Blacksmith cache][blackcache]

**B07 — Price the serial part of a verdict.** Keep the five EPYC 9454P workers, each with 96 threads, 128 GB and NVMe/XFS, split across DE and FI.
On current traces identify which surviving link, compiler or test span holds up the slower witness. Exclude tests deleted with their job type.
If 40 s of a 100 s path are serial CPU work, speeding only that part by 1.5× gives 60 + 40/1.5 ≈ 87 s before placement.
A faster first witness earns no green-time saving if the second still takes 100 s. Record both waits, memory pressure and the opportunity cost in concurrent slots.
After floor fixes, apply §9's capacity triggers and seek an owner decision for a paired hardware trial. Requalify the execution fingerprint and output digests before reuse.
Blacksmith's reported boost clock and Ubicloud's Premium results justify investigating single-thread latency. Neither measures this workload. [Blacksmith hardware][blackcpu], [Ubicloud Premium][ubip]

**B08 — Separate action lookup, byte availability and authority.** REAPI maps a digest of command, input Merkle root and execution properties to output descriptors.
Add a batched digest-and-size presence query to Velnor's configured store. A sender transmits missing chunks and pins the verified closure through atomic manifest publication.
The 415/419 saving counts requests and depends on residency. The design's roughly 1% new bytes was one edit. Neither figure predicts the overall cache hit rate.
An action-result descriptor can locate a binary. A Velnor verification record additionally binds executor identity, observed exits, inventory, witness slots, scope and active epoch.
An eviction between probe and read is recoverable by fetching by digest or recomputing. A corrupt, incomplete or revoked record cannot satisfy an obligation.
Use the existing store and ledger. This requires neither a REAPI server nor a Cargo-to-Bazel migration. [REAPI specification][reapi], [EngFlow][engflow]

**B09 — Balance the work inside each witness.** Learn durations by stable test ID, binary, profile and execution class.
Schedule longest first into eligible partitions. Give unknown IDs a conservative time estimate and run them. Use the released inventory and bind the executable, tools, inputs, order seed and shard layout.
Each witness must independently report the exact expected ID multiset with passing exits. A missing, duplicate, skipped, timed-out or cancelled shard blocks.
Ordering and shared fixtures can change behaviour even with complete ID counts. Keep interacting tests together. Qualify independent partitions against unsplit runs with planted order/fixture failures in lab C.
Until then run the original suite for receipts. Preserve the complete Git differential corpus and split looped cases only when every original assertion survives.
The break-even includes binary movement, shard setup, transfer and contention for both witnesses. Four shards cannot shorten one indivisible 80 s test. [Buildkite splitting][bksplit], [CircleCI splitting][circlesplit]

**B10 — Quarantine the source of noise without hiding its property.** Start with zero retries for verdicts. A rerun in a diagnostic namespace records timing, seed, node and contention.
Keep the failed required verdict. Investigate the §1.3 timeout starvation and event-file race first. Count unique false failures in paired samples in both directions.
Two false failures in seven days already open an issue under v3. Give it an owner, reproduction, replacement property and expiry.
Move the noisy test to diagnostic execution only after an independently blocking equivalent check has been demonstrated with the same planted fault and the inventory change is authorised.
Expiry restores the original blocker unless the fix has landed. Owner approval alone cannot make a missed regression acceptable under decision 11.
The first win is fewer unnecessary ejections. At independent 8.1% false failures, four queued entries have a 1 − 0.919⁴ ≈ 29% chance of at least one, a sensitivity example, not a measured queue rate. [Buildkite splitting][bksplit], [Google TAP][tap]

**B11 — Use impact analysis to find work that may already have proof.** Start every request with the full released obligation vector and intact checkout.
A graph can propose a smaller key. Accept it only with a reviewed contract that accounts for source, transitive tools and executables, config ancestors, enumerated directories, symlinks, failed lookups and non-file inputs.
On a cache hit no process runs to expose a missing dependency. A past read trace or a clean sample therefore cannot justify excluding a path.
Constrain the obligation's possible inputs by construction or keep its whole-tree key. A missing enforcement capability stays wide. Uncontrolled clock, network, randomness or event identity makes the work fresh under C5.
In this repository 79 of 86 files using `CARGO_MANIFEST_DIR` climb above their package. Count enrolled *obligation-seconds* before forecasting a gain.
Exercise cross-root fixture changes and newly created sibling files. Suspend suspect reuse immediately, revoke the affected epoch on a wrong key and re-execute. [Google TAP][tap], [Aviator batching][aviator]

**B12 — Spend predictions on time to first red.** Begin with a simple baseline of changed-package tests and recent failures first.
Compare an ML ranking only if the baseline leaves material waiting time. Use the same released test set, permitted orderings, independence requirements and capacity.
Scores have no path to the required status. Missing history, a bad model or an unfamiliar root restores ordinary scheduling.
Meta reports detecting over 99.9% of faulty changes with predictive selection. The remaining misses preclude using such a score as permission to skip.
Record time to first *confirmed* failure and time to all-green separately. Predicting a pass contributes zero to the latter's proof. [Meta study][meta], [Google transition prediction][predict]

**B13 — Batch execution, keep a verdict for every prefix.** On admission, verify each latest PR head. Speculative queue-tip trees may populate candidate records in spare batch capacity.
For each real group, rederive its inventory, ordered base and keys at GitHub's actual SHA. Resolve current comparable records, run misses and C5 work, then post the group's own status.
Fast-forward here means resolving an identical verification vector for the new subject after these checks. GitHub alone advances the branch.
**From my knowledge**, GitHub documents separate controls for build concurrency and merge limits. Raising the latter cannot combine CI builds. Keep ALLGREEN, depth two and zero minimum wait under §8.3.
After a failed prefix, bisect in diagnostic batch capacity. A single reproducible monotone culprit among eight ordered changes can be narrowed in three halving decisions.
Two interacting PRs can each pass alone. Preserve the failing combined result and let GitHub issue replacement groups. A diagnostic subset cannot green any other prefix.
Charge speculative runner-minutes and ejection cost. In the 172-entry replay, equal trees were 16% historically. The 60–75% projection depends on quicker enqueue and still permits C5 misses. [GitHub queue settings][ghq], [Mergify][mergify], [Graphite][graphite]

**B14 — Keep asynchronous confirmation behind an already complete gate.** TAP's optimistic submission can shorten waiting by accepting a chance of a later failure.
That version conflicts with decisions 7 and 11. Velnor can move only independent corroboration, audit and promotion work behind landing.
Journal A1 for every landed first-parent commit, including intermediate commits of one landing. Reserve a slot per site. A lag over six hours holds trusted generations and releases under §6.5.
A disagreement freezes affected reuse and publication, invalidates open successes and triggers a cold review of the exposed window. This containment cannot retroactively make a false green correct.
A required test still running asynchronously keeps the pre-merge status pending. [Google TAP][tap], [Trunk batching][trunk]

**B15 — Lock writers briefly and keep readers alive.** Serialise fetch, ref publication and maintenance per configured repository per node.
After acquiring the writer lock, check whether another request already fetched the exact commit. Pin the object closure for each outstanding reader before releasing the lock.
Read-only alternates borrow objects. **From my knowledge**, Git documents that pruning in the source can break a borrowing clone and that `--dissociate` copies borrowed objects locally.
Use durable reader pins respected by GC, or dissociate a job that needs its own repository. Recovery may drop a pin only after proving the reader absent.
Materialise from the immutable base, apply the Δ, then verify commit, whole tree or requested sparse projection, modes, symlinks and submodule gitlinks. Apply §7.3 source times.
A failed verification refetches/reseeds and blocks execution until it passes. Race a forced ref update, concurrent GC and a reader crash in qualification.
The ≤1 s checkout target versus 6 s historical median includes all these steps. A warmed mirror alone earns no such claim. [Buildkite mirrors][mirrors], [Git clone][gitclone]

## Ideas to reject

- **Optimistic landing, mute-on-failure and retries until green.** All three can turn an observed failure into a landed commit. A rollback still leaves a failed commit in history.
  Use early failure reports for developers, complete every required witness before success, and use A1 to decide when candidate bytes can become trusted.
- **A new merge queue or direct fast-forward of `main`.** Several fastest-runner critiques suggest reclaiming GitHub's 19 s group-creation and 26 s landing medians.
  These come from different spans. Their sum is a planning allowance, and decision 7 assigns landing to GitHub. A green tip cannot cover a failing intermediate commit.
- **Last-success-wins volumes as build evidence.** A PR that exits zero may still have omitted a test, used stale output or written data under an incomplete key.
  User cache keys may intentionally be approximate. Keep that convenience on the Actions path and exclude those bytes from receipt-producing verification.
- **Predictive skips or blanket fan-out.** A model can miss a novel regression. A test can depend on an earlier test's state.
  Retain whole-suite execution where partition independence is unresolved and use predictions only to shorten time to actionable red.
- **Per-push speculation before the label.** A mirror may fetch objects and an empty pool may replenish. Neither authorises a checkout-specific compile or test.
  Keep pre-push verification an explicit agent request in a sealed environment and keep draft/ready transitions idle. Decision 12 also requires the latest head to pass before queue admission.
- **A new platform, fleet or runtime on the critical path.** A Bazel rewrite, a production Firecracker backend, purchased Ryzen nodes and macOS release restoration all exceed the present design.
  Reconsider individual mechanisms under decision 17. Per job type, pass the oracle, migrate every caller and delete the old route together. Retain the deliberately selectable Official executor and the two required backends.

## Where Velnor should lead

**Make a verified obligation portable from a developer's pre-push run to a released binary.**
No system in the [dossier][L] documents this complete contract for GitHub Actions and Cargo. The same released inventory can resolve a sealed pre-push run,
reuse equal candidate records across concurrent PRs, verify each exact GitHub group and promote the digest of the qualified release artifact.
New comparable keys still need two independent witnesses. A1 cold-checks each landed commit before trust advances. Reuse across stages does not remove those executions.
Keep development-profile tests and qualify the actual optimised release bytes separately as §8.4 specifies. A missing digest at release holds publication instead of triggering an untracked rebuild.

**Bind every green to the tree actually tested.**
Strengthen §§5.5 and 8.2 by recording the subject kind, SHA, verified tree, inventory digest, executor identity, epoch and request generation at the status writer.
If head A fails alone but passes when combined with queued fix B, the combined proof cannot green A. Verify A's own vector for its head status. Prepare A+B separately and join exactly matching obligations.
Unlabelling must fence late completions and leave an unarmed PR blocked. §8.2's unchanged prior success and shared-SHA cases need a lab A admission test.
If GitHub cannot express distinct eligibility for two PRs at the same SHA with one App status, block ambiguous admission and take the per-PR policy question to the owner.
Keep a distinct status for each real group SHA. A stale write, removed label, destroyed group, unavailable fence or missing witness must never convert a pending request into success.

**Give operators a reason for each saved or repeated execution.**
At lookup, record whether a witness was reused, joined, newly scheduled or forced fresh, with the exact mismatching field or C5 input.
Record the wall time of the slower witness and the occupied minutes of *both*. A single Docker host can run jobs locally but cannot fill two distinct-node witness slots by starting two containers.
Uncontrolled external inputs force fresh execution. If lab C2 finds correlated twins, rerun the changed obligations on the real group even on identical trees. A root with unequal warm and cold binaries keeps its dependants cold.
Sample or audit work even outside the planner's predicted changed set. A selector that missed an input can make a changed-only audit miss it too.
Publish proof and timing separately for Native and deliberately configured Official. Reference runs on each backend exercise the same failure cases in isolation and never mint production evidence.

**Prioritise by the failure each change removes.**
First close subject-binding, mirror-reader lifetime and immutable-generation gaps. Next add missing-blob batching and witness-aware placement, then qualify timing splits and the Official cache proxy.
Reserve loaded capacity for the two witnesses and A1 before scheduling diagnostics, replication or speculative prefixes. Use the §10 per-job-type gates and delete the superseded route at cutover.
The nine [fastest-runner critiques][F] disagree about how much a warm conventional runner could save and which long tests survive migration.
Across the critiques, the largest uncertainties concern unproved narrow keys, time-dependent obligations, witness correlation, page-cache pressure and a changing PR mix. [Comparison of the nine critiques][C]
Report label-to-verdict and push-to-merge by PR class, including lockfile, new executor and wide changes. Count actual exact-vector hits separately from identical trees.
The 35 s verdict estimate concerns the 47% helper/generator cohort. It cannot price a currently unqualified cache proxy, a new processor or the slower half of a cold/warm witness pair.

[A]: ../../pr-worktree/docs/proposed/velnor-runner-architecture.md
[O]: ../../pr-worktree/docs/research/velnor-runner-2026-11/owner-decisions.md
[L]: ../../pr-worktree/docs/research/velnor-runner-2026-11/runner-landscape.md
[F]: ../../pr-worktree/docs/research/velnor-runner-2026-11/fastest/
[C]: ../../pr-worktree/docs/research/velnor-runner-2026-11/fastest-comparison.md
[depot]: https://depot.dev/docs/github-actions/overview
[gitlab]: https://docs.gitlab.com/runner/executors/docker_autoscaler/
[bkvol]: https://buildkite.com/docs/pipelines/hosted-agents/cache-volumes
[nsvol]: https://namespace.so/docs/architecture/storage/cache-volumes
[runson]: https://github.com/runs-on/runs-on
[builders]: https://depot.dev/docs/container-builds/overview
[buddy]: https://www.buildbuddy.io/blog/fast-runners-at-scale
[warp]: https://www.warpbuild.com/docs/ci/features/snapshot-runners
[dcache]: https://depot.dev/blog/github-actions-cache
[toolurl]: https://raw.githubusercontent.com/actions/toolkit/main/packages/cache/src/internal/config.ts
[toolcache]: https://raw.githubusercontent.com/actions/toolkit/main/packages/cache/src/cache.ts
[blackcache]: https://info.blacksmith.sh/task/blog/co-located-artifact-caching-github-actions
[blackcpu]: https://info.blacksmith.sh/task/blog/faster-disk-io-github-actions-runners
[ubip]: https://www.ubicloud.com/blog/ubicloud-premium-runners-40-faster-builds-10x-larger-cache
[reapi]: https://github.com/bazelbuild/remote-apis/blob/main/build/bazel/remote/execution/v2/remote_execution.proto
[engflow]: https://docs.engflow.com/re/index.html
[bksplit]: https://buildkite.com/docs/test-engine/test-splitting
[circlesplit]: https://circleci.com/docs/guides/optimize/parallelism-faster-jobs/
[tap]: https://abseil.io/resources/swe-book/html/ch23.html
[aviator]: https://docs.aviator.co/mergequeue/concepts/batching
[meta]: https://arxiv.org/pdf/1810.05286.pdf
[predict]: https://hackthology.com/speculative-testing-at-google-with-transition-prediction.html
[ghq]: https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/configuring-pull-request-merges/managing-a-merge-queue
[mergify]: https://docs.mergify.com/merge-queue/batches/
[graphite]: https://graphite.com/docs/graphite-merge-queue
[trunk]: https://docs.trunk.io/merge-queue/optimizations/batching
[mirrors]: https://buildkite.com/docs/agent/v3/git-mirrors
[gitclone]: https://git-scm.com/docs/git-clone
