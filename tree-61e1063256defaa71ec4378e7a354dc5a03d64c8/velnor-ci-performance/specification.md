# Implementation specification

## 1. Objective

Reduce CI completion time on GitHub-hosted runners and Velnor Scale Set workers. Keep the same required checks and execution semantics. Use `velnor-actions` to generate all consumer workflow changes. Keep the official GitHub runner and isolated per-job DinD.

This specification defines new requirements. A proposed field or path is not an assertion that the current product already supports it. Reuse an existing typed contract where one exists.

Read `analysis.md`, `evidence-index.md`, and `branch-inventory.csv` first. Use the source and runtime identities in those files as starting points. Refresh their current state before a merge or deployment.

## 2. Model and execution contract

Use **GPT-6-Luna**, reasoning effort **max**, for the coordinator, implementation, test implementation, integration, release work, and fixes. Use **GPT-6.1-Sol**, reasoning effort **medium**, for every source review, design review, implementation review, performance-method review, and final verification review.

Resolve the exact provider model identifiers from runtime metadata. Record the requested name, resolved identifier, effort, task, and agent ID. Do not guess an identifier from a display name. Do not use an alias that can route to another model. Do not use automatic fallback. A role without the required verified model must not perform that role's work.

The coordinator delegates implementation work aggressively. Assign separate file ownership. Limit simultaneous compilers to the available CPU, memory, and disk. More agents must not mean more competing full workspace builds. Sol reviewers produce specific findings. Luna implementers resolve them. Sol reviewers then verify the resolution.

Work without questions. Resolve ambiguity from source, tests, primary documentation, and measurements. Keep one integration branch per repository where practical. Make small commits after coherent changes. Push verified progress frequently. Do not overwrite another agent's changes. Merge current main into a working branch instead of rebasing shared work.

## 3. S01 — Evidence and measurement contract

**Related findings:** F01–F12. **Priority:** P0.

Create one measurement record per job and one per meaningful phase. Include repository, run, attempt, job, task, lane, PR head, tested merge, generator source, generator digest, official runner version, daemon digest, image digests, and toolchain. Include the actual Docker provider, guest CPU architecture, allocated vCPU, memory, and data filesystem for local workers.

Record these timestamps separately: run created, job created, demand received, admission, worker preparation, runner registration, job assigned, setup start/end, checkout start/end, tool restore/install, source restore/fetch, object restore, Clippy, test build, test execution, documentation, report upload, post actions, worker exit, and cleanup. An unavailable measurement is null with a reason. It is not zero.

Measure origin bytes, local seed bytes, expanded bytes, restore duration, export duration, peak physical disk, peak memory, CPU pressure, and eligible queue depth. Keep compiler-estimated saved time separate from measured wall time. Do not add nested phases twice.

Fetch all job pages and the correct attempt. Retain both successful and failed observations. Classify cancellations, timeouts, queued jobs, and missing logs separately. Calculate matched ratios only from complete comparable samples. Preserve the raw evidence outside public logs when it contains private repository details.

**Acceptance:** The collector reproduces the supplied 43-row snapshot and AMQ phase comparison. It does not turn queued placeholders into zero wait. It records a new candidate product tuple after each rollout. Tests cover pagination, reruns, missing fields, duplicate records, clock problems, and partial artifacts.

## 4. S02 — Fast branch integration

**Related findings:** F04, F05, F07, F11. **Priority:** P0.

Use #29 as the first MBX lifecycle integration candidate. Compare `codex/mbx-lifecycle-fix` against it and current main. Port unique useful fixes. Do not merge competing lifecycle owners. Verify existing #26/#27 changes before reapplying anything.

Review all #25 changes. Extract worker-progress fixes separately when tar qualification would delay an otherwise independent repair. Adapt the unique capacity change to the new worker code. Pull coherent test-preparation and selection fixes from `perf/cache-selection-qualification` in small commits.

Treat #28 and #20 as broader integration work. Extract needed patches promptly. Do not wait for their unrelated changes. Reconcile the remaining related changes later. Give every branch a disposition: merged, equivalent fix merged, superseded, retained fixture, or excluded with evidence.

Treat `owned-source/*` as foreign-source references until ancestry and executing consumers are established. Do not merge unrelated repository roots into the product. Use a qualified upstream release when it contains the necessary change. An unconsumed source snapshot is not a deployed optimization.

**Acceptance:** Each small patch has a Sol review, targeted tests, a source commit, a release or deployment record, and a consumer result. Branch disposition covers all twenty reviewed refs and any new relevant refs. No broad draft PR is a mandatory dependency for an independent P0 repair.

## 5. S03 — Generated checkout and task selection

**Related findings:** F01, F11. **Priority:** P0 for checkout; P1 for selection.

Generate one explicit consumer checkout before each local composite action. Use a workspace-local action reference. Remove the nested checkout when it has the same source purpose. Preserve a distinct checkout only when a task demonstrably needs another repository or revision.

Validate every generated local action path. Include the complete action tree in drift verification. Generate twice from clean directories and compare output bytes. Verify workflow size and referenced-action limits against the pinned platform behavior. Preserve required job identities, permissions, and protected checks.

Move cheap task selection before expensive worker allocation. Load only the tools required for planning. Do not install every compilation tool before acquiring a prebuilt planner. When dependency metadata requires a tool, document it and cache its minimal closure.

Use both base and head graphs for changed dependency selection. Include deleted files, renamed files, build scripts, generated-source inputs, feature changes, toolchain changes, and shared workspace configuration. Unknown changes must expand the selection safely.

Generate an explicit skipped-obligation receipt for work that is not required. Required must verify this receipt. A missing report, cancelled job, or failed post action must not count as a valid skip. During full paired qualification, force all required obligations to execute on both lanes. Keep this mode distinct from normal affected-work CI.

**Acceptance:** AMQ no longer downloads the consumer repository as an action archive. It has one consumer checkout. A documentation-only change does not allocate unnecessary compile workers. Deleted-input tests still select affected tasks. Full paired mode does not reuse another lane's test result.

## 6. S04 — Tool and Cargo source cache ownership

**Related findings:** F02, F03. **Priority:** P0.

Define an explicit owner for each cache layer. Derive restore and save paths from one typed value. Include a schema version, platform, exact toolchain, required component set, layout, and trust scope in the identity. Record the GitHub cache version as well as its key.

A tool payload contains the required Mise installation data, Rustup home, and executable links. Do not include registry tokens, Git credentials, shell startup files, unrelated global packages, or host-specific secrets. Use the minimal Rust installation profile and required components. Do not download documentation components when the workload does not use them.

A source payload contains immutable registry archives, index data where needed, and Git dependency objects. Do not mix mutable tool binaries and credentials into the source layer. For different hosted and local paths, either use a canonical relocatable format or separate explicit namespaces with one correct producer per namespace. Do not claim cross-lane sharing from matching key text alone.

Restore before installation. Probe the exact tool and source closure. Install or fetch only missing entries. Resolve all selected offline-build inputs, including necessary target-specific dependencies. Do not fetch an unrelated nested workspace in every job. Preserve dependency verification and locked builds.

PRs remain read-only for trusted caches. Create an authorized trusted writer to prime the cache. Verify save success, then a fresh worker restore. A PR rerun alone is not this proof. Do not write a narrow partial payload under a key that every later producer treats as a complete immutable cache.

**Acceptance:** Three fresh-worker runs demonstrate cold creation, warm reuse, and stable reuse. A lockfile, toolchain, component, or layout change produces the required invalidation. Missing source entries recover without an incorrect success. Warm compatible runs have no required tool or source-content downloads from origin.

## 7. S05 — MBX and Rust compile lifecycle

**Related findings:** F04–F06. **Priority:** P0.

Use one MBX object-cache lifecycle. Remove obsolete bundle restore, import, export, and save paths after the native route is verified. Give the MBX action the actual Rust toolchain. Use one pinned MBX installer. Before PATH reuse, require the exact executable and version. Prove that an absent executable cannot silently install latest.

Validate each cache key and restore prefix. Keep target, ABI, compiler, features, flags, profile, MBX format, and trust scope compatible. Ensure the post action uses the same binary and object store as compilation. Distinguish transport misses, missing objects, ineligible inputs, format changes, and compiler-input changes.

For test preparation, build test binaries, not an unrelated development output. Reuse the performance branch's Nextest/Cargo preparation changes. Preserve separate release or application-build obligations where required. Avoid a second equivalent test compilation. Keep doctests separate when Nextest does not run them.

Benchmark compatible compile groups. Compare per-crate jobs with a bounded group that reuses a common dependency graph. Keep per-task reports. Never share writable target directories between concurrent incompatible commands. Do not reduce required feature combinations to make caches hit.

Nextest archives are optional. Use them only when compile reuse saves more time than archive creation and transport. Bind an archive to source, target, profile, features, dynamic libraries, and toolchain. In paired runner qualification, do not use a hosted-built archive as proof that local compilation works.

Keep automatic collection safe for active users. Budget physical space for objects, targets, export staging, compressed archives, and service images. Bound export and restore sets. Use useful deltas where the qualified upstream implementation supports them. Do not disable GC or hide failed exports.

**Acceptance:** An authorized writer and two readers prove a useful object hit. A controlled one-file change recompiles its required closure, not every unrelated crate. Active-object and export races pass. Low-disk tests finish without corruption. Test preparation still produces and executes the expected tests.

## 8. S06 — Worker progress and capacity

**Related findings:** F07. **Priority:** P0.

Keep one bounded admission policy for the host. Release an exited worker's occupied capacity only after required cleanup reaches a safe state. Recover journal intent after a daemon restart. Re-read demand and occupied slots after a release. Do not hold an admission lock across image download, archive extraction, Git fetch, Docker startup, or long network calls.

Separate listener progress from slow host effects. Use bounded queues, concurrency, deadlines, cancellation, and retries. Preserve idempotence for completed-worker redelivery. Recover only the documented expired-session condition. Do not suppress unrelated API errors.

Measure the actual Docker guest resources. Use its effective CPU and memory allocation, cgroup pressure where available, and the filesystems used for images and caches. A Mac host load number or `df /` is not an adequate substitute when work executes elsewhere. Make telemetry probes asynchronous and bounded. Define a visible degraded policy for missing telemetry. Support drain mode with zero new admissions.

Tune with controlled worker counts of one, two, and four, then larger counts only when resources permit. Measure total completion time and tail latency. Include test subprocesses, compilers, DinD, preload workers, and other repositories. Do not impose arbitrary per-container limits that contradict the host policy. Prevent oversubscription through admission and task concurrency.

**Acceptance:** More eligible jobs than slots run through several waves. No job remains blocked behind stale occupancy. A healthy controller with a free compatible slot reacts to received eligible demand within 30 seconds or two normal poll cycles, whichever is larger. This is a controller target, not a GitHub queue SLA. Cancellation, restart, lost network, and Docker restart leave no orphan worker pair or false available slot.

## 9. S07 — Host action archive store

**Related findings:** F08. **Priority:** P1, after checkout-once generation.

Use the official runner's action archive cache interface first. For Linux runner 2.337.0, the inspected path layout is a repository name with `/` replaced by `_`, followed by the resolved SHA and `.tar.gz`. Derive the exact layout from the pinned runner source. Do not normalize case or invent archive names independently.

Publish an archive only after authorized resolution, digest verification, and archive-structure validation. Reject path traversal, unsafe links, duplicate ambiguous members, and incomplete downloads. Write to a temporary location, then publish atomically. Use per-entry locks to combine concurrent fetches. A cache fill cannot block listener progress.

Mount the cache before the official runner starts. Set `ACTIONS_RUNNER_ACTION_ARCHIVE_CACHE` in the worker environment. Expose only the repository/action entries authorized for this job. Never mount the host's complete private cache into an unrelated job. Never expose host credentials.

Keep the normal authorized download path for a missing entry. Quarantine a corrupt entry before use. Do not treat authorization failure as permission to read a cached private archive. Record source, bytes, and duration for every hit or miss.

Ordinary cached archives still require copy and extraction. Benchmark optional immutable unpacked trees or private copy-on-write copies separately. Do not enable shared writable action trees. Verify JavaScript, Docker, composite, pre, and post actions that access their own files.

**Acceptance:** The second fresh worker performs no origin action-content download for the seeded action set. It still resolves valid permissions and exact SHAs. A cache-poisoning test and a cross-repository access test fail safely. Archive absence completes through the normal cold path.

## 10. S08 — Host Git object seeds

**Related findings:** F09. **Priority:** P1.

Maintain one host-owned object source per repository and authorization scope. Do not keep job worktrees as the shared source. Fetch with the host's authorized service context. Do not run repository hooks, build scripts, or action code on the control plane.

Prepare a private checkout for the assigned worker. Preserve the original repository URL and exact requested commit. Include the synthetic PR merge when the workflow requests it. Do not replace it with the PR head. Handle force-updated PR refs, missing merge refs, shallow history, submodules, and LFS.

Prototype seeding with the pinned official checkout action. Verify that its cleanup and origin checks do not discard the seed. Use ordinary Git features or a narrowly scoped Velnor preparation helper. Do not invent unsupported checkout inputs. Remove persistent credentials after the checkout step.

Use private copies, reflinks, or a reference clone with dissociation. Alternatively, hold a read-only immutable object lease until the worker is gone. Garbage collection must not delete objects still used by a worker. Never expose shared mutable `.git` metadata or another authorization scope.

**Acceptance:** Warm exact-commit jobs transfer no consumer Git object content from origin. Normal authentication or ref metadata requests remain permitted. Compare `git rev-parse HEAD`, required parent identities, submodules, LFS data, file modes, and test inputs against a cold official checkout. A missing seed remains a correct cold checkout.

## 11. S09 — Tool, source, and DinD seeds

**Related findings:** F10. **Priority:** P1.

Build a content-addressed seed manifest with exact tools, components, source-lock identities, platform, digests, and authorized repositories. Keep it distinct from the consumer workflow configuration. Project only the allowed read-only seed data into a worker. Put writable installs and build output in private worker storage.

Preload required service and test image digests into each isolated inner daemon. Complete service-image preparation before GitHub's service initialization where possible. An image in the outer daemon alone is not sufficient. Compare verified OCI archives, a scoped local registry, and qualified immutable storage snapshots. Select the simplest option that improves full job time.

Cap preload download and extraction concurrency. Deduplicate simultaneous misses. Use least-recently-used eviction only for unleased data. Preserve reserve disk space. Do not warm every branch, target, or image without a demonstrated consumer.

**Acceptance:** Fresh inner daemons start the qualified service set without repeated origin image pulls after warmup. Concurrent jobs can use the same service port without conflict. Seeds survive worker deletion. Job-specific containers, volumes, networks, worktrees, and credentials do not survive cleanup.

## 12. S10 — Host diagnosis and heavy tests

**Related findings:** F05, F07, F09, F12. **Priority:** P1.

Record whether execution is native amd64 or emulated. Measure the provider's extraction, compression, file creation, Git checkout, and compile throughput. Compare Linux-managed storage with Mac-shared storage. Attribute network delay with time-to-first-byte and bytes-per-second measurements, not a guessed geographic cause.

A native amd64 execution host is a deployment option only after emulation is established as a major limit. Do not label native arm64 results as linux/amd64 parity. Keep target and artifact identities explicit.

Trace heavy tests by service pull, service startup, migration, fixture, body, and cleanup. Reuse a fixture within a test scope only when it preserves the test contract. Keep concurrent data isolated. Replace fixed waits with bounded readiness conditions where source evidence justifies the change. Do not increase parallel tests until CPU, memory, storage, and service pressure are measured.

**Acceptance:** The heavy Ethereum suite retains its tests and assertions. Before/after results use the same source or a documented semantics-preserving test-fixture change. Report fixture and body time separately. Demonstrate cleanup after successful, failed, and cancelled test runs.

## 13. S11 — Reports, release, and deployment

**Related findings:** F04, F07, F11. **Priority:** P0 for release provenance; P1 for full reporting.

Use unique report identity across lane, task, run, and attempt. Verify artifact IDs and digests, not ambiguous names alone. Bind each accepted result to the tested commit, obligation, tools, and input closure. Reject missing, duplicate, stale, mismatched, cancelled, and failed results. Do not hide failed post actions behind an earlier task success.

Publish an immutable source-bound generator after required source checks for that exact commit. Use focused tests for rapid PR review; retain protected checks. Do not publish a trusted product merely because a workflow can be manually dispatched. Keep provenance permissions separate from untrusted build execution.

Regenerate the full consumer output tree with the exact published binary. Commit its manifest and generated actions together. Verify generated drift with that binary. Publish daemon and images separately when only those products change. Record their digests. Drain old workers before replacing the daemon where necessary. Verify the running process and newly created worker images, not only the installed files.

Keep a tested rollback tuple. Roll back a faulty small release without reverting unrelated completed improvements. When access is unavailable, report the exact blocked gate and continue independent work. Do not claim deployment or completion without the required proof.

**Acceptance:** Each rollout has source SHA, build run, artifact digest, consumer commit, actual deployed digest, and validation run. Protected main is green for its current commit after integration. Required rejects the negative fixtures above.

## 14. S12 — Performance gates and final completion

**Priority:** P1 confirmation; P2 maintenance.

Use a fixed workload set: AMQ, an RPC binding crate, a migration crate, a gRPC server, eth-processor-app, and the complete Required workflow. Match target, toolchain, features, test set, worker resources, and cache state. Measure both full paired mode and affected-work mode.

For each small release, require focused tests, one cold functional run, one authorized cache writer when applicable, and two independent fresh warm readers. Ship that small qualified release without waiting for all later work. Record each observation separately. Additional comparable observations may support median and nearest-rank p95 analysis, but no fixed sample count is a release or completion gate. Do not describe mixed workloads as repeated samples of one workload.

The following are **proposed acceptance targets**, not measured improvements:

| Gate | Target |
| --- | --- |
| Consumer archive duplication | Zero consumer-repository action-archive fetches; one required consumer checkout |
| Warm required tool and source content | Zero origin content bytes when the exact verified seed/cache closure is present |
| Warm local action content | Zero origin content bytes for the seeded exact action set |
| Warm Set up job | Median at most 30 s; p95 at most 60 s for a fixed qualified workload class |
| Healthy local admission | The S06 response budget, with no stale-capacity stall |
| Warm hosted end-to-end CI | At least 30% below a controlled baseline with the same required work |
| Warm Scale Set end-to-end CI | At least 50% below a controlled baseline with the same required work and host resources |
| Ordinary changed-code CI | Treat 120 s as the desired operating budget; report every violation, including queue and post time |

The heavy observed suite already spends about 230 s in test execution. A 120-second complete-pipeline result therefore needs real test scheduling or fixture work. It cannot come from cache claims alone. Do not manufacture that result by omitting required tests.

A failed target remains an open performance item. Record its measured cause and next experiment. Do not replace a target silently. A product can receive a small functional release while the full performance goal remains open.

Complete P2 work: archive/extraction tuning, restore-byte efficiency, seed eviction, bounded diagnostics, version freshness, tests for later runner upgrades, qualification documentation, and CI/CD coverage beyond this Rust run. Exclude unrelated product refactors with evidence. Keep every related branch disposition explicit.

**Final gate:** Every mandatory checklist item has code or an evidence-backed not-applicable disposition, a Sol review, and the specified test evidence. All required performance targets pass, or the goal remains explicitly incomplete. Do not claim that no optimization can ever exist. Claim only that the defined workload, identified causes, and review findings are resolved.
