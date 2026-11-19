# Owner decisions for the Velnor runner

The owner's binding decisions, in the order they were made during this research. Decisions 1–12 were given to the Rust-only redesign round; decisions 13–24 came after it and shaped the final design. Later decisions amend earlier ones where they say so.

## Decisions 1–12

1. **Rust only.** Velnor executes GitHub Actions jobs itself with a native Rust executor. GitHub's official C# runner (Runner.Listener and Runner.Worker) is never used in production, not even as a fallback.
2. **The official runner is only a conformance oracle.** The latest official runner release runs the same jobs in a lab, and a differential harness proves that Velnor behaves the same.
3. **Latest GitHub APIs only, nothing legacy.**
   - In: scale-set sessions, broker long-poll, JIT config, run-service acquire/renew/complete, the Results service for logs/timeline/summaries, cache service v2, artifacts v4, OIDC.
   - Out: the legacy session with AES message encryption, legacy pipelines and distributed task, the legacy expression evaluator, cache v1, artifacts v3.
4. **No Node and no JavaScript actions.** Velnor implements natively in Rust the closed set of actions velnor-new's generated workflows use (actions/checkout, actions/cache, jdx/mise-action, jdx/mr-boxington-action, actions/attest, and so on). Unknown actions are refused before the job starts.
5. **velnor-legacy stays archived.** Code may be copied from it into velnor-new, reviewed and adapted to the latest APIs, and owned there. The repository is never reactivated or depended on.
6. **Experimental dependencies are fine** (for example mbx-cache and blaze). Judge them only on evidence.
7. GitHub stays the platform: code, PRs, the merge queue, the workflow YAML format and its API.
8. **No pull requests from forks.** The code is public, but only the project's own people and agents open PRs. Drop all fork handling. Design isolation for trusted authors whose code may be buggy, not for hostile outsiders. PR results must still never write into trusted `main` caches or receipts without verification: that is a correctness rule, not a security rule.
9. **macOS release builds are paused.** Nobody uses them now; the owner builds and installs on macOS locally with cargo. So no velnor-new job needs GitHub-hosted machines: every job runs on Velnor (Linux, the owner's cluster). Disabling the macOS release workflow is a phase-0 work item. macOS returns later, so keep the design open to adding it, but don't spend phase time on it.
10. **Build once, verify once, reuse everywhere (maximum speed AND maximum reuse).** Each unit of work (dependency fetch, compile, test, package, sign or attest, publish) runs once per unique input. Every later stage reuses its result by content address: PR run, merge-queue run, push to `main`, release, nightly or audit runs, and cache warmers. A release PROMOTES the exact artifacts that `main` built and verified; it never rebuilds them. Doing the same operation twice is allowed only where it is measurably faster (speculation, racing two paths, a local copy beating a fetch), with the numbers shown. Duplicate work that also makes things slower is a red flag the design must remove. Resolve the profile tension explicitly: release binaries use an optimised profile while CI tests use debug. Choose, with numbers, between testing the release build, building release artifacts once at merge time, or something better, and never rebuild silently. The same principle applies to PRs, at three levels: (a) within one PR across its pushes: re-verify only the packages a push changed and their dependents; a rebase-only push with the same tree gets its verdict instantly; a re-run with no code change, such as an `edited` trigger, costs zero; a new push never waits for the cancelled run of the previous push; (b) across different PRs: unchanged parts of the workspace are verified once and shared between concurrent PRs; stacked PRs reuse their base PR's results; two PRs with identical changes are detected by tree content; (c) before the PR: an agent's pre-push verification, if run in the same sealed Velnor sandbox, is reused instead of repeated. PR-produced results may be reused for identical inputs, but never become `main`'s trusted caches or receipts without verification.
11. **Correctness first: fast and incorrect is wrong.** Correctness is the hard constraint; speed is optimised only inside it. Any element that could turn a real failure green is rejected, however fast. A result is reused only when its key provably covers EVERYTHING that can change it: sources, toolchain, dependency versions, env, flags, features, the binaries a test executes (for example `CARGO_BIN_EXE_*`), data files, network, time and randomness. When in doubt, re-execute. For every reuse or skip mechanism you propose, give its soundness case: (1) why the key is complete; (2) the failure mode if it is not; (3) how that failure is detected (for example sampled re-execution of reused results, or a full cold audit of every landed commit); (4) the automatic fallback (reuse off until the cause is fixed). False reds (flakes) have a budget; false greens have ZERO tolerance.
12. **CI runs on demand, not on every push.** Agents push many intermediate commits, and evidence shows 45% of PR runs were cancelled, so per-push full runs waste most of their work. A push starts no heavy verification. Full PR verification starts only through the owner's label flow below; the merge queue always verifies what it merges. Merge rules do not change: merging still requires a passing verdict on the exact merged tree, so on-demand never lowers confidence. Specify the trigger through GitHub's own surface (the `labeled` and `unlabeled` events, `synchronize`, `merge_group`) **The owner's exact trigger flow (binding):** (1) a draft PR receiving pushes runs nothing; (2) marking it ready for review still runs nothing; (3) adding the PR label (for example `velnor:verify`) starts the PR's CI on the current head; (4) while the label is present, every new push starts a new pipeline and the previous one is cancelled immediately, with no stall; (5) removing the label stops all further PR runs. Nothing else starts PR CI: no auto-labelling from ready-for-review or auto-merge. The label only starts verification and is never the gate. **A PR can be merged or enter the merge queue ONLY when its LATEST commit has a passing CI verdict** (binding): the required check on the PR head must be green. An unlabelled PR never gets a green required check, so it can never merge. A push after a green run invalidates it: the new head must pass again, started automatically while the label is present. Velnor must never report success for a commit it did not verify; 'not requested' shows as pending or a non-success state that GitHub's ruleset treats as blocking. The merge queue is an ADDITIONAL check, never a substitute: it verifies the PR combined with the current `main`, or the queue tip, and reuses the PR's verdict only when that tree is identical to the one the PR passed. Specify the ruleset settings (required checks on PRs and on `merge_group`) that enforce this. Verify how label events reach Velnor: workflow triggers from `GITHUB_TOKEN` changes are suppressed, but App webhook delivery may differ. Cite GitHub docs or the API, and give a fallback.

## Decisions 13–24

13. **Multi-platform by design: Kubernetes and Docker.** The executor core is platform-agnostic: protocol client, expression engine, step execution, native actions, caches and ledger, label flow, conformance. A platform abstraction (a trait, refining the `Runtime` trait proposed in PR #1324) is the only layer that knows where jobs run. It covers environment provisioning with a warm pool, mounts for caches and work dirs, process execution and log streaming, limits, cancellation, teardown, crash recovery and ownership proofs. There are two first-class backends:
   - Kubernetes: pods on the Hetzner cluster.
   - Docker: containers on one host, for local macOS (Docker Desktop and similar) and any Linux host with Docker.
   Each backend declares its capabilities (reflink, shared page cache, privileged dind, network policy and so on). The core picks the fastest CORRECT path; a missing capability falls back to a slower but correct path, never an unsafe one. Conformance runs per backend. velnor-new's `velnor-host` (Docker on macOS, journal, recovery, ownership proofs) and velnor-legacy's Docker and Firecracker backends are candidate inputs: reviewed under decision 17, never assumed to be the base.
   "Sandbox" is the design's abstract name for one job's isolated environment: a pod (or a container in a warm pod) on Kubernetes, a container on Docker. Speed comes from pre-warming, not from the name.
14. **velnor-legacy is very old legacy.** Port only code that makes sense; re-architect or rewrite anything poorly designed instead of copying it. The repository stays archived; ported code is owned in velnor-new.
15. **All work is done by agents.** Size every phase by its real constraint (CI and merge-queue throughput, review gates, owner decisions, traffic-bound evidence, shared capacity), in wall-clock hours or days, with agent-hours separate. No team-style week calendars.
16. **Transition: per-job-type migration.** A job type moves to the Rust executor as soon as it passes conformance for that job type. There is no moving back (decision 18): its old path is deleted in the same change, and a divergence is fixed forward. Until then it stays on today's system, which isn't part of the new design and gets no new investment. Order the migration to relieve the critical path first.
17. **Nothing is final: always review, and redesign what is poor (a standing project rule).** No existing implementation is the final view: velnor-new's own code (`velnor-host`, its journal and recovery, the `Runtime` trait proposed in PR #1324, `velnor-gen`, `velnor-ci`, the workflows) as much as velnor-legacy. Each component is reviewed on its merits. It is reused only when it is well designed for this architecture and goal; otherwise it is redesigned. Reuse is a per-component decision made after review, never a default. The final design must show, for every component it reuses, why it passed review, and must name what is redesigned and why.
18. **Migrate and delete; never keep the old version (ALWAYS).** When something is redesigned, every caller migrates to the new version and the old one is deleted in the same step. No v1 next to v2, no backward-compatibility layers, shims, flags that keep the old path alive, deprecated aliases, or legacy fallbacks. Applied to the transition: a job type that moves to the Rust executor has its old route (`runs-on` label, generated workflow bits, wrapper code) deleted in that change. The official-runner oracle catches divergence BEFORE the move, so nothing needs a fallback. When the last job type moves, delete the ARC scale set and the ARC path that runs production jobs on the official runner. The ability to run the official runner is KEPT, but only as the reference executor of decision 19. Every phase in the design must list what it deletes.
19. **Keep the official runner as Velnor's built-in reference executor (verification only, permanent).**
   - Velnor can run GitHub's latest official runner on either platform backend (Docker and Kubernetes) behind the same executor interface as the native Rust executor.
   - It is used only to verify correctness, in three modes:
     1. the conformance gate (the corpus run through both executors on every executor change and every new official release);
     2. sampled shadow runs of real velnor-new jobs, to catch drift on real workloads;
     3. on demand ("run this job both ways and diff").
   - Its results NEVER produce verdicts, receipts, caches or releases, and it is never a fallback.
   - The official runner image and its launch code are kept for this role. Under decision 17, `velnor-runner-host`'s official-runner wrapper is reused for it only if it passes review; otherwise it is redesigned.
   - This amends decision 18's deletion list.
20. **Two interchangeable executors behind one interface; Velnor Native is the recommended default (supersedes decision 19 where they conflict; amends decisions 1, 4 and 18).**
   - **Velnor Native (Rust)** is the default and the recommendation, because it is much faster: native actions, no Node, warm sandboxes, local state.
   - **GitHub Official** is the latest official runner, driven by Velnor through the same executor interface.
   - Both get everything around the executor: platform backends (Docker and Kubernetes), warm pools, scheduling, the label flow and the `Required` check, logs and timeline, the ledger and caches.
   - Which executor runs is a deliberate config choice in `.velnor/`, per job type or globally. Velnor never switches silently on failure, so this is not an automatic fallback.
   - Verdicts from either executor count. The executor's identity is part of every result key, so results are never reused across executors: switching costs one re-verification and can never produce a false green.
   - The conformance gate, sampled shadow runs and on-demand "run both and diff" (decision 19) keep proving the two equivalent and quantify the native speed-up.
   - Amendments:
     - Decision 1 becomes "native by default; official is a supported, deliberately chosen alternative".
     - Decision 4 ("no Node") applies to the native executor; the official executor runs JS actions on Node by nature.
     - Decision 18: the official executor is maintained code, not a legacy remnant. ARC is still deleted: Velnor's own dispatcher drives both executors.
   - The design must show the speed difference per job type, so the recommendation is backed by numbers.
21. **An extensible isolation and platform abstraction: design for more, build only two now.**
   - **Implement now:** Kubernetes (pods) and Docker (containers).
   - **Design** the platform API so future backends need only a new implementation, never a change to the core. Examples, not to be built now: microVMs (Firecracker, Cloud Hypervisor, Kata), and Apple's Containerization framework (each Linux container in its own lightweight VM on Apple silicon).
   - **The core describes environments generically:** CPU, memory, mounts, network policy, lifetime. It never uses pod or container concepts.
   - **Backends declare capabilities that capture real isolation differences:**
     - kernel shared or separate;
     - how directories enter (bind or reflink mount, virtio-fs, disk image);
     - whether the page cache is shared;
     - snapshot and restore, warm pool;
     - nested Docker without privilege;
     - network policy, CPU pinning, arch and OS.
   - **The core branches only on capabilities, never on backend names.** A missing capability falls back to a slower but correct path.
   - **Lifecycle, crash recovery, ownership proofs and teardown are part of the backend contract.**
   - **Conformance is per backend.** A new backend is accepted when it passes the same corpus.
   - The design must specify these traits and types concretely enough that adding a microVM or Apple-containers backend is a contained piece of work.
22. **`actions/cache` is supported natively and is "magically fast"; storage is Velnor's choice and invisible to users.**
   - Users write plain `actions/cache` (key, restore-keys, path, the cache-hit and matched-key outputs, enableCrossOsArchive and so on) with GitHub's exact semantics, including branch scoping: read own ref, base and default branch; write own ref; keys immutable. Conformance proves it against the official runner. There is no new syntax.
   - **Tiers:**
     - L0, node-local NVMe: unpacked once per node, then reflink clones per job;
     - L1, a RustFS cluster per site (https://github.com/rustfs/rustfs; S3-compatible, Apache-2.0, erasure-coded across the site's worker disks), so a node miss is a LAN fetch;
     - L2, RustFS site replication DE↔FI in the background.
     GitHub's cache service is not in the hot path.
   - **Prefetch:** the dispatcher evaluates cache keys ahead of the job (e.g. `hashFiles('Cargo.lock')`) and warms L0 before the first step.
   - **Content-addressed chunks** make saves deduplicated and incremental.
   - **One storage tier for everything:** consider RustFS as the shared backing for actions/cache, artifacts v4, promoted release artifacts, mbx-cache (S3 mode), and cross-node snapshot shipping. Fewer systems to run.
   - **Open (to verify in the runner source):** can the official executor (decision 20) be pointed at a Velnor cache-service-v2-compatible endpoint backed by the same store, by Velnor handing the job message to `Runner.Worker` with rewritten endpoints, so both executors get the fast cache?
   - Correctness first: the cache scope and trust rules (PR writes never reach `main` scope) hold on every tier.
23. **Storage is configurable: one abstraction, selectable backends and tiers.**
   - **Backends:**
     - local filesystem (node NVMe, or a directory on a Mac);
     - RustFS single-node;
     - RustFS multi-node (an erasure-coded cluster per site, optional site replication);
     - any S3-compatible store (RustFS, MinIO, AWS S3, R2) through the same S3 backend.
   - **Tiers are composed in Velnor's configuration** (e.g. `local`, `local → site`, `local → site → cross-site`), each tier naming a backend.
   - **Each data class is routed separately:** actions/cache, artifacts v4, promoted release artifacts, the mbx-cache store, and warm-snapshot shipping.
   - **Defaults per platform:**
     - Docker or local macOS: local only, with no extra services;
     - Kubernetes cluster: node-local NVMe plus a RustFS cluster per site.
   - **Invariants for every configuration:**
     - the user-facing `actions/cache` semantics never change;
     - correctness is enforced by Velnor's cache layer, not by the backend: scoping, PR writes never reaching `main` scope, immutable keys, content verification on read;
     - a misconfiguration can only make things slower, never wrong.
   - **Backends declare capabilities** (reflink, server-side copy, multipart, conditional writes, replication), and the cache layer picks the fastest correct path, as with the platform backends (decision 21).
24. **Node-local repository caches (and, by the same pattern, other upstream mirrors), configurable.**
   - **Each node keeps a bare mirror of each configured repository,** updated by push webhooks (fetching only new objects) plus a periodic catch-up. It covers PR head refs, merge-group refs and tags.
   - **Native `actions/checkout` reads from the mirror:**
     1. it checks that the exact requested commit is present, fetching only the missing objects in one round trip if needed;
     2. it reflink-clones a pre-materialised tree of the node's latest `main`;
     3. it rewrites only the files that differ.
     The target is a local checkout well under 1 s.
   - **New or wiped nodes seed from the site store** (a git bundle on the storage tier, decision 23), not from GitHub.
   - **Configurable:** which repositories, refresh policy, storage tier, and on or off per platform. On local macOS it may reuse the developer's clone.
   - **Correctness:**
     - the checkout must be exactly the requested commit and tree: verify the commit and tree hashes before the first step, and never assume the mirror is fresh;
     - source mtimes after checkout follow the same rule as the warm-snapshot path, so cargo fingerprints stay valid;
     - mirrors are read-only to jobs.
   - **The same mirror pattern applies to other upstreams,** to cut traffic to roughly once per site per new version: the crates.io sparse index and crates (the cluster's existing crates-mirror DaemonSet, currently unused), Rust toolchains, mise tools, and container images through a registry mirror.
   - **The design must show** the traffic and time saved per job and per day.
