# Rust-only redesign by Muse Wonder Full

| Field | Value |
|---|---|
| Model | Muse Wonder Full |
| Model id | `muse-wonder-full-1.0` |
| Client | Muse Code 1.4.20 (muse exec) |
| Reasoning effort | max |
| Date / run | 2026-11-19 |
| Brief | Rust-only redesign brief v1 (2026-11-19): design v2, all earlier rounds, the mbx and blaze experiments, and twelve binding owner decisions |

This is the model's own redesign after the owner's Rust-only decision, unedited except for formatting. It answers the Rust-only brief: a native Rust executor, the official runner only as a conformance oracle, latest APIs only, no Node, build once and reuse everywhere, correctness first, and CI started by a PR label.

---

# Velnor Rust-only runner: redesign for maximum speed at the same confidence

Independent panel answer to `rust-only-brief-v1.md` (2026-11-19). It designs inside the owner's
12 binding decisions. Numbers are tagged **[M]** measured, **[E]** estimated with arithmetic shown,
**[T]** target. Evidence citations use the evidence pack (`evidence.md` sections A to F), the v2
experiments, and `path:line` in `velnor-new` at the pack commit and `actions/runner` v2.338.0.

## 1. Summary

Velnor stops executing verification inside GitHub jobs. A native Rust executor acquires each job, but every `cargo` obligation runs through one idempotent `ensure(tree, obligation)` call on the cluster, keyed by everything the sandbox contains.
The single `Required` job is a witness: it joins work the label webhook already started, streams the logs, and exits with the verdict.
Checkout, cache, artifacts, tools and attestations run natively against node-local stores. Verification runs once per unique input and is reused by receipt across the PR, the merge group, `main` and the release.
A release promotes the exact binaries `main` verified. PR verification runs only while the `velnor:verify` label is present. The official runner runs only in the lab as a differential oracle.

| Measure (labelled PR, final push) | Today [M] | Committed [T] | Stretch [T] |
|---|---|---|---|
| PR verdict P50 / P90 | 26.7 / 92.3 min run wall | **90 s / 4 min** | 35 s / 2 min |
| Push-to-merge P50 / P90 | 85 / 457 min | **3 min / 8 min** | 100 s / 4 min |
| Runner-minutes per merged PR | 285 to 347 [E] | **≤ 35** | ≤ 15 |
| Runner-minutes per release | rebuilds everything | **≤ 5** (promote only) | ≤ 2 |

## 2. The latency budget

The trace follows one typical labelled PR: a `generator/` or `ci-helper/` change, the median class
at 47% of merged PRs [M, 200 merged PRs]. The clock starts at the accepted final push. Unlabelled
pushes cost near zero by design (section 8), so the budget times the labelled final push. Runner
workspace PRs (25%) and engine PRs (8.5%) take longer until the test floor lands. Section 6 gives
those numbers.

### 2.1 Stage by stage

| Stage | Today | This design | Bottleneck today and how it goes |
|---|---|---|---|
| GitHub event, webhook, job offer | Run created in 1 to 2 s [M], but the first job waits. Admission span 2 s p50, 226 s p90, 2,203 s max [M]; a superseding run waits 60 s p50, 626 s p90 for the cancelled run's `always()` gate [M] | **2 s** [E]. The App webhook reaches `velnor-hooks` in 1 to 2 s and starts `ensure` at once. GitHub creates the job in parallel in about 15 s [E] | The stall is a hosted `always()` job in a dead run. One job per run and native cancellation remove the cause (section 8.5). Verification no longer waits for the job at all |
| Acquire | ARC idle runner 2 to 3 s, on-demand pod 12 to 14 s [M]. Scale-set jobs queue 0.2 min median [M, §A T4], but 8 hosted gate jobs queue 4.1 min median, 23.6 min p90 [M, §A T4] | **≤ 1 s** from broker message to first step [T]. The dispatcher holds every JIT identity, picks the node, then acquires | ARC creates a pod after GitHub assigns the job, so placement and pod start sit on the critical path. Pre-created sandboxes and placement before acquisition move both off it |
| Sandbox start | Pod create about 6 s [M, 436 starts], plus dind probe. 57 dind probe failures per hour [M, §B.6] | **≤ 0.3 s** [E]. The node agent hands out a pre-created cgroup slot with its network namespace ready. No pod create, no dind | Every job pays pod scheduling and a privileged Docker daemon it mostly does not need. Only Docker-needing test binaries see a daemon (section 7) |
| Checkout | 6 s median [M, §A T10], from GitHub over 120 ms RTT blob storage [M, §B.5] | **≤ 1 s** [T]. Objects come from the node Git mirror, fetched on the push webhook. The agent materialises a sparse checkout of exactly the obligation's paths | Network fetch per job, 39 times per run. One mirror fetch per push serves every obligation |
| Toolchain and tools | `Prepare pinned tools` 10 s median, 18 s p90 per job, 706 runner-min in the sample [M, §A T9b]. Toolchain restored in 0 of 58 logs [M]. Action downloads about 4.4 s per job [M] | **0 s**. The toolchain and mise tools are read-only node stores, mounted before the job arrives. Native actions need no download | Per-job install into `runner.temp`. A version-keyed store makes the step a mount |
| Dependency fetch | `cargo fetch` 3.5 to 6.1 s direct, 2.7 to 3.3 s via the mirror [M, §B.4.2]. Sources cache missed 48 of 48 Rust jobs [M, §A T9a] | **0 s** on a hit. Crate sources sit in the node store keyed by `Cargo.lock` checksums. A miss fetches from the site crates mirror in about 3 s [M] | The cache key hashes `${{ runner.temp }}`, which differs between hosted and ARC runners, so lookups never match (`crates/velnor-cache-rules/src/lookup.rs:28-31`). Node stores end the round trip |
| Compile | Cold every job. 196 to 497 crates compiled per job [M, §A T9a]. `Build test executables` 81 s median for `velnor-runner-host`, 14 s for `velnor-ci` [M, §A T10]. Nothing survives a job [M, §A T9a, T9b] | **15 s** delta for the median PR [E: 8.8 s warm on an M5 scaled 1.7 times for an EPYC core]. An unchanged tree compiles 0 crates in 0.31 s over a reflink clone [M, cluster experiment P2] | `emptyDir` on ext4 cannot reflink, so a warm target costs an 8.6 s, 10.75 GiB copy [M, P3]. Work directories move to XFS. A reflink clone costs 0.25 s and 2.5 MB [M, P2] |
| Tests | `Unit and integration tests` 192 s median, 417 s p90 for `velnor-runner-host`; 238 s for `velnor-actions-cli`; 10 s median, 62 s p90 for `velnor-ci` [M, §A T10]. Two loop tests take 258 to 274 s and 203 to 271 s. The floor behind them is still 125 to 138 s [M] | **10 s** of parallel lanes for the median PR [E]. Format, clippy, docs and tests run at once in separate target lanes under one jobserver. Changed obligations run three times in parallel (section 6.4) | Serial lanes on one target directory serialise on cargo's lock. `Journal::open` at 93 call sites sets the floor. Section 6.3 breaks it |
| Result reporting | Logs and timeline through the official Worker's queues. Artifacts round-trip through GitHub: a 1.48 GB nextest archive took 36 to 43 s to upload [M] | **1 s** [E]. The executor streams logs, timeline and summaries to Results as steps run. Test binaries move through the site store by digest, never through GitHub | Per-job report uploads and a 39-job fan-in. Receipts stream per obligation as each one finishes |
| The required check | Three serial hosted gates. On 2026-11-18 their queues took 57.0 of a 71.8 min run [M, §A T5]. Plan queue 21.6 min, Required verdict 23.3 min, Required 15.3 min median [M, §A T5] | **5 s** after the last receipt [E]. The witness job completes with the verdict. It runs on Velnor, so its check still reports as GitHub Actions (integration 15368), which the release gate requires (`crates/velnor-ci/src/gate.rs:15,127-159`) | The Free plan's 20 hosted slots, shared with Source qualification (267 runs on 11-18) and Job image candidate (306 runs) [M, §A T11]. No CI job stays hosted |
| Merge-queue entry and its run | Green to enqueue 7.5 min median, 88 min p90 [M]. 0 of 40 merged PRs used auto-merge [M, S2]. Enqueue to merge-group run 19 s, finish to merged 26 s [M]. The group then reruns all 39 jobs: 64.6 runner-min [M, §A T2] | **About 60 s** [E]: enqueue in seconds, 19 s to the group run, a lookup, 26 s to merge. 78 to 89% of groups execute nothing at a 1 to 5 min window [E, two replays of 98 and 142 merges] | The group tests a tree the PR never tested, because `main` moved during hours of waiting: 0 of 40 groups matched on 11-18, 18% on quieter days [M, S4]. A 90 s verdict shrinks the window, and per-package keys cover the rest |
| The merge | Worst-day P50 400 min, of which 278 to 282 min sat between queue entry and merge [M] | Included above. Push-to-merge **3 min P50** committed [T] | Queue depth 2 at an 8.1% false-failure rate. Section 8 gates depth on merges per hour, not hope |

### 2.2 The arithmetic for the median PR

Committed path, worst-case sum: webhook 2 s + fetch, merge tree and plan 2 s + delta build 15 s +
lanes 10 s = 29 s of verification. GitHub creates the witness job in about 15 s in parallel. The
witness joins the running work and completes 5 s after the last receipt. Verdict at about 35 s
[E]. The committed target is 90 s P50 to absorb a cold node (section 6.1), a lockfile change
(10.5% of PRs [M]) and queueing at peak. Push-to-merge adds enqueue, 19 s and 26 s of GitHub
queue latency [M], and a mostly-empty merge group: about 2 min for the median PR [E]. The
committed target is 3 min P50, 8 min P90.

Three facts bound every promise here. First, a PR run with no hosted queue already takes about
11 min [M, 10.9 min over 12 runs], so most of today's 85 min median is waiting, not working.
Second, 45% of PR runs are cancelled [M, 1,156 of 2,558] and 18.3% restart on an unchanged head
[M, 469 of 2,558], so per-push full runs waste most of their work before they start. Third, every
merge runs the same 39 jobs three times [M, §A T7], and the third run failed falsely in 8.1% of
cases [M, 10 of 124] while finding nothing. The design removes work first and speeds up what
remains.

## 2b. Duplicate-work audit

The audit reads the real workflows: `.github/workflows/ci.yml` (39 jobs), the State seal workflow (17
jobs), `product-release.yml` with `product-release-generator.yml`,
`product-release-binary.yml` and `product-release-images.yml`, `release-rust-ci-tools.yml`,
`source-qualification.yml`, `job-image-candidate.yml`, `cache-prune.yml`, `freshness.yml`,
`qualification.yml`, plus `ci-helper/` (`velnor-ci`) and `generator/` (`velnor-gen`,
`velnor-model`, `velnor-release`, `velnor-render`, `velnor-rust`). There is no `release-plz.toml`
at the repository root. `velnor-release` reads a `release-plz.toml` per Cargo root, and the
release workflows build, checksum and attest the assets `velnor-actions-0.1.81-<target>`,
`velnor-host`, `velnor-runner-attestation-helper` and the runner, dind and resource-probe image
tars. The rule for every row: run it once per unique input, reuse it by content address, and keep
a duplicate only when the duplicate is measurably faster.

| Operation | Stages that run it today | Same input? | Today's cost | Decision and mechanism |
|---|---|---|---|---|
| Full 39-job verification (plan, 29 `rust-*`, 2 `check-*`, lints, `Required verdict`, `Required`) | PR run, merge-group run, push-to-`main` run [M, §A T7] | Push run: yes in 120 of 159 merges, identical SHA [M]. Merge group: same tree in only 0 to 18% today [M, S4] | 52.9 + 64.6 + 62.0 runner-min per merge [M, §A T2] | **Reuse by receipt.** Each obligation has an input-addressed key (section 6.4). Every event lists every obligation. An obligation with a passing receipt for its key does not run. The push run keeps one real `Required` job for the release gate and otherwise does release duties only |
| PR push N, then push N+1 of the same PR | Full selected set on each push. Median 3 runs per branch, 45% cancelled [M, §A T7] | Partly. Unchanged packages repeat exactly | About 3 to 4.8 PR runs per merge [E] | **Reuse per package, join in flight.** Push N+1 re-verifies only packages whose closure key changed and their dependents. A superseded push drops its claim. Work another request needs keeps running. The new pipeline never waits for the cancelled one (section 8.5) |
| Rebase-only push (same tree, new parents) | Full run | Yes, identical tree | One full run | **Instant verdict.** Keys hash Git trees, not commits. Every key matches, so the witness completes from receipts in about 5 s [E] |
| Re-run with no code change (`edited`, retarget, manual re-run) | Full run. 18.3% of PR runs start on an unchanged head, most cancelling identical work [M] | Yes | One full run each | **Zero cost.** `velnor-gen` drops the `edited` trigger. A retarget re-plans against the new base without restarting identical obligations. A re-run executes only obligations without a passing receipt |
| Concurrent PRs sharing unchanged workspace parts | Each PR verifies everything it selects, cold | Yes for untouched roots and packages | Multiplied by open PRs | **Verify once, share by key.** Two PRs whose generator root is untouched both read `main`'s receipts for it. In-flight join means the second request waits for the first execution instead of starting a copy |
| Stacked PRs (PR2 on PR1) | PR2 re-verifies PR1's changes | Yes for PR1's packages once PR1 passes | One extra delta build and test pass | **Reuse the base PR's receipts.** The key does not name the PR. PR2 runs only its own delta on top. The merge group still verifies the landed combination in full by lookup |
| Two PRs with identical changes | Both run in full | Yes, identical trees | Two full runs | **Detect by tree.** Identical trees give identical keys. The second PR is a lookup |
| Agent pre-push check, then CI | Agent runs tests locally, CI repeats them | Same tree, different machine and profile today | The agent's local run plus a full CI run | **Reuse only sealed runs.** `velnor verify` sends the tree to the cluster and calls the same `ensure` in the same sandbox class. The push then finds receipts. A run on a developer Mac never counts: another platform, and a key the agent holds |
| PR result promoted into `main` caches and receipts | State bundles written only on `main` today, but the design question is open | Same input, lower trust | — | **Never promote without verification (binding).** A PR execution may satisfy a later request with the identical key, because the agent observed it in a sealed sandbox. It becomes `trusted` only when its exact tree lands on `main` or the builder reproduces it. Only the builder writes snapshot generations (section 6.5) |
| Checkout (61 uses) | Every job of every run, from GitHub | Yes per tree | 6 s × about 39 jobs × 3 runs [M-derived] | **Fetch once per push.** The webhook fetches the commit into every node's mirror once. Each obligation materialises a sparse checkout locally in ≤ 1 s [T] |
| Tool setup: mise, Rust toolchain, components (55 `jdx/mise-action` uses) | Every Rust job | Yes per pin | 10 s median per job [M, §A T9b] | **Eliminate.** Read-only node stores keyed by the pin digest. The native mise action only puts the store on `PATH` (section 4) |
| Action download and Node start | Every job. `PrepareActionsAsync` resolves and downloads before the first step (`ActionManager.cs:79-181`) | Yes per SHA | About 4.4 s per job [M]. Node start alone is only 87 to 147 ms per action [M] | **Eliminate.** Native actions ship in the executor. The gain is locality, not the language |
| `cargo fetch` and registry index | Every Rust job. Sources cache 0 of 48 hits [M] | Yes per lockfile | 2.7 to 6.1 s per job [M, §B.4.2], 86 MB entry [M] | **Fetch once per lockfile per node.** Content-addressed source store. The builder seeds it for each `main` lockfile |
| Compile dependencies (357 crates, runner workspace) | Every job, cold. mbx local lookups hit 40 to 74% within a job only [M, §A T9a] | Yes per lockfile, toolchain and profile | 92 s cold [M, cluster P1]. 10.6 GiB target per job | **Reuse the snapshot.** Plain cargo over a reflink clone of the node's warm `main` generation: 0.31 s, 0 crates [M, P2]. Section 6.1 keeps mbx off the hot path |
| Compile workspace crates | Every job that selects the package, plus clippy and doc lanes rebuild metadata | Yes per closure | `velnor-runner-host` one-file edit 61 s, 159 CPU-s on a Mac [M] | **Build once per configuration.** One build produces test binaries, clippy metadata and doc inputs. Lanes consume them. The crate split (section 6.3) shrinks the unit |
| `velnor-runner-host` default and `local-mode` sweeps | Two jobs compile the 157k-line crate twice: 6.2 + 5.5 runner-min per PR run [M, §A T3] | No. Different features, both required | 11.7 runner-min per run | **Keep both, compile less.** Extract `velnor-runner-local` so the shared code compiles once. Running one configuration only stays rejected: `local-mode` gates production code |
| Tests, clippy, fmt, docs per package | PR (selected), merge group (full), push (full), nightly, state producers | Yes per key | See the full-verification row | **Receipt per obligation.** Changed obligations run three times in parallel and any failure blocks. That restores the detection three full runs give today by accident: `1−(1−p)³` is 49% at p = 0.2 [E] |
| Ledger CLI build (`cargo build -p velnor-actions-cli`) | `ledger-cli` job heads the critical path, then every Rust job downloads the artifact | Yes per tree | 0.9 min plus queue [M, §A T3] | **Eliminate.** The planner is a released helper in the node store. No job builds the tool that plans it |
| Plan computation | `plan` job per run, on a hosted runner | Yes per tree and event | 0.9 min plus 5.5 min median queue [M, §A T4] | **Compute in the ledger.** The released planner runs on the mirror in about 2 s [E]. The witness reports the plan, it does not compute it in a job |
| State seal: 5 producers build, export `mbx cache` bundles, attest, save | the State seal workflow on every push to `main`. `State seal / root`, `/ local`, `/ crates-velnor-runner` succeeded 0 of 18 times [M, §A T9d]. Restore skipped in 3,636 of 3,636 jobs [M] | Partly. Producers rebuild what CI just built | Up to 5 × 60 min producers plus sealers. GitHub cache 3.49 GB in 3 active entries [M, §A T9c] | **Delete the workflow.** The builder builds each `main` generation once per site and warmers take it into node snapshots (section 6.1). No bundle, no tip race, no attested GitHub cache entry on the cluster path. `cache-prune.yml` goes with it |
| Release build: `<bin>-<target>.tar.gz`, `SHA256SUMS`, attestations | `product-release-generator.yml` builds and attests Linux, macOS ARM and macOS Intel assets. `product-release-binary.yml` and `product-release-images.yml` rebuild binaries and image tars. `release-rust-ci-tools.yml` rebuilds `velnor-ci` and `velnor-gen` | Yes. Same tree `main` just verified, different profile today | Full release-profile builds per target, plus verify and attest jobs per target | **Promote, never rebuild (binding).** `main` builds shippable artifacts once into the content-addressed artifact store. The release copies them by digest, writes `SHA256SUMS` from the digests it already holds, and attests the promoted bytes. Section 6.2 resolves the profile tension so the promoted bytes are the tested bytes |
| macOS release builds and qualification | `product-release-generator.yml` macOS jobs, `source-preview-mac-proof.yml`, generator-release macOS composites | Same sources, other target | Hosted Mac minutes per release | **Disable in phase 0 (binding).** Nobody uses them. The owner builds on macOS locally with cargo. The artifact store key already names the target, so macOS returns by adding a builder, not by redesign |
| Runner image build and attestation | `job-image-candidate.yml` runs on every push to any branch or tag: 306 runs on 11-18 [M, §A T11] | Rarely changed inputs | Build, attest and publish jobs per push, all hosted | **Key the image like any artifact.** Build once per image-input digest on Velnor, promote by digest. A push that does not touch `images/` is a lookup |
| Source qualification (plan, selection, 3 harness jobs, each up to 90 min) | Every PR event, all hosted: 267 runs on 11-18 [M] | Mostly the same selection CI computes | Hosted queue that starves the gates | **Move to Velnor or run only in the merge group.** Its checks become obligations with keys. It never shares the hosted pool, because the hosted pool no longer exists for CI |
| Nightly, scheduled and freshness runs | `freshness.yml` weekly, `release-rust-ci-tools.yml` daily schedule | Tip only | One full run each | **Keep as audits, reuse the path.** The nightly run builds cold from an empty target and compares results and test-binary digests with the warm path (section 6.5). It is the one deliberate duplicate: it exists to catch a wrong key, and it runs on batch capacity |
| Cache warmers | None today. Every job warms itself, cold | — | The cold cost above, per job | **One builder per site.** It builds each `main` generation once. Five nodes rebuilding `main` after every merge would run about 120 incremental builds per hour at 8 merges per hour [E]. Warmers copy changed files instead |
| Conformance runs (official runner in the lab) | `qualification.yml` about 70 jobs in hosted and scale-set pairs, manual only | Same cases on both runners by design | Lab capacity | **Keep the duplicate. It is the product.** The differential run is the only place the same job must execute twice (section 5). It never sits on the merge path |
| Speculation: queue-tip trees, bypass trees | Nothing today | Predicted trees | Batch CPU only | **Keep as a faster duplicate.** Verifying `T_i = T_{i−1} ⊕ PR_i` before GitHub asks costs preemptible CPU and saves a full merge-group run when it matches. A wrong guess wastes batch cycles, never a verdict (section 8.2) |
| Racing a local rebuild against a cross-node fetch | Nothing today | Same crates | — | **Keep where measured faster.** A cold node takes 39.9 s by fetch against 92 s cold [M, M3]. A node with an older snapshot rebuilds an 11-crate lockfile delta in 67.4 s against 43.4 s by fetch after another node built it [M, L1]. The agent predicts and picks. Both paths produce identical inputs to the key, so the race cannot change the verdict |

### Totals

| Per unit | Today | This design [T] | How the total is counted |
|---|---|---|---|
| Runner-minutes per merged PR | 285 to 347 [E: 3 to 4.8 PR runs at 52.9, one merge group at 64.6, one push at 62.0] | ≤ 35 committed, ≤ 15 stretch | Every occupied slot once, split completed, cancelled and superseded. The builder, speculation and audits charge to the consuming repository |
| Runner-minutes per release | Full rebuilds per target in three product workflows plus `release-rust-ci-tools.yml` | ≤ 5 | Copy, checksum, attest and publish only. The build minutes were spent once on `main` and counted there |
| Cache GB per merged PR (GitHub) | 3.49 GB held for the repository [M, §A T9c], inside a 10 GB budget, for entries that restore 0 times on the cluster | 0 on the cluster path | Cluster state lives in node and site stores. GitHub cache remains only for jobs that still run hosted, and after phase 0 no CI job does |
| Cache GB per node (cluster) | 0. Nothing survives a job | Last 3 `main` generations per root, about 10.6 GiB logical per runner-workspace generation [M], shared by reflink so one physical copy serves every slot. 40% of the data volume across orgs, alert at 70% [T] | FI workers hold one 3.5 TB data disk each, DE workers 32 to 50 TB [M, §B.1.4] |

## 3. The native Rust runner

### 3.1 Crates and components

All new code is Rust in the `velnor-new` root workspace. Crossing a line budget stops work for
review. `velnor-legacy` reached 804k lines with two files near 37k lines each [M, §E.1]. Budgets
stop that recurrence.

| Component | Crate | Runs as | Responsibility | Budget |
|---|---|---|---|---|
| Dispatcher | `velnor-dispatch` | One service per site, active and standby | Scale-set sessions, broker long-poll, JIT identities and credentials, placement before acquisition, the lease journal | 6k lines |
| Executor | `velnor-exec` | Inside the node agent | Job messages, contexts and expressions, steps and composites, workflow commands and env files, masking, post steps, cancellation and timeouts. Run-service and Results clients | 20k lines |
| Native actions | `velnor-native-actions` | Inside the executor | The closed set (section 4). Refusal of everything else before the first step | 10k lines |
| Conformance | `velnor-conformance` | velnor-lab | Differential runs against the official runner (section 5) | 5k lines |
| Webhook ingress | `velnor-hooks` | One service per site | App webhooks for push, pull request and merge group. Signature check, `ensure`, replay of missed deliveries | 3k lines |
| Ledger | `velnor-ledger` | Library plus one service per site | `ensure` requests, obligation keys, signed receipts, scopes, promotion, audit | 6k lines |
| Node agent | `velnor-node` | Privileged DaemonSet on workers | Hosts the executor. Pre-created sandboxes, result observation, node stores, the `main` builder, harvest, cleanup by UID | 15k lines |
| Store | `velnor-store` | Library plus one service per site | Content-addressed blobs: test binaries, release artifacts, image tars, reports. Snapshot file sync within a site. Eviction | 6k lines |
| Planner and generator | `velnor-ci`, `velnor-gen` | Existing | The plan, obligation commands and the workflow shape. `velnor-gen` emits only qualified actions at qualified SHAs | Existing |

Kubernetes stays the fleet layer. It deploys these services, accounts for every pod and enforces
quotas, priorities and network policy. Velnor places work. The kube scheduler places pods. No pod
is created per job or per obligation (section 7).

```text
GitHub App webhook ──> velnor-hooks ──> velnor-ledger (ensure)
                                            │  plan, keys, receipts
GitHub scale set ────> velnor-dispatch ─────┤  placement, then acquire
                           │ job message    ▼
                           └─────────> velnor-node / velnor-exec
                                          │ obligation steps call ensure
                                          │ other steps run in the job sandbox
                                          ▼
                                   Results, run service
```

### 3.2 Protocol client: latest APIs only

| API | Status | Why |
|---|---|---|
| Scale-set sessions, broker long-poll, JIT runner configuration. Run-service `acquirejob`, `renewjob`, `completejob` | In | How GitHub delivers jobs to self-hosted runners today |
| Results twirp for logs, timeline and step summaries. Cache service v2 (twirp). Artifacts v4. The OIDC token endpoint | In | The services current actions and the current runner use |
| node24-era runner semantics: contexts, env files, defaults, workflow commands | In | What the oracle runs. v2.338.0 forces every JavaScript action to node24 (`HandlerFactory.cs:58-59`) |
| The legacy session with AES message encryption | Out | Replaced by JIT runners and the broker. It needs a private key on disk for no benefit. Legacy bodies are AES-encrypted with an RSA-wrapped key (`MessageListener.cs:458-495`) |
| Legacy pipelines and distributed-task APIs | Out | Superseded by the run service and Results. `velnor-legacy` `protocol.rs` still carries the timeline API. That code is not copied |
| The older `DTExpressions` evaluator | Out | One evaluator only: `Expressions2`, as transcribed from `actions/runner` v2.337.0 |
| Cache v1 and artifacts v3 | Out | GitHub deprecated both |

The dispatcher holds every JIT runner identity itself. `generatejitconfig` takes only
`{name, workFolder}` (`scaleset/client.go:765-796`), and GitHub hands an acquired job to whichever
idle runner polls. A per-pod Listener therefore cannot choose the node. The dispatcher reads each
broker message first. Broker messages carry `runnerRequestId`, repository, owner, job id,
workflow ref, event name and request labels (`scaleset/types.go:21-57`). The dispatcher picks the
node from those fields (warm snapshot, site, class, reserved merge path), and only then calls
`acquirejobs` on the scale set and `acquirejob` on the run service
(`RunServiceHttpClient.cs:70-90`). A second acquire of the same job returns 409
(`RunServiceHttpClient.cs:106-107`), so no lease ever moves. Merge-group offers acquire first,
because the event name is in the offer.

The dispatcher advertises total capacity in `X-ScaleSetMaxCapacity`, as the reference listener
does. Advertising free slots contradicts the reference and `velnor-new`'s own protocol note. A lab
test with capacity 2 and five queued jobs proves the behaviour before cutover. The long poll
returns 202 for no message, 200 for a message, and 401 when the queue token expires, which
triggers one session refresh (`session_client.go:175-190`). A message stays queued until the
dispatcher deletes it, and the dispatcher deletes it only after the acquisition intent is in the
journal.

### 3.3 The job-message model

The run service returns `AgentJobRequestMessage` (`AgentJobRequestMessage.cs:27-47`): plan and
timeline references, job id, display name, request id, `LockedUntil`, container and output
template tokens, resources (containers, endpoints, repositories), `ContextData` (github, needs,
inputs, vars and secrets contexts), workspace options, environment and defaults as template
tokens, mask hints, variables, steps and the file table. Each step is an `ActionStep` with a
reference (registry image, repository path or script), a display-name token, a context name,
environment and input tokens, plus `JobStep` fields for condition, continue-on-error, timeout and
parallel group (`ActionStep.cs:34-54`, `JobStep.cs:29-50`).

`velnor-exec` mirrors this shape with typed Rust structs, copied in spirit from `velnor-legacy`
`job_message.rs` (6,799 lines [M, §E.1]) and cut down. Template tokens stay unevaluated until the
step runs, exactly as the Worker treats them. The well-known `SystemVssConnection` endpoint
supplies the runtime URLs the executor forwards to native actions: `CacheServerUrl`,
`ResultsServiceUrl`, `GenerateIdTokenUrl`, `FeedStreamUrl` and the access token
(`NodeScriptActionHandler.cs:54-93`). `run:` steps receive only the OIDC URL and token, never the
runtime token (`ScriptHandler.cs:313-327`). The executor keeps that split.

The job token, plan id, job id and run-service URL exist only in the job message. The executor
journals all four before it acts. That record is what lets a crashed executor finish or fail the
job (section 3.9).

### 3.4 The expression engine

Copy `velnor-legacy` `crates/velnor-runner/src/expression/` (3,565 lines: lexer 549, parser 497,
evaluator 679, value 512, tests 989 [M, §E.1.1]) into `velnor-new` with its tests. It is a lexer,
a shunting-yard parser and an evaluator transcribed from `Expressions2` in `actions/runner`
v2.337.0 (`expression/mod.rs:1-14`). Built-in functions cover `case`, `contains`, `startsWith`,
`endsWith`, `format`, `join`, `toJSON` and `fromJSON` (`eval.rs:216-247`). The status functions
`success()`, `failure()`, `always()` and `cancelled()`, and `hashFiles`, dispatch through the
executor's context.

Two changes happen at copy time. First, `hashFiles` runs natively in Rust against the sandbox
view. The official Worker spawns its bundled node for it (`HashFilesFunction.cs:73,206`). Velnor
has no node. The semantics, including glob order and the empty-match result, come from the
oracle corpus. Second, the engine is admitted only after it passes the expression corpus from
both `velnor-legacy` and `actions/runner` against the latest official release (section 5). The
official Worker evaluates every template twice, once with a legacy evaluator and once with
`WorkflowParser`, and compares them (`PipelineTemplateEvaluatorWrapper.cs:63-75`). Velnor runs
one evaluator. The corpus carries the cases where the two upstream evaluators disagree, so the
choice is measured, not assumed.

### 3.5 Steps, composites and post steps

The executor prepares every step before the first one runs, as the Worker does in
`PrepareActionsAsync` (`ActionManager.cs:79-181`). Prepare means: resolve each `uses:` reference
against the closed set, expand composites, and refuse the job if any reference or SHA is outside
the qualified set. Refusal happens before any step, with an error that names the action and lists
the supported set. `velnor-gen` emits only qualified actions, enforced by a generator test, so a
new SHA from Renovate passes the harness before it can reach production.

Composite actions inline into the job's step queue with their own pre and post steps cached, as
upstream does (`ActionManager.cs:820-845`). `velnor-legacy` composite scoping
(`execution/composite_scopes.rs`, `executor.rs:15193-15214`) is the reference for input, output
and env scoping. The executor preserves the umbrella rules: a composite's condition gates its
inner steps, its outcome is the last inner failure unless a step sets otherwise, and
`continue-on-error` applies at the level where it is written.

Post steps run in reverse order after the main steps, including after failure and cancellation,
each with the state its main step saved through `GITHUB_STATE`. `actions/cache/save` and the
composite `velnor-tools-cache-scrub` depend on this order. Cancellation during a post step still
runs the remaining post steps up to the job timeout, then kills the process group. The corpus
cancels at every step boundary, post steps included (section 5).

Background steps are real in `velnor-new`: each Rust job runs format, clippy, tests and docs as a
parallel group joined by a wait step [M, §C.1]. The executor runs a `ParallelGroupId` group
concurrently, captures each step's output separately, and fails the group if any member fails.
Output interleaving across the group is normalised in the harness, never in production.

### 3.6 Contexts, env, secrets and masking

Contexts come from `ContextData` in the job message plus the `runner` context the executor sets
itself (OS, arch, temp, tool cache, name), as the Worker does (`JobRunner.cs:~145-165`). The
executor evaluates `if:` conditions, display names, `continue-on-error`, env and input templates,
container tokens and `defaults` locally, in the same order as `StepsRunner`.

Env files follow the file-command protocol: `GITHUB_OUTPUT`, `GITHUB_ENV`, `GITHUB_PATH`,
`GITHUB_STATE` and `GITHUB_STEP_SUMMARY`, handled as `FileCommandManager.cs` handles them,
including blocked env keys and heredoc delimiters. Workflow commands (`::error`, `::warning`,
`::notice`, `::add-mask`, `::stop-commands`, groups) parse from step output. `velnor-legacy`
`workflow_command.rs` (1,521 lines) and `command_files.rs` are the copy source, reviewed against
the oracle.

Masking covers every secret variable value, every `MaskHint` (variable and regex types,
`MaskHint.cs`), every `::add-mask` value, and values the executor itself handles: job tokens,
OIDC tokens and cache credentials. Masking applies to logs, timeline, summaries and error
messages. The harness plants an unmasked secret as one of its ten seeded faults. Secrets never
enter an obligation sandbox. Build and test sandboxes get a scrubbed environment with no
`GITHUB_*` variables, a fixed `HOME`, `PATH` and locale, and no `.git` directory (section 6.4).
The job sandbox alone sees the job environment, and only its non-obligation steps use it.

One race dies with the rewrite. Upstream rewrites `event.json` before every action step
(`ActionRunner.cs:140`, `ExecutionContext.cs:1322-1336`) while a background step may still be
parsing it. The executor writes the event file once, before the first step, and never rewrites
it. Obligations see no event file at all.

### 3.7 Cancellation, timeouts and lock renewal

The Listener starts no Worker before the first renewal succeeds (`JobDispatcher.cs:393-405`). The
executor keeps that rule: acquire, renew once, then run. Renewal continues every 60 s
(`JobDispatcher.cs:775-776`) from the `lockedUntil` in each `RenewJobResponse`
(`RunServiceHttpClient.cs:184-200`), rescheduled at half the remaining lock time with a 60 s cap.
The renewal loop is written fresh. The upstream run-service overload dereferences a null request
when renewal fails (`JobDispatcher.cs:748-852`). That bug is not transcribed.

Cancellation arrives as `JobCancel{JobId, Timeout}` on the broker session
(`JobDispatcher.cs:140-155`). The dispatcher routes it to the owning node. The executor then
cancels the current step's process group, runs post steps inside the timeout, drops the job's
claims on in-flight obligations (executions another request needs keep running), and completes
the job as cancelled. Slot release is immediate. A `renewjob` answer of job-not-found counts as
cancellation. Job and step timeouts use the same path with a timeout conclusion. The force-fail
path with the job's own token (`JobDispatcher.cs:1208-1219`) covers an executor that cannot
recover: the dispatcher fails the job rather than leaving it to time out at GitHub.

### 3.8 Logs and timeline via Results

Completion goes through `completejob` with `CompleteJobRequest{PlanID, JobID, Conclusion,
Outputs, StepResults[], Annotations[], Telemetry[]}` (`RunServiceHttpClient.cs:123-160`). Each
`StepResult` carries the external id, number, name, action name, ref, type, status, conclusion,
timestamps, log URL, log line count and annotations (`StepResult.cs:14-52`). Annotations cap at
10 per type and 4,096 chars per message (`ExecutionContext.cs:144-146`).

Logs, step summaries and timeline go through the Results twirp service
(`ResultsHttpClient.cs:610-632`): signed blob URLs, then a direct upload to blob storage, then
metadata creation. Live lines stream over the feed websocket when `FeedStreamUrl` is present
(`JobServer.cs:172-192`). The executor runs the same four dequeue loops as `JobServerQueue.cs`:
console lines every 250 ms then 500 ms, timeline every 500 ms, blob uploads every 1 s. Console
lines stay best-effort. Blob uploads retry three times with a 30 s timeout. A step result names a
log only after its blob exists. `velnor-legacy` issue 672, broken log upload for failed jobs, is
a regression test here.

Because the executor runs the job through the run service, GitHub reports the check as GitHub
Actions. The release gate accepts a required check only from integration 15368 on a `push` run of
the exact SHA, and `product-release.yml` asserts exactly one job named `Required` in `ci.yml`'s
push run. The workflow keeps that job, that name and that file name. No App posts `Required`.

### 3.9 Crash safety

Journal discipline comes from `velnor-new`'s runner: persist the intent, make the call, record
the outcome. Unknown never counts as absent.

- The **dispatcher** persists the request identity and the acquisition intent before
  `acquirejob`, and the sealed response before any hand-off. A standby reopens sessions from the
  journal with each identity's credential. Fencing is by epoch: a dispatcher that lost its epoch
  acquires nothing.
- The **executor** journals plan id, job id, run-service URL, job token and sandbox UID before
  acting. After a crash it resumes the job or fails it with its own token, never twice. Ownership
  proofs and UID-based absence checks are copied from `crates/velnor-runner-*` with their tests.
- The **ledger** persists an execution's intent (key, node, epoch) before a sandbox starts. The
  agent records the result by compare-and-swap on that epoch. A node that lost its epoch reports
  nothing into the execution. Its sandbox is killed and its process tree's exit is proven before
  the slot frees.
- `ensure` is idempotent. A repeated request returns the same receipts or joins the same
  executions. A restarted agent reconciles sandboxes by UID. An execution whose sandbox is gone
  is scheduled again and never reported twice.
- `velnor-hooks` replays missed App deliveries every minute from the delivery list. A lost webhook
  only delays: the job's own `ensure` call covers the gap.

Fault injection runs in the lab before cutover: crashes around every remote call, journal write
and step boundary, a response dropped after success, a lease lost mid-job, duplicate and
reordered webhooks, a ledger partition, a lost node, the control plane and a whole site.

The cluster runs only released executor versions, never the change under test. A broken release
cannot block its own fix. A release serves 10% of jobs for a day before it serves all of them.
The previous release stays installed, and switching back is drilled weekly. After cutover,
rollback is the previous Velnor release, never the official runner (binding decision 1).

### 3.10 What is copied, and what is written fresh

| Source | Copied into `velnor-new`, reviewed and owned there | Not copied |
|---|---|---|
| `velnor-legacy` (archived) | The expression engine with its tests. The broker, run-service and Results clients and the scale-set session (`scaleset/client.rs`, `session.rs`), minus every legacy path. The native checkout (`checkout.rs` 5,128 lines, `git_mirror.rs` 1,613 lines [M, §E.1.4]) and cache adapters, retargeted to cache v2 only. `workflow_command.rs`, `script_step.rs` semantics. The trust-scope model. Incidents 275, 311, 348, 670, 671 and 672 as regression tests | `executor.rs` (37,028 lines) and `runner.rs` (36,759 lines) as wholes. `protocol.rs` legacy pipelines and distributed-task paths. `gha_cache.rs` v1 service. JavaScript sidecars (`node_action_image`, `executor.rs:652-662`). Docker actions. BuildKit handling. Firecracker |
| `velnor-new` `crates/velnor-runner-*` | Journal discipline, crash recovery, UID-based absence and ownership proofs, with their code. Wire types (`velnor-runner-github`, 6,878 LOC [M, §D.2]) and lifecycle types (`velnor-runner-core`) where they match the latest APIs. `velnor-local` as a harness seed | The executor itself. It runs the official runner in Docker (§D.4.1), which binding decision 1 forbids in production |
| `actions/runner` | The protocol and semantics as ground truth, cited at `path:line`. The latest release as the lab oracle | Any production use |
| ARC | Its scale sets serve `velnor-new` until the cutover gate passes (section 5.3) | Everything after cutover. Its listener and controller exist to run the official runner |

Written fresh: the dispatcher with placement before acquisition, the ledger and `ensure`, the
node agent's sandbox and harvest path, the native actions against node stores, the store, and the
conformance harness. Each copied piece enters with its tests and passes the oracle before it
serves traffic. `velnor-legacy` gets no commits, no git or path dependency and no published
shared crates (binding decision 5).

### 3.11 The fastest job start

Job start, from broker message to first step, targets ≤ 1 s at P50 and ≤ 2 s at P90 [T], against
2 to 3 s for an idle ARC runner and 12 to 14 s on demand [M]. Four mechanisms compound:

1. **Pre-acquired sessions.** The dispatcher holds scale-set sessions open with a warm long poll
   per scale set. No session create sits on the job path.
2. **Pipelined acquire and sandbox start.** The offer names the event, repository and labels
   before acquisition. The dispatcher picks the node from the offer and tells the agent to ready a
   sandbox of the right class while `acquirejob` is still in flight. A lost race (409) frees the
   sandbox. Nothing is journalled as acquired until the response lands.
3. **Pre-warmed sandboxes.** Each agent keeps idle slots per class with the network namespace,
   mounts and store views already built (section 7). Handing one out is a cgroup assignment, not
   a pod create. Slots are single-use and destroyed by UID after one job or obligation.
4. **No process start per job.** The executor is a long-lived part of the node agent. The official
   path starts a Listener, then a Worker process per job, and waits up to 30 s for the job message
   over a pipe (`Worker.cs:23`). Velnor hands the parsed message to a waiting task in memory.

Snapshot and restore (microVM or CRIU) is not needed for the build class, because a slot has no
process state worth restoring: the warm state is files, cloned by reflink in 0.25 s [M]. It stays
an option for a future Docker or fork class, where a running daemon is the state.

## 4. Native actions

`velnor-new`'s workflows reference a closed set [M: `uses:` counts in `.github/workflows/` and
`.github/actions/`]. Velnor implements each entry natively in Rust against node-local state. The
implementations use local state, but their effects must equal the upstream action's: cache
entries, artifacts, outputs and annotations. The harness checks that (section 5).

| Action | Uses [M] | Native design | What it does locally | Expected time against today |
|---|---|---|---|---|
| `actions/checkout` v7.0.1 | 61 | Native git against the node mirror, from `velnor-legacy` `checkout.rs` and `git_mirror.rs`, widened to the inputs `velnor-new` uses (sparse checkout included). Same inputs, outputs and masked git echo as upstream | Objects come from the mirror the push webhook already fetched. No HTTPS clone, no credential helper round trip. For obligation steps the agent materialises the sparse view itself and the step only binds it | ≤ 1 s [T] against 6 s median [M, §A T10] |
| `actions/upload-artifact` v7.0.2, `actions/download-artifact` v8.0.2 | 80, 72 | An artifacts v4 client over `ACTIONS_RESULTS_URL`. Uploads hash each file and store the blob in the site store first, then register it with GitHub | A download whose digest is already in the site store is a local copy, not a network fetch. Run-local artifacts (plan, crate reports, ledger) never leave the site unless a later job on another system needs them | Local hit under 1 s [E]. Today a 1.48 GB archive took 36 to 43 s to upload to GitHub [M]. Uncached uploads pay the same network cost as upstream |
| `actions/cache`, `actions/cache/restore`, `actions/cache/save` v6.1.0 | 2, 18, 5 | A cache service v2 client (twirp `CacheService` over the Results URL) with the action's key and restore-key rules, fronted by the node store | A restore whose entry digest is in the node store reflinks into place. A save writes the node store synchronously and the GitHub entry in the background, so the step returns early. Cache v1 is not implemented (binding decision 3) | Node hit under 1 s [E] against `Restore Cargo sources` 1 s median on a miss path that never restored [M, §A T9b]. The real win is that restores hit at all: 0 of 48 today [M] |
| `jdx/mise-action` v5.2.6 | 55 | The pinned tools (mise 2026.11.1) are already in the read-only tool store. The native action exports the same `PATH` and mise environment the upstream action exports, and verifies the pinned versions | No download, no install, no trust prompt. `install: false` is already the workflow's mode [M, §C.2] | About 10 ms [E] against `Prepare pinned tools` 10 s median, 18 s p90 [M, §A T9b] |
| `jdx/mr-boxington-action` v1.7.3 | 12 | Installs and configures mbx 1.29.2 exactly as the action does, until `velnor-gen` drops the step. Jobs run plain cargo (section 6.1), so the step becomes a no-op shim that only the older engine workflows still call | The mbx binary comes from the tool store. No setup download | About 10 ms [E] against `Set up MBX` 1 s median [M, §A T9b]. The step is deleted at the generator cutover |
| `actions/attest-build-provenance` v4.2.2, `actions/attest` v4.2.2 | 8, 1 | Sigstore signing with the job's OIDC token, then the attestation upload, as upstream does. The subject digest comes from the artifact store, so the step never re-hashes a file it can look up | The OIDC token comes from `GenerateIdTokenUrl` in the job message. Signing stays a network operation by necessity | Same network cost as upstream, minus about 100 ms of Node start [E]. Its input is a promoted artifact, so no build precedes it (section 2b) |
| `asamarts/alint` v0.21.0 | 1 | The pinned alint release binary from the tool store, run with the action's inputs (`config`, `fail-on-warning`, `format`, `path`, `version`) | The binary is local. No action download | About 1 s [E], the lint itself. Today the job also pays hosted queue and checkout |
| Local composites: `./.github/actions/declared-task-0` (279 uses), `velnor-tools-prelude-u26`, `velnor-tools-cache-restore`, `velnor-tools-cache-scrub`, `velnor-tool-seed`, `velnor-verify-*`, `u26`, `generator-release-*`, `actions/setup-velnor` | About 300 | Composite semantics in the executor (section 3.5). The tool composites mostly become native store checks, because the stores they restore and verify are node-local by construction | `velnor-verify-restored-rust` and the manifest checks verify store digests in memory instead of walking a restored tree | Milliseconds each [E]. Today `Prepare Rust components` and the verify steps add 1 to 2 s per job across 3,636 jobs [M, §A T9b] |
| `actions/github-script` v9.0.0 | 3 | None. It runs JavaScript, which binding decision 4 forbids. `velnor-gen` replaces its three uses (a ruleset-drift check and two qualification steps) with `velnor-ci` commands before cutover | The replacement commands run as ordinary `run:` steps against the GitHub API | Same API cost. The uses leave the closed set, so the executor never sees them |
| Qualification Docker and node test actions (`qualification.yml`) | 4 | Refused. Those cases become refusal checks in the corpus: the harness asserts that Velnor refuses them and the official runner runs them, and records the difference as intended | Nothing local. Refusal is the behaviour | Instant. The cases prove the boundary instead of crossing it |
| Any other action, or an unqualified SHA | — | Refusal before the first step, naming the action and listing the supported set | — | Instant. `velnor-gen`'s closed-set test makes this unreachable from generated workflows |

Reusable workflows (`workflow_call`, one pinned `source-preview-publication.yml` SHA) are not
actions. GitHub expands them into jobs before Velnor sees them, and each resulting job goes
through the same admission. Docker actions and `docker://` references are refused, as
`velnor-legacy` refused them at planning [M, §E.1.2]. Job containers and service containers stay
supported through the Docker class only where a workflow declares them. `velnor-new`'s generated
workflows declare none on the CI path.

## 5. The conformance oracle

The official runner is never a fallback (binding decision 1). It is the lab oracle that proves the
native executor behaves the same (binding decision 2). `velnor-conformance` runs each corpus case
on Velnor and on the latest official release in velnor-lab and compares the results.
`qualification.yml`, which already pairs hosted and scale-set jobs, is the seed.

### 5.1 Corpus, normalisation and comparison rules

The corpus has three parts.

1. **The full job set.** Every job of `velnor-new`'s 16 workflow files under each event that
   triggers it, at pinned commits, with recorded failures, cancellations and re-runs. Release
   workflows publish to a lab repository and lab registries, never to production.
2. **A semantics corpus.** `velnor-legacy` and `actions/runner` expression tests. Every context:
   github, runner, job, steps, matrix, needs, inputs, vars, and secrets masking. `if:` with
   `success()`, `failure()`, `always()` and `cancelled()`. `continue-on-error` at step, composite
   and job level. Outputs through `GITHUB_OUTPUT`, step outputs, job outputs and `needs`.
   `GITHUB_ENV`, `GITHUB_PATH`, `GITHUB_STATE` and `GITHUB_STEP_SUMMARY`. Workflow commands and
   annotations. Post steps and their order. Cancellation at every step boundary. Job and step
   timeouts. Cache and artifact round trips. OIDC tokens. Background step groups. Defaults for
   shell and working directory.
3. **Refusals.** Actions outside the set, unqualified SHAs, Node and Docker actions, and legacy
   features. The expected outcome is a refusal from Velnor and a run from the oracle, recorded as
   intended divergence, not a failure.

The lab repository dispatches each case twice from the same commit and event, once per runner
label. The harness pairs jobs by workflow, job and matrix cell, and collects runs, jobs, steps,
logs, annotations, summaries, caches, artifacts and attestations through the GitHub API.

Normalisation removes what cannot match: ids, timestamps and durations. Runner names, labels,
versions and machine names. Workspace, temp and tool-cache paths. Tokens. The official runner's
own lines (action downloads, banners, Node set-up). ANSI codes. The interleaving of concurrent
output inside a background group. Contexts compare as JSON with those fields removed.

Pass means exact equality after normalisation: step outcomes and conclusions, outputs, contexts
and env, expression results, annotations, step summaries, cache and artifact effects (keys, hits,
file lists, digests), post-step order and results, cancellation and timeout outcomes, and the
same timeline structure and user-visible log lines. Any other difference fails the case unless an
owner-signed allow-list entry with its reason covers it. The gate is 100% of cases.

The harness proves it can fail. Ten planted differences must each be caught: a wrong output, a
secret left unmasked, a reordered post step, a missing post step, a wrong `cancelled()`, a lost
annotation, a wrong cache key, a changed artifact digest, a swallowed non-zero exit and a
zero-test pass. A harness that passes a planted fault blocks cutover by itself.

### 5.2 Running against each new official release

The harness runs on every change to the executor, the native actions or the protocol code, as a
blocking check. It runs nightly. It runs again within 24 hours of each official runner release,
found through the releases API. Velnor reports the runner version it last passed against, so
GitHub's 63-day window, after which the service stops assigning jobs to an old runner [M, §F.6.1],
becomes the deadline for passing each new release. A canary job runs every 10 minutes through
each executor path in the lab, so protocol drift shows up as a failing case within hours instead
of a production incident. A deprecation tracker lists GitHub's announced dates for every API in
section 3.2.

### 5.3 Pass criteria for cutover

ARC with the official runner serves `velnor-new` until all four hold:

1. The lab corpus passes 100% against the latest official release on two consecutive nightly
   runs, with every allow-list entry signed off.
2. For two weeks and at least 300 merges, a non-blocking twin of every `velnor-new` job runs on
   Velnor, with publication off and outside the required checks. Every verdict difference is
   either fixed and replayed, or shown by paired reruns to be a flake on both sides.
3. Per job class, Velnor's twin is no slower than the official job at P90, and its job start is
   ≤ 2 s at P90.
4. Fault injection (section 3.9), the switch to the previous release, and control-plane and site
   loss are qualified (section 7).

Then `velnor-gen` routes `velnor-new` to Velnor's scale sets, and the same change deletes ARC's
`velnor-new` scale sets. The phase targets in section 11 do not depend on the cutover date: until
it, the same `ensure` path serves jobs on ARC through a `velnor-ci witness` step.

## 6. Compile, cache and test

### 6.1 The cache decision, on measured numbers

The cluster experiment (velnor-190 and velnor-5, 96 vCPU, 125 GiB, runner workspace 357 crates,
`cargo build --locked --workspace --all-targets` at `/w/velnor-new`, 32 jobs,
`CARGO_INCREMENTAL=0`, rust 1.99.1, mbx 1.29.2) decides the hot path:

| Scenario [M] | Wall | Compiled | Disk and network |
|---|---|---|---|
| Plain cargo, cold (P1) | 92.0 s | 357 | 10.6 GiB target |
| Plain cargo over a reflink clone of the warm target (P2) | 0.25 s clone + 0.31 s | 0 | +2.5 MB |
| Plain cargo over a full copy on ext4 `emptyDir`, today's layout (P3) | 8.6 s copy + 0.23 s | 0 | +10.75 GiB on the OS disk |
| mbx cold, empty server (M1) | 93.2 s | 357 | Uploads 8.7 GiB |
| mbx same node and path, managed target present (M2) | 0.32 s | 0 | Path-keyed managed target |
| mbx store warm, managed target gone (M2b) | 7.2 s | 0 (496 hits) | +1.94 GiB |
| mbx second node, empty store, warm server (M3) | 39.9 s | 0 (496 hits) | Downloads 8.7 GiB at about 215 MB/s |
| Lockfile change, 11 crates: plain over clone (L1p) | 67.4 s | 11 | +8.7 GiB, stale outputs stay |
| Lockfile change: mbx same node (L1m) | 88.4 s | 11 | +15.5 GiB, uploads 6.8 GiB |
| Lockfile change: mbx second node after the first built it (L1mb) | 43.4 s | 0 by fetch (29 hits) | Downloads 6.8 GiB |

**The hot path is plain cargo over a reflinked warm snapshot.** Each sandbox gets a reflink clone
of its node's warm snapshot of the trusted `main` generation, on the XFS volume at the fixed path
`/w/velnor-new`. Two rules keep it sound. Unchanged files keep the snapshot's recorded mtimes and
every other file is newer than every output, so cargo's time-based fingerprints rebuild exactly
what changed. `velnor-ci inputs restore` does this today. The agent does it for sandboxes, which
have no `.git`. And the path never changes: on the Mac both engines rebuilt the same 6 workspace
crates at every new path, because a path crate's identity includes its absolute path [M, Mac
experiment]. A changed file carrying an older mtime is a phase-1 test, because the measurement
took its times from one archive.

**mbx never runs in a job.** Its 0.32 s no-op needs one managed target per path, which concurrent
jobs at the fixed path would share. Plain cargo beat it on a same-node lockfile change, 67 s
against 88 s. Its uploads are the problem: 6.8 GiB published for an 11-crate change, and on the
Mac about 230 MiB of never-reused path-crate outputs stored per checkout, because mbx does not
cache path crates in this layout (144 hits, 10 units not looked up [M, Mac experiment]). The
cross-node layer is therefore not mbx's per-crate protocol. It is file sync by content hash: the
builder publishes a generation manifest (path, hash, size) to the site store, and each node's
warmer fetches only changed files into its snapshot. A one-file `main` change moves megabytes, not
gigabytes. A cold node still pays one full generation copy, about 8.7 GiB at about 215 MB/s or
about 40 s, the same bytes mbx moves [E from M3].

`mbx-cache` stays only behind a measured threshold, as an experiment beside the sync layer: keep
it while at least 10% of build executions would otherwise start cold or mismatched and are served
from it, or while it saves at least 20 runner-minutes a day of foreground builds. Retire it after
four weeks below both. At about 300 build executions a day, 10% at 24 to 52 s saved each is 12 to
26 runner-minutes [E], so the two limbs agree. The prediction here is retirement: sync makes
nodes rarely cold, and mbx's one unique strength, partial reuse when lockfiles differ, is exactly
the case its path-crate gap weakens. Overlayfs stays the remedy if page cache runs out. Clones do
not share page cache: each fully read clone held 14.29 GB resident, and the snapshot's pages were
evicted [M, E1d]. A build sandbox budgets its cache inside its 24 GiB limit, about 7 GiB above
the heaviest job's 16.8 GiB peak [M, §B.2.2]. The agent records memory pressure per sandbox.

**blaze is rejected on evidence (binding decision 6: judge on evidence only).** None of the five
workspaces builds fully with canary 0.20261105.4: a `no_std` proc-macro dependency, the lib and
bin same-name integration-test bug that blocks `velnor-ci` and `velnor-gen`, an unsupported
const-eval case, a front-end blow-up on `velnor-runner-host`, and missing `CARGO` env [M, blaze
B1]. On the one buildable subset it ran cold 12 times slower and edit-rebuild 5 to 15 times
slower than cargo incremental, used 18 to 44 GB of daemon memory against cargo's under 5 GB, and
missed a clippy `deny` lint that cargo catches: a false green in place of clippy [M, blaze B2 to
B5]. It always builds with its own nightly 1.101.0 and ignores the 1.99.1 pin, and its own docs
say CI should stay on the project toolchain. It is ineligible for required checks and useless as
pre-push feedback while it cannot run the suites. Revisit only on a release that builds all five
workspaces and matches clippy.

**Soundness of compile reuse (binding decision 11).** (1) The key covers lockfile checksums,
toolchain digest, profile, flags, features and the fixed path. The snapshot is only a starting
target directory: cargo still fingerprints every input, so a stale view is slow, not wrong,
except for the one case cargo cannot see, a file it never read. (2) Failure mode: a build script
output survives from a file no longer declared, and a tree passes that no clean build accepts.
(3) Detection: nightly and every-20th-merge cold builds from an empty target in a fresh sandbox
compare results and test-binary digests with the warm path. A lineage restarts cold on every
lockfile or toolchain change and at least daily. (4) Fallback: retire the lineage, delete the
generation, and build cold until the cause is fixed. A confirmed disagreement re-runs five times
first, to separate a flake from a wrong key.

### 6.2 The profile tension: test the bytes you ship

Today CI tests the default dev and test profiles [M, §C.7]. Releases build `ci-helper` and
`generator` with `[profile.release] opt-level = "s", lto = "thin", strip = true, panic = "abort"`
(`Cargo.toml:13-17`, `Cargo.toml:35-39`). The tested bytes are never the
shipped bytes. Three options:

| Option | Compile cost | Confidence |
|---|---|---|
| A. Keep dev tests. Build release once on `main`, promote it | Dev delta about 15 s [E] plus one release build per root on `main` (cold 92 s dev, release higher [E]) | The shipped binary never ran the suite. Optimisation, LTO and panic-mode bugs ship untested |
| B. Test the release build as-is | One build. Test execution gets faster: optimised dependencies cut the slowest test class's CPU by 66% [M], though that figure was measured with debug assertions off in dependencies | Debug assertions and overflow checks are off, so a class of failures stops firing. `panic = "abort"` changes unwind behaviour under test |
| C. One `verified` profile for both (chosen) | One build. Release codegen for every crate, assertions on | The tested binary is the shipped binary, stripped. Strip changes no code |

**Choice: C.** Define `[profile.verified] inherits = "release"`, with `opt-level = 2`,
`debug = "line-tables-only"`, `debug-assertions = true`, `overflow-checks = true`,
`panic = "unwind"`, `lto = false`, `codegen-units = 16`, `strip = false`. Every verification
build uses it. Shipping is `strip` on the promoted binary, which is the only transform, and it is
verified by re-checking the stripped digest against the manifest. `panic = "abort"` and thin LTO
are dropped. The size cost is measured in phase 1 on the real binaries. If it exceeds 15% on any
shipped binary, that binary alone keeps an LTO link as a separate obligation whose smoke suite
(`--help`, version, one golden invocation) runs against the LTO bytes, and the gap is recorded as
an accepted risk, never silent.

The arithmetic favours C. A warm delta build is dominated by the changed crates, so release
codegen adds seconds, not minutes, to the 15 s median delta [E]. Test execution returns more than
that: even half the measured 66% CPU cut on the slowest suites saves about 60 s of the 192 s
`velnor-runner-host` test step [E from M]. And C deletes the second full build at release time
entirely (section 2b). Nothing rebuilds silently: the artifact key names the profile, so a
profile change is a new artifact and a full re-verification. The four combinations of `opt-level`
and debug assertions, with overflow checks fixed on, are measured on a worker in phase 1 before
the profile lands. If assertions-on costs more than 25% of test CPU against assertions-off, the
owner decides again with both numbers in hand.

### 6.3 Breaking the 125 to 138 s single-test floor

Splitting the two loops (40 cases at `crates/velnor-actions-cli/tests/image_publication_public.rs:85-92`,
258 to 274 s; 6 cases in `unbound_retirement/tests/shape.rs:97-106`, 203 to 271 s [M, §C, S8])
does not reach 60 s. Excluding them, the longest test is still 132 s median in actions-cli, 125 s
in runner-host and 138 s in local-mode, with a median of 12, 41 and 51 other tests over 60 s, plus
a 924.8 s receipt-integrity test that builds the same state four times [M, 28 job logs]. No shard
count shortens one test. The programme, in order:

1. **The journal template.** `Journal::open` has 93 call sites in `velnor-runner-host` and is the
   unit of cost. Build one pre-migrated journal per schema version with the real code. Each test
   reflink-clones the closed template into a private fixture and keeps its own reopen,
   corruption-rejection, readback and preserved-file assertions. Gate: `Journal::open` on a
   template ≤ 1 s on a worker [T]. Migrations, historical upgrades and real deadlines stay
   separate obligations, because a template would hide exactly what they test.
2. **One test per case, prefixes built once.** The two loops, the 12-case `blob_faults` loop and
   the receipt-integrity test build their expensive valid prefix once per test binary, then clone
   it per case. Cases then run in parallel across the slot's threads and, later, across nodes.
3. **De-serialise the goldens.** Twelve archive-guard goldens run serially by
   `threads-required` (`.config/nextest.toml:17-19` [M, S8]) and `ci-helper` tests run on one
   thread. Private temp directories and ports per case remove both serialisations. The 89 s image
   publication fixture step (23% of the actions-cli job [M, §A T10]) folds into the warm
   `--all-targets` build instead of building cold examples per job.
4. **Split the crate.** `velnor-runner-host` (156,822 LOC [M, §D.2]) splits into five or six
   crates along module boundaries, one test binary per package, with `velnor-runner-local`
   extracted. A one-file edit then recompiles one small crate, not 61 s and 159 CPU-s of monolith
   [M]. `CARGO_INCREMENTAL` stays off until measured on a worker. Input keys tolerate its
   binaries either way, because keys hash inputs, not outputs.
5. **A per-test budget.** No test over 10 CPU-seconds measured alone, outside a recorded list
   that must shrink. Non-blocking in phase 1, blocking for new tests from phase 2. Committed: no
   test over 60 s after phase 2. Stretch: 15 s, which needs about a threefold cut per remaining
   case [E] and is killed if cases share state that cannot be separated.
6. **Fix the harness flaws that fake slowness.** Name the nextest profile in every generated
   command. The 2 s kill sits in `[profile.default]`, not `[profile.ci]`, and the golden
   workflow's commands pass no profile, so the kill would apply after the generator cutover [M].
   Remove `--config build.jobs=2` and `--jobs 2` (`mise.toml:90-193`, eight sites [M]) and set
   `CARGO_BUILD_JOBS` and nextest threads from the sandbox CPU count. `maintenance-helpers` falls
   from 8.5 min to about 5 min on that change alone [E], before its checks gain keys.

### 6.4 Verdict reuse without trusting anything unproven

An obligation is one unit of verification: format, lint, docs, a per-crate check, one test binary
under one configuration, or a named check. The released planner lists every obligation of the
tree on every event. Nothing is selected away. An obligation is skipped only when a receipt with
its exact key exists.

**The key is the hash of what the sandbox contains.** Per Cargo root first, per package after its
experiment: the Git tree hashes of the package's dependency closure from cargo's package graph,
of the shared files (toolchain pin, `mise.*`, `.github/`, `.velnor/`) and of declared extra
inputs, plus a definition digest (command, configuration, features, profile, nextest profile,
helper version, toolchain, runner image, sandbox class, CPU count) and an execution manifest
(event class, allowed env and values, image digests, path prefix, file modes and symlink
targets). Hashing takes under 5 ms from the commit, with no build [E].

**Soundness case (binding decision 11).** (1) Why the key is complete: the sandbox is a sparse
checkout of exactly the key's paths, so an input outside the key is not on disk. Tree hashes
include directory membership, so enumeration and negative lookups are covered. The binaries a
test executes are inputs, not outputs: 171 `CARGO_BIN_EXE_*` uses in 102 test files [M] mean the
closure includes every binary and example from `cargo test --no-run --message-format=json`.
(2) Failure mode if it is not: a test probes for a file, takes another branch when it is absent,
and passes. `velnor-gen`'s `golden.rs:19-40` enumerates a samples directory and asserts only that
it is not empty [M]. (3) Detection: the receipt-minting run logs every lookup outside the
declared set that returns `ENOENT`. A class enrols only when its restricted runs pass 100% and
match an unrestricted run's test inventory and outcomes. A failure names an undeclared input to
declare. After enrolment, 2% of reuses re-execute, and every landed tree executes in full once
within 24 h on batch capacity. A new reuse class starts in shadow at 100% re-execution and
graduates per obligation kind after about 3,000 clean reuses. (4) Fallback: a confirmed
disagreement retires that key version. Reuse for the class turns off until the cause is fixed and
re-enrolled. False greens have zero tolerance. Flakes have a budget (section 8.4).

**Only what Velnor observed is evidence.** A receipt is minted only from processes the node agent
started in a sandbox it created, running the released plan's command graph on the inputs it
mounted, with every exit and the expected test inventory observed. A missing, skipped, cancelled,
masked or zero-test result fails. A job's own report is never evidence. A lab test checks that a
forged passing report yields zero receipts. Receipts are signed per node and sandbox class by a
signer outside every sandbox, written to an append-only log per site, and verified before any
verdict uses them. They expire after 7 days as a backstop. Revoking a signer or a reuse class
raises the policy epoch: queued entries pause for re-verification, and landed commits that relied
on the revoked receipts are listed.

**Uncomparable work always runs.** Obligations that read the network, the event payload, the
clock inside an assertion, unseeded randomness, Docker images not pinned by digest, or the
commit's identity, or that publish anything, run fresh every time. For tests this is enforced, not
declared: a loopback-only network namespace and no event file. `cargo-deny` joins the keyed set
by pinning the RustSec database by commit, advanced by an automated PR. Today it reads live
advisories with no offline flag (`ci.yml:127`), so one new advisory ejects every queued entry.
An obligation counts as event-sensitive until an enforced run shows otherwise.
`velnor-runner-host/build.rs:69-80` reads `.velnor/runner.lock` from the Git object store [M]. It
stays uncomparable until it reads a declared file.

**Selection stays sound or wide.** The plan comes from a released helper, never from the change
under test. A file the plan cannot place selects everything. Nothing is ever skipped relative to
a merge group's predecessor: `velnor-ci` diffs a group against the entry ahead
(`event.rs:42-45`), so a failed predecessor's change would never be selected, and HEADGREEN would
land it. Merge groups stay full and are satisfied by lookup. Declaring the three crate-root build
scripts' inputs fixes the engine bug that forces heavy jobs into 97.4% of PR runs
(`crates/velnor-actions-rust/src/tasks.rs:254` sets `undeclared_reads` for every crate with a
build script [M]).

Test-binary digests are recorded with every receipt as an audit. They never join the key, so no
bit-identical build is required. Closures finer than packages stay a phase-4 experiment behind
the same enforcement.

### 6.5 Scopes, promotion and the builder

Only trusted builds of `main` write snapshot generations and the artifact store. One builder per
site builds each `main` generation at batch priority and publishes its manifest. Warmers sync
changed files. A build that can mint a reusable receipt reads only the trusted generation plus
its own delta, never a PR-scope target an earlier push wrote. That costs the delta build, 10 to
20 s for the generator and 45 to 80 s for `velnor-runner-host` [E], and it retires the per-PR
cache scope. The agent harvests a sandbox's outputs when the build phase ends, before any test
runs, so a test that writes into `target/` cannot poison a later request. A merge group never
writes the trusted scope, because it may never land. Its receipts become trusted when its exact
SHA is on `main`. An image, toolchain or helper bump changes every definition digest, so the
builder seeds the next generation and its receipts before the bump lands. With the ledger
unreachable, nothing is satisfied by lookup. Obligations run fresh. Slow, never wrong.

### 6.6 Test-level scheduling across the cluster

Inside one request the agent builds test binaries once per configuration, then runs format, lint,
docs, one combined check-alone step and tests in parallel lanes, one target lane per check kind,
under one jobserver as the CPU budget. Clippy and doc metadata come from the builder's generation
where the closure matches.

Spreading one suite across nodes (phase 4) happens only when the arithmetic wins: summed test
time over the slot's threads must exceed the longest test plus the cost of moving binaries. The
executor fetches each changed test executable and its verified fixtures by digest from the site
store, longest first, and checks that every expected case reported exactly once. Moving the full
1.48 GB archive to four workers in 5 s from one sender would need 9.5 Gbit/s [E], so only changed
binaries move. A week of per-test CPU-seconds from nextest JSON output, plus same-site `iperf3`,
decides it. Until then, 480 threads serve concurrent requests, not one suite.

## 7. Scheduling and the cluster

The cluster is five EPYC workers at 96 threads and about 125 GiB each, 480 threads in total,
across DE and FI with 35 ms between sites over WireGuard [M, §B.0, §B.5]. Three orgs share it:
tailrocks (max 60 runners), ChainArgos (60) and jackin (40) [M, §B.3.2]. Worker CPU sampled at 92
to 95% on 2026-11-18 [M, §B.2.1]. That load is duplicate work. Section 2b removes most of it
before any scheduler tuning matters.

**Sandboxes without a pod per job.** Each node agent runs as one privileged DaemonSet pod and
carves single-use sandboxes directly: a cgroup, a mount namespace with the sparse checkout and
store views, a network namespace (loopback-only for tests, allow-list for the job class), no
service-account token, seccomp `RuntimeDefault`, no capabilities. Creating a pod takes about 6 s
[M]. Creating a namespace takes milliseconds [E]. Kubernetes still schedules and accounts for the
agent pods, enforces per-org quotas at the namespace level, and carries the network policies.
Sandboxes are never reused. A job can leave processes, images and config behind, as a poisoned
persistent store showed in `velnor-legacy` [M, §E.6]. Cleanup is by UID, with process-tree exit
proven. If the namespace approach fails qualification, the fallback is pre-created single-use
pods at the same sizes. The verdict path does not change, only the start cost.

**Sizes from one memory budget.** A worker holds 125.5 GiB. Less 6 GiB system, 2 GiB eviction
margin and 8 GiB of gate slots leaves 109.5 GiB. Build sandboxes request 16 CPU with no CPU limit
and 24 GiB memory, request equal to limit. The heaviest job peaked at 16.8 GiB [M, §B.2.2]. Four
build slots fit per worker, idle ones included. Two stay pre-created, resized from the last hour's
arrivals. Gate slots take 1 CPU and 2 GiB. Docker slots take 12 CPU and 24 GiB on two tainted
nodes, one per site, and only for the test binaries that need a daemon (`local_pair_public`,
`local_host_public`, `velnor-local` Docker tests). Placement puts that build on the Docker node,
so its binaries clone locally in about 1 s [E]. `CARGO_BUILD_JOBS` and nextest threads equal the
sandbox CPU count, because a sandbox without a CPU limit sees all 96.

**Zero queue, by reservation, not preemption.** Each site reserves one 16-CPU build slot and 2
gate CPUs for merge-group and gate work, about 7.7% of allocatable CPU [E]. At 95% utilisation,
waiting grows to about 19 times service time, so the merge path never queues behind PR work.
Priorities: merge and gate first, then PR work for every org at equal priority inside per-org
quotas, then batch (builder, speculation, audits, scheduled runs). Only batch is preempted. The
wait clock starts at eligibility. Target: eligible-to-start p90 ≤ 2 s for merge and gate work [T].
ChainArgos and jackin keep guaranteed quotas and separate namespaces and stores. Nothing crosses
an organisation. They keep ARC until `velnor-new`'s cutover has soaked for four weeks, then move.

**Placement.** Every node is warm for `main`, so placement rarely decides warmth. Per obligation
the scheduler minimises predicted finish time: slot availability, view set-up, predicted rebuild
and test time on that node, plus a penalty when the request's other obligations and artifacts sit
in the other site. An idle cold node beats a busy warm one. A request stays in one site. Nothing
on the verification path reads across the 35 ms link synchronously. Placement regret is measured:
drop to site stickiness if a site fetch at P90 costs no more than a delta rebuild at P50, or if
regret stays under 5 s at P50.

**Isolation for trusted authors (binding decision 8).** There are no fork PRs. The design isolates
buggy code from the project's own people and agents, not hostile outsiders. Build sandboxes still
get no GitHub token, no `.git`, no event file and an allow-list network (crates mirror, site
store), because those are correctness properties of the key, not security theatre. Runner
credentials stay in the dispatcher, never in a sandbox. The required checks bind to the Actions
integration in the ruleset. Today `Required verdict` and `Required` name no integration, so any
app, or a workflow token with `statuses: write`, can post them [M, S3]. That closes in phase 0.
The Docker class is the one privileged surface. Image builds move to a rootless BuildKit service
per site and test containers run as sibling sandboxes through the agent. If more than 5% of the
Docker suites need a real daemon, a microVM Docker class replaces privileged sidecars. A Docker
node's signing key signs Docker-class obligations only, never a trusted receipt.

**Host and kernel tuning, ranked by gain per effort.** (1) Gate jobs on the cluster: −5.8 to
−57 min per run [M, §A T5]. (2) Honest requests, kube and system reservations (3 CPU, 6 GiB), a
2 GiB `memory.available` eviction threshold, `podPidsLimit` 16384, per-org quotas. Today pods
request 2 CPU and burst to 83.7 cores [M, §B.2.2], which stretches jobs 1.2 to 2.4 times under
load. (3) Remove the `-j2` caps. (4) Toolchain, tools and action state as read-only node stores:
14 to 18 s per job [M-derived]. The toolchain mounts read-only only with a workflow that expects
it. Today's fails on a read-only `RUSTUP_HOME` (`ci.yml:427,438`). (5) Work directories on the XFS
local-path volume with project quotas. `emptyDir` is ext4 on the OS disk and cannot reflink [M].
If the kubelet root moves too, eviction thresholds become fixed sizes, because 5% of a 50 TB
volume locks out 2.5 TB. (6) Warm snapshots with plain cargo (section 6.1). (7) The crates mirror
through env vars: 1 to 3 s per job and no internet dependency [M, §B.4.2]. (8) Debuginfo to line
tables for workspace crates and none for dependencies: −10 to −24% cache size and −20 to −27% CPU
on a runner-host rebuild [M]. Measure-first items get no credit: CPU governor or EPP to
performance (2 to 10% [E], unreadable read-only today [M, §B.1.6]), same-site 10 Gbit/s, static
CPU manager on one node, THP and writeback, mold (Rust 1.99 already links with rust-lld). SMT
stays on, the kernel stays 6.12, flannel over WireGuard stays. Tmpfs work directories and volume
striping stay rejected: reflinks cannot cross into tmpfs, FI workers hold one data disk each, and
the slow tests are CPU-bound.

**Control plane.** k3s has one server today [M, §B.1.5]. Losing it stalls pod creation and
cleanup in both sites. The executor cutover waits until control-plane and site loss are
qualified: three servers across failure domains, or independent admission per site with durable
fencing. Each site runs its own dispatcher, hooks, ledger, store and builder, so one site serves
alone at reduced capacity. Receipts replicate asynchronously and are hints across sites, never
authority: a site that cannot verify a receipt's log entry re-executes.

**More hardware, only on triggers.** One rented high-clock node (desktop-class cores at 5.5 GHz
or more) in phase 1 for the test-floor work: 1.3 to 1.5 times on the serial path [E]. Same-site
10 Gbit/s when test spreading needs it. More NVMe on FI workers when eviction drops live
generations. More EPYC workers when CPU stays above 70% at peak after dedupe, or the
eligible-wait for merge work exceeds a p90 of 10 s for two weeks.

## 8. The merge queue and GitHub integration

### 8.1 The trigger flow (binding decision 12)

A push starts no heavy verification. The flow, exactly as the owner set it:

1. A draft PR receiving pushes runs nothing.
2. Marking it ready for review still runs nothing.
3. Adding the label `velnor:verify` starts the PR's CI on the current head.
4. While the label is present, every new push starts a new pipeline and the previous one is
   cancelled immediately, with no stall (section 8.5).
5. Removing the label stops all further PR runs. In-flight obligations finish only if another
   request needs them. Their receipts stay valid for identical keys.

Nothing else starts PR CI. No auto-labelling from ready-for-review or auto-merge. A push webhook
still fetches objects into the node mirrors and may warm snapshots speculatively at batch
priority. Warming is not verification: it mints no receipt and posts no check.

The workflow trigger, as `velnor-gen` emits it:

```yaml
on:
  pull_request:
    types: [labeled, unlabeled, synchronize]
  push:
    branches: [main]
  merge_group:
    types: [checks_requested]
concurrency:
  group: velnor-ci-${{ github.event.pull_request.number || github.ref }}
  cancel-in-progress: ${{ github.event_name == 'pull_request' }}
jobs:
  required:
    name: Required
    if: >-
      github.event_name != 'pull_request' ||
      contains(github.event.pull_request.labels.*.name, 'velnor:verify')
    runs-on: [velnor, ubuntu-26.04-scale-set]
```

Guard details matter. A `synchronize` event on an unlabelled PR creates a run whose one job is
skipped by the `if:`. A skipped required check counts as passing in GitHub, which would let an
unlabelled PR merge. That is why the gate does not rest on the workflow alone (section 8.3). The
`unlabeled` event runs no verification. It exists so `velnor-hooks` learns to stop. The executor
also re-reads the label set from the job message context before it starts obligation work, so a
label removed between run creation and acquisition still stops the run. The `edited` type is
gone. A base retarget reaches the ledger through the App webhook and re-plans without a new run.

**How label events reach Velnor, verified.** Events created with the repository's `GITHUB_TOKEN`
do not create workflow runs. The exceptions are `workflow_dispatch` and `repository_dispatch`,
plus `pull_request` `opened`, `synchronize` and `reopened`, which create runs in an
approval-required state. Other types, `labeled` and `edited` named explicitly, create no runs
([GITHUB_TOKEN](https://docs.github.com/en/actions/concepts/security/github_token)). The owner's
flow has people and agents apply the label as themselves, so the workflow path works. Two cases
need the fallback. A label applied by a workflow using `GITHUB_TOKEN` starts nothing. And any
future automation that labels on the owner's behalf must not use `GITHUB_TOKEN`.

**Fallback: the App webhook is the primary trigger, the workflow is the reporter.** Suppression
applies to workflow-run creation, not to webhook delivery. The GitHub App still receives the
`pull_request` `labeled` and `unlabeled` deliveries whoever applied the label. `velnor-hooks`
treats that delivery as the start signal and calls `ensure` at once. If the label came from
`GITHUB_TOKEN`, no run exists yet, so the App creates one through `workflow_dispatch` on the PR
head ref, which always creates a run, with the PR number and head SHA as inputs. The dispatched
run's `Required` job joins the `ensure` work the webhook started. This keeps one code path: the
webhook starts verification, a run reports it. The App path is tested in the lab with a label
applied by `GITHUB_TOKEN` before phase 2 ships. If App dispatch ever fails, verification still
completes and the merge group still gates the merge (section 8.3). Only the PR check display
degrades.

### 8.2 No duplicate verification across PR, merge group and `main`

Every event lists every obligation. Satisfaction is by key, never by conclusion:

| Event | What runs |
|---|---|
| Labelled PR push | Obligations without a passing receipt for their key at the PR's merged tree (`git merge-tree` of head into current base) |
| Merge group | Every obligation listed. Most satisfied by lookup. The rest run. A merge tree that differs from GitHub's is a key miss, never a wrong verdict |
| Push to `main` from a merge | Release and publication duties only. One real `Required` job, satisfied by lookup on the same tree |
| Direct push to `main` | Everything without a receipt. No shortcut |

Replayed over 100 real merges, the root-jobs merge groups execute fall from 500 (full) to 425
with exact-tree receipts and 125 with per-root keys [E]. Per-package keys add 3 to 10 points of
merge-group emptiness on top of the window effect in section 2.1 [E].

**Speculation on the queue tip.** GitHub builds a merge group on the queue entry ahead, not on
`main`: 122 of 193 merge-group runs sat on an unlanded group commit [M]. From phase 0 a read-only
shadow predicts each group's tree. From phase 3 the ledger verifies ordered prefixes
`T_i = T_{i−1} ⊕ PR_i` and single-ejection bypass trees (7 trees for 4 queued PRs) at batch
priority through the same `ensure`. When GitHub creates the group, its obligations are already
receipted. Kill the mechanism below a 70% tree match over 50 merges. A wrong guess wastes
preemptible CPU. It can never produce a verdict, because the verdict recomputes the inventory
from the actual group commit.

### 8.3 The gate: ruleset settings

Merging requires a passing verdict on the exact merged tree, and a PR may merge or enter the
queue only when its latest commit holds a passing verdict (binding). Velnor never reports success
for a commit it did not verify. The ruleset `protect-main` (id 24396608 [M, S3]) becomes:

- **Required status checks:** `Required verdict` and `Required` during migration, then `Required`
  alone at the generator cutover. Each bound to the GitHub Actions integration (15368) by
  `integration_id`, so no other app and no workflow token can post them.
- **Require branches to be up to date:** off. The merge group tests the real combination, which
  is stronger than an up-to-date branch.
- **Merge queue:** required. Squash, `min_entries_to_merge` 1, `max_entries_to_merge` 10,
  `min_entries_to_merge_wait_minutes` unchanged (it never applies at a minimum of 1; enqueue to
  group run already measures 19 s [M]), `check_response_timeout_minutes` 90 falling to 20 in
  phase 3. Grouping HEADGREEN until speculation keeps bypass trees verified, then ALLGREEN
  (section 8.4). `max_entries_to_build` stays 2 until the under-load sampler has data, then moves
  only when a replay with real arrivals and all failures counted shows more merges per hour:
  about 4 at ≤ 3% false failures, 8 at ≤ 2% with bypass trees. At today's 8.1%, depth 4 disturbs
  29% of windows and depth 8 disturbs 49% [E: `1−0.919ⁿ`].
- The queue requires the same `Required` check on the merge-group commit. The workflow therefore
  triggers on `merge_group` `checks_requested`, which GitHub requires for queued validation to
  run at all ([Managing a merge queue](https://docs.github.com/en/enterprise-cloud@latest/repositories/configuring-branches-and-merges-in-your-repository/configuring-pull-request-merges/managing-a-merge-queue)).

**Why an unlabelled PR cannot merge.** The required check must report success on the PR's latest
commit. An unlabelled PR never starts a run, so its head holds no `Required` check at all, which
GitHub treats as pending and blocking. The one hole is the skipped-job rule: a run whose required
job is skipped reports the check as passing. Three defences close it. The generated workflow has
no path filters and no job-level skip that can produce a green `Required` without the executor
running: the label guard sits on the witness job, and a skipped witness is the risk, so the
generator also emits a second, always-running one-step job that fails when the event is a PR
without the label and succeeds otherwise. Its name joins the required list during the label-flow
migration. The executor independently refuses to complete any PR job as success unless the label
was present at acquisition or the run came through App dispatch for a labelled PR. And the merge
queue re-verifies regardless: even a wrongly green PR check only buys queue entry, where the
group run decides. A push after a green run moves the head, so the green check no longer sits on
the latest commit and the PR blocks again until the new head passes. While the label is present
that run starts automatically on `synchronize`.

**The release gate stays as it is.** `velnor-ci release-gate` keeps only `push` runs on the exact
SHA and accepts the check only from integration 15368. `product-release.yml` asserts exactly one
job named `Required` in `ci.yml`'s push run. The design keeps that job, that name and that file.
Promotion (section 6.2) happens downstream of the gate: the release reads the gate's SHA, then
copies the artifacts that SHA's `main` build already produced.

### 8.4 HEADGREEN, auto-merge and agent behaviour

HEADGREEN lands a group when its tip passes, so a failed entry can land inside a passing group.
It happened once in 15 days: 1 of 31 failed merge-group commits is on `main` [M]. The contract
therefore states plainly that every tip of `main` is verified, and lists each group's
intermediate commits for bisection and release tooling. No release is cut from an intermediate
commit. ALLGREEN becomes affordable once queue-tip speculation keeps the bypass trees verified,
because an ejection then costs a lookup instead of a rebuild. The switch is the owner's call,
gated on the shadow's match rate.

Agents enable auto-merge when they open a PR. Today 0 of 40 merged PRs did [M, S2], and the
median PR waited 7.5 min between green and enqueue. Agents stop merging `main` into their
branches: a catch-up merge restarts verification for no confidence, because the group tests the
real candidate. Agents add `velnor:verify` when the PR is ready to land, not before, and run
`velnor verify` before the final push so the labelled run finds receipts waiting. Auto-merge plus
the label is the whole contract. The label starts verification. It is never the gate.

### 8.5 The cancelled-run stall, removed

Evidence: every sampled PR run delayed over 10 s had an unfinished earlier run on the branch, and
got its first job within 5 s of that run ending. The wait is 60 s p50, 626 s p90, 2,203 s max
[M]. The cause is structural. `concurrency.cancel-in-progress` cancels the old run, but its
`Required` job has `if: always()`, so the cancelled run still needs a hosted runner to finish
dying, and the new run's jobs wait for the group to free.

Four changes remove it. One job per run means there is no `always()` gate job left in a cancelled
run. The witness runs on Velnor, so even the dying run's last act takes seconds, not a hosted
queue. The executor completes a cancelled job as soon as its process group is dead and its post
steps finish or time out, releasing the concurrency group at once. And verification itself does
not wait for the run at all: the `synchronize` webhook starts `ensure` for the new head while the
old run is still cancelling, and obligations the old run started that the new head still needs
keep running under in-flight join. The admission span (run created to first job) becomes a
reported metric with a phase-0 gate of p90 ≤ 5 s.

### 8.6 Flakes

A false failure is a required run that fails where an immediate rerun passes. Today's one-direction
discordance is 8.1% [M, S1]. Budget: ≤ 3% from phase 1, ≤ 1% from phase 3, each claimed only on
enough paired reruns in both directions under representative load. About 300 pairs bound 1% at
95%: zero disagreements in 299 pairs give a one-sided bound just under 1% [E]. Retries stay at 0
in the required profile, so a flaky test fails the run and is counted. Two false failures in seven
days earn an owner-labelled issue. Quarantine needs a blocking check covering the same property
or an expiring owner exception, and the test keeps running visibly. Changed obligations running
three times in parallel (section 6.4) raise both detection and the flake sample at once, which is
the point: the budget is measured where the risk is.

## 9. What would make CI not a bottleneck at all

CI stops being a bottleneck when the wait for a verdict is shorter than the time a person or an
agent needs to read it. For this repository that means a labelled PR merges faster than its author
can switch tasks. The theoretical floor, stage by stage:

| Term | Floor | Why it cannot go lower |
|---|---|---|
| Push to webhook delivery | 1 s [E] | GitHub creates push-event runs in 1 to 2 s [M]. Delivery over the public internet cannot beat that |
| Fetch, merge tree, plan and keys | 1 s [E] | Tree hashes cost under 5 ms [E]. The rest is one mirror fetch of the pushed objects and one planner run |
| Sandbox and view | 0.3 s [E] | A reflink clone alone measures 0.25 s [M, P2] |
| Delta compile, median PR | 8 s [E] | One changed crate and its dependents through rustc on one EPYC core chain. The serial dependency chain of a cold runner build is about 170 s [E], so warmth is not optional at the floor. It is the floor |
| Test lanes, median PR | 5 s [E] | The longest single changed test after section 6.3, run three times in parallel. Parallelism cannot beat one test |
| Witness job and check | 3 s [E] | GitHub creates the job in about 15 s in parallel [E], so it hides under the build. Completion is one `completejob` round trip plus Results metadata |
| Enqueue, merge group, merge | 45 s [M] | Enqueue to group run 19 s plus finish to merged 26 s. Both are GitHub's queue machinery. Only a Velnor merge train removes them (below) |

The floor is therefore about **20 s to a PR verdict and about 65 s from final push to merged**
for the median PR [E, sum above]. Two terms dominate and neither is compute: GitHub's 45 s of
queue latency, and the single longest test. Everything else is already at seconds.

The design in this document reaches 35 s and about 2 min at stretch (section 2.2). Three further
steps close the gap to the floor, each behind its own experiment. First, verification before the
push: `velnor verify` overlaps the delta build with the agent's own final edits, so the push
finds receipts and the verdict collapses to webhook plus witness, about 5 s [E]. Second, the
15 s test stretch and test spreading (section 6.6) pull the lane term toward 5 s for runner
workspace PRs as well. Third, a Velnor merge train replaces GitHub's queue: Velnor fast-forwards
`main` by compare-and-swap to the exact commit whose obligations all hold, which verifies every
tip by construction and lands a green PR about 5 s after its verdict. The costs are real.
Squashed PRs must still show as merged, and a Velnor outage stops merges until the queue rule
returns. It runs as a two-week shadow first and dies below a 95% tree match. It is phase 4,
stretch only, and the owner's decision.

The irreducible costs, stated once: one network round trip to learn about the push, one rustc pass
over the changed code, one execution of the longest changed test, and, while GitHub owns the
merge, 45 s of its queue. Any design that promises less is either not verifying the tree it lands
or not using GitHub's merge. At the floor, CI is 65 s of a workflow whose human steps take
minutes. It is no longer the bottleneck. The bottleneck becomes writing the change.

## 10. Critique of v2

v2 (`velnor-runner-architecture.md`) is the strongest base any panel round produced. Its
obligation ledger, its same-confidence contract and its cache measurement all stand. This design
keeps them. What follows is only what the owner's later decisions and the evidence overturn, by
v2 section.

**v2 section 1.4 and section 10 keep per-push verification as the unit of speed.** Binding
decision 12 ends that. v2's phase 3 starts `ensure` on every push webhook. With 45% of runs
cancelled and agents pushing intermediate commits, that spends most compute on trees nobody asked
to verify. Overturn: the webhook only mirrors and warms. The label starts verification (section
8.1). Push-to-merge is then meaningless without naming the labelled final push, and v2 never
redefines the clock. This design does (section 2).

**v2 section 3.4 and section 6.1 still execute verification inside the shape of a job.** The
witness exists in v2, but the design keeps sizing pods per obligation and keeps `velnor-ci
witness` as a step. That leaves pod create (6 s [M]) and kube scheduling on the path to a 35 s
verdict. Overturn: sandboxes are agent-carved namespaces (section 7). Pods deploy agents, never
jobs. Job start ≤ 1 s needs it.

**v2 section 3.8 keeps a fork class on hosted runners, and section 13 question 7 keeps macOS
hosted.** Binding decisions 8 and 9 delete both. There are no fork PRs, so the fork-conditional
`runs-on`, the microVM fork class and the fork trust scope are dead design. macOS release builds
pause, so every job runs on Velnor from phase 0 and no capacity plan needs hosted Macs. Keeping
either reopens the hosted queue that caused 79% of the worst day's wall time.

**v2 section 5.1 keeps mbx plus `mbx-cache` as the cross-node layer with jobs taking a warmer
refill.** The cluster numbers support the hot path but not the cross-node choice. mbx published
6.8 GiB for an 11-crate change [M, L1m], stores about 230 MiB of never-reused path-crate outputs
per checkout [M, Mac], and needs TLS plus `MBX_REMOTE_TRUST_WRITES=true` plus a loopback
forwarder to run at all in the experiment. File sync by hash moves the same cold bytes and
megabytes per incremental generation. Overturn the default: sync first, `mbx-cache` as a
threshold-gated experiment predicted to retire (section 6.1).

**v2 never resolves the profile tension.** Its keys name a profile, but the tested profile and
the released profile stay different, so its release path still rebuilds what `main` tested.
Binding decision 10 forbids the silent rebuild and demands a choice with numbers. Section 6.2
makes it: one `verified` profile, stripped for shipping. v2's section 6.4 table also lets the
push run skip obligations on the merge group's say-so in phase 1 wording. This design forbids
that at every phase: satisfaction is by key only.

**v2 section 6.5 gates queue depth on the false-failure rate alone.** Genuine failures disturb
windows too: 3 of 40 queue exits already fail with full PR runs [M, S2]. Depth must be gated on
merges per hour from a replay with all failures counted (section 8.3). v2 also leaves HEADGREEN
versus ALLGREEN as owner question 3 without the mechanism that makes ALLGREEN cheap. Speculated
bypass trees are that mechanism, so the question gets a gated answer, not an open one.

**v2 has no cancelled-run stall fix beyond moving `Required` to the cluster.** Moving it shrinks
the stall to one warm job. It does not remove the cause, an `always()` job in a dead run holding
the concurrency group. One job per run plus native cancellation plus webhook-started verification
removes it (section 8.5). v2's own admission-span metric (p90 226 s [M]) demands the stronger fix.

**Decisions from the log this design overturns:** N09 and N18 as written (push-webhook
verification start becomes label-webhook start). N54's default (mbx-cache kept) becomes sync kept,
mbx-cache experimental. I091 and N65 (forks and macOS hosted) are deleted by binding decisions 8
and 9. I082 (agent-carved sandboxes, experiment-gated in v2) becomes the default, with pods as
the fallback. N31 (merge train) stays an owner decision but gains a floor argument: without it,
GitHub's 45 s is the largest term left (section 9).

## 11. Phases

Targets hold per PR class, on at least 300 merges or four weeks, with intervals beside each P50
and P90. A P90 needs at least 200 matched heads across peak and quiet periods. A phase that misses
its gate does not start the next until the miss is explained. The first five work items are marked
**W1** to **W5**. They need nothing else first.

| Phase | Scope | PR verdict P50/P90 [T] | Push-to-merge P50/P90 [T] | Runner-min per merged PR [T] | Cache and conformance [T] |
|---|---|---|---|---|---|
| 0, week 1. Config only, on ARC | **W1:** regenerate the workflow so Plan, lints and both Required jobs run on a `velnor-gate` scale set (the generator release merged but the workflow was never regenerated [M]). **W2:** disable macOS release workflows. Narrow `job-image-candidate` to `images/**`. Source qualification to the cluster or the queue only. Bind required checks to integration 15368. Fixed absolute cache paths. `event.json` snapshot fix. Auto-merge on, no catch-up merges. Push-to-merge, admission-span and flake instrumentation. Queue-tip shadow. The label flow in shadow: count what it would have skipped | ≤ 12 / ≤ 20 min | ≤ 25 / ≤ 60 min | Baseline re-measured, split completed, cancelled, superseded | Admission span p90 ≤ 5 s. Hosted queue on the critical path p90 ≤ 1 min |
| 1, weeks 2 to 3. Warmth and the floor | **W3:** work directories on XFS with project quotas. Per-node warm `main` snapshots at `/w/velnor-new` with the mtime rule. One builder per site plus hash-sync warmers. Snapshot-miss rate by cause. **W4:** the test-floor programme (journal template, loop splits, de-serialised goldens, `-j` caps out, nextest profile named). **W5:** selection fixed in the engine (declared build-script inputs, shadow-replayed over 200 PRs). The `verified` profile measured in four combinations. Node tool stores, crates mirror wired. Honest sizes, reservations, quotas. Push run reduced to release duties. On velnor-lab: the native executor MVP, native actions and the conformance harness | ≤ 5 / ≤ 10 min (stretch 3.5 / 8) | ≤ 12 / ≤ 30 min (stretch 10 / 22) | ≤ 150 | Warm view set-up ≤ 1 s p90. `Journal::open` ≤ 1 s. False failures ≤ 3% when pairs allow the claim. Lab corpus pass rate published daily |
| 2, weeks 3 to 6. Cutover and the ledger | Executor cutover behind the section 5.3 gate. `velnor-gen` writes one job named `Required` whose obligation steps call `ensure`. Ledger with per-root keys, receipts, in-flight join, signer and log. The `velnor:verify` label flow live with the App-dispatch fallback (section 8.1). The crate split. the State seal workflow and `cache-prune.yml` deleted. `mbx-cache` threshold evaluated. Release promotion on the `verified` profile for Linux artifacts | ≤ 2 min / ≤ 6 min (stretch 80 s / 4 min) | ≤ 5 / ≤ 15 min (stretch 3.5 / 9) | ≤ 60 | ≥ 65% of merge groups execute nothing. No test over 60 s. Job start ≤ 2 s p90. Conformance 100% on two consecutive nights, then blocking |
| 3, weeks 6 to 10. Verification before the job | `velnor-hooks` starts `ensure` on label, merge-group and queue-tip events. Per-package keys after the enrolment experiment. Queue-tip speculation live. `velnor verify` for agents. Queue depth from the replay. `check_response_timeout` to 20 min | ≤ 90 s / ≤ 4 min (stretch 60 s / 2.5 min, ≤ 20 s after a preflighted push) | ≤ 3 / ≤ 8 min (stretch 1.75 / 5 min) | ≤ 35 (stretch 15) | ≥ 85% of merge groups execute nothing. First cargo ≤ 5 s after the label. False failures ≤ 1%. Compile-cache hit: 0 crates on ≥ 80% of executions whose lockfile matches the snapshot [T] |
| 4, weeks 10 to 18. Stretch | Test spreading across nodes where the arithmetic wins. Test-binary closures behind enforcement. Docker without a privileged sidecar. The merge-train shadow, then the owner's decision | 30 s / 100 s, stretch only | 100 s / 4 min, about 50 to 75 s with the train, stretch only | ≤ 15 | Conformance within 24 h of every official release, as a standing gate |

Week-1 experiments run beside phase 0 on velnor-lab and choose what phases 2 and 3 build: webhook
arrival over 50 pushes (kill the 60 s stretch above 8 s p50), sparse-checkout enrolment per
package (kill package keys above 20% failures to enrol), the queue-tip shadow (kill below 70%),
the four profile combinations, and one commit built cold on two nodes and warm with every test
binary compared.

## 12. Risks

| Risk | Detection | Containment |
|---|---|---|
| A key misses an input and reuse gives a false green | Shadow re-execution of every new class. The 2% sample. Cold differential builds. Every landed tree in full within 24 h. `ENOENT` telemetry from minting runs. Post-merge watch: revert rate, red-`main` rate, failures on receipt-passed trees | Retire the key version at once. Reuse off for the class until fixed and re-enrolled. Revert the bad merge by a normal PR. The policy epoch pauses queued entries that carry the suspect receipts |
| A stale compile view hides a failing build | The cold build disagrees with the warm path. The older-mtime test in phase 1. Only the builder promotes, and receipts record producer and scope | Retire the lineage or delete the generation. Requests build cold: slow and correct |
| The executor departs from GitHub semantics | The harness on every change, nightly, and within 24 h of each official release. The twin-job shadow. Ten planted faults that must be caught | Block the release. Switch to the previous Velnor release. Before cutover, ARC keeps serving. After cutover there is no official-runner fallback, by binding decision 1, so the previous release is the rollback and it is drilled weekly |
| GitHub changes the protocol, webhooks or the queue | The harness per release. A canary job every 10 min. The deprecation tracker. The reported last-passed runner version against the 63-day window | Fix and release inside the window. A job's own `ensure` covers webhook gaps. The App replays missed deliveries every minute |
| The label flow fails closed the wrong way: a skipped job greens an unverified PR | Lab cases for every label transition, including a `GITHUB_TOKEN` label, a label removed mid-run and a retarget. A standing audit that no `Required` success exists without a matching set of receipts | The always-running guard job, the executor's own label check, and the merge group as the final gate (section 8.3). Any success without receipts pages the owner and freezes merges for the repository |
| A signer leaks or a forged receipt reaches a verdict | Every verdict carries its policy epoch. Signature, class and log entry checked before use. Audits re-execute on a different node | Raise the epoch. Revoke the key and its receipts. Pause and re-verify queued entries. List landed commits that relied on them |
| The dispatcher, ledger or ingress is down | Replay backlog, jobs waiting at GitHub, session and ledger health | Obligations run fresh without lookup. The standby dispatcher reopens sessions from its journal. Slow, never wrong |
| False failures rise as work packs denser | The paired-rerun sampler under load. Flake counts per test. Merges per hour | Lower the per-node build cap. Hold queue depth at 2. Quarantine only under section 8.6 |
| Page cache or disk runs out | Memory pressure per sandbox. Store budgets with alerts at 70%. Eviction of live generations | Overlayfs for shared page cache. Least-recently-used eviction inside per-org caps. More NVMe on the section 7 trigger |
| Other orgs, speculation or the builder starve real work | Wait per organisation. Batch-class CPU share. Eligible-wait for merge work | Quotas and the reserved merge path. Batch runs at the lowest priority and is preempted first |
| Control-plane or site loss | Qualified before cutover by fault injection, then by game days | Three servers across failure domains, or independent admission per site. One site serves alone at reduced capacity |
| The new code grows like `velnor-legacy` | Line budgets checked in CI. Phase gates | Stop and review at a budget. Nothing copies from `velnor-legacy` without its tests and an oracle pass |
