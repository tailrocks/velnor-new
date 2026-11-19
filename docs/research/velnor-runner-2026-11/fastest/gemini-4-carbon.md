# Is this the fastest runner? Critique by Gemini 4 Carbon

| Field | Value |
|---|---|
| Model | Gemini 4 Carbon |
| Model id | `gemini-4-carbon` |
| Client | Antigravity CLI 1.5.9 (agy -p) |
| Reasoning effort | high |
| Date / run | 2026-11-19 |
| Brief | fastest-runner critique (2026-11-19): the final design, research folder and evidence |

This is the model's own critique of the final design against the claim that Velnor is the fastest possible runner at the same confidence, unedited except for formatting.

---

# Critical review of the Velnor "fastest runner" claim

## Verdict

No. "The fastest runner ever" is unproven, and v3 is not the fastest zero-false-green runner for `velnor-new`.
Velnor will beat GitHub-hosted `ubuntu-26.04` [M, 57 min worst-day Free-plan gate queue, 44 to 61 s cache restores] and today's untuned ARC pods [M, 8.6 s ext4 copies, DinD in every pod, three 39-job runs per merge].
Yet PR speed comes from skipping Actions jobs via webhook ledger obligations and XFS reflinks, not from replacing the C# runner in Rust [M, Node adds 87 to 147 ms per step, idle ARC starts in 2 to 3 s, and Native minus Official is unmeasured].
Against Namespace or Blacksmith microVM runners and Buildkite stateful agents, Velnor leads on warm clones [M, 0.56 s], though Buildkite or tuned ARC running `ensure` over XFS and local mirrors lands within seconds of it.
Against Bazel or Buck2 with remote execution, Cargo roots, cross-tree test reads [M, 79 of 86 `CARGO_MANIFEST_DIR` files], and binary-wide C5 clock tainting lose on incremental verification, and even on Cargo v3 leaves 45 to 120 s on the table.

## Where the time goes, per stage

Section 3 of `velnor-runner-architecture.md` budgets only the 47% PR cohort that touches
`generator/` or `ci-helper/`. Almost every Goal figure is an unverified target `[T]` or arithmetic
estimate `[E]`. On the Hetzner cluster [M, five 96-thread EPYC 9454P workers with 125.5 GiB RAM and
XFS NVMe, 35 ms DE-to-FI RTT, 120 ms RTT to US GitHub], the true zero-false-green floor is lower
than v3 in merge and compile stages, while v3's own rules push several stages above its estimates.

| Stage | Today in v3 and basis | v3 Goal and estimate | Audit of v3's number | True floor at zero false greens |
|---|---|---|---|---|
| 1. Event, webhook, offer | `1 to 2 s` push to run, `2 s` P50 and `226 s` P90 to first job, `5.5 min` hosted `Plan` queue [M, §A T4, V5] | `1 to 2 s` webhook [E], `10 s` reconciler [T] | App webhook delivery to Hetzner is unmeasured (`evidence.md` has zero webhook timings, and nodes accept no webhooks today, N23). | `0.2 to 0.3 s` (one HTTPS push at 120 ms RTT), or `0 s` post-push when preflighted by `velnor verify`. |
| 2. Acquire | `13 s` P50, `27 s` P90, `<= 3 s` on idle ARC pod [M, n=1,124, V5] | `0 s` on verdict path, `<= 1 s` P50 and `<= 2 s` P90 on Actions jobs [T] | Zero holds on PRs because obligations skip Actions. Actions jobs need `acquirejob` plus first `renewjob` (N85) across 120 ms RTT. | `0 s` on App path, `0.3 to 0.5 s` (two US round trips) on Actions jobs. |
| 3. Sandbox start | `~6 s` ARC pod plus `3 to 4 s` DinD probe [M, 436 starts, 57 probe warnings per hour] | `<= 0.3 s` [T] (cites `0.25 s` reflink [M]) | Cites `cp --reflink` (`0.25 s` inside a running pod, P2) as full sandbox start. Kubernetes cannot attach new host bind mounts to a running unprivileged pod. | `0.01 to 0.03 s` via Linux namespaces and cgroups in `velnor-node`, `0.4 to 1.5 s` via per-job K8s pods. |
| 4. Checkout | `6 s` P50, `8 s` P90 per job (`110 MiB`, 31 to 39 jobs) [M, §A T10] | `<= 1 s` [T], incl. `0.1 to 0.3 s` full-tree hash [E] | Hashing thousands of tracked files is metadata and syscall bound, not `1 GB per second` sequential SHA-1 (`rustfs.md` R2 took `4.7 s` to clone 6,053 files under load). | `0.02 s` mount of a read-only `EROFS` or `overlayfs` `main` image plus `0.2 to 0.4 s` delta fetch and delta hash. |
| 5. Toolchain and tools | `10 s` P50, `18 s` P90 tools, `2 s` mise, `4.4 s` action tarballs [M, §A T9b] | `~0 s` [T] | Sound once the `EROFS` tool store is mounted read-only and native actions drop tarball downloads. | `< 0.01 s` (pre-mounted read-only `PATH`). |
| 6. Dependency fetch | `3.5 to 6.1 s` (`2.7 to 3.3 s` via `crates-mirror`, 0 of 48 cache hits) [M, §B.4.2] | `0 s` warm, `~3 s` lockfile delta [E] | Sound for unchanged lockfiles via pre-unpacked per-lockfile `CARGO_HOME`. | `0 s` unchanged, `0.5 to 2.0 s` for changed crates. |
| 7. Compile | `81 to 88 s` `runner-host`, `47 s` `orchestrator`, `14 s` `velnor-ci` [M, §A T10], `92 s` cold, `0.31 s` 0-crate warm, `67.4 s` 11-crate bump [M, S12] | `<= 25 s` [T], `15 s` [E] | `15 s` is an unmeasured small-root estimate after 3,000-run graduation. Before graduation, witness 2 builds cold (`92 to 180 s`). Keeps `CARGO_INCREMENTAL=0` and default `cc`. | `0.3 s` unchanged, `3 to 6 s` leaf edit with incremental or split crates plus `mold` or `wild` and unpacked split debuginfo. |
| 8. Tests | `velnor-ci` `10 s` P50 and `62 s` P90, `actions-cli` `238 s`, `runner-host` `192 s` and `417 s` P90 [M, §A T10], `924.8 s` and `679 s` suites [M] | `<= 60 s` [T], `10 s` [E] (stretch `<= 15 s`) | Assumes per-root keys hold and `ci-helper` (`test-threads=1`) parallelises to `10 s`. Omits the 53% non-helper cohort, C5 re-runs, and `679 s` `non_inputs_differential`. | `0 s` unaffected, `2 to 5 s` parallel `velnor-ci` and `velnor-gen`, `10 to 25 s` heavy suites with cluster fan-out and `opt-level=3` deps. |
| 9. Result reporting | `36 to 43 s` for `1.48 GB` archive upload, `49 s` `Required verdict` [M] | `<= 1 s` [T] | Sound for Ed25519 receipt signing and local site-ledger writes. Actions jobs still upload v4 artifacts to GitHub. | `< 0.05 s` to local site ledger. |
| 10. Required check | `1.6` and `24.3 min` verdict queue plus `1.9` and `21.4 min` `Required` queue [M, §A T4] | `<= 1 s` [T] | Fenced writer sends `POST /statuses/{sha}` then read-back `GET` (§6.5) over 120 ms RTT. | `0.3 to 0.5 s` (two sequential HTTPS calls over warm keep-alive). |
| 11. Merge queue | `7.5 min` green to enqueue (0 of 40 auto-merge), `19 s` to group, `19.9` and `84.1 min` group run [M] | `<= 30 s` [T], `~25 s` [E] | `19 s` group creation [M] plus `~6 s` lookup holds only on `60 to 75%` exact-tree hits with zero C5 obligations. Misses and C5 re-run twins (`+15 to 90 s`). | `19.5 s` median on GitHub Merge Queue, `0 s` with a Velnor CAS merge train (N31). |
| 12. Merge | `26 s` median [M] | `26 s` [M] | Fixed external tax kept by Decision 7 (GitHub Merge Queue). | `26 s` on GitHub Merge Queue, `0.3 s` atomic Git ref CAS in a Velnor merge train. |
| **End-to-end** | **Verdict `26.7` and `92.3 min`, Merge `85` and `457 min` [M, proxy]** | **Verdict `<= 90 s` and `<= 4 min` (`~35 s` [E]), Merge `<= 3` and `<= 8 min` (`~85 s` [E])** | **`~35 s` and `~85 s` hold only on graduated, zero-C5, 47%-cohort happy paths.** | **Verdict `1.0 s` exact-tree, `6 to 12 s` leaf edit. Merge `~52 s` on GitHub MQ, `7 to 15 s` on Velnor merge train.** |

For the excluded 53% of PRs (engine crates, `.velnor/` configs, lockfiles, and the new `executor/`
workspace), whole-tree compile plus tests starts at `6` to `18 min` [E, §1.4] and `~110` runner-min
until per-root keys and compile reuse both graduate. Even on the 47% cohort, GitHub's Merge Queue
alone (`19 s` plus `26 s` [M]) consumes 53% of v3's `~85 s` push-to-merge estimate.

## The strongest objections

### 1. Production `SystemTime::now()` calls plus T06 binary merging taint almost every obligation as C5

- **Evidence.** Contract C5 (§1.2) and §6.1 mark an obligation ("one test binary under one
  configuration", §1.2 C1) as C5 whenever its sources or workspace closure read the wall clock. C5
  work runs fresh with two witnesses on every PR push, rebase, and merge group, with no TTL. In
  `pr-worktree` today, `SystemTime::now()` appears in library crates across nearly every Cargo root,
  including `ci-helper/crates/velnor-ci/src/io/mod.rs:211` (plus five test files in
  `velnor-ci/tests/it/`), `local/crates/velnor-local/src/io/mod.rs:371`,
  `crates/velnor-actions-contract/src/workflow/group_ledger/emit.rs:261`,
  `crates/velnor-actions-orchestrator/src/config.rs:323`, and
  `crates/velnor-runner/crates/velnor-runner-github/src/registration/runners/absence.rs:345`. Item
  T06 (§8.5) merges 356 integration-test binaries into one `tests/it` binary per package, so a
  single clock read in a package or its dependency closure forces the entire package's tests into
  C5. Meanwhile, v3 uses a clock shim only in nightly Audit A2 (§6.5), never in production.
- **Likelihood.** High. Verified directly in repository source files.
- **Deciding measurement.** Run V12's C5 classifier across all 30 packages today and sum the test
  CPU marked C5. If C5 covers `velnor-ci` or more than 10% of test runner-minutes, Stage 11's `25 s`
  merge-group lookup and §4's `<= 50` runner-min goal fail without a production clock shim.

### 2. Root-climbing and directory-listing tests collapse per-root keys back to whole-tree keys

- **Evidence.** Hitting `<= 90 s` verdicts and `<= 50` runner-min requires per-root keys, whereas
  whole-tree keys cost `~110` runner-min and `6` to `18 min` verdicts (§1.4, §4). Section 6.1
  concedes that 79 of 86 files using `CARGO_MANIFEST_DIR` climb out of their package and 70 test
  files list directories. Even the 47% `generator/` and `ci-helper/` cohort climbs out of its own
  Cargo roots inside its monolithic `tests/it` binaries. Specifically,
  `ci-helper/crates/velnor-ci/tests/it/crates_mirror/fixture.rs:209` climbs to `../../..` and
  `io_report.rs:308` reads `../../../generator/samples`, while
  `generator/crates/velnor-gen/tests/it/setup_action.rs:42` reads `actions/setup-velnor/action.yml`
  and `golden.rs:260` reads `../../../.velnor/config.toml`. Furthermore, every per-root contract
  includes root `Cargo.*`, `mise.*`, `.github/`, and `.velnor/` (§6.4), so any config, workflow, or
  lockfile edit invalidates all roots at once.
- **Likelihood.** High.
- **Deciding measurement.** Trace one full test run of all five Cargo roots under
  `strace -f -e trace=%file` (counting `ENOENT` and `getdents64`) and replay 200 merged PR diffs
  against the resulting contracts to measure what fraction of PRs skip untouched roots without
  source refactoring.

### 3. The speedup comes from bypassing GitHub Actions, while the native Rust runner's own gain is unmeasured

- **Evidence.** On PRs and merge groups, v3 eliminates Actions jobs and runs `velnor-ci` directly
  from webhooks (§8.1). Where Velnor actually executes Actions jobs, `decisions.md` (H1, §b, §c)
  shows that Node action startup costs `87 to 147 ms` per step [M], ARC scale-set queue wait is
  `0.2 min` median [M, §A T4], idle ARC pods start in `2 to 3 s` [M], Native minus Official speed
  is unmeasured (§5.3), and `velnor-legacy` built `804k` lines of native Rust runner with zero
  accepted speedup (`evidence.md` §E.7). Calling Velnor "the fastest Actions runner" credits the
  Rust step runner for gains that come from skipping Actions and mounting local XFS state.
- **Likelihood.** Certain. Stated directly in §5.3 and `decisions.md` H1.
- **Deciding measurement.** Execute O04's paired benchmark comparing `velnor-exec` (Native) against
  `velnor-official` (`Runner.Worker` v2.338.2) on the same Kubernetes backend, XFS generation
  clone, node Git mirror, and `EROFS` tool store.

### 4. Unshared XFS reflink page cache (`14.29 GB` per read clone) caps slot density and risks NVMe stalls

- **Evidence.** Cluster experiment `mbx-cache-cluster.md` (E1d) proved that XFS reflink clones do
  not share the Linux page cache. Reading two clones consumed `2 x 14.29 GB` of RAM and evicted the
  warm base snapshot from `10.0 GB` down to `4.9 GB`. That forces a `24 GiB` budget per sandbox and
  caps each 96-thread, `125.5 GiB` worker at four build slots (§9). Those 20 cluster slots must
  serve three organisations (`tailrocks`, `ChainArgos`, `jackin`, 160 max runners today, §B.3.2),
  twin witnesses, cold A1 audits, batch product builds, and queue speculation. When real compiles
  and links read rlibs (unlike P2's metadata-only `0`-crate check), page-cache eviction degrades
  warm builds into `10.6 GiB` NVMe reads.
- **Likelihood.** Medium to high under burst load (`92 to 95%` worker CPU observed on 2026-11-18,
  §B.2.1).
- **Deciding measurement.** Run Lab B2 on one worker with four and six concurrent sandboxes
  performing 1-file edits, links, and nextest runs beside one cold A1 build, comparing XFS reflink
  against read-only `overlayfs` lower layers on major page faults, NVMe read MiB per second, and
  P90 build time.

### 5. Simultaneous twin witnesses share cluster load, risking either doubled merge-group runs or doubled false reds

- **Evidence.** Ruling Q12 replaces two time-separated runs (PR and merge group, hours apart) with
  two simultaneous witnesses on two nodes. Today's `8.1%` false-failure rate on already-green SHAs
  (`S1`, 10 of 124 push runs) aligns with cluster-wide load spikes (§A T6, T8). If simultaneous
  twins correlate under shared site load and violate `(1 - p)^2` in Lab C2, §1.2's mandatory
  fallback forces every merge group to re-run changed obligations even on an identical tree, wiping
  out the `60 to 75%` instant merge-group lookup. Conversely, independent twins with zero retries
  nearly double the per-PR false-red rate from `f` to `2f - f^2`.
- **Likelihood.** Medium.
- **Deciding measurement.** Run Lab C2 at `>= 70%` cluster CPU load using both seeded flaky tests
  and the contention-sensitive suites from `S1`, comparing simultaneous twin discordance against
  runs spaced 30 minutes apart.

### 6. Cold second witnesses during 3,000-run graduation and `CARGO_INCREMENTAL=0` keep non-leaf Rust edits slow

- **Evidence.** Until each Cargo root records 3,000 clean warm-versus-cold digest comparisons
  (§7.3), every changed obligation's second witness builds cold (`92 s` on the runner root [M],
  `~3 min` estimated on the largest root, `delivery-plan.md` line 698), and any toolchain or profile
  change resets the generation key. Even after graduation, `CARGO_INCREMENTAL=0` recompiles whole
  crates (`186.8k`-line `velnor-actions-orchestrator`, `88.7k`-line `workflow-renderer`,
  `40.6k`-line `velnor-ci`, `20k`-line `velnor-exec`) on any 1-line internal edit. In `blaze.md` B3,
  `CARGO_INCREMENTAL=0` was `4.4 to 6.1x` slower than `CARGO_INCREMENTAL=1` (`9.2 s` vs `1.5 s` and
  `12.2 s` vs `2.8 s`).
- **Likelihood.** High for the 53% of PRs outside small leaf crates and after every toolchain bump.
- **Deciding measurement.** Benchmark 1-line internal edits across the five largest surviving
  crates on an EPYC worker at 16 CPUs with `CARGO_INCREMENTAL=0` versus `1` across 300 historical
  diffs (Lab B4).

### 7. Headline budgets omit 53% of PRs, misplace `receipt_integrity.rs`, and underprice `ALLGREEN` queue ejections

- **Evidence.** Section 3's `~35 s` verdict and `~85 s` push-to-merge estimates cover only the 47%
  `generator/` and `ci-helper/` class, leaving out root engine crates, lockfile bumps (`67.4 s`
  compile for `fastrand`, L1p), shared config changes, and Docker-class suites. Section 8.5 and
  `delivery-plan.md` T02 also claim the `924.8 s` receipt-integrity test survives
  `velnor-runner-host` deletion while forbidding `velnor-runner-host` work, yet that test sits
  inside `velnor-runner-host`
  (`crates/velnor-runner/crates/velnor-runner-host/src/host_controller/loop/polling/restart_tests/bootstrap/receipt_integrity.rs`).
  Meanwhile, `non_inputs_differential.rs` (`679 s` in `velnor-actions-orchestrator`) stays whole on
  the merge gate until D14. Under `ALLGREEN` at depth 2 to 4, one bug or 1 to 3% false red ejects
  the failing entry and rebuilds every queued entry behind it.
- **Likelihood.** Certain for cohort skew and the `receipt_integrity.rs` error, medium to high for
  P90 queue stalls under burst arrivals (Ruling V6).
- **Deciding measurement.** Replay 300 historical PRs across all PR classes with burst arrivals and
  1 to 3% false-red ejections, reporting unweighted global P50 and P90 push-to-merge.

### 8. Per-request Kubernetes pods cannot attach dynamic mounts in `<= 0.3 s`, and RustFS L1 `strict` mode is unmeasured on Linux

- **Evidence.** Stage 3's `<= 0.3 s` sandbox budget cites only a `0.25 s` XFS file clone inside a
  live pod, omitting `Platform::provision` and `attach` (§5.4). Kubernetes cannot inject fresh host
  bind mounts into an already-running unprivileged container without pre-mounting a shared host
  parent directory. Meanwhile, L1 defaults every RustFS 1.0.6 bucket to `strict` fsync durability
  (§7.1), which ran at `8 obj/s` and `43.9 MiB/s` on macOS (`rustfs.md` R1), has zero Linux NVMe
  measurements, and carries open node-return (`#9480`) and backfill-stall (`#8768`) defects.
- **Likelihood.** High for K8s pod mount attachment exceeding `0.3 s`, medium for RustFS L1 write
  tail stalls.
- **Deciding measurement.** Time 200 full `provision + attach + spawn + teardown` cycles on k3s at
  80% CPU load (P04), and run ST06's Linux XFS `strict` versus `relaxed` fsync test before enabling
  L1.

## Missed speed

Every change below keeps contracts C1 through C7 (zero false greens) intact on the existing cluster.

1. **Velnor-owned CAS merge train (promoting `N31` over Decision 7).** Saves `~45 s` per merge at P50 (`19 s` group
   creation plus `26 s` merge [M]) and `1 to 5 min` during queue bursts. Verify `merge-tree(queue_prefix, head)` with
   two witnesses and ALLGREEN per-commit receipts, post `Velnor / Verified`, and advance `refs/heads/main` by atomic
   GitHub ref compare-and-swap under a ruleset restricting fast-forwards to the `velnor-verify` App. Disjoint root
   changes can also bypass a failed predecessor without serial queue rebuilds.
2. **Production deterministic clock shim (`vDSO` or `LD_PRELOAD`) plus isolating clock tests from `tests/it`.** Saves
   `15 to 90 s` on rebases and `60 to 75%` of merge groups, plus `10 to 25` runner-min per PR [E]. Fix `CLOCK_REALTIME`
   to a timestamp recorded in the obligation key (explicitly allowed by §1.2 C5 while keeping `CLOCK_MONOTONIC` real
   for timeouts and keeping nightly Audit A2 at `+8d` and `+30d`). That removes `velnor-ci`, `velnor-local`, and
   `velnor-actions-*` from C5 so identical trees hit in `< 1 s`.
3. **Read-only `overlayfs` or `EROFS` lower layers for generations and sealed `main` checkouts.** Reclaims `42 to 70 GB`
   of duplicate page cache per worker [M, `14.29 GB` per clone in E1d] and saves `0.2 to 0.4 s` per checkout plus `5 to
   25 s` of cold-read stalls [E]. Kernel-enforced immutable lower layers share one page-cache footprint across 6 to 8
   slots per worker, and mounting a sealed `main` image with only PR deltas in `upperdir` avoids hashing `110 MiB` of
   unchanged source files before step 1.
4. **Direct Linux namespace and cgroup v2 slots in `velnor-node` (`I082`) instead of per-job Kubernetes pods.** Cuts
   sandbox provision and mount attach from `0.5 to 2.0 s` down to `10 to 30 ms` [E]. `velnor-node` spawns disposable
   mount, PID, user, IPC, and loopback net namespaces with `cgroup.kill` teardown proofs per obligation, attaching
   per-obligation views directly without `kube-apiserver` round trips.
5. **Fast linker (`mold` or `wild`), `split-debuginfo = "unpacked"`, immediate dependency `opt-level = 3` (Ruling Q7),
   and intra-site test shard fan-out (`N91`).** Cuts warm edit, compile, link, and test wall time by `15 to 60 s` [M
   and E]. Today the repo configures no linker or debuginfo flags (`evidence.md` §C.7), writing `8.7 GiB` on an
   11-crate bump (L1p). `mold` or `wild` with unpacked split debuginfo avoids copying multi-hundred-MB DWARF sections
   during link (`-24%` size in `#585`), dependency `opt-level = 3` with assertions on already cut test CPU by `67%`
   (`23.3 s` to `7.7 s`, Q7), and sharding slow `tests/it` binaries across idle node slots eliminates single-node tails.
6. **Day-1 post-link ELF commit stamping (`N79`) plus agent pre-push `velnor verify` (`N07`).** Saves `~8` runner-min
   per merge (`410 to 534 s` product build [M]) and drops post-label PR verdict wait from `~35 s` to `1 to 3 s` [E].
   Patching a fixed ELF `.note` section before smoke tests and Sigstore attestation builds release bytes once per code
   closure instead of once per merge tip, while agent preflight populates candidate receipts before the PR is labelled.

## What to measure first to earn the "fastest" claim

Run every benchmark on the Hetzner EPYC 9454P workers with exit-code and expected-test-ID verification enabled,
reporting run count, P50, P90, range, and the measured limiter.

1. **Enforced key and C5 survival replay (200 historical PRs across all cohorts).** Trace all five Cargo roots under
   eBPF (`openat2`, `statx`, `getdents64`, `ENOENT`) and run V12's C5 scan. **Pass criterion.** At least `70%` of PRs
   skip untouched roots without root-climbing or `SystemTime::now()` C5 fallback, and at least `60%` of merge groups
   execute zero obligations.
2. **Paired executor isolation benchmark (20 interleaved three-way runs per cohort at 15% and 80% node CPU).** Compare
   `velnor-exec` (Native), `velnor-official` (`Runner.Worker` v2.338.2), and a minimal `ensure` command runner over
   identical XFS generations, Git mirrors, and `EROFS` tools. **Pass criterion.** Native beats Official by at least
   `5 s` P50 and `10 s` P90 per job, with dispatch-to-step-1 latency `<= 0.5 s` P50 and `<= 1.0 s` P90.
3. **Six-sandbox concurrency bakeoff (5 interleaved rounds on one 125.5 GiB worker).** Run six concurrent single-file
   edit, compile, link, and test sandboxes beside one cold A1 build, comparing K8s pods plus XFS reflink against
   `velnor-node` namespaces plus read-only `overlayfs`. **Pass criterion.** Full `provision + attach + exec` takes
   `<= 0.3 s` P50 and `<= 0.6 s` P90, with zero OOM or evictions and `<= 25%` P90 build slowdown versus an idle node.
4. **Under-load twin independence and flake trial (300 paired runs at `>= 70%` cluster CPU plus 20 seeded flaky
   tests).** Compare simultaneous two-node twins against runs spaced 30 minutes apart. **Pass criterion.** Joint pass
   rates match `(1 - p)^2` without triggering the merge-group re-execution fallback, and per-PR two-witness false reds
   stay `<= 2%` at 95% confidence.
5. **Compiler, linker, and incremental soundness matrix (5 interleaved reps per cell plus 300 historical diffs and 20
   planted faults).** Test `rust-lld`, `mold`, and `wild` crossed with unpacked split debuginfo, `CARGO_INCREMENTAL`
   `0` and `1`, and dependency `opt-level` `0` and `3` on `velnor-actions-orchestrator`, `velnor-ci`, and `executor/`.
   **Pass criterion.** Zero false greens on planted faults, and all-root warm edit-compile-test finishes in `<= 25 s`
   P50 and `<= 60 s` P90.
6. **All-cohort push-to-merge shadow and external runner bakeoff (300 merges plus 20-commit bakeoff).** Measure
   unweighted global clocks across 100% of PRs (ejections and all runner-minutes included), and run a 20-commit bakeoff
   against GitHub-hosted `ubuntu-26.04`, a Namespace-class runner, and a Buck2 remote-execution prototype on
   `generator/` and `ci-helper/`. **Pass criterion.** Global verdict `<= 60 s` P50 and `<= 180 s` P90, push-to-merge
   `<= 120 s` P50 (`<= 60 s` with Velnor merge train), zero A1 cold mismatches, and fastest P50 and P90 wall time on
   identical commits.
