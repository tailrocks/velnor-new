# Rust-only redesign by Gemini 4 Carbon

| Field | Value |
|---|---|
| Model | Gemini 4 Carbon |
| Model id | `gemini-4-carbon` |
| Client | Antigravity CLI 1.5.9 (agy -p) |
| Reasoning effort | high (model maximum; max rejected) |
| Date / run | 2026-11-19 |
| Brief | Rust-only redesign brief v1 (2026-11-19): design v2, all earlier rounds, the mbx and blaze experiments, and twelve binding owner decisions |

This is the model's own redesign after the owner's Rust-only decision, unedited except for formatting. It answers the Rust-only brief: a native Rust executor, the official runner only as a conformance oracle, latest APIs only, no Node, build once and reuse everywhere, correctness first, and CI started by a PR label.

---

# Velnor runner: Rust-only architecture

## 1. Summary

Velnor replaces GitHub's C# runner, ARC, Node actions, and per-push full runs with a pure-Rust control and execution plane (`velnor-hooks`, `velnor-dispatch`, `velnor-ledger`, `velnor-node`, `velnor-exec`, `velnor-native-actions`, `velnor-store`) speaking only GitHub's latest scale-set, Run Service, Results Twirp, Cache v2, Artifacts v4, and OIDC APIs.
PR verification is strictly on-demand via the `velnor:verify` label; unlabelled PRs execute zero jobs while a dual required-check ruleset (`Required` on GitHub Actions `15368` plus `velnor/verdict` on the Velnor App) blocks unverified heads without the GitHub `conclusion: skipped` false-green hole.
When `velnor:verify` is applied or updated (`synchronize`), `velnor-hooks` starts `ensure(tree)` within 1.2 s inside pre-warmed Linux namespace and cgroup v2 sandboxes mounted over multi-lower `overlayfs` snapshots of `main` on XFS, overlapping GitHub's 8 s job-offer delay.
Every unit of work executes at most once per content-addressed input closure across PR pushes, concurrent or stacked PRs, `merge_group` (with `2k - 1` queue-tip and bypass speculation), `main`, and `release-plz` promotion; shipped `--release` binaries build once in `merge_group` in parallel with `--profile ci` (`opt-level = 2` dependencies with debug assertions on) tests and promote by digest without rebuilds.
GitHub's latest official runner runs only in `velnor-lab` as a continuous differential conformance oracle (`100%` pass gate before cutover and within 6 hours of every upstream runner release).

| Metric | Today [M] | Committed (Phase 3) | Stretch (Phase 4) |
|---|---|---|---|
| PR verdict (P50 / P90) | 26.7 min / 92.3 min | <= 25 s / <= 55 s (calc: 17 s / 47 s) | <= 12 s / <= 25 s (calc: 9 s / 22 s) |
| Push-to-merge (P50 / P90) | 85 min / 457 min | <= 75 s / <= 150 s (calc: 72 s / 143 s) | <= 60 s / <= 105 s (calc: 58 s / 99 s) |
| Runner-minutes / merged PR | 347 min [EST] | <= 8 min | <= 3.5 min |

## 2. The latency budget

The trace below follows one typical `velnor-new` pull request from `git push` (with `velnor:verify` present, or from
adding `velnor:verify` on the final head) to merged on `main`. P50 represents the median PR (47% touch only
`generator/` or `ci-helper/` [EV: `v2/velnor-runner-architecture.md:698`]); P90 represents a `crates/velnor-runner` or
multi-root PR with a `Cargo.lock` or `merge_group` miss. Tags mark `[M]` measured in the repo or cluster experiments,
`[EV]` cited from `evidence.md` or `v2`, and `[EST]` calculated with explicit arithmetic.

| Stage | Today P50 / P90 | Design Committed P50 / P90 | Design Stretch P50 / P90 | Bottleneck today and removal mechanism |
|---|---|---|---|---|
| 1. GitHub event, webhook, job offer | 2 s / 226 s (max 2,203 s) [EV]; job offer 8 s / 15 s [EV] | Webhook: 1.2 s / 2.5 s [EV] (job offer 8 s / 15 s overlapped) | 0.0 s post-push (`velnor verify` pre-push); job offer 8 s / 15 s overlapped | Superseded hosted `always()` `Required` jobs stall new runs up to 2,203 s (`F-H06`). Removed by `velnor-hooks` starting `ensure` on the App webhook in 1.2 s and cluster-only `Required` cancelling in <0.5 s. |
| 2. Acquire (`acquirejob`) | Hosted gates: 246 s / 1,416 s per gate (3,420 s sum on 11-18); cluster: 12 s / 18 s [EV] | 0.18 s / 0.35 s [EST] (overlapped with stages 3 to 8) | 0.18 s / 0.35 s [EST] (overlapped) | 20-slot Free hosted pool serialises `Plan`, `Required verdict`, and `Required` (`evidence.md:166,195`). Removed by moving all gates to cluster `velnor-dispatch` (1 RTT = 120 ms [EV] + 60 ms journal `fsync`) with reserved merge/gate cgroups. |
| 3. Sandbox start | 12 to 14 s cold ARC pod; 2 to 3 s warm `minRunners` [EV] | 0.03 s / 0.08 s [EST] | 0.02 s / 0.05 s [EST] | K8s pod schedule (~6 s) + `init-dind-externals` (1.5 s) + `dind` probe (3.5 s) + `.NET` startup (`v2/decisions.md:152,272`). Removed by pre-warmed Linux user/mount/PID/net namespaces + cgroup v2 (`15 ms`) + `overlayfs` mount (`5 ms`) + seccomp (`10 ms`), or `80 ms` Firecracker snapshot restore for Docker suites. |
| 4. Checkout | 6.0 s / 8.0 s per job x up to 39 jobs [EV] | 0.30 s / 0.60 s [EST] | 0.05 s / 0.20 s [EST] | Action tarball download (4.4 s) + WAN clone per job (`evidence.md:244`). Removed by node L1 bare Git mirror (`150 ms` 1-RTT delta fetch on webhook; `0 ms` if pre-pushed) + local sparse tree materialisation (`150 ms`; compare `0.25 s` measured XFS clone, `mbx-cache-cluster.md:34`). |
| 5. Toolchain and tools | 14.0 s / 26.0 s per job (`0/58` cache hits) [EV] | 0.01 s / 0.02 s [EST] | 0.01 s / 0.02 s [EST] | Per-job `mise` + `rustup` setup into ephemeral `runner.temp` (`evidence.md:335-350`). Removed by read-only bind mounts of pre-provisioned `/store/toolchains/<sha256>` and `/store/mise/<sha256>` (`10 ms`, 0 network bytes). |
| 6. Dependency fetch | 4.5 s / 6.1 s (`0/48` scale-set cache hits) [EV] | 0.02 s / 0.80 s [EST] | 0.01 s / 0.30 s [EST] | `RUNNER_TEMP` path mismatch broke `@actions/cache` version hash (`proposal.md:45`), forcing 59 MB WAN fetch (`evidence.md:578`). Removed by read-only L1 pre-extracted crate store (`--offline` on the 89.5% of PRs keeping `Cargo.lock`; `<0.8 s` via node `crates-mirror` on lockfile edits). |
| 7. Compile | 81 s / 126 s (`runner-host` 81/116 s + Clippy 70/91 s; `maintenance-helpers` 486 s) [EV] | 8.5 s / 24.0 s [EST] | 0.30 s / 8.0 s [EST] | 0% cross-job compile reuse, `--jobs 2` cap, 157k-LOC `velnor-runner-host`, 356 test binaries (`evidence.md:241-306`). Removed by `overlayfs` over warm `main` (`0.31 s` measured unchanged build, `mbx-cache-cluster.md:34`), 6-way `velnor-runner-host` split, 1 integration binary per crate, parallel `.rmeta` lane, and `velnor verify` pre-push CAS in Stretch. |
| 8. Tests | 192 s / 497 s (`actions-cli` 238/284 s, `local-mode` 246/497 s) [EV] | 6.0 s / 18.0 s [EST] | 3.0 s / 9.0 s [EST] | 924.8 s `receipt-integrity` test, 40x/12x/6x subprocess loops, 93 `Journal::open` migrations, 6,425-glob git loop, `opt-level = 0` deps (`v2:648-650`). Removed by SQLite journal template (`<50 ms`), 1-test-per-case split, `[profile.ci.package."*"] opt-level = 2` (`-66%` test CPU [M]), 24-thread execution (3x on touched tests), and 480-thread site sharding in Stretch. |
| 9. Result reporting | 36 s / 65 s (artifact upload + `Required verdict` 42 s) [EV] | 0.40 s / 0.80 s [EST] | 0.30 s / 0.60 s [EST] | Multi-job artifact handoff (`velnor-crate-*` -> `Required verdict`). Removed by Ed25519 receipt append (`20 ms`) + parallel Results Twirp final log/timeline flush, Run Service `completejob` (`250 ms`, 2 RTTs), and App `velnor/verdict` check update (`130 ms`). |
| 10. The required check | 6 s run + 114 s / 1,284 s hosted queue [EV] | 0.10 s / 0.20 s [EST] | 0.10 s / 0.20 s [EST] | Separate hosted `required-gate` job (`ci.yml:614`). Removed because the single cluster `Required` witness job IS the check that called `completejob` in stage 9; GitHub ruleset evaluates within ~100 to 200 ms. |
| **PR Verdict Subtotal** | **26.7 min / 92.3 min** [EV] (10.9 min unqueued [EV]) | **16.6 s / 47.0 s** [EST] (**commit <= 25 s / <= 55 s**) | **8.6 s / 22.0 s** [EST] (**stretch <= 12 s / <= 25 s**) | Critical path is `max(webhook + stages 3..8, webhook_to_job_offer + acquire) + stages 9..10`. In Stretch P50, `8.0 s + 0.18 s + 0.40 s = 8.58 s` (GitHub job offer bounds pre-pushed work). |
| 11. Merge-queue entry & `merge_group` run | 450 s / 5,280 s manual wait + 19 s + 1,194 s / 5,046 s run [EV] | 29.5 s / 56.3 s [EST] (`2 s` auto-merge + `19 s` MQ + `8.5 s` receipt hit; P90 `3+30+23.3 s`) | 28.0 s / 42.0 s [EST] (`2 s` + `19 s` + `7.0 s` receipt hit; P90 sharded delta `9 s`) | `0/40` auto-merge (`7.5 min` wait) + full cold 39-job `merge_group` (`19.9 min` P50). Removed by mandatory auto-merge, 89%+ exact-tree receipt reuse at `<1 min` window (`v2:719`), and `2k - 1` queue-tip/bypass speculation pre-running during GitHub's `19 s` group creation. |
| 12. The merge | 26 s / 45 s [EV] (+ 19.4 min duplicate `main` `push` run after merge [EV]) | 26.0 s / 40.0 s [EV] (`main` `push` verifies receipts in `<1 s`, 0 builds) | 21.0 s / 35.0 s [EST] (`min_entries_to_merge: 1`, zero post-merge stall) | GitHub's internal merge-queue ref update takes `26 s` P50 (`v2/design-feedback-comparison.md:90`); kept per Decision 7 and 12. Post-merge `main` push promotes artifacts in `<1 s` instead of running 62.0 runner-min. |
| **Push-to-Merge Total** | **85 min / 457 min** [EV] (400 min P50 on 11-18 [EV]) | **72.1 s / 143.3 s** [EST] (**commit <= 75 s / <= 150 s**) | **57.6 s / 99.0 s** [EST] (**stretch <= 60 s / <= 105 s**) | **68x faster P50 and 183x faster P90 (Committed)** at identical or higher verification confidence. |

## 2b. Duplicate-work audit

Audit across all 16 `.github/workflows/*.yml` files (`ci.yml`, `state-seal.yml`, `cache-prune.yml`, `freshness.yml`,
`homebrew-adoption.yml`, `job-image-candidate.yml`, `product-release*.yml`, `release-rust-ci-tools.yml`,
`qualification.yml`, `source-preview-*.yml`, `source-qualification.yml`), generated `rust-ci.yml` and `release.yml`
(`generator/crates/velnor-render/src/{steps,release,release_binaries}.rs`), `ci-helper/`, and PR lifecycle dimensions.

### Profile tension resolution (explicit, with numbers)

Today CI compiles and tests with `[profile.dev]` / `[profile.test]` (`opt-level = 0` on all dependencies except `sha2`
and `turso_core`, `Cargo.toml:69-73`, `crates/velnor-runner/Cargo.toml:74-75`), producing a `10.64 GiB` `target/`
(`mbx-cache-cluster.md:33`) and slow unoptimised dependency execution in tests, then rebuilds shipped binaries from
scratch in `--release` (`opt-level = "s"`, `lto = "thin"`, `strip = true`, `panic = "abort"`,
`generator/Cargo.toml:35-39`) during release workflows. Testing everything in `--release` is rejected:
`panic = "abort"` breaks unwind-based test harnesses and `#[should_panic]`, `debug-assertions = false` and
`overflow-checks = false` weaken confidence (Decision 11), and `lto = "thin"` across 356 integration test binaries
(`design-feedback.md:122`) multiplies link cost on unshipped test harnesses. Rebuilding `--release` at tag time is
also rejected: it violates Decision 10 and ships binaries whose optimised codegen never ran qualification before
tagging. **Chosen design:**
1. **All CI checks and tests use `--profile ci`:** workspace crates compile at `opt-level = 0`,
   `debug = "line-tables-only"`, `debug-assertions = true`, `overflow-checks = true`, `incremental = false` (`-24%`
   cache size, `-20% to -27%` compile CPU `[EV: v2:787]`); external dependencies (`[profile.ci.package."*"]`) compile
   once per `Cargo.lock` into the warm `main` `overlayfs` lowerdir at `opt-level = 2` with `debug-assertions = true`
   and `overflow-checks = true`, cutting integration-test CPU by **66%** (`142.5 -> 47.9 CPU-s`
   `[M: design-feedback.md:115]`) with zero loss of assertion coverage.
2. **Shippable release binaries (`velnor-ci`, `velnor-gen`, `velnor-local`, `velnor-host`, `velnor-actions` for
   `linux-x64` and `linux-arm64`) build once in `merge_group`:** whenever a shippable binary's package closure changes
   (including a `release-plz` `Cargo.toml` version-bump PR), `merge_group` runs a parallel
   `release-artifact:<pkg>:<target>` obligation over a warm `main` `--release` dependency lowerdir (`8 to 14 s` on 24
   threads; compare `56 s` cold `mbx-cache` release build `[M: mbx-cache-cluster.md:11]`), smoke-tests the exact
   `--release` binary, packs `<bin>-<target>.tar.gz`, records its `blake3`/`sha256` digest in `velnor-store` and a
   signed receipt, and adds `0 s` to `merge_group` wall time because it runs alongside the `6 to 18 s` test lane.
   `main` and `release` promote that exact tarball and never invoke `rustc`.

### Operations table across all stages and PR dimensions

| Operation | Stages running it today | Same input? | Today's cost (runner-min, cache GB) | Decision (with numbers) | Mechanism |
|---|---|---|---|---|---|
| Intermediate PR pushes (draft or unlabelled) & `ready_for_review` | Every push (`45.2%` of 2,558 PR runs cancelled `[EV: evidence.md:45]`) | No (unfinished WIP) | ~95 runner-min / merged PR wasted | **Eliminate (`0 min`).** Run nothing until `velnor:verify` is applied (Decision 12). | App webhook filter + dual ruleset (`velnor/verdict` stays `queued`/pending). |
| No-code-change reruns (`pull_request.edited`, branch retarget with identical merge tree) | `ci.yml:4` (`469 / 2,558 = 18.3%` of PR runs `[EV: F-H07]`) | Yes | ~32 runner-min / merged PR | **Reuse (`<0.1 s`, `0.01 min`).** Drop `edited` trigger; identical tree hits existing receipts. | `ensure(merge_tree_oid)` ledger lookup (`velnor-ledger`). |
| Push N -> Push N+1 within one `velnor:verify` PR | Full 39-job `ci.yml` (`52.9 min` per run `[EV: evidence.md:82]`) | Unchanged packages: Yes | 52.9 runner-min per push | **Reuse unchanged packages (`1.4 min` delta vs `52.9 min`).** Cancel Push N in `<0.5 s`, harvest completed `.rlib`/`.rmeta` units, re-verify only packages whose closure tree hash changed. | Per-package closure key `blake3(closure_trees, manifest)` + signed receipts. |
| Rebase-only push (`git rebase main`) | Full 39-job `ci.yml` | Yes when `merge-tree(head, main)` matches prior tested tree (or per-package closures match) | 52.9 runner-min | **Reuse (`<1 s` verdict on identical merge tree; `0` crates compiled on unchanged closures).** | Keys are computed on `git merge-tree --write-tree(base, head)`, invariant to commit timestamps/parents. |
| Concurrent PRs touching disjoint roots/packages | Full 39-job `ci.yml` per PR (`97.4%` ran heavy jobs `[EV: v2:632]`) | Yes on untouched packages | 52.9 runner-min x N PRs | **Reuse (`100%` hit on untouched roots/packages; in-flight join deduplicates simultaneous identical obligations).** | Site-wide `velnor-ledger` receipt table + `ensure` singleflight coalescing. |
| Stacked PRs (`PR2` targeting `PR1` or stacked commits) | Full 39-job `ci.yml` on both | Yes on `PR1`'s untouched packages | 52.9 runner-min per layer | **Reuse `PR1`'s verified package receipts (`PR2` compiles/tests only `PR2 - PR1` delta).** Promoted to `trusted` only after `merge_group` against `main`. | Content-addressed per-package receipts scoped as `candidate` until `main` lands. |
| Agent pre-push (`velnor verify`) vs PR CI | Local agent build + full CI rebuild | Yes when run in cluster sandbox | ~15 min agent wait + 52.9 min CI | **Reuse (`0 s` rebuild on push).** `velnor verify` uploads `<200 KB` thin-pack to cluster `velnor-hooks` and runs `ensure` in the sealed Linux sandbox before `git push`. | Same `velnor-node` sandbox signer mints receipts keyed by tree OID. |
| Toolchain (`rustup`) & pinned `mise` tools setup | All 39 `ci.yml` jobs + `state-seal` + releases (`0/58` toolchain hits `[EV]`) | Yes | ~16 runner-min / run, `0.60 GB` GH cache | **Eliminate per-job install (`10 ms` bind mount).** Provision once per pin into node L1 XFS `/store/{toolchains,mise}/<sha256>`. | Content-addressed read-only node store mounted into sandbox. |
| `cargo fetch` / crate sources | 30 Rust jobs + `ledger-cli` + `maintenance-helpers` x5 (`0/48` scale-set hits `[EV]`) | Yes (`89.5%` of PRs share `Cargo.lock`) | ~3.5 runner-min / run, `0.17 GB` GH cache | **Reuse (`20 ms` `--offline`).** Pre-extracted crate store on node L1; lockfile deltas fetched once via site `crates-mirror`. | Read-only `/store/cargo-vendored/<lock_hash>` + site `crates-mirror`. |
| Planner (`plan`) & `ledger-cli` helper build | `plan` + `ledger-cli` jobs in every PR, MG, `push`, `state-seal` | `ledger-cli` rebuilt from source every run (`0.9 min`) | 1.8 runner-min + 10 to 45 min hosted queue | **Eliminate job hops (`<50 ms` in-process).** Released `velnor-ci` binary runs directly inside `velnor-ledger` on webhook arrival (`docs/goals.md` rule 12). | Pre-installed pinned `velnor-ci` release digest in `/store/tools`. |
| Compile workspace (`fmt`, `clippy`, `doc`, `test --no-run`) | 30 Rust jobs + `maintenance-helpers` (`build.jobs=2`) + `local-mode` duplicate build | Yes per configuration | ~38 runner-min / run; `322 GiB` ephemeral writes | **Reuse across lanes & runs (`0.31 s` unchanged `[M]`, `8.5 s` median delta).** Keep 2 passes max per changed crate (1 `.rmeta` pass shared by `clippy`+`doc`, 1 `--profile ci` bin/test link pass) because parallel `.rmeta` finishes faster than waiting on full codegen. | Multi-lower `overlayfs` over warm `main` XFS snapshot + shared jobserver; extract `velnor-runner-local` crate to stop rebuilding 482 crates twice. |
| Unit & integration test execution (`cargo nextest`) | PR + `merge_group` + `main` `push` (`3x` full runs `[EV: evidence.md:207]`) | Yes when closure matches; touched tests run 3x intentionally | ~24 runner-min x 3 runs = 72 min | **Reuse unchanged by receipt (`0 s`); duplicate ONLY touched test binaries 3x in parallel (`+1.2 runner-min`, `0 s` wall penalty on idle cores) to catch `p=0.2` flakes at `49%` vs `20%` (`v2:94`).** | Signed `ObligationReceipt` keyed by package/binary input closure + sandbox manifest. |
| `merge_group` verification | Full 39-job `ci.yml` (`64.6 runner-min` `[EV: evidence.md:83]`) | `89%` identical tree at `<2 min` window `[EV: v2:719]` | 64.6 runner-min / merge | **Reuse receipts (`0.15 min` witness on 89% hit); keep speculative duplicate on `2k - 1` queue-tip/bypass trees (`~1.5 min` batch CPU) because it saves `20 to 45 s` wall time on queue advances/ejections.** | Exact obligation-key lookup + preemptible batch `ensure(T_i)` & `ensure(T'_i)`. |
| `push` to `main` CI run | Full 39-job `ci.yml` (`120/159 = 75.5%` same SHA `[EV: evidence.md:207]`) | Yes (`100%` identical SHA to `merge_group`) | 62.0 runner-min (`8.1%` false-red rate!) | **Reuse 100% of `merge_group` receipts (`0.05 min` `Required` witness).** Zero `rustc` or `nextest` runs on `main` pushes from the merge queue. | Receipt lookup by exact commit tree + `Required` (`15368`) `completejob` for `release-gate`. |
| Cache sealing & pruning (`state-seal.yml` 16 jobs, `cache-prune.yml`) | Every `main` push (`state-seal` failed `16/18` on large roots `[EV]`) | Yes (re-compiles `main` without tests) | ~28 runner-min / `main` push; `3.37 GB` GH cache | **Eliminate completely (`0 min`, `0 GB` GH cache).** Site `main` builder updates `overlayfs` snapshot once per site (`~0.9 min/site`) and replicates `<150 MB` zstd layer to site workers (`<1 s` vs `92 s` compile). | Retire `state-seal.yml` and `cache-prune.yml`; local XFS generation GC keeps last 3 `main` layers. |
| Auxiliary PR/push workflows (`source-qualification.yml` 5 jobs, `job-image-candidate.yml` 3 jobs) | Every PR / every push on hosted runners | Mostly unchanged on non-generator/non-image PRs | ~14 runner-min / PR + hosted queue saturation | **Fold into `velnor-ci` obligation graph (`0 min` when unselected).** Run only when generator goldens or runner image inputs change. | Input-addressed obligation selection in `velnor-ledger`. |
| Release builds, `<bin>-<target>.tar.gz`, `SHA256SUMS`, SLSA attestations (`release.yml`, `product-release*.yml`, `release-rust-ci-tools.yml`) | Tag / manual / `workflow_run` (`35+` jobs in `product-release*`; macOS paused per Decision 9) | Yes (identical `main` commit tree already verified in `merge_group`) | 38 to 240 runner-min / release; `1.8 GB` artifacts | **Promote exact `merge_group`-built artifacts (`0.5 runner-min` total, `0` `rustc` invocations).** Pause macOS workflows (Phase 0). Sign SLSA provenance over `velnor-store` digest on `main`; `release-plz` tags and publishes promoted tarballs + `SHA256SUMS`. | `release-artifact` CAS blob in `velnor-store` + OIDC Fulcio/GitHub Attestations API promotion. |
| Nightly / scheduled audits & `freshness.yml` | `freshness.yml` + `rust-ci.yml` `schedule` | Yes (`main` tip) | ~65 runner-min / night | **Keep 1 cold differential build per night + every 20th merge on idle batch cores (`~0.4 min` amortised / PR)** to verify cache/receipt soundness (Decision 11). | Cold sandbox (`empty` lowerdir) comparing binary digests and test outcomes against warm receipts. |
| Conformance oracle (`qualification.yml` 71 jobs -> `velnor-conformance`) | Manual `qualification.yml` (`71` hosted+cluster jobs) | Yes unless executor/actions/runner version changed | ~90 runner-min when run | **Run on-change (executor/action/protocol closure edits), nightly, and on upstream `actions/runner` release (`0 min` on ordinary generator/engine PRs).** | `velnor-conformance` differential harness in `velnor-lab`. |

### Totals: today vs Rust-only design

| Metric | Today | Rust-only Design (Committed / Stretch) | Reduction |
|---|---|---|---|
| **Runner-minutes per merged PR** (all workflows, cancelled + MG + `main` + warmers + amortised audit) | **347 runner-min** [EV: `v2/design-feedback-comparison.md:100`] | **8.0 runner-min / 3.5 runner-min** (`3.2` PR + `2.4` MG/spec + `2.0` `main` warmer + `0.4` audit) | **-97.7% / -99.0%** |
| **Runner-minutes per release** (Linux binaries, tarballs, `SHA256SUMS`, attestations, publish) | **38 to 240 runner-min** [EV/M: `release-rust-ci-tools.yml` to `product-release*.yml`] | **0.5 runner-min** (promote pre-built CAS tarballs + attestations, 0 `rustc` runs) | **-98.7% to -99.8%** |
| **GitHub Actions Cache GB** | **3.49 GB** (`0%` scale-set Rust hit rate) [EV: `evidence.md:13,360`] | **0.0 GB** (all jobs run on cluster; GH cache retired) | **-100%** |
| **Cluster disk written per merged PR** | **~966 GiB** (`10.75 GiB` ext4 copy x 30 jobs x ~3 runs) [M/EST] | **~0.6 GiB** (`<150 MB` `overlayfs` upperdir delta x 4 lanes) | **-99.9%** |

## 3. The native Rust runner

### Crates, components, and hard line budgets

All runner crates live in `velnor-new`'s workspace with `#![forbid(unsafe_code)]` outside the thin `velnor-sandbox`
Linux syscall module (`clone3`, `pivot_root`, `mount`, `seccomp`, `cgroup.kill`). Crossing a crate's line budget fails
CI for architectural review.

| Crate | Runs as | Responsibility | Source disposition | Budget |
|---|---|---|---|---:|
| `velnor-proto` | library | Scale-set v6.0-preview, Broker V2, Run Service, Results Twirp + WebSocket, Cache v2 Twirp, Artifacts v4 Twirp, OIDC wire types & HTTP/2 clients | Copy & strip legacy paths from `velnor-legacy/crates/velnor-runner/src/{scaleset/*,protocol.rs,job_message.rs,runner_json.rs}`; fix `summary_url` bug (`protocol.rs:9796`) | 8,000 |
| `velnor-expr` | library | `Expressions2` lexer, parser, evaluator, `ContextValue`, `hashFiles`, `TemplateToken` expander | Copy from `velnor-legacy/.../expression/*` (3,565 LOC) + `velnor-model/src/context_value.rs` (1,740 LOC) | 5,500 |
| `velnor-dispatch` | 1 HA active/standby pair per site | Holds scale-set sessions, long-polls Broker V2, manages virtual JIT runner pool in RAM, places jobs onto nodes *before* `acquirejob`, journals leases | Adapt `velnor-legacy/.../scaleset/{session,client,credentials,backoff}.rs` + `velnor-new` SQLite WAL journal (`crates/velnor-runner`) | 5,000 |
| `velnor-exec` | inside `velnor-node` | Executes `PipelineAgentJobRequest`: contexts, steps, composites, `background:`/`wait-all`, LIFO `post:`, file/stdout commands, 11-encoder secret masking, lease renewal, Results streaming, crash recovery | Copy pure state machines from `velnor-legacy/.../execution/{step_outcome,step_conditions,post_drain,composite_scopes,cancel}.rs`, `workflow_command.rs`, `command_files.rs`, `velnor-model/src/redaction.rs`; write orchestrator fresh (discarding 37k-LOC `executor.rs` & 36.7k-LOC `runner.rs`) | 10,000 |
| `velnor-native-actions` | linked into `velnor-exec` | Closed native Rust action registry (`checkout`, `cache`, `upload/download-artifact`, `mise-action`, `mr-boxington-action`, `attest*`, `create-github-app-token`, `alint`, `setup-velnor`, composites) + pre-job refusal gate | Adapt `velnor-legacy/.../{checkout,git_mirror}.rs` + `gha_cache.rs` v2 subset; write `attest` (Sigstore/Fulcio) & App-token fresh | 7,500 |
| `velnor-sandbox` | inside `velnor-node` | Pre-warmed Linux user/mount/PID/IPC/UTS/net namespaces, cgroup v2 CPU/RAM/PID controllers, `overlayfs` views, `pivot_root`, Landlock/seccomp, Firecracker microVM driver for Docker suites | Fresh Rust (`rustix` + `libc`), borrowing UID/cgroup ownership proofs from `velnor-new/crates/velnor-runner` | 4,500 |
| `velnor-hooks` & `velnor-ledger` | 1 service per site | Verified App webhook ingress (`labeled`, `unlabeled`, `synchronize`, `merge_group`, `push`), `ensure(tree)` DAG scheduler, Ed25519 receipt log, speculation, cold/sampled audits | Fresh | 7,000 |
| `velnor-node` & `velnor-store` | 1 daemon per worker + 1 CAS per site | Hosts `velnor-exec`, maintains warm `main` `overlayfs` lowerdirs, bare Git mirror, tool store, `blake3` CAS (`MBXPACK1`-inspired streaming packs), test shard runner | Fresh | 9,000 |
| `velnor-conformance` | `velnor-lab` service + CLI | Differential runner against latest upstream `actions/runner`, normaliser, drift canary | Fresh | 4,000 |

**Total new/adapted runner codebase: <= 60,500 LOC** (versus 803,434 LOC in `velnor-legacy` and 157,000 LOC in
`velnor-runner-host` alone). Nothing from `velnor-legacy`'s `_apis/distributedtask` timelines/feeds
(`runner.rs:11105-19190`), AES `MessageListener`, Cache v1 (`_apis/artifactcache`), `docker_lease.rs` (21.6k LOC), or
`buildkit.rs` (10.3k LOC) is copied.

### Protocol client (latest APIs only)

1. **Scale-set control loop (`velnor-dispatch`):** Signs RS256 GitHub App JWT (`iat = now - 60`, `exp = iat + 540`) ->
   `POST /app/installations/{id}/access_tokens` -> `POST /orgs/{org}/actions/runners/registration-token` ->
   `POST /actions/runner-registration` (`Authorization: RemoteAuth {reg_token}`) yielding
   `{url: actions_service_url, token: admin_token}` (`actions-runner` / `ARC/github/actions/actions.go:1-54`). Opens
   scale-set session via
   `POST {actions_service_url}/_apis/runtime/runnerscalesets/{id}/sessions?api-version=6.0-preview` (`PATCH` on
   `401 MessageQueueTokenExpiredError`) and long-polls `GET {messageQueueUrl}?lastMessageId={id}` (300 s timeout) with
   **`X-ScaleSetMaxCapacity: max(busy_slots + free_slots, 1)`** (never `free_slots`, avoiding the drain-mode stall
   documented in `scaleset/README.md:42-49`).
2. **Virtual JIT pool & Broker V2 (zero AES):** Pre-mints ephemeral JIT runner identities via
   `POST .../generatejitconfig?api-version=6.0-preview` and decodes base64 `.runner`, `.credentials`, and
   `.credentials_rsaparams` in memory (`Runner.cs:234-268`), never touching disk. Exchanges RS256 `client_assertion`
   JWT (`5 min` TTL) at `oauthEndpointUrl` for a Bearer token (`VssOAuthJwtBearerAssertion.cs:120-147`). Opens Broker
   V2 session `POST {ServerUrlV2}/session` with `BrokerSession: true` and **no `EncryptionKey`**
   (`BrokerMessageListener.cs:65-200`). Long-polls `GET {ServerUrlV2}/message?sessionId={guid}&status=Online...`;
   `RunnerJobRequest` arrives as **plaintext JSON**
   `RunnerJobRequestRef {id, runner_request_id, should_acknowledge, run_service_url, billing_owner_id}`
   (`RunnerJobRequestRef.cs:6-22`). Sends best-effort `POST {ServerUrlV2}/acknowledge` (5 s timeout).
3. **Run Service (`acquirejob` / `renewjob` / `completejob`):** Calls `POST {run_service_url}/acquirejob`
   (`AcquireJobRequest {jobMessageId, runnerOS: "Linux", billingOwnerId}`) -> receives **plaintext JSON**
   `AgentJobRequestMessage` (`RunServiceHttpClient.cs:70-121`; `404`/`409`/`422` treated as non-retriable skips).
   Extracts per-job `SystemVssConnection` URL and `AccessToken` (`JobRunner.cs:64-70`), completes mandatory initial
   `POST {systemUrl}/renewjob` before running user steps (`JobDispatcher.cs:394-419`), and finishes with
   `POST {systemUrl}/completejob` carrying snake_case `stepResults`, `outputs`, `annotations`, and `conclusion`
   (`CompleteJobRequest.cs:10-41`).

### Job-message model, expression engine, steps, contexts, and masking

- **Job-message model:** Parses `MessageType == "PipelineAgentJobRequest"` (`AgentJobRequestMessage.cs:91-514`),
  rejecting any legacy `ServerTask`/`AgentJobRequest` message at boundary deserialisation. Decodes typed
  `TemplateToken` (`0=String..7=Null`, `TokenType.cs:7-21`), `PipelineContextData`
  (`0=String..5=CaseSensitiveDictionary`), polymorphic `JobStep` (`Type == 4` `ActionStep` with `IsBackground`,
  `Type == 5` `BackgroundStepControl` for `wait`/`wait-all`/`cancel` used in `ci.yml:730-746`), `Variables`, `Mask`,
  `Resources.Endpoints`, and `FileTable`.
- **Expression engine (`velnor-expr`):** Exact `DTExpressions2` semantics (`AR/src/Sdk/DTExpressions2`):
  `MaxLength = 21,000`, `MaxDepth = 50`, `MaxMemory = 1 MiB`, `TemplateMemory` `10 MiB`
  (`ExpressionConstants.cs:30-31`). Case-sensitive literals (`null`, `true`, `false`, `NaN`, `Infinity`),
  `OrdinalIgnoreCase` functions and property lookups except `env` on Linux (`CaseSensitiveDictionaryContextData`,
  `StepsRunner.cs:104`), unknown root context = parse error, JS `Number()` coercion (`0x`/`0o` int32, `"" -> 0`,
  `true -> 1`), short-circuiting `&&`/`||` returning the raw operand value (`And.cs:39-48`), `case()` requiring strict
  `Boolean` predicates (`Case.cs`), context-sensitive `success()`/`failure()` inspecting `github.action_status` inside
  composite `Main` steps (`SuccessFunction.cs`), and native Rust `hashFiles` streaming SHA-256 over `GITHUB_WORKSPACE`
  globs in `<15 ms` instead of spawning Node (`hashFiles.ts:23-52`).
- **Step, composite, background, and `post:` semantics:** Pre-job steps run FIFO (`pre-if` default `always()`,
  `ActionManifestManager.cs:458-500`). Main steps wrap bare `if:` expressions without status functions in
  `success() && (<expr>)` (`WorkflowTemplateConverter.cs:1850-1869`). `post:` steps push onto `PostJobSteps` **only
  when their corresponding `Pre` or `Main` step starts** (`ActionRunner.cs:113-137`) and drain in **strict LIFO**
  order after implicit `wait-all` (`StepsRunner.cs:59-77`). Composites expand recursively with isolated `inputs` and
  `siblingScopeName` so `steps.<id>` remains readable in `post:` (`CompositeActionHandler.cs:32-185`).
  `continue-on-error: true` converts only `Failed` (never `Canceled`) into
  `outcome = "failure", conclusion = "success"` *after* merging `CommandResult` (`StepsRunner.cs:348-350`,
  `ExecutionContext.cs:1424-1455`). Background steps (`ci.yml`'s parallel `Format`/`Clippy`/`Doc`/`Tests`) run up to
  semaphore `10` with cloned `github` file-command paths, deferring env/output merges until `wait`/`wait-all`
  (`BackgroundStepCoordinator.cs:29-135`).
- **Contexts, env/command files, and secret masking:** Populates all 11 contexts (`github`, `env`, `vars`, `job`,
  `steps`, `runner`, `secrets`, `strategy`, `matrix`, `needs`, `inputs`). Allocates per-step
  `_runner_file_commands/{prefix}{uuid}` files (`FileCommandManager.cs:19-495`): rejects `NODE_OPTIONS` in
  `GITHUB_ENV`, enforces strict `\n` heredoc matching on Linux (`482-494`), caps `GITHUB_STEP_SUMMARY` at `1 MiB`
  (`258-267`). Parses `::cmd k=v::data` unescaping `%0D/%0A/%3A/%2C/%25`, supports `::stop-commands::{token}`
  (auto-masking tokens `>6` chars), and runs the **11 upstream `ValueEncoders`** (`Base64` + shift-1/shift-2, JSON,
  URI, XML, CLI, expression `''`, quote-trim, PowerShell `&`, `ValueEncoders.cs:16-188`) plus multiline line-split
  registration over all secrets, `MaskHints`, OIDC JWTs, and `Endpoint` tokens, merging adjacent spans into `"***"`
  before any byte leaves memory (`SecretMasker.cs:216-297`).

### Cancellation, timeouts, lock renewal, Results streaming, and crash safety

- **Lock renewal (fixing both upstream and v2 bugs):** Upstream `JobDispatcher.cs:750,810` crashes on retry with a
  null `request`, while Run Service `LockedUntil` leases can be as short as `30 to 60 s`. `velnor-exec` renews on an
  independent Tokio RT task at **`min(25 s, (locked_until - now) / 2)`** using a freshly cloned
  `RenewJobRequest {plan_id, job_id}` and retains the JIT RSA private key in `velnor-dispatch` to mint fresh
  `SystemVssConnection` / OAuth tokens if a job crosses the 1-hour token boundary.
- **Cancellation & timeouts:** Enforces step `timeout-minutes` (`Failed`) and Broker `JobCancellation` /
  `cancel-timeout-minutes` (default 5 min, worker deadline `timeout - 15 s`, `JobDispatcher.cs:1269-1286`). On cancel,
  updates `job.status = Cancelled`, re-evaluates active steps' `if:` (`always()`/`cancelled()` steps keep running!),
  and escalates `SIGINT (7.5 s) -> SIGTERM (2.5 s) -> cgroup.kill` (`ProcessInvoker.cs:32-33` + atomic Linux cgroup v2
  kill replacing upstream's racy PID walk).
- **Logs, timeline, and summaries via Results only:** Streams live console lines over WebSocket `FeedStreamUrl`
  (`1,024`-byte frames, `ResultServer.cs:167-295`), posts step transitions to Twirp
  `WorkflowStepUpdateService/WorkflowStepsUpdate` with monotonic `change_order`, and uploads 2 MiB paged `AppendBlob`
  log chunks (`&comp=appendblock`, finalised with `x-ms-blob-sealed: True`) plus scrubbed `BlockBlob` step summaries
  via `results.services.receiver.Receiver/*` (`ResultsHttpClient.cs:584-636`), using `summary_url`
  (`Contracts.cs:25`). Zero calls touch `_apis/distributedtask`.
- **Crash safety:** Every external mutation follows `persist intent -> execute -> persist outcome` in `velnor-node`
  and `velnor-dispatch`'s SQLite WAL journals (`synchronous = NORMAL` + `fsync` on lease transitions). Before
  `acquirejob`, `velnor-dispatch` logs `(request_id, jit_id, epoch)`; before step N starts, `velnor-exec` logs
  `(job_id, step_idx, cgroup_uid)`. On `velnor-node` restart, surviving sandboxes are reattached by `cgroup_uid` +
  pidfd; orphaned sandboxes are killed via `cgroup.kill`, and unfinished steps fail closed with
  `completejob(InfrastructureFailure)` exactly once via idempotency token.

### Starting a job in `< 50 ms` locally (`< 220 ms` including WAN `acquirejob`)

1. **Hot pool per worker:** `velnor-node` keeps 6 pre-created Linux sandboxes idle in RAM (`< 8 MiB` total vs
   `45 MiB x N` K8s pods): user/PID/IPC/UTS/net namespaces already cloned, seccomp-bpf loaded, cgroup v2 paths open,
   and a zygote helper parked on `read(eventfd)`.
2. **Pipelined placement + `acquirejob`:** `velnor-dispatch` holds pre-registered Scale-Set/Broker V2 sessions. When
   `JobAvailable` / `RunnerJobRequest` arrives, `velnor-dispatch` picks the target worker in `< 1 ms` (by warm `main`
   generation and free CPU slots) and fires `POST /acquirejob` (`~180 ms` WAN RTT) **in parallel** with an intra-site
   RPC (`< 1 ms`) telling `velnor-node` to mount the `overlayfs` view
   (`lowerdir=/store/snapshots/main-<gen>,upperdir=...,workdir=...`, `~5 ms`) and bind-mount
   `/store/{toolchains,mise,cargo}` (`~10 ms`).
3. **Instant handoff:** When `acquirejob` returns, the sandbox is already mounted and waiting; `velnor-exec` validates
   the closed action allowlist in `< 0.5 ms`, fires the initial `renewjob` (`~120 ms`, pipelined with local `checkout`
   mirror attach if step 1 is `actions/checkout`), and wakes the zygote (`< 2 ms`). First step process runs in
   **`~30 ms` local CPU/kernel time** (`~300 ms` from broker offer including WAN RTTs, vs `12 to 14 s` on ARC today).

```text
GitHub Webhook (labeled / synchronize / merge_group) ──(1.2 s)──► velnor-hooks
                                                                       │
                                                       ensure(merge_tree, plan)
                                                                       ▼
GitHub Broker V2 ──(8.0 s)──► velnor-dispatch              velnor-ledger (site)
                                 │    │                    ├── hit? return signed receipt (<5 ms)
         parallel sandbox mount  │    │ POST /acquirejob   └── miss? place on worker w
         (<15 ms overlayfs+L1)   ▼    ▼                              │
                              velnor-node ◄──────────────────────────┘
                              ├── velnor-sandbox (24T CCD cgroup v2, pivot_root, lo netns)
                              ├── velnor-exec ──► Results Twirp + WebSocket + completejob
                              └── Ed25519 signer ──► velnor-store (CAS) + velnor/verdict
```

## 4. Native actions

`velnor-native-actions` compiles directly into `velnor-exec` and implements the closed union of `velnor-new`'s engine
allowlist (`crates/velnor-actions-actionlint/src/actions.rs:6-20`, `.velnor/version-policy.toml:41-120`) and
`velnor-gen`'s 8-action allowlist (`generator/crates/velnor-render/src/rules.rs:8-21`). Before step 1 starts,
`velnor-exec` validates every `uses:` reference in `PipelineAgentJobRequest.Steps` against
`(owner/repo[/path], qualified_commit_sha)`; any unknown action, floating tag, `using: node20|node24`, or
`using: docker` manifest fails the job immediately with zero user code executed.

| Action (pinned version & SHA) | Native Rust design in `velnor-native-actions` | Local node/site operation replacing WAN | Today [M/EV] | Design [EST] |
|---|---|---|---|---|
| `actions/checkout` (`v7.0.1`, `3d3c42e5...`) | `gix` + local `git` checkout supporting `ref`, `fetch-depth` (`1`, `2`, `0`), `sparse-checkout`, `sparse-checkout-cone-mode`, `clean`, and `persist-credentials: false` (scrubbing `.git/config` in `main` and `post:`). | Binds read-only `objects/info/alternates` to node L1 bare mirror `/store/git-mirrors/velnor-new.git` (already updated on webhook). Zero action tarball fetch (`-4.4 s`), zero full pack download. | 6.0 s P50 / 8.0 s P90 (`+1.5 s` action dl) | **0.18 s P50 / 0.45 s P90** |
| `actions/cache`, `/restore`, `/save` (`v6.1.0`, `55cc8345...`) | Computes exact `@actions/cache` `version = sha256(resolved_paths \| "zstd-without-long")`, prefix `restore-keys` order, `cache-hit` / `cache-matched-key` outputs, and `GITHUB_STATE` handoff to `post:`. | Materialises directories via XFS reflink / `overlayfs` from node L1 or streams `MBXPACK1` zstd frames from site L2 `velnor-store` (`215+ MB/s` LAN vs `13.6 to 22.4 MB/s` Azure WAN). Dual-writes Twirp `CacheService` only in lab conformance mode. | 1 to 61 s (`44 to 61 s` on 1.92 GB `[EV]`; `0/48` sources hit) | **0.05 s P50 / 1.20 s P90** |
| `actions/upload-artifact` (`v7.0.2`, `cf430e03...`) & `download-artifact` (`v8.0.2`, `9000827c...`) | Implements Twirp `ArtifactService/{CreateArtifact,FinalizeArtifact,ListArtifacts,GetSignedArtifactURL}` (`version: 7`), `pattern`, `merge-multiple`, `if-no-files-found`, and int64 string/number JSON decoding. | Stores intra-run blobs in site L2 `velnor-store` (`blake3`) and reflinks on download without WAN round-trips; pushes real `application/zip` `BlockBlob` to Azure asynchronously (or synchronously for release/lab jobs). | 2 to 43 s up / 1 to 28 s down (`1.48 GB` = 43 s `[EV]`) | **0.08 s P50 / 0.90 s P90** (`2.5 s` external zip) |
| `jdx/mise-action` (`v5.2.6`, `f5be966e...`) | Parses `.velnor/mise.toml` / `mise.toml` + `version` (`2026.11.1`, `2026.11.9`) + `sha256`, verifies digests, bind-mounts read-only tool directories, prepends `GITHUB_PATH`, exports `MISE_*` env. | Reads pre-populated node L1 `/store/mise/<tool>@<ver>` (`100%` hit). Zero GitHub API queries, zero tarball downloads, zero `$RUNNER_TEMP` extraction. | 12.0 s P50 / 23.0 s P90 (`evidence.md:335-345`) | **0.01 s P50 / 0.02 s P90** |
| `jdx/mr-boxington-action` (`v1.7.3`, `88acf2bb...`) | Transition shim until `ci.yml` regenerates without `mbx`: exposes `/store/tools/mbx-1.29.2`, sets `GITHUB_ENV` (`MBX_CACHE_DIR`), emits `post:` stats. | Bind-mounts local binary (`0` WAN bytes); retired once `velnor-gen` owns all Rust workflows. | 1.0 s P50 / 3.0 s P90 (`evidence.md:348-349`) | **0.005 s P50** (`0 s` post-cutover) |
| `actions/attest-build-provenance` (`4d101475...`) & `actions/attest` (`1e69f48a...`, `v4.2.2`) | Native DSSE / in-toto SLSA v1.0 builder + `aws-lc-rs` ECDSA P-256 ephemeral signer: fetches OIDC token (`audience=sigstore`), gets Fulcio cert + TSA timestamp, posts bundle to `/repos/{o}/{r}/attestations`. | Hardware SHA-256 subject hashing (or pre-computed `velnor-store` digest) with zero Node24/npm cold boot; only 3 tiny TLS JSON calls (`OIDC`, `Fulcio+TSA`, `Attestations`). | 6.0 to 14.0 s | **0.90 s P50 / 1.80 s P90** |
| `actions/create-github-app-token` (`v3.3.0`, `64ab729a...`) | Signs RS256 App JWT (`aws-lc-rs`), calls `POST /app/installations/{id}/access_tokens`, masks token before writing `GITHUB_OUTPUT`, revokes token in `post:` via `DELETE /installation/token`. | Local RS256 signature (`<1 ms`), zero action download; 1 WAN REST call on `main` + 1 on `post:`. | 3.0 to 6.0 s | **0.20 s P50 / 0.45 s P90** |
| `asamarts/alint` (`v0.21.0`, `d61e4c80...`) | Invokes pre-baked `/store/tools/alint-0.21.0` with `.alint.yml`, `--fail-on-warning`, and `--format github`. | Local exec from read-only `/store/tools`; zero release download. | 4.0 to 9.0 s | **0.15 s P50 / 0.40 s P90** |
| `actions/setup-velnor` & `./.github/actions/*` composites | Native composite runner + shortcut for `actions/setup-velnor`: matches `sha256-linux-x64` against `/store/tools/velnor-ci-<sha256>` and reflinks into `$RUNNER_TEMP/velnor-ci-bin`. | Zero GitHub Release download of `velnor-ci`; in-memory composite step expansion. | 2.0 to 5.0 s | **0.005 s P50 / 0.01 s P90** |
| `actions/github-script` (`3` uses), `configure-aws-credentials` (`0` uses), Docker/JS test actions (`4` uses) | **Not implemented; refused pre-job.** The 3 `github-script` uses (`release-rust-ci-tools.yml:15` tag-ruleset drift + 2 `qualification.yml` helpers) move into `velnor-ci check-rulesets`; `qualification.yml` JS/Docker cases become refusal tests. | Eliminates Node24 and Docker action attack/maintenance paths completely (Decision 4). | 2.0 to 8.0 s | **0.0 s (retired / refused)** |

## 5. The conformance oracle

`velnor-conformance` runs in `velnor-lab` (an isolated GitHub repository + dedicated scale-set pair:
`velnor-oracle-official` running the unmodified upstream C# `actions/runner` container and `velnor-oracle-rust`
running `velnor-dispatch` + `velnor-exec`). In oracle mode, `velnor-native-actions` enables synchronous GitHub Cache
v2 and Artifacts v4 writes (`VELNOR_GHA_CACHE_MIRROR=1`) so externally observable side effects are identical.

### Differential corpus (420 cases across 4 suites)

1. **Production workflow replay (85 cases):** Every job of `velnor-new`'s workflows (`ci.yml`, `rust-ci.yml`,
   `release.yml`, `release-rust-ci-tools.yml`, `product-release-images.yml`, `product-release-generator.yml` Linux
   lanes, `freshness.yml`) dispatched across `pull_request`, `merge_group`, `push`, and `workflow_dispatch`, covering
   green passes, planted compiler/test failures, and mid-flight cancellations.
2. **Protocol & expression semantics (240 cases):** All upstream `DTExpressions2` unit vectors + `velnor-legacy`'s
   989-line expression suite (`expression/tests.rs`): `NaN`/`0x`/`0o` coercion, short-circuit value preservation on
   `&&`/`||`, strict-boolean `case()`, case-sensitive `env` vs case-insensitive contexts, `hashFiles` symlink/ordering
   edge cases, nested composites (`steps.<id>` visibility in `post:`), LIFO `post:` ordering when step K fails before
   step K+1 starts (`ActionRunner.cs:113-137`), `background: true` + `wait`/`wait-all`/`cancel` (`ci.yml:730-746`),
   `continue-on-error` `outcome` vs `conclusion` on `Failed` vs `Canceled`.
3. **Boundary, masking & fault cases (65 cases):** All 11 `ValueEncoders` (Base64 0/1/2-byte shifts, URI/JSON/XML/PS
   escapes, multiline secrets), `::stop-commands::` token auto-masking (`>6` chars), `NODE_OPTIONS` rejection in
   `GITHUB_ENV`, Linux `\n`-only vs `\r\n` heredoc delimiters (`FileCommandManager.cs:482-494`), 1 MiB
   `GITHUB_STEP_SUMMARY` scrubbing (`summary_url`), OIDC token request at `t = 0 ms` after job start, step
   `timeout-minutes` vs job `cancel-timeout-minutes` (`SIGINT 7.5 s -> SIGTERM 2.5 s -> kill`).
4. **Refusal corpus (30 cases):** Unqualified action SHAs, `@v4` tags, `actions/github-script`, `using: node24`,
   `using: docker`, legacy `distributedtask` payloads. Asserts Velnor fails before step 1 with zero user process
   spawned.

### Normalisation and comparison rules

Both runners execute the exact commit and event payload in isolated cache/artifact scopes. `velnor-conformance`
fetches Check Runs, Job/Step REST objects, raw job & step logs, annotations, step summaries, Cache v2 entries,
Artifacts v4 zips, and Attestation bundles, normalising **only**:
- Ephemeral identifiers and clocks: `run_id`, `job_id`, `step.external_id` GUIDs, ISO-8601 timestamps, elapsed
  durations, runner/machine names, `RUNNER_TRACKING_ID`.
- Root prefixes: `/home/runner/_work` vs `/w`, `/tmp/_runner_file_commands/<guid>`.
- Official-runner chrome lines: `Current runner version:`, `Download action repository`, Node setup banners, and
  concurrent `background: true` stdout line interleaving (compared per-step before merge, and as multiset across
  concurrent background steps).

**Comparison requires 100% exact equality** on: job conclusion, ordered step list
`(name, status, outcome, conclusion)`, evaluated `env` and `steps.*.outputs` JSON dumps, workflow annotations
`(level, file, line, message)`, scrubbed step-summary Markdown bytes, Cache v2
`(key, version, archive file tree + sha256)`, Artifacts v4 `(name, unzipped file tree + sha256)`, SLSA attestation
subject digests and Verified status under `gh attestation verify`, and zero unmasked secret substrings across raw
logs/summaries/diag blobs. Any mismatch not covered by an owner-signed, expiring entry in
`.velnor/conformance-allowlist.toml` fails the run.

### Catching upstream releases and live GitHub protocol drift within hours

1. **On every PR touching `velnor-{proto,expr,dispatch,exec,native-actions,sandbox}`:** Full 420-case differential
   suite runs as a blocking check (`~4 min` parallelised across `velnor-lab`).
2. **Against every new `actions/runner` release (`<= 2 hours`):** A `velnor-lab` cron polls
   `GET /repos/actions/runner/releases/latest` every **30 minutes**. When a new tag appears (well inside GitHub's
   63-day runner deprecation window), it builds the oracle image, runs all 420 cases + 10 planted-bug mutation checks
   (proving the harness still catches a dropped mask, reordered `post:`, or wrong `cancelled()`), and opens a P0 alert
   issue + blocks `velnor-exec` runner-version bump on any diff.
3. **Against unannounced live GitHub SaaS backend drift (`<= 1 hour`):** GitHub deploys Broker V2, Run Service,
   Results Twirp, Cache v2, and Artifacts v4 changes continuously without cutting a runner release. Every **60
   minutes**, `velnor-conformance --canary` dispatches a 45-second golden workflow probing every wire endpoint
   (`sessions`, `acquirejob`, `renewjob`, `WorkflowStepsUpdate`, `AppendBlob` + `summary_url`, `CacheService`,
   `ArtifactService`, `OIDC`, `completejob`) on both Velnor and `actions/runner`, while `velnor-proto` logs schema
   telemetry (`#[serde(deny_unknown_fields)]` in shadow deserialiser) so any new or renamed JSON/Twirp field triggers
   an alert within **1 hour**, before strict fields break production.
4. **Cutover pass criteria:** (a) `100.0%` pass (420/420) on two consecutive nightly runs against the latest
   `actions/runner`; (b) `10/10` planted-fault detection checks green; (c) 200 real `velnor-new` PR/MG shadow runs
   with zero unexplained verdict, output, or secret-masking divergence; (d) chaos fault-injection pass (kill -9 at
   every step/RPC boundary, zero duplicate `completejob` or leaked processes).

## 6. Compile, cache and test

### Choosing the compile & cache engine from measured evidence

| Candidate | Measured performance on `velnor-new` | Soundness & operational evidence | Verdict |
|---|---|---|---|
| **`blaze`** (`0.20261105.4` canary / `0.20261027.0` stable) | **Fails 5/5 workspaces** (`blaze.md:30-38`). On buildable 4-crate subset: cold `215.5 s` vs cargo `17.9 s` (**12x slower**); warm edit `21.3 s` vs cargo `1.5 s` (**14x slower**); RSS `18 to 44 GB` vs cargo `<5 GB` (`blaze.md:45-57,76`). | **False green (fatal):** `blaze check` exits `0` on `unwrap_used = "deny"` (`blaze.md:73`); `#1604` exits `0` on `compile_error!`; ignores `rust-toolchain.toml` (`1.99`) for its own `1.101` nightly (`blaze.md:27`); hashes path string instead of bytes on `CARGO_BIN_EXE_*` (`late.rs:5-8`). | **Reject completely.** Violates Decision 11 (false greens) and is an order of magnitude slower. |
| **`mbx` (`1.29.2`) + `mbx-cache` (`0.0.1`)** | Same-node warm without shared target (`M2b`): **`7.2 s`, `+1.94 GiB`** (`mbx-cache-cluster.md:38`). Lockfile edit (`L1m`): **`88.4 s`, `+15.5 GiB`**, `6.8 GiB` upload (`:42`). Cold-node fill (`M3`): **`39.9 s`**, `8.7 GiB` uncompressed download (`:39`). Mac path change: missed all 6 workspace crates, leaked `230 MiB`/checkout (`mbx-cache-mac.md:27`). | Requires `RUSTC_WRAPPER` process shims, `MBX_REMOTE_TRUST_WRITES=true`, TLS or loopback `socat` (`mbx-cache-cluster.md:21-22`), stores uncompressed blobs (`16.3 GB` server after 1 lock edit), and `M2`'s `0.32 s` relies on a single mutable target dir unsafe for concurrent jobs. | **Reject on hot path AND retire `mbx-cache` server.** Replaced by faster, simpler `overlayfs` zstd layer streaming (`0.7 to 13 s` vs `39.9 s`). |
| **Plain `cargo` + XFS `cp --reflink=always` clone** | Cold `92.0 s` (`10.64 GiB`); warm clone `0.25 s` (`+2.5 MB`) + `0.31 s` build (`0` crates) at fixed `/w/velnor-new`; lockfile edit `67.4 s` (`mbx-cache-cluster.md:33-41`). | Sound at fixed `/w/velnor-new`, **but E1d (`fincore`, `:76-81`) proved XFS reflinks do NOT share Linux page cache:** 2 clones burned `2 x 14.29 = 28.6 GB` RAM and evicted the base snapshot; 4 sandboxes per node would burn `57.2 GB` RAM. | **Use XFS reflink ONLY for background snapshot compaction**, not per-sandbox live mounts. |
| **Plain `cargo` + Multi-Lower `overlayfs` over XFS snapshots + `velnor-store` zstd layers (CHOSEN)** | Mount `< 5 ms` (`O(1)` vs `250 ms` clone), `0 B` pre-build disk, `0.31 s` unchanged build, `8.5 s` P50 single-crate delta; intra-site `main` delta layer (`50 to 200 MiB` zstd) replicates in **`0.7 s`** (full `2.8 GiB` zstd cold seed in **`13 s` vs `39.9 s` `mbx-cache`**). | Kernel `overlayfs` (`6.12.94`) maps read-only `lowerdir` files directly to the single XFS base inode, **sharing 100% of `14.3 GB` page cache across all concurrent sandboxes on the worker (`4x` RAM reduction)** while isolating writes in tmpfs/XFS `upperdir`. | **Adopt everywhere.** Fastest hot path, shared page cache, zero `RUSTC_WRAPPER` bugs, smallest network transfer. |

**Compile architecture details:**
- Every sandbox mounts the workspace at fixed canonical path `/w/velnor-new` (preventing path-hash invalidation
  measured in `mbx-cache-mac.md:28`) with `CARGO_INCREMENTAL=0`, `SOURCE_DATE_EPOCH=1`, and
  `--remap-path-prefix=/w/velnor-new=.`.
- Each site's `main` builder compiles each landed `main` commit once over `lowerdir=main-<G>` into
  `upperdir=layer-<G+1>`, freezes `layer-<G+1>`, streams it (`zstd -3`) to same-site workers in `< 1 s`, and every 8
  generations compacts `lowerdir` via `cp --reflink=always` on XFS (`0.25 s`) so `overlayfs` depth never exceeds 8
  layers.
- Within a sandbox, `clippy`+`doc` share one `--profile ci` metadata `overlayfs` view (`target-meta/`) while
  `cargo nextest archive` / `--no-run` uses a sibling `overlayfs` view (`target-bin/`) over the same `lowerdir`,
  eliminating `.cargo-lock` serialisation (`docs/hard-decisions.md:56-62`) while sharing one GNU Make `jobserver` FIFO
  across the sandbox's 24 threads.
- **Breaking the compile floor:** Split `velnor-runner-host` (`157k` LOC, `61 s` edit-compile) into 6 domain crates
  (`journal`, `sandbox`, `lease`, `transport`, `reconcile`, `host`), extract `velnor-runner-local` into its own crate
  instead of feature-rebuilding 482 crates twice (`evidence.md:260-275`), and consolidate the 356 separate
  `tests/*.rs` integration binaries (`177` in `workflow-renderer`, `109` in `orchestrator`, `44` in `actions-cli`,
  `26` in `runner-host`, `design-feedback.md:122`) into **1 `tests/it/main.rs` binary per package** (saving ~340
  redundant rust-lld link passes!).

### Breaking the 125 to 138 s (and 924.8 s) single-test floor & cluster-wide scheduling

Measured slow-test root causes and concrete fixes targeting **no single test `> 12 s` (Committed `<= 15 s`)**:
1. **`receipt-integrity` (`924.8 s`, `F-H33`) & `Journal::open` (`93` call sites, `125 to 138 s` floor in
   `runner-host` / `local-mode`):** Tests run 52 sequential SQLite schema migrations (`turso` / SQLite) and build full
   publication histories 4 times from empty DBs. **Fix:** Pre-migrate a canonical closed SQLite database + valid
   receipt prefix once in a `LazyLock` test-suite fixture and `cp --reflink=always` (`< 1 ms` per test) into each
   test's tempdir before running corruption/reopen assertions (`924.8 s -> ~4 s`; `Journal::open` `>1 s -> <15 ms`).
2. **`non_inputs_differential.rs:198-264` (`6,425` globs spawning `7` `git` subprocesses each = ~45,000 processes in 1
   test, `679 s` suite):** **Fix:** Replace subprocess `git clone`/`read-tree`/`ls-files` loop with an in-memory
   `gix_worktree` sparse-index matcher against one pre-built fixture repo + rayon parallel check, keeping a 32-case
   subprocess differential sanity check against real `git` (`~135 s -> 1.8 s`).
3. **`image_publication_public.rs:86,257` (`40x` `RejectionCase` loop = `258 to 274 s`; `12x` `BlobFault` loop) &
   `unbound_retirement/tests/shape.rs:97` (`6x` loop = `203 to 271 s`):** **Fix:** Emit 1 `#[test]` per case via
   declarative macro over a shared immutable registry fixture (`274 s -> 7 s` across 24 threads) and fold the `89 s`
   cold `image-publication-fixture` example build (`evidence.md:255`) into `--all-targets`.
4. **Artificial nextest serialisation (`golden_collector_public` 12 tests with
   `threads-required = "num-test-threads"`, `.config/nextest.toml:19,46`, `212 to 246 s`;
   `ci-helper/.config/nextest.toml:4` `test-threads = 1`):** **Fix:** Isolate tempdirs, ephemeral `127.0.0.1:0` ports,
   and env snapshots so `threads-required` and `test-threads = 1` are deleted (`246 s -> 14 s`; `ci-helper`
   `10 s -> 3 s`).
5. **`[profile.ci.package."*"] opt-level = 2` (with `debug-assertions = true`, `overflow-checks = true`):** Cuts
   cryptographic/SQLite/OCI/tar dependency CPU in tests by **66%** (`142.5 -> 47.9 CPU-s` `[M]`).
6. **Cluster-wide test scheduling (480 threads):** Each test binary emits `nextest list --message-format json`;
   `velnor-ledger` maintains an EWMA duration table per `(binary, test_name)`. Whenever a request's predicted test
   CPU-seconds divided by 24 sandbox threads exceeds `max_single_test + 1.5 s` (the time to stream unified test
   binaries from `velnor-store` to sibling workers in the same site at `215+ MB/s`, ~`0.4 s` for 1 consolidated
   `80 MB` test binary!), `velnor-ledger` shards test cases across up to **5 workers x 96 threads (288 threads per
   site in FI, 192 in DE)** using Longest-Processing-Time-First (LPT) bin packing. Changed tests execute **3x in
   parallel** across shards; every expected test name in the plan inventory must report `3/3` `PASS` or the obligation
   fails closed.

### Soundness cases for every reuse and skip mechanism (Decision 11)

Every reuse mechanism obeys zero-tolerance for false greens:

- **S1. Warm `overlayfs` Cargo compile reuse (`main` lowerdir + monotonic mtimes):**
  1. *Why the key is complete:* Lowerdirs are built exclusively by the sealed `main` builder in a network-isolated,
     fixed-path (`/w/velnor-new`) sandbox; when mounting a PR sandbox, unchanged files inherit the exact recorded
     `(mtime, size)` from the snapshot manifest while every modified/added file gets a monotonic timestamp strictly
     newer than `max(snapshot_output_mtime) + 2 s`, and `Cargo.lock` + `rust-toolchain.toml` + `RUSTFLAGS` +
     `--profile ci` are baked into the snapshot generation ID.
  2. *Failure mode if incomplete:* Cargo skips recompiling a dirty crate (`mtime` collision or untracked
     `include_bytes!` outside crate dir), linking stale object code that masks a compile or test failure.
  3. *Detection:* Nightly and **every 20th merged commit** run a full cold differential build (`empty` lowerdir)
     comparing compiler exit codes, test outcomes, and `blake3` digests of compiled test/release binaries against the
     warm build; work item 1 explicitly tests backdated-mtime edits.
  4. *Automatic fallback:* Any cold-vs-warm verdict mismatch or unexpected dirty/clean mismatch atomically bumps
     `SNAPSHOT_EPOCH` in `velnor-ledger`, disabling `overlayfs` lowerdir mounts cluster-wide (falling back to clean
     builds) and paging via P0 GitHub issue until root-caused.

- **S2. Content-addressed obligation receipts (across PR pushes, concurrent/stacked PRs, `merge_group`, and `main`):**
  1. *Why the key is complete:*
     `ObligationKey = blake3(DOMAIN_TAG || planner_release_sha || sandbox_image_sha || toolchain_sha || rustflags || cargo_profile || nextest_profile || cpu_threads || closure_git_tree_oids || extra_declared_file_oids || sorted_env_allowlist || bin_exe_target_digests)`.
     Crucially: (a) the sandbox runs inside a `pivot_root` mount namespace populated **exclusively** with the sparse
     files in `closure_git_tree_oids || extra_declared_file_oids`, so reading any undeclared repo file fails
     deterministically with `ENOENT` instead of silently succeeding; (b) `loopback`-only netns, scrubbed
     `GITHUB_*`/git metadata, fixed locale/TZ, and no wall-clock dependency; (c) all 8 workspace `CARGO_BIN_EXE_*`
     binaries and 3 example fixtures (`velnor-actions`, `velnor-ci`, `velnor-gen`, `velnor-local`,
     `velnor-archive-guard`, `velnor-golden-collector`, `velnor-local-test-job-hook`, `velnor-resource-probe-release`)
     have their exact executable `blake3` digests hashed into `bin_exe_target_digests` (`171` call sites covered!).
     Receipts are signed with per-class Ed25519 keys held in `velnor-node` outside the sandbox and expire after 7
     days; PR-minted receipts carry scope `candidate` and promote to `trusted` only after `merge_group` verifies the
     commit that lands on `main`.
  2. *Failure mode if incomplete:* A nondeterministic or environment-leaking test passes once and its
     `ObligationReceipt` skips re-execution on an affected tree.
  3. *Detection:* (a) New obligation classes shadow-run `100%` of receipt hits until **3,000 consecutive
     zero-divergence matches**; (b) post-graduation, **5% of receipt hits** re-execute asynchronously on a different
     worker, and **100% of landed `main` commits** run a full uncached cold verification on batch capacity within 6
     hours. If a disagreement fails on >=2 of 5 immediate reruns, it is classified as a soundness bug (false green),
     not a flake.
  4. *Automatic fallback:* A single confirmed false-green mismatch immediately increments `POLICY_EPOCH` in
     `velnor-ledger`, invalidating all existing receipts for that obligation class, forcing 100% live execution,
     halting `merge_queue` entries relying on that epoch, and opening a P0 issue listing any landed `main` commits in
     the window.

- **S3. Agent pre-push `velnor verify` reuse:**
  1. *Why the key is complete:* Local macOS developer/agent runs **never** mint receipts. `velnor verify` uploads a
     `<200 KB` git thin-pack to `velnor-hooks` and executes inside the **identical** cluster `velnor-node` Linux
     sandbox under the released planner, producing the exact `S2` `ObligationKey` and Ed25519 signature.
  2. *Failure mode if incomplete:* Working-tree uncommitted dirty files or local env flags diverge from the pushed
     commit tree.
  3. *Detection:* `velnor verify` builds `git write-tree` from a clean index snapshot and keys receipts strictly by
     that tree OID; when `git push` arrives, `merge-tree(main, pushed_head)` only hits if the 256-bit tree OID matches
     bit-for-bit. Audited under `S2`'s 5% sampler + 100% post-merge cold run.
  4. *Automatic fallback:* Tree mismatch misses harmlessly and runs normal CI; any `S2` audit mismatch disables
     `candidate`-scope pre-push receipts via `POLICY_EPOCH`.

- **S4. Release binary & SLSA attestation promotion (`merge_group` -> `main` -> `release`):**
  1. *Why the key is complete:*
     `ReleaseArtifactKey = blake3("release-v1" || target_triple || toolchain_sha || release_profile_toml || cargo_lock_sha || pkg_closure_tree_oids || cargo_pkg_version)`.
     Built in an offline, scrubbed sandbox on `merge_group` (including on `release-plz`'s version-bump PR!).
  2. *Failure mode if incomplete:* An untracked build-script env var or timestamp changes the binary that a clean tag
     build would produce.
  3. *Detection:* Every nightly cold audit rebuilds `--release` from an empty directory with `SOURCE_DATE_EPOCH=1` and
     compares the resulting ELF SHA-256 against the promoted `velnor-store` artifact; `velnor-ci release publish`
     verifies `SHA256SUMS` and `gh attestation verify` against the exact tagged commit tree before publishing
     (`draft: false`).
  4. *Automatic fallback:* Digest mismatch or missing receipt aborts promotion (never silently swaps an unverified
     binary) and runs a full clean `--release` + qualification build before release.

- **S5. Node L1 read-only stores (`toolchains`, `mise` tools, vendored Cargo crates, pinned RustSec DB):**
  1. *Why the key is complete:* Every store path is content-addressed by upstream cryptographic digest (`sha256` of
     toolchain manifest, `mise` binary/tool tarball, `Cargo.lock` `.crate` checksums, and
     `.velnor/rustsec-commit.lock` git tree OID) and mounted read-only (`MS_RDONLY | MS_NOSUID | MS_NODEV`) from
     root-owned XFS directories (`uid 0`, sandbox runs as unprivileged `uid 10000+`).
  2. *Failure mode if incomplete:* Bit-rot or partial write during node provisioning corrupts a tool or `.crate`.
  3. *Detection:* Provisioner writes to temp dir, verifies full tree `blake3`/`sha256`, `fsync`s, and atomically
     renames; hourly background scrub re-hashes L1 store manifests against `.velnor/version-policy.toml`.
  4. *Automatic fallback:* Scrub hash mismatch quarantines the node in `velnor-dispatch` (`drain` state) and
     re-provisions from upstream/site L2 before accepting jobs.

## 7. Scheduling and the cluster

### Topology, site autonomy, and placement

The cluster comprises **5 dedicated AMD EPYC 9454P/9454 workers (480 SMT threads, 627.5 GiB RAM)** across two Hetzner
regions linked by a `35 ms` RTT `wireguard-native` mesh (`evidence.md:440-500,577`), plus `velnor-237` (i5-13500, 20T,
62.6 GiB, `NoSchedule` control plane in DE):
- **Site FI (`288` threads, 60% capacity):** `velnor-241`, `velnor-5`, `velnor-190` (`96` threads, `125.5 GiB` RAM,
  `2 x 3.84 TB` NVMe XFS each).
- **Site DE (`192` threads, 40% capacity):** `velnor-240` (`49.9 TB` XFS), `velnor-105` (`34.6 TB` XFS), `96` threads
  and `125.5 GiB` RAM each.

**Removing the single-server SPOF:** Today `velnor-237` is a single k3s server (`evidence.md:535`); if it reboots, pod
creation halts in both sites. In our design, k3s expands to a 3-server embedded-etcd quorum (`velnor-237` + small
control slices or 2 cheap 4-vCPU Hetzner nodes across DE and FI), **and** `velnor-node` creates Linux namespace/cgroup
sandboxes directly via kernel syscalls without calling `kube-apiserver` on the job path. Each site runs an independent
active/standby pair of `velnor-hooks`, `velnor-dispatch`, `velnor-ledger`, and `velnor-store`.

**Placement formula (zero cross-site chatter):** Every `ensure(tree)` request is pinned to one site (`60%` FI, `40%`
DE by weighted capacity; spilling whole requests across the `35 ms` link only when local predicted wait exceeds
`2.0 s`). Within a site, `velnor-ledger` assigns each obligation to worker $w$ minimising:
`Finish(w) = T_queue(w) + T_layer_sync(w) + T_compile_est(w) + T_test_est(w)` where `T_layer_sync(w) = 0 s` when
worker $w$ already holds the current `main` `overlayfs` lowerdir (`>99%` of runs, since `main` delta layers are
`50 to 200 MiB` zstd and replicate in `0.7 s`), and `0.7 s` otherwise.

### Zero queue, zero cold start, and saturating all 480 threads

- **Zen 4 CCD-aligned elastic cgroup v2 slots:** Each EPYC 9454P has 8 Core Complex Dies (CCDs) of 6 physical cores /
  12 threads sharing a 32 MiB L3 cache. On each `125.5 GiB` worker, reserving `6 GiB` (`3 CPU`) for host/daemons and
  `2 GiB` eviction margin leaves `117.5 GiB` and `93` threads:
  - **4 primary build/test slots** of `24` threads (`2` adjacent CCDs = `64 MiB` L3) and `24 GiB` hard `memory.max`
    (`96 GiB` total; peak observed job RSS is `16.8 GiB`, `evidence.md:525`, and `overlayfs` shares the `14.3 GB`
    `target/` page cache across all 4 slots).
  - **8 gate/witness micro-slots** of `1` thread and `1.5 GiB` RAM (`12 GiB` total).
  - **Elastic burst across all 480 threads:** When fewer than 4 build slots are active on a worker, `velnor-node`
    dynamically expands the active sandboxes' `cpuset.cpus` and GNU Make `jobserver` pipe tokens across all idle
    non-reserved CCDs (up to `88` threads on one node!), reclaiming tokens at `rustc` invocation boundaries in
    `<10 ms` when another job arrives. Combined with LPT test sharding (`Section 6`) and parallel root obligations, a
    single multi-root `merge_group` or cold rebuild fans out across **all 288 threads in FI (or all 480 threads across
    FI + DE for independent root closures)**.
- **Zero queue (`p90 <= 0.5 s` from eligibility):** Each site reserves `1` 24-thread build slot and `2` gate slots
  (`52 / 480 = 10.8%` of cluster threads) exclusively for `velnor-merge` and `velnor-gate`. Preemptible `velnor-batch`
  work (speculation, `main` warmer, cold audits) borrows idle capacity at `cpu.weight = 10` (vs `10000` for
  `velnor-merge` and `1000` for `velnor-pr`) and is killed in `<20 ms` via `cgroup.kill` if `memory.current` threatens
  foreground headroom. With on-demand `velnor:verify` + receipts cutting compute per PR by `97.7%`
  (`347 -> 8.0 runner-min`), steady-state `velnor-new` utilisation sits under `8%`, keeping queue wait at **`0.0 s`
  P50 / `< 0.5 s` P90**.
- **Zero cold start:** Pre-forked zygote namespaces (`30 ms`), resident `overlayfs` lowerdirs (`5 ms`), read-only L1
  stores (`10 ms`), and webhook pre-start (`1.2 s`) put `rustc` on CPU before GitHub even offers the Actions job
  (`8 s`).

### Isolation of trusted buggy code (Decision 8) and multi-org tenancy

- **Trusted-author sandbox boundary (no fork PRs):** Fork PRs are closed/refused at `velnor-hooks` (Decision 8).
  Isolation protects against accidental bugs (fork bombs, stray writes, port/IPC clashes, network leaks, cache
  poisoning):
  1. Unprivileged userns (`uid 10000` mapped to unique per-slot host UID range `100000 + slot * 65536`), `CLONE_NEWNS`
     + `pivot_root` exposing **only** declared input trees and read-only L1 stores
     (`MS_RDONLY | MS_NOSUID | MS_NODEV`), private XFS `overlayfs` `upperdir` capped at `25 GiB` via XFS project quota
     (`prjquota`), `CLONE_NEWPID` (`pids.max = 8192`), `CLONE_NEWIPC`, `CLONE_NEWNET` (`lo` only during `build` and
     `test`), `PR_SET_NO_NEW_PRIVS`, all Linux capabilities dropped, and `RuntimeDefault` seccomp.
  2. **Correctness boundary:** Sandbox processes hold **zero** GitHub tokens, runner OAuth keys, or Ed25519 receipt
     keys, and have **zero** write path to `/store` or `velnor-ledger`. Only `velnor-node` (outside the sandbox)
     harvests outputs into `candidate` scope or signs receipts after verifying process exit `0` and full expected
     nextest test counts.
  3. **Docker-needing tests (`34` tests in `local_pair_public` and `local_host_public`):** Privileged `docker:dind`
     sidecars (`57` probe failures/hr, `evidence.md:600`) are **eliminated**. Those 34 tests execute inside
     pre-snapshotted **Firecracker KVM microVMs** (`80 ms` snapshot restore from `/store/vm-snapshots/docker-ready` on
     `/dev/kvm`, 8 vCPU / 16 GiB RAM, CoW rootfs) running an isolated in-guest `dockerd` pre-loaded with base images,
     torn down on exit.
- **Sharing nodes with `ChainArgos` and `jackin-project`:** Both orgs stay in isolated k8s namespaces
  (`arc-runners-chainargos` max 60, `arc-runners-jackin` max 40, `evidence.md:471-478`) under top-level
  systemd/kubelet cgroup slices enforcing guaranteed shares (`tailrocks 60% = 288T`, `ChainArgos 25% = 120T`,
  `jackin 15% = 72T`, work-conserving burst via `cpu.weight`, hard RAM caps `68 GiB / 28 GiB / 18 GiB` per worker) and
  separate XFS project-quota trees (`FI` 3.84 TB disks: `tailrocks <= 1.4 TB`, `ChainArgos <= 600 GB`,
  `jackin <= 400 GB`). Zero caches, mirrors, or credentials cross orgs.

### Host and kernel tuning (`Debian 13`, Linux `6.12.94`)

1. **Storage & kubelet:** Relocate all sandbox and pod `emptyDir`/ephemeral storage from unmirrored `ext4` OS NVMe
   (`/dev/nvme0n1p3`) onto `/var/lib/rancher` (`XFS`, `noatime`, `reflink=1`, `prjquota`). Replace `evictionHard: 5%`
   (which strands `2.5 TB` on `50 TB` `velnor-240`) with fixed
   `imagefs.available: 150Gi, nodefs.available: 50Gi, memory.available: 2Gi`, `kubeReserved: {cpu: 1, memory: 2Gi}`,
   `systemReserved: {cpu: 2, memory: 4Gi}`, and `podPidsLimit: 16384`.
2. **CPU, VM, & network sysctls:** Set `amd_pstate=active` + `energy_performance_preference=performance` (`+4 to 8%`
   Zen 4 boost clock on serial rustc/rust-lld steps), `transparent_hugepage/enabled = madvise`,
   `vm.dirty_background_ratio = 5`, `vm.dirty_ratio = 20`, `vm.max_map_count = 1048576`, and
   `net.core.rmem_max = wmem_max = 67108864` with GRO/GSO verified on `flannel-wg`.

## 8. The merge queue and GitHub integration

### On-demand PR trigger flow (`velnor:verify`) & closing the `skipped` false-green hole

**Critical GitHub platform trap discovered and closed:** Under GitHub Repository Rulesets
(`Troubleshooting required status checks`), if a workflow triggers on `pull_request: [synchronize]` and a required job
uses `if:` to skip when `velnor:verify` is absent, GitHub sets that job's check run to `conclusion: "skipped"`, which
**satisfies `required_status_checks` and allows an unverified PR to merge (a catastrophic false green violating
Decision 11 and 12)!** Conversely, failing the job on draft pushes turns every WIP commit red and spams failure
notifications.

**Exact solution:** The repository ruleset on `refs/heads/main` requires **two** bound checks:
1. `Required` bound to **GitHub Actions (`integration_id: 15368`)**, which satisfies `velnor-ci release-gate`
   (`ci-helper/crates/velnor-ci/src/gate.rs:15,277-288`) and `product-release.yml:86`.
2. `velnor/verdict` bound to **Velnor GitHub App (`integration_id: <velnor_app_id>`)**, which is the cryptographic
   gate enforced on every commit SHA.

Workflow `.github/workflows/ci.yml` sets:
```yaml
on:
  pull_request:
    types: [labeled, synchronize]
  merge_group:
    types: [checks_requested]
  push:
    branches: [main]
  workflow_dispatch:
    inputs:
      pr_number: { required: false, type: string }
```
With job condition on `Required`:
`if: github.event_name != 'pull_request' || (github.event.action == 'labeled' && github.event.label.name == 'velnor:verify') || (github.event.action == 'synchronize' && contains(github.event.pull_request.labels.*.name, 'velnor:verify'))`

**Exact state-by-state behaviour matching all 5 binding rules of Decision 12:**
1. **Draft PR opened or receiving pushes:** `opened` is not in `ci.yml`; on `synchronize` without `velnor:verify`,
   `ci.yml` dispatches **0 runner jobs** (`Required` skips on GitHub's server in `0 ms`), while `velnor-hooks`
   receives `pull_request.{opened,synchronize}` and posts `POST /repos/{o}/{r}/check-runs`
   `{name: "velnor/verdict", head_sha, status: "queued", output: {title: "Not requested. Add label velnor:verify"}}`
   (no `conclusion`!). Ruleset sees `velnor/verdict` as **pending/incomplete (`queued`)** -> **blocks merge and
   merge-queue entry, runs 0 jobs, sends 0 failure emails.** (Even if `velnor-hooks` were unreachable,
   `velnor/verdict` is absent = `Expected`, which **also blocks**!)
2. **Marking PR ready for review (`ready_for_review`):** Ignored by both `ci.yml` and `velnor-hooks`. **Runs nothing;
   PR remains blocked (`queued`).** Never auto-adds `velnor:verify`.
3. **Adding `velnor:verify` (`pull_request.labeled` where `label.name == 'velnor:verify'`):** `velnor-hooks`
   transitions `velnor/verdict` to `in_progress` and immediately starts `ensure(merge_tree(base, head_sha))`
   (`1.2 s`); in parallel, `ci.yml` starts the single `Required` job on `velnor-gate`, which joins `ensure` and calls
   `completejob`. Both `Required` and `velnor/verdict` turn `success` ONLY if every selected obligation for `head_sha`
   passed. (Adding any *other* label does not match `label.name == 'velnor:verify'`, running nothing.)
4. **New push while `velnor:verify` is present (`synchronize`):** New `head_sha` initially has **no** check runs
   (`Expected` = blocking, instantly invalidating the prior commit's green!). `velnor-hooks` cancels the previous
   head's `ensure` lease in `<50 ms`, sends `POST /actions/runs/{old_run_id}/cancel` (`velnor-exec` kills the old
   cluster job in `<0.5 s`, giving **zero stall!**), sets `velnor/verdict` `in_progress` on the new `head_sha`, and
   starts `ensure` reusing all unchanged package receipts from Push N.
5. **Removing `velnor:verify` (`pull_request.unlabeled` where `label.name == 'velnor:verify'`):** `velnor-hooks`
   immediately cancels any in-flight `ensure` and Actions run for that PR, sets any in-progress `velnor/verdict` to
   `conclusion: "cancelled"` (blocking), and subsequent pushes hit rule (1) (`0` jobs).

### How label events reach Velnor (`GITHUB_TOKEN` vs App webhook + fallbacks)

Per GitHub's official documentation (*Triggering a workflow from a workflow*,
`docs.github.com/en/actions/writing-workflows/choosing-when-your-workflow-runs/triggering-a-workflow#triggering-a-workflow-from-a-workflow`,
and *Webhook events and payloads*, `docs.github.com/en/webhooks/webhook-events-and-payloads#pull_request`):
- **GitHub Actions suppression:** Events caused by `GITHUB_TOKEN` (installation token of `github-actions[bot]`, App ID
  `15368`) **do NOT trigger** `on: pull_request: types: [labeled]` workflows, *except* `workflow_dispatch` and
  `repository_dispatch`.
- **GitHub App webhook delivery:** External GitHub Apps subscribed to `Pull requests` (`velnor-hooks`) **DO receive**
  the `pull_request` (`action: "labeled" | "unlabeled"`) HTTP `POST` webhook even when the actor used `GITHUB_TOKEN`
  (because GitHub's loop-suppression filter sits inside the Actions workflow trigger router, not the App webhook
  dispatcher).
- **Normal path:** Humans (UI) and agents (`gh pr edit --add-label velnor:verify` via OAuth/PAT or Velnor's own App
  token) trigger **both** `velnor-hooks` and `ci.yml` natively.
- **Fallback A (`GITHUB_TOKEN` added the label):** When `velnor-hooks` receives `pull_request.labeled`
  (`velnor:verify`) and detects `sender.login == "github-actions[bot]"` (or sees 0 matching `ci.yml` runs on
  `head_sha` after `8 s` via `GET /repos/{o}/{r}/actions/runs?head_sha={sha}`), `velnor-hooks` removes and re-applies
  `velnor:verify` (or calls `workflow_dispatch` on `head.ref`) authenticated as the **Velnor GitHub App**, firing
  `Required (15368)` cleanly.
- **Fallback B (Missed App webhook):** Every `30 s`, `velnor-hooks` replays failed deliveries from
  `GET /app/hook/deliveries` and reconciles `GET /repos/{o}/{r}/pulls?state=open` for any `velnor:verify` PR whose
  `head_sha` lacks an active/completed `velnor/verdict`.

### Exact Repository Ruleset configuration (`refs/heads/main`)

Configured via `PUT /repos/tailrocks/velnor-new/rulesets/24396608` (`docs.github.com/en/rest/repos/rules`), enforced
by GitHub on both PR merge/enqueue and `merge_group` commits:
```json
{
  "name": "main-protection",
  "target": "branch",
  "enforcement": "active",
  "conditions": { "ref_name": { "include": ["refs/heads/main"], "exclude": [] } },
  "rules": [
    {
      "type": "required_status_checks",
      "parameters": {
        "do_not_enforce_on_create": false,
        "strict_required_status_checks_policy": false,
        "required_status_checks": [
          { "context": "Required", "integration_id": 15368 },
          { "context": "velnor/verdict", "integration_id": 9900001 }
        ]
      }
    },
    {
      "type": "merge_queue",
      "parameters": {
        "merge_method": "SQUASH",
        "grouping_strategy": "ALLGREEN",
        "max_entries_to_build": 6,
        "max_entries_to_merge": 6,
        "min_entries_to_merge": 1,
        "min_entries_to_merge_wait_minutes": 0,
        "check_response_timeout_minutes": 15
      }
    }
  ]
}
```
Setting `strict_required_status_checks_policy: false` stops wasteful catch-up merges of `main` into PR branches while
`merge_queue` (`ALLGREEN`) guarantees every landed commit passes against the exact tip of `main`.

### Zero duplicate verification, `2k - 1` queue speculation, `Required` / `release-gate`, auto-merge, and zero cancel stall

1. **PR -> `merge_group` -> `main` (verify once, lookup everywhere):** `merge_group` always lists 100% of workspace
   obligations against `T_mg` (`docs/hard-decisions.md` rule 3). When `T_mg == T_pr` (`89%` of merges at `<2 min`
   verdict window `[EV: v2:719]`), 100% of obligations hit signed receipts in `<50 ms` (plus any parallel
   `release-artifact` tarball if not yet built). When `main` advanced, **only packages whose closure tree changed in
   `T_mg` vs `T_pr` execute**; untouched roots hit `main` or `T_pr` receipts. When `main` receives the `push` event on
   `T_mg`, 100% of obligations hit `T_mg`'s receipts in `<50 ms`, `Required (15368)` completes green in `<1 s`
   (satisfying `velnor-ci release-gate` and `product-release.yml:86` with zero code runs!), and `velnor-ledger`
   promotes `T_mg`'s receipts and release artifacts to `trusted`.
2. **Speculation on the queue tip (`2k - 1` trees):** `63%` (`122/193`) of real `merge_group` runs built on an
   unlanded predecessor `[EV: F-H04]`. Whenever PRs $P_1 \dots P_k$ ($k \le 6$) sit in the queue (or have
   `velnor:verify` + auto-merge ready to enqueue), `velnor-ledger` computes with `git merge-tree --write-tree` in
   `<10 ms` both the **cumulative prefix trees** $T_i = T_{i-1} \oplus P_i$ and the **single-ejection bypass trees**
   $T'_i = T_{i-2} \oplus P_i$ ($2k - 1 = 11$ trees max for $k = 6$) and runs missing obligations at preemptible
   `velnor-batch` priority during GitHub's `19 s` `merge_group` creation lag. Under `ALLGREEN`, even if $P_1$ fails
   and is ejected, $P_2$'s replacement `merge_group` tree $T'_2 = \text{main} \oplus P_2$ is **already verified in the
   ledger**!
3. **Agent workflow & zero cancelled-run stall:** Agents open PRs without `velnor:verify`, iterate freely at `0` CI
   cost, and when ready run `velnor verify` (optional pre-push warmup) + `gh pr edit --add-label velnor:verify` +
   `gh pr merge --auto --squash`, and **never** merge `main` into PR branches. Because `Plan`, `Required verdict`, and
   `Required` are collapsed into **one** cluster-executed `Required` job with `<0.5 s` cgroup kill (instead of 3
   hosted `ubuntu-26.04` jobs waiting `23 min` in a 20-slot pool for `if: always()`), the `60 s` P50 / `2,203 s` max
   cancelled-run admission stall (`F-H06`) drops to **`0.0 s`**.

## 9. What would make CI not a bottleneck at all

CI ceases to be a bottleneck when **feedback arrives inside an agent's or human's local thought loop (`<= 5 s`
pre-push) and landing a verified PR takes no longer than GitHub's irreducible state-machine transitions (`< 60 s` with
GitHub Merge Queue, `< 4 s` without it).**

### Irreducible physical and platform lower bounds on `velnor-new`

| Cost component | Physical / protocol limit | Value | Removable? |
|---|---|---|---|
| 1. `git push` pack upload (EU -> GitHub US, `120 ms` RTT) | 3 TLS/pkt-line RTTs + `<20 KB` delta pack | **0.45 s** | Irreducible on GitHub.com (`120 ms` transatlantic RTT `[EV: evidence.md:577]`). |
| 2. Commit tree sync to cluster | `0.00 s` after push if streamed via `velnor verify` thin-pack before/during `git push`; `1.20 s` via App webhook otherwise | **0.00 s** (pre-streamed) / **1.20 s** | Webhook delay overlapped completely by pre-push sync. |
| 3. Closure key graph + `overlayfs` sandbox mount | In-RAM `libgit2`/`gix` tree OID lookup + `mount("overlay")` + `clone3` | **0.025 s** | Kernel syscall + hash floor (`~25 ms`). |
| 4. Incremental leaf Rust compile + link (when not pre-verified) | 1 leaf crate (`rustc` check/codegen + `rust-lld` ELF link) on 4.0 GHz Zen 4 with hot page-cache `.rmeta`/`.rlib` | **1.10 s** (`0.00 s` post-push if pre-verified) | Irreducible single-crate `rustc` + `rust-lld` work (`0 s` after push when overlapped with `git push`!). |
| 5. Affected test binary execution (3x parallel across 288 site threads) | Process fork/exec + in-memory/reflinked test assertions (after SQLite template fix) | **0.40 s** (`0.00 s` post-push if pre-verified) | Overlapped to `0 s` post-push when `velnor verify` ran on the tree! |
| 6. GitHub Actions job offer + `acquirejob` + `completejob` | GitHub workflow engine queue (`~8.0 s` P50) + 2 WAN RTTs (`0.45 s`) | **8.45 s** (`0.25 s` if Check Run API only) | `8.0 s` is GitHub Actions' internal scheduler floor; drops to `0.25 s` (`1` REST `POST /check-runs`) if `Required` is bound solely to `<VELNOR_APP_ID>`. |
| 7. GitHub Merge Queue tax (`enqueue -> merge_group` `19 s` + `green -> merged` `26 s`) | GitHub internal merge-queue state machine (`[EV: v2/decisions.md:142, F-B10]`) | **45.0 s** (+ `8.5 s` MG Actions job = **53.5 s**) | **Irreducible while Decision 7 & 12 mandate GitHub's Merge Queue.** Drops to **`0.50 s`** only if Velnor's own CAS Merge Train fast-forwards `main` via GitHub's Git Database / GraphQL API. |

### The three absolute floors and the architecture that reaches them

1. **Floor A, strictly inside Binding Decisions 7 & 12 (GitHub Merge Queue + Actions `Required` job `15368`):**
   **`~58 s` push-to-merge (`~8.6 s` PR verdict)**. Reached by our **Phase 4 Stretch design**: agent runs
   `velnor verify` (or automatic `pre-push` git hook streaming the tree to `velnor-hooks` `3 s` before `git push`
   finishes), so receipts are already sealed when `Required` is offered at `t = 8.0 s`; `merge_group` hits
   pre-speculated receipts in `0.05 s` and pays only GitHub's `19 s + 8.5 s + 26 s` queue floor.
2. **Floor B, inside GitHub Merge Queue, migrating `velnor-ci release-gate` from `15368` to `<VELNOR_APP_ID>`:**
   **`~47 s` push-to-merge (`~0.7 s` PR verdict on pre-verified push, `~3.2 s` cold leaf push)**. Eliminates both
   `8.0 s` Actions job-offer waits (PR and `merge_group`) by letting `velnor-ledger` post `Required` directly via
   Checks API while `velnor-exec` runs only when YAML custom steps exist.
3. **Floor C, absolute GitHub-platform physical floor (if owner ever replaces GitHub's Merge Queue UI with Velnor's
   atomic CAS Merge Train):** **`1.3 s` push-to-merge (pre-verified) / `3.5 s` (un-preflighted leaf edit)**.
   `velnor-ledger` maintains a single linearisable queue of `T_i = T_{i-1} \oplus P_i`, verifies all obligations +
   `3x` touched tests + `--release` binaries in cluster sandboxes, posts green Check Runs on $P_i$ and $T_i$, and
   calls GraphQL `mergePullRequest(expectedHeadOid: P_i)` / `PATCH /git/refs/heads/main` with compare-and-swap on
   `T_{i-1}`, eliminating GitHub's idle `45 s` merge-queue polling timers with **identical cryptographic C1 to C7
   guarantees**.

## 10. Critique of v2

Reviewed section-by-section against the 12 binding decisions (`rust-only-brief-v1.md:5-20`), upstream `actions-runner`
/ `velnor-legacy` code, and the new Mac/cluster/blaze experiments.

| v2 Section | What v2 gets wrong or misses under the binding decisions |
|---|---|
| **§1 Goal & contract** | **1. Assumes every-push PR CI (`4.77` runs/PR),** violating Decision 12 (`velnor:verify` on-demand). **2. C1 (`lines 57-60`) accepts `HEADGREEN` landing unverified broken intermediate commits** on `main` and merely "listing" them afterwards, violating Decision 11 (zero false-green tolerance). **3. Targets (`90 s / 4 min` verdict, `3 / 8 min` merge, `35` min CPU)** leave `3.5x to 4x` latency and `4.4x` compute on the table. |
| **§3 Architecture & runner** | **1. Runs ARC + GitHub's C# runner in production in Phases 0 and 1 (`lines 324, 370, 827`),** building throwaway ARC capability scale sets (`velnor-merge`/`gate`/`build`/`docker`) despite Decision 1 banning C# runners from production. **2. Keeps fork PRs and macOS builds on GitHub-hosted C# runners (`lines 336, 887`),** violating Decisions 1, 8, and 9. **3. Renews job leases every `60 s` (`lines 246, 354`),** risking lease expiry on short `LockedUntil` windows (`30 to 60 s`); must be `min(25 s, (locked_until - now)/2)`. **4. Copies `velnor-legacy` Results client without noting `blob_url` vs `summary_url` bug (`protocol.rs:9796` vs `Contracts.cs:25`).** **5. Omits `actions/create-github-app-token` from §3.5 (`lines 263-275`),** which breaks `velnor-gen`'s own `release.yml` (`rules.rs:8-21`) under pre-job action refusal! **6. Keeps privileged `dind` pods (`line 334`).** |
| **§4 Verification ledger** | **1. Discards cross-push (`Push N -> N+1`) and stacked-PR reuse (`lines 429-432`),** rebuilding the whole PR delta from `main` on every push instead of reusing `candidate`-scoped package receipts (violating Decision 10a/10b). **2. Defers `CARGO_BIN_EXE_*` executable digest hashing to Phase 4 stretch (`lines 409-410`),** which makes Phase 3 per-package test reuse **unsound** across `171` binary-spawning test sites (`velnor-actions-cli` has no `lib.rs`!). **3. Lacks any `merge_group -> main -> release` binary/tarball promotion ledger,** leaving releases rebuilding from source (violating Decision 10). |
| **§5 Cache system** | **1. Chooses XFS `cp --reflink` over `overlayfs` (`lines 484, 517`) despite `E1d` (`mbx-cache-cluster.md:76-81`) proving reflink clones duplicate `14.29 GB` of Linux page cache per sandbox (`57.2 GB` for 4 jobs, blowing past §7's `7 GiB` cache headroom inside `24 GiB` pods!).** **2. Operates per-site `mbx-cache` (`300 GiB`, TLS, GC, `RUSTC_WRAPPER`)** when `overlayfs` zstd layer replication (`0.7 s` delta / `13 s` cold vs `39.9 s` `mbx-cache`, `67.4 s` plain vs `88.4 s` `mbx` lock edit) wins every metric with zero wrapper bugs. **3. Retains L3 GitHub cache (`lines 562, 590`)** even though zero jobs remain on hosted runners. |
| **§6 Workflow & tests** | **1. Keeps `pull_request: [opened, synchronize, reopened, ready_for_review]`** instead of `velnor:verify`, and **misses GitHub's `conclusion: skipped` false-green trap** when skipping `Required` on `synchronize`, plus `GITHUB_TOKEN` label-trigger suppression! **2. Punts `[profile.ci]` dependency optimisation to Q8 (`line 888`) and misses unifying `--release` binary builds into `merge_group`.** **3. Misses the `6,425`-glob `git`-subprocess test (`non_inputs_differential.rs:198`) and the `356` separate integration test binaries.** |
| **§7 & §8 K8s & tuning** | **Per-obligation K8s Pod creation (`6 s` creation, `kube-apiserver` dependency, no CCD pinning)** caps job start at `1 to 2 s` and stalls on `velnor-237` control-plane blips; `velnor-node` cgroup v2 + namespaces start in `30 ms` with 2-CCD (`24T`) L3 locality and elastic 480-thread `jobserver` burst. |
| **§9 to §13 Phases & questions** | **1. Conformance release polling is `24 h` (`line 310`) with no hourly live-SaaS canary** to catch unannounced GitHub Twirp/Broker drift *within hours*. **2. Leaves 11 open owner questions (`§13 Q2 to Q12`),** including Q12 (retrying failed untouched tests to turn red into green), which risks masking real cross-cutting regressions if a key had an undeclared read. |

### Specific `v2` decisions overturned (`decisions.md`)

1. **Overturn `N54` / `I035` / `I036` (XFS reflink hot path + `mbx-cache` cross-node server):** Replace per-job
   `cp --reflink` with multi-lower `overlayfs` (keeping `14.3 GB` page cache shared once per worker; using XFS reflink
   only for 8-layer background compaction) and replace `mbx`/`mbx-cache` with `velnor-store` zstd `overlayfs` layer
   streaming.
2. **Overturn `N64`'s ARC Phase 0 to 1 staging & `I079` (ARC capability scale sets):** Do not build throwaway ARC
   scale sets or run `velnor-ci witness` on GitHub's C# runner; build `velnor-dispatch` + `velnor-exec` first against
   `velnor-lab` and cut production straight from current state to pure-Rust Velnor.
3. **Overturn `N65` / `I091` / `N11` (hosted runners for forks and macOS):** Drop all fork handling (Decision 8) and
   disable macOS release workflows in Phase 0 (Decision 9); `0` hosted jobs remain.
4. **Overturn `I074` / `N33` (every-push PR CI):** Implement strict `velnor:verify` label-driven CI with dual
   `Required (15368)` + `velnor/verdict (<VELNOR_APP_ID>)` ruleset protection (Decision 12).
5. **Overturn `I032` / `§4.2` (no cross-push or stacked-PR reuse):** Enable `candidate`-scoped per-package receipt and
   `upperdir` harvest reuse across PR pushes, concurrent PRs, stacked PRs, and `velnor verify` pre-push, gated from
   `trusted` promotion until `merge_group` passes.
6. **Overturn `N02` deferral (waiting until Phase 4 to hash `CARGO_BIN_EXE_*`):** Include all reachable `[[bin]]` and
   `--example` target digests in `ObligationKey` from day 1 of package reuse.
7. **Overturn `I082` / `I084` (K8s Pod per obligation + privileged `dind`):** Use `velnor-node` Linux namespace +
   CCD-aligned cgroup v2 sandboxes (`30 ms`) and Firecracker KVM snapshot restore (`80 ms`) for the 34 Docker tests.
8. **Overturn `N28` (`HEADGREEN` default) & `Q12` (silent retry of failed tests):** Enforce `ALLGREEN` backed by
   `2k - 1` bypass speculation, and never auto-retry a failed test into a silent green without owner-visible
   quarantine.

## 11. Phases

Every phase ships behind a measurable exit gate over `>= 100` real runs (or full 420-case oracle suite) before
advancing. From Phase 1 onward, GitHub's official C# runner never runs in production; rollback switches to the pinned
`N - 1` Velnor Rust release (`< 30 s`).

| Phase (Timeline) | Core deliverables | PR verdict P50 / P90 | Push-to-merge P50 / P90 | Runner-min / merged PR | Cache / receipt hit % | Conformance pass % |
|---|---|---|---|---|---|---|
| **Phase 0** (Days 1 to 4) | Pause macOS workflows; `velnor-hooks` + `velnor:verify` label flow + dual ruleset (`velnor/verdict` + `Required`, `min_wait=0`, auto-merge); `velnor-conformance` 420-case oracle live in `velnor-lab`; SQLite `Journal` template (`<50 ms`), test-loop splits, 356->30 test-binary consolidation, `--profile ci` (`opt-level=2` deps); XFS quotas + 3-node k3s etcd HA. | <= 6.0 m / <= 12.0 m | <= 15 m / <= 35 m | <= 95 min (was 347) | 0% cross-job (prep) | >= 95.0% in `velnor-lab` |
| **Phase 1** (Weeks 1 to 2) | **Pure-Rust cutover:** `velnor-{proto,expr,dispatch,exec,sandbox,native-actions}` (all 9 native actions incl. `create-github-app-token` & `attest*`), pre-warmed 30 ms namespace/cgroup slots, `overlayfs` warm `main` snapshots + `velnor-store` zstd layers, Firecracker Docker microVMs, L1 read-only tool/crate/git stores. **Delete `tailrocks` ARC & C# runners.** | <= 90 s / <= 3.0 m | <= 4.0 m / <= 8.0 m | <= 28 min | >= 92% crate compile hit | **100.0%** (420/420 x2 nights + 200 shadows) |
| **Phase 2** (Weeks 3 to 4) | Collapse `ci.yml` / `velnor-gen` to single `Required` witness job + `velnor-ledger` `ensure(tree)`; per-package closure receipts (`CARGO_BIN_EXE_*` + `pivot_root` sparse enforcement) across pushes, concurrent & stacked PRs, `merge_group`, and `main`; 6-way `velnor-runner-host` split + `velnor-runner-local`; parallel `merge_group` `--release` tarball build + zero-rebuild `release-plz` promotion; delete `state-seal.yml`, `cache-prune.yml`, & GH cache (`0 GB`). | <= 40 s / <= 90 s | <= 110 s / <= 3.5 m | <= 12 min | >= 96% compile; >= 80% MG receipt hit | **100.0%** + hourly live canary |
| **Phase 3 (Committed)** (Weeks 5 to 6) | `velnor-hooks` 1.2 s webhook pre-execution (overlapping GitHub's 8 s job offer); `2k - 1` cumulative + bypass queue speculation (`max_entries_to_build: 6`, `ALLGREEN`); 3x parallel touched-test execution; 5% live sampler + 100% post-merge cold audit + `POLICY_EPOCH` auto-fallback. | **<= 25 s / <= 55 s** | **<= 75 s / <= 150 s** | **<= 8.0 min** (`0.5 min` / release) | **>= 98% compile; >= 89% MG receipt hit** | **100.0%** (`<= 1 h` drift alert) |
| **Phase 4 (Stretch)** (Weeks 7 to 8) | `velnor verify` `<200 KB` thin-pack pre-push sandbox execution + site-wide 288/480-thread LPT test-case sharding across workers via `velnor-store`. | **<= 12 s / <= 25 s** | **<= 60 s / <= 105 s** | **<= 3.5 min** | **>= 99% compile; >= 93% MG receipt hit** | **100.0%** |

### The first 5 work items (marked `[W1]` to `[W5]`, starting in parallel on Day 1)

- **`[W1]` Pause macOS workflows & wire `velnor:verify` on-demand gating (`velnor-hooks` + Ruleset):** Disable macOS
  jobs in `product-release-{binary,generator}.yml`, `release-rust-ci-tools.yml`, and `qualification.yml` (Decision 9);
  deploy `velnor-hooks` posting `velnor/verdict` (`queued` when unlabelled), update `ci.yml` triggers (`labeled`,
  `synchronize`, `merge_group`, `push`, no `edited`/`opened`/`ready_for_review`), add `GITHUB_TOKEN` App re-dispatch
  fallback, and update Ruleset `24396608` (`velnor/verdict` + `Required`, `min_entries_to_merge_wait_minutes: 0`).
  *Gate:* Draft/unlabelled pushes run 0 jobs and stay blocked (`queued`); labelled push starts immediately; removing
  label cancels in `<1 s`.
- **`[W2]` Stand up `velnor-conformance` (420-case differential oracle) in `velnor-lab`:** Build the pairing
  dispatcher, log/timeline/cache/artifact/attestation normaliser, 10 planted-fault self-tests, 30-min upstream
  `actions/runner` release poller, and 60-min live-SaaS canary. *Gate:* Catches 10/10 planted faults on `velnor-lab`.
- **`[W3]` Build `velnor-proto`, `velnor-expr`, `velnor-dispatch`, `velnor-exec`, & `velnor-native-actions`:** Copy
  and adapt `velnor-legacy`'s `expression/*`, `scaleset/*`, modern V2 `protocol.rs` (fixing `summary_url` at `:9796`
  and removing all `_apis/distributedtask` + AES + Cache v1 code), `workflow_command.rs`, `command_files.rs`,
  `redaction.rs`, and `execution/*` state machines; implement all 9 closed actions (including
  `create-github-app-token` and Sigstore `attest*`) + pre-job refusal + `min(25 s, (locked_until - now)/2)` renewal +
  SQLite WAL lease journal. *Gate:* Passes `420/420` (`100.0%`) in `velnor-conformance`.
- **`[W4]` Provision XFS `overlayfs` node stores, cgroup v2 sandboxes, & 3-node k3s HA (`velnor-node` +
  `velnor-sandbox` + `velnor-store`):** Configure XFS `prjquota` on `/var/lib/rancher`, fixed-GiB kubelet
  `evictionHard`, 3-server k3s etcd quorum, pre-populated L1 stores (`toolchains`, `mise`, vendored crates, bare Git
  mirror, Firecracker Docker VM snapshot), warm `main` `overlayfs` lowerdirs + zstd site replication, and prove
  backdated-mtime + cold-vs-warm reproducibility. *Gate:* Sandbox starts in `<50 ms`, unchanged build compiles `0`
  crates in `<0.4 s`, 4 concurrent sandboxes share `14.3 GB` page cache without eviction.
- **`[W5]` Break the test and compile floors in `velnor-new`:** Land `--profile ci` (`opt-level = 2` deps with
  `debug-assertions = true`), pre-migrated SQLite `Journal` reflink fixture (`<50 ms`), 1-test-per-case splits (`40x`,
  `12x`, `6x`), `gix` rewrite of `non_inputs_differential.rs`, removal of `threads-required` / `test-threads = 1` /
  `build.jobs = 2`, 356 -> 30 integration test binary consolidation, and 6-way split of `velnor-runner-host` +
  `velnor-runner-local`. *Gate:* Zero tests `> 15 s` solo; `ci-helper` suite `<= 4 s`; `velnor-runner` edit-compile
  `<= 20 s`.

## 12. Risks

| Top risk | How it is detected | Automatic containment & resolution |
|---|---|---|
| **1. False green from an incomplete `ObligationKey` or `CARGO_BIN_EXE_*` / undeclared file read** | `pivot_root` sparse sandbox turns undeclared repo reads into immediate `ENOENT` test failures; 100% shadow re-execution for first 3,000 hits per class; 5% ongoing cross-node re-execution sample; 100% cold uncached re-verification of every landed `main` commit within 6 h. | Single confirmed mismatch (`>=2/5` reruns fail) atomically increments `POLICY_EPOCH` in `velnor-ledger`, disabling receipt reuse for that class (`100%` live runs), pausing `merge_queue` entries using old-epoch receipts, and filing a P0 issue listing affected `main` commits for revert. |
| **2. Stale `overlayfs` compile cache or mtime skew masks a broken build** | Work item `[W4]` backdated-mtime gate; nightly + every-20th-merge cold differential build comparing `rustc` exit status, test verdicts, and ELF `blake3` digests against warm `overlayfs` builds. | Any divergence bumps `SNAPSHOT_EPOCH`, unmounts warm `overlayfs` lowerdirs across all workers (falling back to clean builds on 480 threads in `~45 s`), and rebuilds a clean `main` base layer. |
| **3. Unannounced GitHub SaaS protocol drift (Broker V2, Run Service, Results Twirp, Cache v2, Artifacts v4)** | **60-minute** `velnor-conformance --canary` probing all wire RPCs side-by-side against `actions/runner` + shadow `deny_unknown_fields` schema telemetry; **30-minute** `actions/runner` release poller running all 420 differential cases within 2 h of any upstream release. | Schema warning pages immediately while tolerant prod parser continues; hard RPC failure automatically switches `velnor-dispatch` to the pinned `N - 1` `velnor-exec` binary (or ships hotfix inside hours, backed by GitHub's 63-day runner protocol window). |
| **4. `conclusion: skipped` or `GITHUB_TOKEN` suppression breaks `velnor:verify` gating** | Continuous conformance check testing all 5 `velnor:verify` transitions (`draft push`, `ready_for_review`, `labeled` by user & `GITHUB_TOKEN`, `synchronize`, `unlabeled`) + ruleset drift check every 10 min via `velnor-ci check-rulesets`. | Dual ruleset (`velnor/verdict` `queued` + `Required`) fails closed (`Expected` = blocking) even if `velnor-hooks` crashes; `velnor-hooks` 8 s watchdog + 30 s `/pulls` poller re-dispatches suppressed `GITHUB_TOKEN` label events via App token. |
| **5. `velnor-exec` regression or crash mid-job strands locks or double-completes** | 420-case blocking differential gate on every runner PR + kill-9 fault-injection matrix across every journal state; Prometheus alert on `InfrastructureFailure` rate `> 0.5%` or lease-renewal errors. | New runner versions canary on `10%` of workers for 24 h; automatic rollback to `N - 1` Velnor binary on `>1%` infra error; SQLite WAL `intent -> call -> outcome` journal + `min(25 s, (locked_until-now)/2)` renewal prevents orphaned locks and duplicate `completejob`. |
| **6. Speculative `ALLGREEN` bisection storm or noisy-neighbour starvation from `ChainArgos` / `jackin`** | Queue-wait SLO (`velnor-merge` eligible-to-start `p90 > 1.0 s`), per-sandbox PSI (`memory.pressure`, `cpu.pressure`), and paired-rerun flake rate (`> 1.0%`). | Hard org cgroup caps (`60% / 25% / 15%`), `10.8%` dedicated `velnor-merge` thread reservation, `<20 ms` `cgroup.kill` preemption of `velnor-batch` speculation, and `2k - 1` pre-verified bypass trees keeping `ALLGREEN` ejections `0 s`. |
| **7. Disk exhaustion on FI workers (`3.84 TB` NVMe) or site / control-plane outage** | Disk usage alerts at `65%`, XFS per-org/per-sandbox `prjquota` hard limits, and cross-site health probes every `5 s`. | Automatic LRU eviction keeping last 3 `main` `overlayfs` generations; 3-node k3s etcd quorum + site-autonomous `velnor-{hooks,dispatch,ledger,store}` pairs allow either FI (`288T`) or DE (`192T`) to carry 100% of load solo if the other site goes dark. |
