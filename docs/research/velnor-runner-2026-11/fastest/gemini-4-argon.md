# Is this the fastest runner? Critique by Gemini 4 Argon

| Field | Value |
|---|---|
| Model | Gemini 4 Argon |
| Model id | `gemini-4-argon` |
| Client | Antigravity CLI 1.5.9 (agy -p) |
| Reasoning effort | high |
| Date / run | 2026-11-19 |
| Brief | fastest-runner critique (2026-11-19): the final design, research folder and evidence |

This is the model's own critique of the final design against the claim that Velnor is the fastest possible runner at the same confidence, unedited except for formatting.

---

# Independent review of the Velnor v3 runner architecture

**Goal.** You want an honest test of whether Velnor v3 is the fastest possible CI runner for `tailrocks/velnor-new` at zero false-green tolerance, and whether the evidence supports calling it "the fastest runner ever".

## Verdict

**Not yet, and not as a runner.** For `tailrocks/velnor-new`, Velnor v3 will beat GitHub-hosted runners (`26.7 min` P50 PR wall `[M, proxy]`, `44 to 61 s` cache restores `[M]`) and Kubernetes ARC (`13 s` P50 acquire `[M]`, `6 s` pod start `[M]`, `39 jobs x 3 runs` per merge `[M]`) by 15x to 40x `[E]`.
Almost all of that gain comes from bypassing GitHub Actions on pull requests (`ensure` via a GitHub App webhook), fixing workflow self-harm (`build.jobs=2`, hosted gate queues, ext4 `emptyDir` copies), and cloning warm `target/` trees with XFS reflinks (`0.25 s` `[M]`).
As a runner engine, Velnor Native's speed advantage over `Runner.Worker` on the same warm sandbox and stores is unmeasured (`§5.3` `[E]`), and its Kubernetes pod model (`≤ 300 ms` `[T]`) is slower than bare-metal Linux namespace runners (`I082`, `< 25 ms` `[E]`) and persistent Buildkite-style agents.
Against Bazel or Buck2 with Remote Build Execution (RBE), Velnor's cargo-root generation model is coarser because `CARGO_INCREMENTAL=0` (`ruling Q17`) forces full-crate recompiles (`59 to 61 s` on `157k` to `187k` LOC crates `[M]`) and 79 of 86 `CARGO_MANIFEST_DIR` callers force test keys back to the whole tree (`§6.1` `[M]`).
Even on `tailrocks/velnor-new`, 11 clock-reading test files (`C5`) block instant merge-group lookups (`§6.1` `[M]`), and the cold second witness keeps PR verdicts at `6 to 18 min` `[E]` until 3,000 comparisons graduate per root.

## Where the time goes, per stage

Section 3 of the architecture budgets a typical pull request from the 47% cohort that touches only `generator/` or `ci-helper/`.
That table combines one real cluster measurement (`S12` on an unchanged tree) with unmeasured targets `[T]` and optimistic estimates `[E]`.
It leaves out the other 53% of pull requests, the mandatory cold second witness during graduation, and the `C5` fresh-work rule.

| Stage | Today `[M]` | Design goal `[T]` and estimate `[E]` | Measured vs estimated basis | True floor (47% small-root cohort vs 53% large-root or whole-tree cohort) |
|---|---|---|---|---|
| 1. Event, webhook, planner | `1 to 2 s` push to run, `2 s` P50 (`226 s` P90) to first job, `5.5 min` P50 (`23.1 min` P90) hosted Plan queue, `60 s` P50 (`626 s` P90) cancel stall `[M]` | `1 to 2 s` webhook, `≤ 10 s` reconciler `[T]` | GitHub job offer (`5 s` queued, `8 s` running `[M]`, `V5`) is removed from PRs. Webhook plus `git merge-tree` plus `velnor-ci` planner is estimated `[E]`. | **1.5 to 3.0 s** on webhook hit (`11 s` if dropped to the 10 s REST reconciler) `[E]`. |
| 2. Job acquire | `13 s` P50 (`27 s` P90) job created to started on ARC (`n = 1,124` `[M]`, `3 s` idle `[M]`) | Not on PR verdict path. Actions jobs `≤ 1 s` P50 (`≤ 2 s` P90) `[T]` | Off the PR path by `Q2`. Actions-path acquire plus mandatory first `POST /renewjob` (`N85`) before step 1 at `120 ms` US RTT `[M]` is unmeasured `[E]`. | **0 s** on PR and merge-group verdicts. **1.5 to 3.0 s** on acquired Actions jobs `[E]`. |
| 3. Sandbox start | `6 s` ARC pod creation (`436 starts` `[M]`) plus DinD probe (`57/h` failures `[M]`) | `≤ 0.3 s` `[T]` | Only XFS `cp --reflink` (`0.25 s` `[M]`, `S12`) is measured. Warm K8s pod claim, mount attach, and exec (`P04` goal `≤ 1 s` `[T]`) are unmeasured. | **0.5 to 1.0 s** with K8s warm pods plus reflink (`0.28 s` with direct namespaces `I082`, **2.5 to 5.0 s** for private DinD `P05`) `[E]`. |
| 4. Checkout and mtimes | `6 s` P50 (`8 s` P90) per job across 31 to 39 jobs (`110 MiB` `[M]`), Plan `12 s` `[M]` | `≤ 1.0 s` `[T]` | **Estimated `[E]`.** Reflink cloning the `110 MiB` tree, rewriting delta blobs, setting `M_G` mtimes (`N106`), and SHA-1 hashing every staged file (`0.1 to 0.3 s` `[E]`, `C10`) are untimed together. | **0.4 to 1.2 s** (`< 0.05 s` if an `overlayfs` lower layer avoids full-tree re-hashing) `[E]`. |
| 5. Toolchain and tools | `10 s` P50 (`18 s` P90) tools, `2 s` (`5 s` P90) mise, `4.4 s` action downloads per job `[M]` | `≈ 0 s` `[T]` | **Estimated `[E]`, sound.** A read-only bind mount of `/opt/velnor` and native Rust actions perform zero downloads or extractions. | **0.05 s** (mount exists in warm sandbox) `[E]`. |
| 6. Dependency fetch | `3.5 to 6.1 s` direct (`2.7 to 3.3 s` via cluster mirror `[M]`), sources cache `0 of 48` hits `[M]` | `0 s` on lockfile hit (`≈ 3 s` on change) `[T]` | **Measured `[M]`** for mirror fetch (`2.7 to 3.3 s`) and `0 s` for `--offline` over a pre-populated crate store. | **0 s** on lockfile hit, **2.7 to 3.3 s** on lockfile change `[M]`. |
| 7. Compile | `14 s` P50 (`23 s` P90) `velnor-ci`, `81 s` (`116 s` P90) `runner-host`, `8.5 min` critical path `[M]` | `≤ 25 s` `[T]` (`15 s` `[E]`) | `0.31 s` `[M]` is a **0-crate no-op** (`S12`). A 1-file edit with `CARGO_INCREMENTAL=0` (`Q17`) took `61 s` on `runner-host` `[M]` (`59 s` on cluster `L1p` `[M]`). Cold build is `36 s` (`generator`) to `92 s` (`runner`) `[M]`. | **Before graduation (cold 2nd witness): 36 to 120 s `[M]`. After graduation (`INCR=0`): 8 to 20 s vs 45 to 65 s `[E]`.** (`1.5 to 6 s` with `INCR=1` `[M]`). |
| 8. Tests | `10 s` P50 (`62 s` P90) `velnor-ci`, `192 s` (`417 s` P90) `runner-host`, `238 s` `actions-cli`, `486 s` `maintenance-helpers` `[M]` | `≤ 60 s` `[T]` (`10 s` `[E]`) | `10 s` `[M]` is `velnor-ci` median alone. Surviving single tests run `924.8 s` (`receipt-integrity`), `679 s` (`non_inputs_differential`), `258 to 274 s` (`image_publication_public`), and `125 to 138 s` behind the loops `[M]`. | **Before T02, T05, and per-root enrolment: 138 to 925 s `[M]`. After all three: 8 to 15 s vs 45 to 90 s `[E]`.** |
| 9. Reporting and status | `36 to 43 s` for `1.48 GB` archive upload, `49 s` `Required verdict`, `3.5 min` P50 (`45.7 min` P90) gate queues `[M]` | `≤ 2 s` combined (`≤ 1 s` report, `≤ 1 s` status) `[T]` | **Estimated `[E]`.** Out-of-pod Ed25519 receipt signing (`V02`), site log append, fenced writer lease check, `POST /statuses/{sha}` (`120 ms` RTT `[M]`), and read-back (`L03`). | **0.4 to 1.0 s** (plus `35 ms` WAN RTT when executed in FI against DE home site) `[E]`. |
| 10. Merge queue and merge | `7.5 min` P50 green to enqueue (`0 of 40` auto-merge), `19 s` enqueue to group, `19.9 min` P50 (`84.1 min` P90) group run, `26 s` merge `[M]` | `≤ 30 s` (`≈ 25 s` `[E]`) queue entry and run, plus `26 s` merge `[M]` | `19 s + 26 s = 45 s` is measured GitHub queue latency `[M]`. That leaves `6 s` in the `51 s` total for group verification, which assumes an instant tree lookup (`60 to 75%` `[E]`, `16%` today `[M]`). | **60 to 75% hit path (only if C5 = 0): 47 to 52 s `[M+E]`. Today (11 C5 test files) or 25 to 40% miss path: 75 to 180 s `[E]`.** |
| **Total PR verdict and Push-to-merge** | **26.7 min P50 (92.3 P90) and 85 min P50 (457 P90) `[M, proxy]`** | **≤ 90 s P50 (≤ 4 min P90) and ≤ 3 min P50 (≤ 8 min P90) `[T]` (`≈ 35 s` and `≈ 85 s` `[E]`)** | **Every stage on the verdict path except `0.25 s` reflink and `19 s + 26 s` GitHub queue is `[E]` or `[T]`.** | **Pre-graduation (Days 5 to 10): 5.5 to 6.5 min and 9 to 12 min `[E]`. Post-graduation: 24 to 42 s (47% cohort) or 75 to 140 s (53% cohort), and 71 to 190 s to merge `[E]`.** |

Three structural gaps separate the design's `35 s` PR verdict estimate from the true floor:
1. **Only zero-crate compilation is measured on the cluster.** The `0.31 s` build in `S12` compiled zero crates on an unchanged commit `[M]`. Once a source file changes, `CARGO_INCREMENTAL=0` (`ruling Q17`) recompiles the modified crate in full and re-links its test executable, which takes `8 to 20 s` in `generator/` or `ci-helper/` `[E]` and `59 to 61 s` in a `157k`-line crate `[M]`, plus `dep-info` blob verification (`§7.3` step 5).
2. **Graduation blocks warm second witnesses.** Until 3,000 clean warm-against-cold comparisons accumulate per Cargo root (`§1.2` `C6`, `§7.3`), Witness 2 builds cold on every new key (`36 to 120 s` compile alone `[M]`), bounding PR verdicts to `5.5 to 6.5 min` P50 (`§1.4` `[E]`).
3. **GitHub's merge queue sets a 45 s post-green floor.** Even when a merge group executes zero obligations, GitHub takes `19 s` median to form the group and `26 s` median to merge it (`[M]`, `§3`).

## The strongest objections

### 1. Whole-tree test keys and C5 clock reads prevent per-root skipping and instant merge-group verdicts
- **Evidence.** Section 6.1 reports that 79 of 86 `.rs` files using `CARGO_MANIFEST_DIR` climb out of their package and 70 test files list directories `[M]`, concluding that *"most test keys stay whole-tree."*
  Section 6.1 also finds 11 test files that read the wall clock `[M]`, classifying their obligations as `C5`.
  Under section 1.2 (`C5`) and section 6.1, `C5` obligations run fresh on every request (exact-tree rebases and merge groups included), and an instant verdict requires *"zero C5 obligations and zero misses."*
  Consequently, a one-line edit in `generator/` invalidates every whole-tree test key in the repository, and zero merge groups can resolve by instant lookup because the `C5` obligation set is non-empty on every commit.
- **Likelihood.** **Certain (100%)** on the current codebase, until those 79 path-climbing files, 70 directory-listing tests, and 11 clock-reading tests are refactored.
- **Proof or refutation measurement.** Run the `V07` eBPF syscall tracer and `V12` `C5` scanner across all 30 packages on a one-line edit to `generator/crates/velnor-render/src/lib.rs` and on an identical-tree merge group.
  The design's `35 s` PR and `25 s` merge-group estimates hold only if `≥ 80%` of total test CPU-seconds enroll narrow keys and `0` blocking obligations remain in `C5`.

### 2. `CARGO_INCREMENTAL=0` creates a 45 to 65 s single-crate compile floor on large workspace crates
- **Evidence.** Ruling `Q17` and section 7.3 mandate `CARGO_INCREMENTAL=0` on every build that mints a receipt or ships bytes.
  Warm generations skip unchanged upstream crates (`0.31 s` when 0 crates compile `[M]`, `S12`), but Cargo with `CARGO_INCREMENTAL=0` compiles a modified crate from scratch as one compilation unit.
  The repository contains `velnor-actions-orchestrator` (`186.8k` LOC), `velnor-runner-host` (`156.8k` LOC), `velnor-actions-workflow-renderer` (`88.7k` LOC), `velnor-actions-contract` (`47.3k` LOC), and `velnor-ci` (`40.6k` LOC) (`evidence.md` `§C.5` `[M]`).
  A one-file edit to `velnor-runner-host` with `CARGO_INCREMENTAL=0` took `61 s` on Mac (`I073` `[M]`) and `59 s` on a 96-thread cluster worker (`L1p` in `mbx-cache-cluster.md` `[M]`), versus `10.9 s` with `CARGO_INCREMENTAL=1` `[M]`.
  Even after `velnor-runner-host` is deleted at `F01`, edits to `velnor-ci`, the new `20k`-line `velnor-exec`, or root crates cannot meet the `15 to 25 s` compile budget non-incrementally.
- **Likelihood.** **High (85%)** for any PR touching `velnor-ci`, `velnor-model`, `velnor-exec`, or the root workspace.
- **Proof or refutation measurement.** On an idle 96-thread worker (`velnor-190`), time `cargo nextest run --no-run --locked --offline` with `CARGO_INCREMENTAL=0` over a reflinked warm generation after a one-line edit to `velnor-ci`, `velnor-model`, and `velnor-actions-orchestrator`.
  The `≤ 25 s` compile goal fails if any of the three exceeds `25 s` median over 5 runs.

### 3. The cold second witness and 3,000-comparison graduation gate hold PR verdicts at 6 to 18 minutes
- **Evidence.** Section 1.2 (`C6`) and section 7.3 require every new reuse class to run in shadow for about 3,000 clean comparisons per obligation kind (and 3,000 warm-against-cold binary digest sets per Cargo root) before graduating.
  Until graduation, Witness 2 builds cold on every new key (`92 s` compile on the runner root `[M]`, `S12`), which is why Milestone `5.2` estimates `≈ 6 min` P50 and `18 min` P90 PR verdicts (`§1.4` `[E]`).
  At 2.44 merge-queue arrivals per hour (`~58` merges per day `[M]`, `V6`), organic traffic needs 25 to 50 days to reach 3,000 comparisons per root.
  Generating them via lab replays (`delivery-plan.md` `§4.2`, `3,000` cold builds per root) dumps hundreds of slot-hours onto a 5-node cluster already running at `92 to 95%` CPU (`evidence.md` `§B.2.1` `[M]`).
- **Likelihood.** **High (85%)** that either graduation slips well past Day 7.5 to 10 or cold replay batches contend with foreground PRs and `A1` audits.
- **Proof or refutation measurement.** Measure sustained cold-replay throughput (clean comparisons per hour) on the 5 workers under normal multi-org load (`tailrocks`, `ChainArgos`, `jackin`) with `cpu.idle` cgroups active, verifying that 3,000 comparisons per root finish within 72 hours while foreground slot wait stays `≤ 2 s` P90.

### 4. XFS reflink clones duplicate 14.29 GB of Linux page cache per active sandbox
- **Evidence.** Cluster experiment `S12` (`mbx-cache-cluster.md` `§E1d` `[M]`) proved that XFS reflink clones do not share page cache.
  Reading two clones of the `10.64 GiB` target consumed `14.29 GB` of resident memory each (`28.58 GB` total) and evicted the warm snapshot from `10.0 GB` to `4.9 GB`.
  Section 9 schedules four `24 GiB` build sandboxes per `125.5 GiB` worker (`109.5 GiB` available).
  When four sandboxes on one node read their cloned `target/` during linking, clippy, rustdoc, or test startup alongside `16.8 GiB` peak compiler RSS (`§B.2.2` `[M]`), memory demand exceeds `110 GiB`, forcing page-cache thrashing and disk re-reads.
- **Likelihood.** **High (80%)** whenever 3 to 4 build or link sandboxes run concurrently on one worker.
- **Proof or refutation measurement.** Run `Lab B2` with 1, 4, and 6 concurrent sandboxes on `velnor-190` over XFS reflink clones versus a read-only `overlayfs` lower layer.
  Refuted only if 4 concurrent reflink sandboxes show `< 15%` wall-time degradation versus 1 sandbox and `0` major page-fault stalls.

### 5. Velnor Native (`velnor-exec`) is not on the PR or merge-group verdict path, and its speed gain over `Runner.Worker` is unmeasured
- **Evidence.** Under sections 5.5 and 8.1, PRs and merge groups have no GitHub Actions run after Step 4.
  Obligations are plain `cargo` commands executed by `velnor-ci` inside a warm sandbox.
  The `20k`-line `velnor-exec` YAML, step, and expression interpreter and `10k`-line `velnor-native-actions` only execute release, image, scheduled, and post-push `audit wait` jobs.
  Furthermore, section 5.3 admits that the speed delta between `velnor-exec` and `velnor-official` (`Runner.Worker`) on the same warm sandbox, tool store, and generations is **unmeasured** (`[E]` `4 to 5 s` Worker setup plus `87 to 147 ms` per Node action `[M]`, `decisions.md` `§H1`).
- **Likelihood.** **Certain (100%)** that `velnor-exec` saves `0 s` on `ensure()` obligation runs, and **High (80%)** that it saves `< 5 s` per job over `velnor-official` when both share the node mirror, tool store, and L0 cache.
- **Proof or refutation measurement.** Run work item `O04` with 50 paired executions of each workflow job type on `velnor-exec` and `velnor-official` sharing the same K8s warm pool, tool store, and L0 cache.
  Measure the median and P90 wall-time difference per job type.

### 6. Surviving single-test floors (`924.8 s` and `679 s`) bound the critical path regardless of runner speed
- **Evidence.** Section 8.5 notes that deleting `velnor-runner-host` at `F01` removes `Journal::open`, but leaves behind the `924.8 s` `receipt-integrity` test, the `679 s` `non_inputs_differential.rs` test (`~45,000` `git` processes), the `258 to 274 s` `image_publication_public.rs` 40-case loop, and the `132 s` `actions-cli` test `[M]`.
  Group 23 finding `A15` (`decisions.md` line 883) rejected replacing `non_inputs_differential.rs` with `gix` until the legacy paths are deleted or a matcher is qualified on the complete corpus (`§8.5`).
  No runner can execute a `679 s` or `924.8 s` test in `60 s` without changing the test code.
- **Likelihood.** **Certain (100%)** for every root-workspace build and every whole-tree cache miss until `T02` and `T05` land and pass review.
- **Proof or refutation measurement.** Time `cargo nextest run --profile ci` on the root workspace in a 16-CPU sandbox before and after `T02` and `T05`, verifying that the slowest test binary finishes in `≤ 60 s` with zero dropped assertions.

### 7. Simultaneous twin witnesses risk correlated passes and add filter-batch overhead
- **Evidence.** Ruling `Q12` and section 1.2 run every new obligation key twice in parallel on two nodes before merge.
  Unlike today's PR and merge-group runs (separated by 20+ minutes and different base states), simultaneous twins execute at the exact same second under the same cluster load and generation `M_G`.
  In addition, because `cargo-nextest 0.9.151` has no shuffle flag, section 6.2 lists a binary's tests and invokes nextest repeatedly in seeded batches via filter expressions, adding unmeasured process and binary startup overhead to fast suites like `velnor-ci` (`781` tests in `10 s` `[M]`).
- **Likelihood.** **Medium-High (70%)** that load-driven or clock-driven flakes correlate across simultaneous twins (`P(both pass) > (1 - p)^2`), which under `Q12` forces the merge group to re-execute all changed obligations even on an identical tree.
- **Proof or refutation measurement.** Execute `Lab C2` (20 seeded flaky tests at `p = 0.05 to 0.20` plus the 10 `S1` historical flaky suites, simultaneous twins vs 30 min apart), and benchmark `velnor-ci`'s 781 tests as one nextest call versus `V03` seeded filter batches.

### 8. The DE and FI two-site split (35 ms WAN) and single-LV XFS layout on FI workers conflict with home-site services and RustFS disk rules
- **Evidence.** The cluster spans Germany (`velnor-240`, `velnor-105`, control-plane `velnor-237`) and Finland (`velnor-241`, `velnor-5`, `velnor-190`), separated by `35 ms` over public-internet WireGuard (`evidence.md` `§B.0`, `§B.5` `[M]`).
  Sections 6.5 and 7.2 pin the repository's fenced status writer and authoritative cache index to one home site, while FI holds 60% of worker capacity (3 of 5 workers).
  Furthermore, section 7.1 states that *"each worker gives RustFS one drive on its own device or partition, never the L0 filesystem"*, and section 7.5 proposes making each FI worker's RustFS drive *"a partition of that disk."*
  However, `evidence.md` `§B.1.4` shows that 100% of each FI worker's `3.84 TB` data NVMe is already allocated to one XFS logical volume (`/dev/mapper/data-rancher`, `3,838 GB` `[M]`), and Linux XFS filesystems cannot be shrunk in place.
- **Likelihood.** **Certain (100%)** that FI workers cannot yield a separate partition from `/dev/mapper/data-rancher` without wiping and rebuilding their XFS data volume, and **High (75%)** that cross-site calls add tail latency when DE's 2 workers saturate.
- **Proof or refutation measurement.** Inspect `vgs` and `lvs` on `velnor-241`, `velnor-5`, and `velnor-190` (confirming 0 free physical extents), and run `ST06` (node-loss `#9480`, backfill `#8768`, Linux `fsync`, and `8.7 GiB` snapshot transfer) across the `35 ms` DE-to-FI link.

### 9. Per-group release-bytes builds and GitHub US artifact uploads inflate merge-group and job tails
- **Evidence.** Until post-link commit stamping (`N79`) is proven, section 8.4 builds release-profile musl bytes (`opt-level = "s"`, `lto = "thin"`, `x86_64` and `aarch64` under `qemu`) in every merge group because `option_env!("VELNOR_COMMIT")` is compiled in (`ruling Q6`), adding `~8 runner-min` per landed tip (`§4` `[E]`).
  For the tools release PR, that musl LTO build and `qemu` smoke test are part of the merge group's blocking verdict (`§8.4`).
  Meanwhile, section 5.6 requires `actions/upload-artifact` (77 uses) to upload every artifact to GitHub's v4 service in the US (`120 ms` RTT, `13.6 to 22.4 MB/s` measured from pods `[M]`, `evidence.md` `§B.4.2`) so `gh run download` sees it.
- **Likelihood.** **Certain (100%)** that tool-release merge groups cannot finish in `25 s` while compiling thin-LTO musl binaries for two architectures, and that any multi-hundred-MB artifact upload takes `15 to 60 s` over the WAN.
- **Proof or refutation measurement.** Time the cold and warm merge-group build of `velnor-ci` and `velnor-gen` release-profile musl targets (`x86_64` and `aarch64` plus `qemu` smoke) on `velnor-190`, and measure `A02` upload latency for `100 MB` and `1.48 GB` artifacts before and after post-link commit stamping.

## Missed speed

Each change below preserves the exact C1 to C7 zero-false-green contract while removing a real latency floor that Velnor v3 leaves on the table.

1. **Replace GitHub's Merge Queue with a Velnor-owned compare-and-swap merge train (`N31`) to save `45 s` per merge at P50 (`19 s + 26 s` `[M]`) and minutes on queue ejections.**
   Velnor already computes `git merge-tree(queue_tip, head)`, verifies the exact resulting tree under ALLGREEN with two witnesses, and holds an exclusive fenced writer per repository (`§6.5`).
   Having the fenced writer create the squash commit with that exact verified tree ID, post `Velnor / Verified`, and advance `refs/heads/main` via GitHub's Git Database API (`PATCH /git/refs/heads/main` with `force: false` compare-and-swap on the expected parent SHA) lands a verified PR in **1 to 2 s** instead of waiting `19 s` for GitHub to emit `merge_group.checks_requested` and `26 s` for GitHub to merge (`45 s` median `[M]`, which is 53% of the design's `85 s` push-to-merge budget).
   On a queue failure, Velnor switches immediately to its pre-verified single-ejection speculative tree (`N19`) without waiting for GitHub queue teardown.

2. **Use direct Linux namespaces plus cgroup v2 slots (`I082`) over a read-only `overlayfs` lower layer (`Q16`) instead of single-use Kubernetes pods and XFS reflinks to save `0.5 to 1.2 s` sandbox startup and `~43 GiB` RAM per worker (`+50%` build slots).**
   Have `velnor-node` spawn obligations directly via `clone3(CLONE_NEWNS | CLONE_NEWPID | CLONE_NEWNET | CLONE_NEWIPC | CLONE_NEWUTS | CLONE_NEWCGROUP)` into pre-configured cgroup v2 slices (`< 25 ms` vs `300 to 1,000 ms` for K8s Pod creation `[E]`), mounting the sealed generation `G` as a kernel-enforced read-only `overlayfs` `lowerdir` with a per-sandbox XFS `upperdir`.
   Unlike XFS reflinks (`14.29 GB` duplicate page cache per read clone `[M]`, `S12`), every sandbox on the worker shares one `14.29 GB` page-cache copy of `target/`.
   That reclaims `~43 GB` of RAM across 4 sandboxes, allowing **6 build slots per worker instead of 4** (30 cluster slots vs 20) and eliminating page-cache eviction stalls during concurrent linking.

3. **Run Witness 1 with `CARGO_INCREMENTAL=1` and Witness 2 (plus `A1`) with `CARGO_INCREMENTAL=0`, and split the remaining `40k` to `187k` LOC crates to save `35 to 50 s` on single-file edits.**
   Section 7.3 bans incremental compilation (`Q17`) because warm incremental CGUs differ by digest from cold non-incremental builds and could hide a stale CGU bug.
   Because every new key already runs **two independent witnesses** (`§1.2`), running Witness 1 with `CARGO_INCREMENTAL=1` over an incremental generation and Witness 2 with `CARGO_INCREMENTAL=0` over a clean generation makes a false green impossible.
   Both witnesses must pass every expected test ID before the record turns green, and Witness 2 plus `A1` verify the clean non-incremental binary digests.
   Once `Lab B4` passes 300 historical diffs and 20 planted failing edits, and `velnor-actions-orchestrator` (`186.8k` LOC), `velnor-actions-workflow-renderer` (`88.7k` LOC), and `velnor-ci` (`40.6k` LOC) split into `10k`-LOC leaf crates, a one-file edit compile drops from **59 to 61 s down to 1.5 to 6.0 s** (`[M]`, `blaze.md` `B3` and `decisions.md` `I073`).

4. **Eliminate the full-tree `110 MiB` SHA-1 re-hash (`C10`) by verifying only the `overlayfs` `upperdir` delta against a pre-hashed read-only lower layer to save `0.4 to 0.8 s` per sandbox.**
   Hashing `110 MiB` of source files and walking `M_G` to set mtimes on every sandbox start (`§7.4` `[E]`) repeats work that is invariant for the sealed base tree.
   When the base tree is mounted as a read-only `overlayfs` lower layer with `M_G` mtimes already set once at generation seal time, checkout only writes, stamps, and hashes the delta files `Δ` in `upperdir` (typically `< 10` files) and verifies that `upperdir` contains no unexpected paths, cutting checkout verification to **`< 20 ms`** `[E]`.

5. **Refactor the 11 clock-reading test files (from `C5` to `C2`) and the 79 path-climbing test files (from whole-tree to per-package) to save `10 to 60 s` on PRs and unlock true `0 s` merge-group verification.**
   Injecting a key-bound deterministic clock (or `LD_PRELOAD` `clock_gettime` shim) into the 11 clock-reading test files moves them out of `C5` while nightly `A2` (`+8d` and `+30d` clock shift) continues to catch date bombs.
   Replacing `CARGO_MANIFEST_DIR/../../..` climbs and unscoped directory listings with explicit fixture lists in the 79 test files lets `V07` enroll narrow per-package contracts.
   Together, this raises instant merge-group tree lookups from **0% today to 60 to 75%** (saving `10 to 60 s` wall time and `12 to 38` runner-minutes per merge) and skips unaffected test binaries across roots.

6. **Add a deterministic `--order-seed` flag to the pinned `cargo-nextest` in the tool store instead of spawning filter-expression batches (`V03`) to save `2 to 8 s` per test witness.**
   Patching a 50-line seeded permutation into the tool store's digest-pinned `cargo-nextest 0.9.151` binary (bound into every key's definition digest, `§6.1`) runs all tests of a consolidated `tests/it` binary (`T06`) in one process invocation with a reproducible witness seed, avoiding repeated nextest startup and filter-expression parsing.

## What to measure first to earn the "fastest" claim

Before claiming "the fastest runner ever", run these six benchmarks on the real Hetzner workers and publish the raw distributions (median, P90, min and max, run count, error count, and named limiter).

| # | Benchmark | Method | Pass criteria to earn the claim |
|---|---|---|---|
| **1** | **Read-audit key narrowing and C5 share (`V07` + `V12`)** | Run the eBPF file-access tracer (`openat*`, `stat*`, `readlinkat`, `getdents64`, `ENOENT`) and `C5` classifier across all 30 packages over 100 replayed PR commits. | eBPF overhead is `≤ 3%` wall time. At least `70%` of test CPU-seconds enroll narrow per-root or per-package keys. Zero blocking `C5` obligations remain on an identical-tree merge group. |
| **2** | **Single-file edit compile floor on warm generations (`Lab B4`)** | On an idle 96-thread worker (`velnor-190`) over a warm generation `G` (including `dep-info` blob check and mtime stamping), run 5 alternating builds of a 1-line edit to `velnor-ci`, `velnor-model`, and `velnor-actions-orchestrator` under `CARGO_INCREMENTAL=0` and `1`, plus 20 planted older-mtime and build-script faults. | P50 compile plus test-link wall time is `≤ 15 s` (P90 `≤ 25 s`) for `generator/` and `ci-helper/` edits and `≤ 35 s` for root edits. All `20 of 20` planted faults are caught. Zero digest mismatches occur on unchanged units. |
| **3** | **Concurrent sandbox memory and page cache (`Lab B2`, reflink vs `overlayfs`)** | Run 1, 4, and 6 concurrent build sandboxes on one `125.5 GiB` worker, each building and linking a 1-file edit over XFS reflink clones vs a read-only `overlayfs` lower layer. Record `fincore` page cache, RSS, `pgmajfault`, and wall time. | At 4 concurrent sandboxes, P90 wall time degrades `≤ 25%` vs 1 sandbox, total node memory stays `≤ 109.5 GiB` with `0` OOM or watermark evictions, and binary digests match a cold build `100%`. |
| **4** | **Velnor Native vs GitHub Official on identical warm infrastructure (`O04`)** | Execute 50 interleaved pairs per job cohort through `velnor-exec` and `velnor-official` sharing the exact same warm sandbox pool, node git mirror, read-only tool store, and L0 cache tier. | Conformance parity is `100%` with `≥ 95%` mutant kill rate, and Velnor Native shows a statistically significant P50 and P90 wall-time win with the limiter attributed per stage. |
| **5** | **Twin-witness independence and batch-filter overhead (`Lab C2` + `V03`)** | Run 20 tests with seeded failure probability `p` in `0.05 to 0.20` and the 10 `S1` flaky suites as simultaneous twins on two nodes vs 30 min apart across 300 trials. Time `velnor-ci` (781 tests) in one invocation vs seeded batches. | Simultaneous twin joint pass rate is within `±20%` relative of `(1 - p)^2`. Seeded-order execution overhead is `≤ 1.5 s` per test binary. |
| **6** | **End-to-end matched-cohort production trial under load (`N40`, 300 merges)** | Measure true `push-to-merge` and `PR verdict` clocks across `≥ 300` merges (`≥ 200` matched heads across peak `92 to 95%` CPU and quiet hours, both DE and FI sites, all PR classes) with twins, `A1` cold audits, builder, and multi-org load active. | All-cohort PR verdict P50 is `≤ 90 s` (P90 `≤ 4 min`), push-to-merge P50 is `≤ 3 min` (P90 `≤ 8 min`), paired false-failure rate is `≤ 1%` over 300 pairs, runner-minutes per merged PR are `≤ 50`, and `A1` finds `0` false greens. |
