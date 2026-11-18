# Review by Gemini 4 Carbon

| Field | Value |
|---|---|
| Model | Gemini 4 Carbon |
| Model id | `gemini-4-carbon` |
| Client | Antigravity CLI 1.5.9 (agy -p) |
| Reasoning effort | high |
| Date / run | 2026-11-19 |
| Brief | v1 review brief (2026-11-19): critique 3 anonymised proposals; same evidence pack |

This is the model's own cross-critique of anonymised proposals, unedited except for formatting. The key at the end maps the letters to models; the reviewer did not see it.

---

# Velnor runner research: cross-critique

Your goal is to evaluate Proposals G, E, and F against the evidence pack and repository source code so the final Velnor architecture
reaches minimum push-to-merge time without losing verification confidence. Three principles read this session shaped this critique.
`Prove It Works` drove checking every cited file, line number, C# method, and run log directly in the repositories instead of trusting
`[M]` tags. `Explain the Number` exposed invalid denominators and parallelism math in `mbx` hit rates, sharding estimates, and memory
budgets. `Attack the Premise` exposed that all three proposals build compiler caching around `mbx` even though `velnor-new` already tested
nine `mbx` variants in issue `#585` and dropped `mbx` in `velnor-gen`.

## Proposal G

### 1. Weakest assumptions

1. **One shared `MBX_CACHE_DIR` per node and trust scope with per-PR `overlayfs` mounts ([proposal-G.md:219-231](file:///Users/donbeave/.cache/ci-ambition/panel/gemini-4-carbon/review-input/proposal-G.md#L219-L231)).**
   Proposal G cites [velnor-legacy/crates/velnor-runner/src/mbx_store.rs:14-19](file:///Users/donbeave/.cache/ci-ambition/refs/velnor-legacy/crates/velnor-runner/src/mbx_store.rs#L14-L19) for the 144 GiB of dead trees (line 198), yet misses lines 6 to 13 directly above.
   `velnor-legacy` abandoned a shared store-root `MBX_CACHE_DIR` for per-slot directories because `mbx` serializes on a registrar `flock`
   and Cargo locks the target tree. Mounting per-PR `overlayfs` views over a `trusted` lowerdir while `main`, `merge_group`, and background
   warmers mutate that lowerdir (line 298) triggers undefined Linux overlayfs behavior (`ESTALE` and corrupt dentries), isolates copied-up
   `flock` files so `mbx`'s node-wide CPU scheduler (line 310) stops coordinating across PRs, and breaks XFS `FICLONE` reflinks across mount
   boundaries (`EXDEV`). Proposal G also misses issue `#585` (`comment 6856187529`), where all nine `mbx` settings recompiled 10 to 68
   crates on zero-change warm runs while plain Cargo with `velnor-ci inputs restore` compiled zero. Running eight concurrent PR slot builds
   over `overlayfs` while a warmer writes the lowerdir, compared against per-job XFS reflinked `target/` directories, settles this.
2. **Landlock file sandboxing makes a >= 90% test-verdict cache sound ([proposal-G.md:250-252](file:///Users/donbeave/.cache/ci-ambition/panel/gemini-4-carbon/review-input/proposal-G.md#L250-L252)).**
   On Linux 6.12 (`landlock(7)` CAVEATS), Landlock cannot restrict or observe path-metadata syscalls (`stat(2)`, `access(2)`, `chdir(2)`,
   `readlink(2)`) and has no recording mode to emit input declarations. Tests that branch on file existence, modes, or symlinks (such as
   `velnor-new`'s `non_input_present` and provenance checks) bypass Landlock unless isolated in a sparse mount namespace. Worse, on the 24%
   of PRs touching `velnor-runner-host` and 14% touching root engine crates (lines 260 to 261), Cargo re-links all downstream test binaries
   with new digests (0% verdict hit rate), while Docker suites are excluded from verdict caching (line 251). On PRs that do not touch those
   crates, package selection already skips them without a verdict cache. Replaying 200 merged PR diffs to count how often an affected
   non-Docker test binary keeps an identical SHA-256, and testing Landlock against `std::path::Path::exists`, settles this.
3. **L0 mounts trust-scoped caches via `ACTIONS_RUNNER_HOOK_JOB_STARTED` and serves local cache and artifact APIs ([proposal-G.md:184](file:///Users/donbeave/.cache/ci-ambition/panel/gemini-4-carbon/review-input/proposal-G.md#L184)).**
   In [actions-runner/src/Runner.Worker/JobRunner.cs:146-241](file:///Users/donbeave/.cache/ci-ambition/refs/actions-runner/src/Runner.Worker/JobRunner.cs#L146-L241) and [JobExtension.cs:208-313](file:///Users/donbeave/.cache/ci-ambition/refs/actions-runner/src/Runner.Worker/JobExtension.cs#L208-L313), `Runner.Worker` creates `_work` and `_work/_temp`, runs
   `PrepareDirectory`, evaluates job-level `env:` (`MBX_CACHE_DIR`, `CARGO_HOME`), and runs `PrepareActionsAsync` (line 301) *before*
   `ACTIONS_RUNNER_HOOK_JOB_STARTED` executes inside `stepsRunner`. Mounting a workspace in that hook either masks `_actions` and `_temp` or
   requires host mount propagation that breaks Proposal G's "no host mounts" sandbox rule (line 301). Under L0 (`run.sh`),
   [actions-runner/src/Runner.Worker/Handlers/NodeScriptActionHandler.cs:58-74](file:///Users/donbeave/.cache/ci-ambition/refs/actions-runner/src/Runner.Worker/Handlers/NodeScriptActionHandler.cs#L58-L74) also overwrites `ACTIONS_CACHE_URL` and `ACTIONS_RESULTS_URL` from
   GitHub's `SystemVssConnection`, so unmodified cache and artifact actions ignore local endpoints until L1 rewrites the job message.
   Running one L0 job on `velnor-lab` settles both points immediately.
4. **`merge_group` finishes in <= 40 s from PR cache hits despite strict `Trusted` vs `PR` scope isolation ([proposal-G.md:277](file:///Users/donbeave/.cache/ci-ambition/panel/gemini-4-carbon/review-input/proposal-G.md#L277)).**
   Proposal G places `merge_group` in `Trusted scope` (line 230) and confines PR runs to an untrusted overlay that `Trusted scope` never
   reads. Every crate edited in the PR, its reverse dependents, and its re-linked test binaries are therefore cold misses in `Trusted scope`
   during `merge_group`, and 38 of 45 sampled `merge_group` trees also differed from the PR head tree ([evidence.md:209](file:///Users/donbeave/.cache/ci-ambition/panel/evidence.md#L209)). Measuring
   `merge_group` wall time on runner and engine PRs with separated `Trusted` and `PR` stores settles this.

### 2. Factual errors

- **No-journal `completejob` recovery ([proposal-G.md:187](file:///Users/donbeave/.cache/ci-ambition/panel/gemini-4-carbon/review-input/proposal-G.md#L187)).** `POST {run_service_url}/completejob`
  ([actions-runner/src/Sdk/RSWebApi/RunServiceHttpClient.cs:123-160](file:///Users/donbeave/.cache/ci-ambition/refs/actions-runner/src/Sdk/RSWebApi/RunServiceHttpClient.cs#L123-L160)) requires `SystemVssConnection.Url`, the per-job OAuth `AccessToken`, `PlanID`,
  `JobID`, and `BillingOwnerId`. Those arrive only once in `AgentJobRequestMessage` from `acquirejob` and do not exist in Scale Set messages
  ([evidence.md:1181](file:///Users/donbeave/.cache/ci-ambition/panel/evidence.md#L1181)). A restarted node agent with no journal and dropped per-job layers lacks the URL, IDs, and token needed to fail lost jobs.
- **L1 `ProcessChannel` framing ([proposal-G.md:185](file:///Users/donbeave/.cache/ci-ambition/panel/gemini-4-carbon/review-input/proposal-G.md#L185)).** [actions-runner/src/Runner.Common/ProcessChannel.cs:12-19](file:///Users/donbeave/.cache/ci-ambition/refs/actions-runner/src/Runner.Common/ProcessChannel.cs#L12-L19) is only `enum MessageType`.
  [actions-runner/src/Runner.Common/StreamString.cs:18-19](file:///Users/donbeave/.cache/ci-ambition/refs/actions-runner/src/Runner.Common/StreamString.cs#L18-L19) (and lines 81 to 88) encodes the string payload in **UTF-16LE** (`new UnicodeEncoding()`) with a
  4-byte byte length over inherited local OS anonymous pipes (`spawnclient`). `velnor-dispatch` cannot pipe across nodes without a local
  `velnor-node` pipe shim, first-`renewjob` gate ([actions-runner/src/Runner.Listener/JobDispatcher.cs:395-406](file:///Users/donbeave/.cache/ci-ambition/refs/actions-runner/src/Runner.Listener/JobDispatcher.cs#L395-L406)), and UTF-16LE encoder.
- **`mbx-cache` OIDC authorization ([proposal-G.md:311](file:///Users/donbeave/.cache/ci-ambition/panel/gemini-4-carbon/review-input/proposal-G.md#L311)).** Both [velnor-new/.github/workflows/ci.yml:35-40](file:///Users/donbeave/Projects/tailrocks/velnor-new/.github/workflows/ci.yml#L35-L40) and
  [velnor-new/generator/samples/velnor-new/expected/.github/workflows/rust-ci.yml:18-19](file:///Users/donbeave/Projects/tailrocks/velnor-new/generator/samples/velnor-new/expected/.github/workflows/rust-ci.yml#L18-L19) forbid `id-token: write` on build jobs (enforced by
  [velnor-new/generator/crates/velnor-gen/tests/it/release_yml_security.rs:55](file:///Users/donbeave/Projects/tailrocks/velnor-new/generator/crates/velnor-gen/tests/it/release_yml_security.rs#L55)), so CI jobs cannot mint GitHub OIDC tokens.
- **`velnor-gen` job count, cache driver, hit-rate formula, debuginfo, and `runs-on` ([proposal-G.md:269](file:///Users/donbeave/.cache/ci-ambition/panel/gemini-4-carbon/review-input/proposal-G.md#L269), lines 301, 336, 363, and 368).**
  [rust-ci.yml:174-195](file:///Users/donbeave/Projects/tailrocks/velnor-new/generator/samples/velnor-new/expected/.github/workflows/rust-ci.yml#L174-L195) emits 10 jobs (not 7) and uses plain Cargo `target/` caching with zero `mbx` steps. `velnor-gen` already defaults
  `ci_debuginfo` to `LineTablesOnly` ([velnor-new/generator/crates/velnor-model/src/repo.rs:332](file:///Users/donbeave/Projects/tailrocks/velnor-new/generator/crates/velnor-model/src/repo.rs#L332)) and renders a static string `runs-on`
  ([velnor-new/generator/crates/velnor-render/src/workflow.rs:67](file:///Users/donbeave/Projects/tailrocks/velnor-new/generator/crates/velnor-render/src/workflow.rs#L67)) with no fork-routing expression. Even if `mbx` ran over warm `target/` directories, Cargo
  skips `rustc` on ~445 Fresh crates, driving `hits / (hits + misses + not looked up)` toward 0% when caching works best while also omitting
  `mbx`'s fourth `bypassed` counter.
- **PR `#1312` job count, `mise` download, T11 wait, and read-only Rust mount ([proposal-G.md:248](file:///Users/donbeave/.cache/ci-ambition/panel/gemini-4-carbon/review-input/proposal-G.md#L248), lines 8, 67, 142, and 329).** Run
  `44308933193` (PR `#1312`) executed 20 non-skipped jobs (12 scale-set plus 8 hosted), not 12. `mise 2026.11.1` is baked into runner image
  `.4` (`Setup Mise` median 2 s in [evidence.md:350](file:///Users/donbeave/.cache/ci-ambition/panel/evidence.md#L350), whereas 4.6 s in G line 95 was `cargo fetch`). T11's 86 queued jobs had median waits of 10.4 min and
  9.1 min ([evidence.md:390-391](file:///Users/donbeave/.cache/ci-ambition/panel/evidence.md#L390-L391)), not 20 to 40 min. Mounting the Rust toolchain read-only under `ci.yml` fails [velnor-new/.github/workflows/ci.yml:438](file:///Users/donbeave/Projects/tailrocks/velnor-new/.github/workflows/ci.yml#L438)
  (`rust_installed_manifest_unwritable`).

### 3. Missed bottlenecks or risks

- **Dropping CI's `push` trigger breaks releases, cache saves, and pruning ([proposal-G.md:276](file:///Users/donbeave/.cache/ci-ambition/panel/gemini-4-carbon/review-input/proposal-G.md#L276)).** `velnor-ci release-gate`
  ([velnor-new/ci-helper/crates/velnor-ci/src/gate.rs:137-155](file:///Users/donbeave/Projects/tailrocks/velnor-new/ci-helper/crates/velnor-ci/src/gate.rs#L137-L155)), `velnor-gen`'s `release.yml` ([velnor-new/generator/crates/velnor-render/src/release.rs:95-105](file:///Users/donbeave/Projects/tailrocks/velnor-new/generator/crates/velnor-render/src/release.rs#L95-L105)),
  [velnor-new/.github/workflows/product-release.yml:86](file:///Users/donbeave/Projects/tailrocks/velnor-new/.github/workflows/product-release.yml#L86), and [velnor-new/.github/workflows/cache-prune.yml:20](file:///Users/donbeave/Projects/tailrocks/velnor-new/.github/workflows/cache-prune.yml#L20) all require `event == "push"` on `main`, and
  direct pushes bypassing the queue would land unverified.
- **Dropped `local-mode` coverage and ruleset `24396608` deadlock in Phase 1 ([proposal-G.md:368](file:///Users/donbeave/.cache/ci-ambition/panel/gemini-4-carbon/review-input/proposal-G.md#L368)).** Switching to `rust-ci.yml` as-is drops
  `velnor-runner-host --features local-mode` (which caught 4 of the 14 `merge_group` failures in line 46), omits `alint`, `cargo-deny`,
  `cargo-machete`, `jactionlint`, and `velnor-archive-guard` scripts, and emits only `Required` while ruleset `24396608` blocks merges
  missing `Required verdict` ([evidence.md:660](file:///Users/donbeave/.cache/ci-ambition/panel/evidence.md#L660)).
- **Control-plane exposure on `velnor-237` and dependency profile drift ([proposal-G.md:183](file:///Users/donbeave/.cache/ci-ambition/panel/gemini-4-carbon/review-input/proposal-G.md#L183), lines 160, 332, and 338).** Under L0 with unchanged
  `[velnor, ubuntu-26.04-scale-set]` labels, idle runners on `velnor-237` accept arbitrary PR build jobs, and even a dedicated `Plan` label
  runs `cargo metadata` on untrusted PR manifests on the single non-HA etcd node. Building runner test dependencies at `opt-level = 3`
  without debug assertions alters test semantics, and system-wide THP `always` risks compaction stalls (`rustc` uses glibc malloc on Linux,
  so `MALLOC_CONF` is a no-op).

### 4. Ideas worth taking

- **Merge-queue service-rate math and identical-SHA flake measurement ([proposal-G.md:35-48](file:///Users/donbeave/.cache/ci-ambition/panel/gemini-4-carbon/review-input/proposal-G.md#L35-L48)).** Quantifying the 281.7 min queue wait
  (`max_entries_to_build: 2` serving 1.9 merges per hour against 7 to 8 merges per hour demand) and the 10.4% flake rate (10 of 96 push
  failures on already-green `merge_group` SHAs, driven by 2 s `slow-timeout` kills under 82.6-core CPU pressure) explains why queue latency explodes.
- **L1 late binding with unmodified `Runner.Worker` ([proposal-G.md:185](file:///Users/donbeave/.cache/ci-ambition/panel/gemini-4-carbon/review-input/proposal-G.md#L185)).** Verified in [actions-runner/src/Runner.Worker/JobRunner.cs:47-70](file:///Users/donbeave/.cache/ci-ambition/refs/actions-runner/src/Runner.Worker/JobRunner.cs#L47-L70),
  `Runner.Worker` reads `.runner` and uses `SystemVssConnection`'s `AccessToken` without needing `.credentials_rsaparams`. Paired with a
  local UTF-16LE `ProcessChannel` shim and a tiny lease journal, Velnor can route jobs to warm nodes *after* `acquirejob` and rewrite
  `CacheServerUrl` and `ResultsServiceUrl` in memory while keeping 100% official step and action compatibility.
- **Profiling `turso_parser` AST clone and drop in `velnor-runner-host` tests and retiring the 110k-line journal and launch code on the cluster ([proposal-G.md:101](file:///Users/donbeave/.cache/ci-ambition/panel/gemini-4-carbon/review-input/proposal-G.md#L101), lines 193 and 207).** Enforcing strict per-crate LOC caps prevents a third 150k-line runner.
- **Advertising `X-ScaleSetMaxCapacity` equal to free CCD-aligned slots, moving kubelet and work storage off OS `ext4` onto XFS NVMe, and pre-seeding `ACTIONS_RUNNER_ACTION_ARCHIVE_CACHE` ([proposal-G.md:329-333](file:///Users/donbeave/.cache/ci-ambition/panel/gemini-4-carbon/review-input/proposal-G.md#L329-L333)).**

### 5. Verdict

Proposal G has the sharpest empirical diagnosis of merge-queue saturation, contention-induced flakes, and `velnor-runner-host` Turso bloat,
and its L1 `Runner.Worker` seam is the cleanest low-risk bridge to warm-node routing, yet it would **not** hit <= 3 min push-to-merge at
equal confidence as written. Its Phase 1 cutover drops `local-mode` tests, deadlocks ruleset `24396608`, and breaks release gates by
deleting `push`. Its L0 hook mounting and no-journal `completejob` recovery fail against `actions/runner`'s execution order and credentials,
and its <= 90 s Phase 3 target relies on a Landlock verdict cache that neither blocks `stat(2)` reads nor hits when `velnor-runner-host` or
engine crates re-link. Its single biggest flaw is centering build caching on one shared node-wide `MBX_CACHE_DIR` with per-PR `overlayfs`
layers over a mutable lowerdir, repeating the registrar `flock` bottleneck documented in `velnor-legacy`'s `mbx_store.rs` while ignoring
`#585`'s proof that plain Cargo over XFS CoW `target/` compiles zero crates on warm runs where `mbx` recompiles 10 to 68.

## Proposal E

### 1. Weakest assumptions

1. **ARC pod-per-job in Phases 1 to 4 delivers 70 to 85% node-local MBX L1 hits and a Copy-on-Write view without writable host mounts or warm-node placement ([proposal-E.md:315-349](file:///Users/donbeave/.cache/ci-ambition/panel/gemini-4-carbon/review-input/proposal-E.md#L315-L349), lines 170, 378, 584, and 684).**
   Proposal E keeps ARC through Phase 4, forbids writable host mounts (line 349), and restricts jobs to scoped API access against a cache
   server. Over an HTTP API with `emptyDir` pods, no filesystem CoW view exists and cross-run node-local L1 hit rate is 0%. Conversely,
   `mbx` `backend: local` (line 584) requires a writable `MBX_CACHE_DIR` for its registrar `flock`, leases, and `task-manifests` predictions
   ([velnor-new/docs/proposed/compiled-state-persistence.md:13](file:///Users/donbeave/Projects/tailrocks/velnor-new/docs/proposed/compiled-state-persistence.md#L13)). Meanwhile ARC's `ghalistener` ([arc/cmd/ghalistener/scaler/scaler.go:270-339](file:///Users/donbeave/.cache/ci-ambition/refs/arc/cmd/ghalistener/scaler/scaler.go#L270-L339))
   patches one integer replica count, so `kube-scheduler` places pods across DE and FI (`35 ms` RTT) blind to cache warmth or sibling test
   shards. Testing one unprivileged ARC pod spec without writable host mounts across DE and FI workers settles this.
2. **MBX is the right compiler cache for `velnor-new` ([proposal-E.md:568-591](file:///Users/donbeave/.cache/ci-ambition/panel/gemini-4-carbon/review-input/proposal-E.md#L568-L591)).**
   Proposal E misses `velnor-new`'s roadmap record (`#585` and `#586`, cited at [velnor-new/docs/roadmap.md:48](file:///Users/donbeave/Projects/tailrocks/velnor-new/docs/roadmap.md#L48)) and
   [velnor-new/docs/implemented/cache-measurements.md:81-108](file:///Users/donbeave/Projects/tailrocks/velnor-new/docs/implemented/cache-measurements.md#L81-L108). Every tested `mbx` mode recompiled 10 to 68 crates on unchanged warm runs due
   to external `OUT_DIR` `StaleDepFingerprint` cascades, `mbx` compile counts agreed with Cargo logs in 1 of 163 runs, and `velnor-gen`
   flags `[wrappers.cargo] command = "mbx"` as a weakening wrapper ([velnor-new/generator/crates/velnor-gen/tests/it/weaken.rs:84-88](file:///Users/donbeave/Projects/tailrocks/velnor-new/generator/crates/velnor-gen/tests/it/weaken.rs#L84-L88)).
   Running `#585`'s `warm` and `leaf` benchmarks on a worker node comparing plain Cargo over XFS reflinked `target/` (`velnor-ci inputs
   restore`) against `mbx` 1.30.0 settles this.
3. **Carrying `velnor-new`'s 157k-LOC journal and ownership engine into the cluster controller ([proposal-E.md:250-258](file:///Users/donbeave/.cache/ci-ambition/panel/gemini-4-carbon/review-input/proposal-E.md#L250-L258)).**
   `velnor-runner-host` (`156,822` code LOC, 52 Turso migrations, [evidence.md:818](file:///Users/donbeave/.cache/ci-ambition/panel/evidence.md#L818)) carries ~60 open recovery and reservation bugs
   ([evidence.md:951-953](file:///Users/donbeave/.cache/ci-ambition/panel/evidence.md#L951-L953), including `#1303 reconcile_gate never observes GitHub`, `#1206`, and `#1005`) and drives CI's slowest `>540 s`
   tests to protect foreign Docker objects on a developer Mac that never exist in dedicated cluster sandboxes. Comparing defect count and
   test runtime of a `<1.5k`-LOC active-lease table against porting `velnor-runner-host`'s 35 schema modules settles this.
4. **Skipping a median of 14 of 39 PR jobs today shows H5 selection already trims meaningful work ([proposal-E.md:42-45](file:///Users/donbeave/.cache/ci-ambition/panel/gemini-4-carbon/review-input/proposal-E.md#L42-L45), line 157).**
   While 14 sub-minute leaf crates skip (10 have zero `nextest` tasks), 20 of 39 jobs, including 100% of the heavy suites
   (`maintenance-helpers`, `velnor-actions-cli`, `velnor-runner-host`, `runner-host (local-mode)`, `orchestrator`), run on every non-docs PR
   (17 of 18 in E's own table at line 83, and 75 of 75 non-docs PRs in [evidence.md:110-115](file:///Users/donbeave/.cache/ci-ambition/panel/evidence.md#L110-L115)). That happens because
   [velnor-new/crates/velnor-actions-rust/src/tasks.rs:254](file:///Users/donbeave/Projects/tailrocks/velnor-new/crates/velnor-actions-rust/src/tasks.rs#L254) marks every `build.rs` crate as `undeclared_reads` and
   [velnor-new/crates/velnor-actions-orchestrator/src/select_reads.rs:23-31](file:///Users/donbeave/Projects/tailrocks/velnor-new/crates/velnor-actions-orchestrator/src/select_reads.rs#L23-L31) forces it plus its reverse dependency closure. Computing
   critical-path wall time with vs without the 14 skipped leaf jobs (0 s difference) settles this.

### 2. Factual errors

- **False claim that the cluster image seeds the Rust toolchain ([proposal-E.md:135-138](file:///Users/donbeave/.cache/ci-ambition/panel/gemini-4-carbon/review-input/proposal-E.md#L135-L138)).** Proposal E claims missing Rust restores say
  little about download time because *"The cluster image already seeds tools."* [velnor-runners/state/RESULT.md:115](file:///Users/donbeave/Projects/tailrocks/velnor-runners/state/RESULT.md#L115) states the opposite:
  *"Not baked in: the Rust toolchain. Generated workflows install it into `RUSTUP_HOME=${{ runner.temp }}/velnor/rustup`, which is per-job."*
  Scale-set Rust jobs spend 10 s median and 18 s p90 installing Rust (`706 runner-min` in [evidence.md:344](file:///Users/donbeave/.cache/ci-ambition/panel/evidence.md#L344)).
- **Wrong and out-of-bounds `path:line` citations ([proposal-E.md:99](file:///Users/donbeave/.cache/ci-ambition/panel/gemini-4-carbon/review-input/proposal-E.md#L99), lines 131 and 257).** [velnor-new/crates/velnor-runner/crates/velnor-runner-core/src/ownership.rs:1-34](file:///Users/donbeave/Projects/tailrocks/velnor-new/crates/velnor-runner/crates/velnor-runner-core/src/ownership.rs#L1-L34)
  has 34 lines total, so `ownership.rs:21-42` runs past EOF. [velnor-new/.github/workflows/ci.yml:2592-2620](file:///Users/donbeave/Projects/tailrocks/velnor-new/.github/workflows/ci.yml#L2592-L2620) contains `MBX stats`, `Upload
  crate reports`, and the `local-mode` header rather than parallel check steps (`2570-2590`). [ci.yml:703-707](file:///Users/donbeave/Projects/tailrocks/velnor-new/.github/workflows/ci.yml#L703-L707) is `Compute state snapshot key`
  (`&s25`), not scale-set `Restore Cargo sources` (`666-674`). Proposal E also cites 15 unsubmitted local scratch files
  (`ci-pack-analysis.txt:7`, `ci-live-analysis.txt:2-15`, `github-main-rules.jsonl:5`) as `path:line` sources.
- **Misread `[[checks]]` selector in `.velnor/config.toml:282-956` ([proposal-E.md:411-417](file:///Users/donbeave/.cache/ci-ambition/panel/gemini-4-carbon/review-input/proposal-E.md#L411-L417)).** `.velnor/config.toml` omits
  `selection = "declared_inputs"`, so `CheckSelection` defaults to `RustChanges` ([velnor-new/crates/velnor-actions-contract/src/config/mise.rs:48-59](file:///Users/donbeave/Projects/tailrocks/velnor-new/crates/velnor-actions-contract/src/config/mise.rs#L48-L59),
  [velnor-new/crates/velnor-actions-orchestrator/src/select.rs:190-202](file:///Users/donbeave/Projects/tailrocks/velnor-new/crates/velnor-actions-orchestrator/src/select.rs#L190-L202)) and runs `check-maintenance-helpers` on 100% of non-docs PRs regardless of `inputs`.
- **Missed `StateCompat` mismatch and stale Blaze and generator facts ([proposal-E.md:352-356](file:///Users/donbeave/.cache/ci-ambition/panel/gemini-4-carbon/review-input/proposal-E.md#L352-L356), lines 138, 418 to 429, and 601).** Even the 6
  `generator` and 4 `ci-helper` seals that succeeded had 0 of 3,636 scale-set restores because `ci.yml:707` hashes `ImageOS`, `$RUNNER_TEMP`,
  and `$GITHUB_WORKSPACE` into `compat` (`#1305` comment `6980735625`), so replacing the tip check alone still yields 0% restores. Blaze had
  releases through `v0.20261105.4` (tested and rejected in `#652`), not `v0.20261027.0`, and `velnor-gen` ([rust-ci.yml:1631-1668](file:///Users/donbeave/Projects/tailrocks/velnor-new/generator/samples/velnor-new/expected/.github/workflows/rust-ci.yml#L1631-L1668)) already
  retired `ledger-cli` and `Required verdict`.

### 3. Missed bottlenecks or risks

- **Merge-queue backlog ignored ([proposal-E.md:514-520](file:///Users/donbeave/.cache/ci-ambition/panel/gemini-4-carbon/review-input/proposal-E.md#L514-L520)).** Proposal E scopes its 4.5 to 10 min target to an already-approved PR *with an
  open merge slot*, ignoring the multi-hour merge-queue backlog on Nov 18 (`max_entries_to_build: 2`, queue depth 25) and proposing no
  concrete queue concurrency target.
- **Phase 4 starves its own trusted `main` cache ([proposal-E.md:335-341](file:///Users/donbeave/.cache/ci-ambition/panel/gemini-4-carbon/review-input/proposal-E.md#L335-L341), line 686).** Proposal E forbids both PR and `merge_group` runs from
  writing `main` caches without a Phase 5 supervisor, yet Phase 4 skips post-merge `main` `push` builds (`<= 10 runner-min`), leaving nothing
  to publish fresh `main` compiler caches.
- **Stranded CPU, OS-disk `emptyDir` I/O, single-workspace blast radius, and Phase 1 gate exposure ([proposal-E.md:554-557](file:///Users/donbeave/.cache/ci-ambition/panel/gemini-4-carbon/review-input/proposal-E.md#L554-L557), lines 436 to 441,
  640, and 683).** Sizing heavy slots at `16 CPU` and `24 GiB` (`4` slots per worker = `64` CPUs + `8` reserved) strands 24 logical CPUs
  (25%) per node because `24 GiB` binds first (`13.4 to 16.8 GiB` actual peak). Proposal E misses that kubelet `rootDirectory: null` puts
  `emptyDir` on the single `ext4` OS disk ([velnor-runners/PLAN.md:45](file:///Users/donbeave/Projects/tailrocks/velnor-runners/PLAN.md#L45)), moves `Required verdict` (`GH_TOKEN`) onto untainted shared DinD
  workers in Phase 1 before Phase 2 adds taints ([velnor-runners/state/RESULT.md:95-96](file:///Users/donbeave/Projects/tailrocks/velnor-runners/state/RESULT.md#L95-L96)), and overlooks that unifying into one root workspace
  makes Hard Decision 11 (`--workspace` compile) build all 30 crates unless `target/` snapshots hit ~100%.

### 4. Ideas worth taking

- **No post-`acquirejob` runner handoff ([proposal-E.md:242-244](file:///Users/donbeave/.cache/ci-ambition/panel/gemini-4-carbon/review-input/proposal-E.md#L242-L244)).** Proposal E identifies that starting a second worker after `acquirejob`
  creates an ambiguous lease owner (`409 AlreadyAcquired`), so native workers must either run standard `node24` JS actions in-sandbox or
  route unsupported jobs before `acquirejob`.
- **Preserving `velnor-ci release-gate` and `workflow_run` contracts on `main` pushes ([proposal-E.md:490-496](file:///Users/donbeave/.cache/ci-ambition/panel/gemini-4-carbon/review-input/proposal-E.md#L490-L496)).** Citing
  [velnor-new/ci-helper/crates/velnor-ci/src/gate.rs:44-156](file:///Users/donbeave/Projects/tailrocks/velnor-new/ci-helper/crates/velnor-ci/src/gate.rs#L44-L156), Proposal E keeps a real `push` receipt on `main` (validating `merge_group`
  provenance, promoting `main` caches, and running full checks on direct pushes) so release workflows never break.
- **Exact `min_entries_to_merge: 1` ruleset analysis ([proposal-E.md:518-520](file:///Users/donbeave/.cache/ci-ambition/panel/gemini-4-carbon/review-input/proposal-E.md#L518-L520)).** Verified on PR `#990` (`45 s` between PR CI completion and
  `merge_group` creation, line 75), `min_entries_to_merge_wait_minutes: 5` applies only when `min_entries_to_merge` is unmet and does not
  stall a queue with `minimum = 1`.
- **Explicit `cargo test --doc` tracking, pre-built candidate fixtures, and splitting 274 s single tests before sharding ([proposal-E.md:448-467](file:///Users/donbeave/.cache/ci-ambition/panel/gemini-4-carbon/review-input/proposal-E.md#L448-L467)).**
  Nextest archives omit doctests, and splitting `image_publication_public`'s 40-case loop and `unbound_retirement` is required before sharding.

### 5. Verdict

Proposal E is the most rigorous of the three on lease ownership, release-gate contracts, and single-test Amdahl floors, so it preserves
verification confidence, yet it falls far short of minimum push-to-merge time. Its 4.5 to 10 min target (before merge-queue wait) is 4x to
8x slower than the hardware allows, leaves 25% of worker CPUs idle, and spends four phases tuning ARC pod-per-job before testing a Velnor
runtime. Its single biggest flaw is assuming ARC pods in Phases 1 to 4 can achieve 70 to 85% node-local Copy-on-Write MBX hits while
forbidding writable host mounts, lacking warm-node or same-site placement across DE and FI, starving `main` cache updates in Phase 4, and
missing `#585`'s proof that `mbx` loses to plain Cargo `target/` snapshots.

## Proposal F

### 1. Weakest assumptions

1. **Post-`acquirejob` admission `admit(&AgentJobRequest) -> Lane::Official` ([proposal-F.md:161-163](file:///Users/donbeave/.cache/ci-ambition/panel/gemini-4-carbon/review-input/proposal-F.md#L161-L163), lines 124, 132, and 333).**
   Proposal F claims `admit(job: &AgentJobRequest, policy)` runs *"BEFORE any side effect"*, inspects `uses:` steps and composites, and
   routes unsupported jobs to `Lane::Official`, where an unmodified official runner starts with a JIT secret and runs broker plus
   `acquirejob`. Scale Set `JobAvailable` messages carry no steps or actions ([velnor-new/crates/velnor-runner/crates/velnor-runner-github/src/poll.rs:59-90](file:///Users/donbeave/Projects/tailrocks/velnor-new/crates/velnor-runner/crates/velnor-runner-github/src/poll.rs#L59-L90)).
   `AgentJobRequest` is returned *only* by the mutating `POST {run_service_url}/acquirejob` call ([actions-runner/src/Runner.Listener/Runner.cs:742-755](file:///Users/donbeave/.cache/ci-ambition/refs/actions-runner/src/Runner.Listener/Runner.cs#L742-L755)). Once
   acquired, starting an unmodified `Runner.Listener` in Lane O fails with HTTP `409 Conflict` (`TaskOrchestrationJobAlreadyAcquiredException`,
   [actions-runner/src/Sdk/RSWebApi/RunServiceHttpClient.cs:106-107](file:///Users/donbeave/.cache/ci-ambition/refs/actions-runner/src/Sdk/RSWebApi/RunServiceHttpClient.cs#L106-L107)), which `Runner.cs:756-764` logs and skips. Calling `acquirejob` once on
   `velnor-lab` and booting `run.sh` with the JIT token settles this immediately.
2. **Controller-native `Plan` and `verdict` in Phase 2 while `velnor-runner-proto` waits until optional Phase 4 ([proposal-F.md:200](file:///Users/donbeave/.cache/ci-ambition/panel/gemini-4-carbon/review-input/proposal-F.md#L200), lines 150, 301, 429, and 439).**
   In [rust-ci.yml:27-90](file:///Users/donbeave/Projects/tailrocks/velnor-new/generator/samples/velnor-new/expected/.github/workflows/rust-ci.yml#L27-L90), `rust-plan` is an in-workflow job whose 25 outputs (`needs.rust-plan.outputs.*`) gate every workspace job in
   GitHub's server-side DAG. Only `POST {run_service_url}/completejob` ([RunServiceHttpClient.cs:123-146](file:///Users/donbeave/.cache/ci-ambition/refs/actions-runner/src/Sdk/RSWebApi/RunServiceHttpClient.cs#L123-L146)) can populate `needs.rust-plan.outputs.*`,
   and that client lives in Phase 4's unbuilt `velnor-runner-proto`. Checking how `needs.rust-plan.outputs` could be populated via the Scale
   Set admin API alone settles this.
3. **Skipping test binaries by `hash(test binary digest + env class)` keeps equal verification confidence ([proposal-F.md:294](file:///Users/donbeave/.cache/ci-ambition/panel/gemini-4-carbon/review-input/proposal-F.md#L294), lines 218 and 434).**
   As [velnor-new/ci-helper/crates/velnor-ci/src/cli.rs:214](file:///Users/donbeave/Projects/tailrocks/velnor-new/ci-helper/crates/velnor-ci/src/cli.rs#L214) states, integration tests read unembedded repo files at runtime.
   [velnor-new/generator/crates/velnor-gen/tests/it/golden.rs:19-24](file:///Users/donbeave/Projects/tailrocks/velnor-new/generator/crates/velnor-gen/tests/it/golden.rs#L19-L24) reads `generator/samples/**` and `.velnor/config.toml`,
   [velnor-new/crates/velnor-archive-guard/tests/golden_collector_public/gate.rs:30-46](file:///Users/donbeave/Projects/tailrocks/velnor-new/crates/velnor-archive-guard/tests/golden_collector_public/gate.rs#L30-L46) runs `scripts/generator-release/source-qualification.sh`
   and reads `MANIFEST.sha256`, and [velnor-new/crates/velnor-actions-cli/tests/image_publication_public/context_fixture.rs:47-84](file:///Users/donbeave/Projects/tailrocks/velnor-new/crates/velnor-actions-cli/tests/image_publication_public/context_fixture.rs#L47-L84) reads
   `.velnor/*.lock`. Editing any of those files leaves the test binary digest unchanged, skipping the broken test 95% of the time and
   violating `brief-v1.md:11` and [velnor-new/docs/goals.md:36](file:///Users/donbeave/Projects/tailrocks/velnor-new/docs/goals.md#L36). Corrupting `generator/samples/velnor-new/expected/.github/workflows/rust-ci.yml`
   and confirming `velnor-gen`'s `it` binary SHA-256 stays identical settles this.
4. **Six 12-CPU shards cut `runner-host` tests to 35 to 50 s (`<= 60 s` gate) and four shards cut `actions-cli` to 60 s without fixing slow tests ([proposal-F.md:296](file:///Users/donbeave/.cache/ci-ambition/panel/gemini-4-carbon/review-input/proposal-F.md#L296), line 435).**
   Proposal F divides `192 to 246 s` and `238 s` by `6` and `4`, forgetting those baselines already ran across 96 uncapped threads
   ([evidence.md:253-271](file:///Users/donbeave/.cache/ci-ambition/panel/evidence.md#L253-L271), line 519) while F caps shard slots at 12 CPUs (`72` and `48` total threads). Nextest cannot split a single `#[test]`,
   so `runner-host`'s `>540 s` and `283 s` single tests ([evidence.md:895](file:///Users/donbeave/.cache/ci-ambition/panel/evidence.md#L895)) and `actions-cli`'s `274 s` 40-invocation `crane` test
   ([velnor-new/crates/velnor-actions-cli/tests/image_publication_public.rs:85-138](file:///Users/donbeave/Projects/tailrocks/velnor-new/crates/velnor-actions-cli/tests/image_publication_public.rs#L85-L138)) plus its `89 s` fixture build keep both suites above 4 min.
   Running 6-shard nextest partitioning on 12-CPU cgroups today settles this.
5. **In-place container scrubbing across 50 untrusted PR jobs paired with `velnor-runner-host`'s 185k-LOC journal ([proposal-F.md:334](file:///Users/donbeave/.cache/ci-ambition/panel/gemini-4-carbon/review-input/proposal-F.md#L334), line 190).**
   `velnor-runner-host` proves deletion via 404 ([ownership.rs:27-34](file:///Users/donbeave/Projects/tailrocks/velnor-new/crates/velnor-runner/crates/velnor-runner-core/src/ownership.rs#L27-L34)), not multi-job container scrubbing, and carries ~60 open bugs
   ([evidence.md:951-953](file:///Users/donbeave/.cache/ci-ambition/panel/evidence.md#L951-L953)). Reusing a live container across 50 untrusted PRs risks cross-job residue in `/dev/shm`, keyrings, or writable slot
   trees when spawning a fresh rootless container over an XFS reflinked rootfs takes `<50 ms`.

### 2. Factual errors

- **False `[M]` claim that self-hosted jobs receive no `CacheServerUrl` and that env injection redirects Lane O caches ([proposal-F.md:136](file:///Users/donbeave/.cache/ci-ambition/panel/gemini-4-carbon/review-input/proposal-F.md#L136), line 424).**
  Proposal F copied an outdated comment from [velnor-legacy/crates/velnor-runner/src/gha_cache.rs:3-5](file:///Users/donbeave/.cache/ci-ambition/refs/velnor-legacy/crates/velnor-runner/src/gha_cache.rs#L3-L5) and tagged it `[M]`. Live scale-set logs in
  [evidence.md:333](file:///Users/donbeave/.cache/ci-ambition/panel/evidence.md#L333) (and line 577) prove scale-set jobs receive `CacheServerUrl` and query GitHub's cache (`48` of `48` misses, `13.6 to 22.4
  MB/s` restores). In Lane O, [NodeScriptActionHandler.cs:58-74](file:///Users/donbeave/.cache/ci-ambition/refs/actions-runner/src/Runner.Worker/Handlers/NodeScriptActionHandler.cs#L58-L74) unconditionally overwrites `ACTIONS_CACHE_URL` and `ACTIONS_RESULTS_URL` from
  `SystemVssConnection`, defeating Phase 1's env injection.
- **Impossible XFS-to-`tmpfs` reflink and worker RAM OOM math ([proposal-F.md:398-400](file:///Users/donbeave/.cache/ci-ambition/panel/gemini-4-carbon/review-input/proposal-F.md#L398-L400), lines 237 and 409).** Linux `FICLONE` cannot reflink from
  an XFS cache volume onto a `tmpfs` workdir (`EXDEV`). Kubernetes also charges `emptyDir{medium: Memory, sizeLimit: 8-16 GiB}` against the
  slot's `14 GiB` cgroup memory limit, leaving `-2 to 6 GiB` for `rustc` (`13.4 to 16.8 GiB` observed peak, [evidence.md:517](file:///Users/donbeave/.cache/ci-ambition/panel/evidence.md#L517)), and `8 * 16 GiB
  = 128 GiB` exceeds the worker's `125.5 GiB` RAM ([evidence.md:442](file:///Users/donbeave/.cache/ci-ambition/panel/evidence.md#L442)). Also, [mbx_store.rs:11-27](file:///Users/donbeave/.cache/ci-ambition/refs/velnor-legacy/crates/velnor-runner/src/mbx_store.rs#L11-L27) isolates `slots/<slot>` without cross-slot
  hardlink or reflink dedup.
- **False claim on `compiler_process_budget = test_process_budget = 2` ([proposal-F.md:288](file:///Users/donbeave/.cache/ci-ambition/panel/gemini-4-carbon/review-input/proposal-F.md#L288)).** Contrary to F's claim that these fields throttle
  jobs to 2 compiler processes (contradicting its own line 47 observation of 88-core bursts), [velnor-new/crates/velnor-actions-contract/src/config/preview_policy.rs:57-88](file:///Users/donbeave/Projects/tailrocks/velnor-new/crates/velnor-actions-contract/src/config/preview_policy.rs#L57-L88)
  and [resources.rs:135-155](file:///Users/donbeave/Projects/tailrocks/velnor-new/crates/velnor-actions-contract/src/config/resources.rs#L135-L155) only validate optional `build_jobs` and `default_shards` caps and emit no Cargo or nextest thread flags.
- **Conflating legacy `ci.yml` ledgers with `velnor-gen`'s `rust-ci.yml`, default linker error, and inventory misattribution ([proposal-F.md:200](file:///Users/donbeave/.cache/ci-ambition/panel/gemini-4-carbon/review-input/proposal-F.md#L200), lines 130, 270, 390, and 401).**
  [rust-ci.yml:1631-1668](file:///Users/donbeave/Projects/tailrocks/velnor-new/generator/samples/velnor-new/expected/.github/workflows/rust-ci.yml#L1631-L1668) has no per-job signed ledgers, no `Required verdict` job (only `Required`), and zero `mbx` steps. Rust 1.99.1 on
  `x86_64-unknown-linux-gnu` already defaults to `rust-lld` (not GNU `ld`), and the `2x3.84 TB + 3x15 TB` disk strings come from
  [velnor-runners/README.md:26-31](file:///Users/donbeave/Projects/tailrocks/velnor-runners/README.md#L26-L31), not `inventory.md` (`3.5T`, `14T`).

### 3. Missed bottlenecks or risks

- **Deferring fork Docker tests cuts verification confidence ([proposal-F.md:353](file:///Users/donbeave/.cache/ci-ambition/panel/gemini-4-carbon/review-input/proposal-F.md#L353), line 464).** Skipping Docker-requiring suites on fork PRs
  pushes failures into `merge_group` (where one failure ejects the batch) instead of running fork and Docker jobs inside KVM microVMs.
- **OS `ext4` disk bottleneck and single-workspace consolidation missed in §4 and §5 ([proposal-F.md:262-270](file:///Users/donbeave/.cache/ci-ambition/panel/gemini-4-carbon/review-input/proposal-F.md#L262-L270), lines 143 and 390).** Proposal F
  misses that kubelet `rootDirectory` defaults to `/` on the unmirrored `ext4` OS NVMe ([velnor-runners/PLAN.md:45](file:///Users/donbeave/Projects/tailrocks/velnor-runners/PLAN.md#L45)), designs §4 and §5 around 5
  permanent workspaces (`5 roots x 5 nodes`) despite noting the move to one root workspace (`crates/velnor-*`) at line 143, and omits
  `local-mode` coverage when cutting over to `rust-ci.yml`.

### 4. Ideas worth taking

- **Separating privileged `dind` into a small on-demand tainted pool while running ~90% of slots rootless without `dind` ([proposal-F.md:335](file:///Users/donbeave/.cache/ci-ambition/panel/gemini-4-carbon/review-input/proposal-F.md#L335)).**
  Removes the 5 s `dockerd` startup probe, 57 hourly `Unhealthy` events ([evidence.md:602](file:///Users/donbeave/.cache/ci-ambition/panel/evidence.md#L602)), and root-equivalent sidecars from ordinary Rust builds.
- **Mixed `12-CPU` shard slots plus `24-CPU` and `28 to 32 GiB` build slots per EPYC worker ([proposal-F.md:398](file:///Users/donbeave/.cache/ci-ambition/panel/gemini-4-carbon/review-input/proposal-F.md#L398), line 409).** Reserving one
  12-thread CCD for the host and coalescing pairs into 24-CPU build slots fits `13.4 to 16.8 GiB` peak builds without stranding 25% of CPUs.
- **Hard `>= 15%` warm-wall gate for building a native executor plus `velnor-legacy`'s incident regression suite ([proposal-F.md:440](file:///Users/donbeave/.cache/ci-ambition/panel/gemini-4-carbon/review-input/proposal-F.md#L440), line 192), and `merge_group` diff-closure selection over `diff(PR-tested-tree, merge-tree)` ([proposal-F.md:278](file:///Users/donbeave/.cache/ci-ambition/panel/gemini-4-carbon/review-input/proposal-F.md#L278)).**

### 5. Verdict

Proposal F ranks the top-level priorities sensibly and cleanly decouples `dind` from rootless slots, yet **neither its latency budget nor its
confidence guarantee survives code inspection**. Its Phase 1 cache env injection is overwritten by `Runner.Worker`, its Phase 2
controller-native `Plan` and `verdict` require Phase 4's unbuilt `velnor-runner-proto`, its `tmpfs` workdir triggers `EXDEV` on XFS reflinks
and OOMs 14 GiB slots, and its sharding math divides 96-thread wall time across 12-CPU shards while leaving `>270 s` single tests intact. Its
single biggest flaw is the broken admission and test-skipping design (`admit(&AgentJobRequest) -> Lane::Official` after `acquirejob`, paired
with binary-digest test skipping), which fails Lane O fallback with HTTP `409 AlreadyAcquired` while silently skipping integration tests whose
runtime-read fixtures, samples, or scripts changed.

## Cross-cutting

### Agreements and conflicts across the proposals

- **Where all three agree (supported by evidence).** All three rank the changes in the right order: remove GitHub-hosted gate queueing first
  (`57.0` of `71.8 min` PR wall on Nov 18, compounded by `573` auxiliary workflow runs hitting the 20-job Free-plan cap), stop running full
  39-job verification three times per merge (`120` of `159` identical-SHA `push` reruns), move caches to node-local NVMe with site-local (`DE`
  vs `FI`, `35 ms` RTT) placement, cap pod CPU to stop 83.7-core thrashing, and treat native Rust step execution as last. All three agree `H1`
  fails on interpreter CPU (`<1 to 3%` of job time), `H3` holds only as a fleet base, `H4` is the biggest build win, and `blaze` is
  disqualified for required checks because it ignores `rust-toolchain.toml` (`1.99.1`), lacks `clippy`, `doc`, and `fmt`, and catches roughly
  two-thirds of borrow errors.
- **Compiler cache engine (`mbx` in G, E, and F vs plain Cargo CoW `target/`).** All three bet on `mbx` while conflicting on storage layout
  (one shared node store plus `overlayfs` in G, API-only store under ARC in E, per-slot `mbx_store.rs` layout in F). **The repo's own evidence
  refutes all three on `mbx`.** [mbx_store.rs:6-13](file:///Users/donbeave/.cache/ci-ambition/refs/velnor-legacy/crates/velnor-runner/src/mbx_store.rs#L6-L13) refutes G's shared `MBX_CACHE_DIR`, HTTP mechanics refute E's CoW claim, and `#585`
  (`comment 6856187529`) plus [rust-ci.yml:174-195](file:///Users/donbeave/Projects/tailrocks/velnor-new/generator/samples/velnor-new/expected/.github/workflows/rust-ci.yml#L174-L195) prove plain Cargo over XFS reflinked `target/` (`cp --reflink=always` plus
  `velnor-ci inputs restore`) compiles **0 crates** on warm runs where every `mbx` mode recompiles 10 to 68.
- **Cluster recovery state (157k-LOC Turso journal in E and F vs zero journal in G).** Proposals E and F import a 52-migration Mac-Docker
  monolith with ~60 open bugs and `>540 s` tests, whereas Proposal G's zero-journal agent cannot call `completejob` after a restart
  ([RunServiceHttpClient.cs:123-160](file:///Users/donbeave/.cache/ci-ambition/refs/actions-runner/src/Sdk/RSWebApi/RunServiceHttpClient.cs#L123-L160)). **Evidence supports a minimal middle path:** disposable per-job CoW sandboxes backed by a `<1.5k`-LOC
  active-lease table persisting `(job_id, plan_id, run_service_url, token_ref, sandbox_uid)` solely for fencing and crash `completejob` reporting.
- **Fallback seam (post-`acquirejob` `Lane::Official` in F vs `Runner.Worker` pipe L1 in G vs pre-`acquirejob` routing in E).**
  [Runner.cs:742-764](file:///Users/donbeave/.cache/ci-ambition/refs/actions-runner/src/Runner.Listener/Runner.cs#L742-L764) refutes F (`acquirejob` returns `409` on second use) and backs **G's L1** (Rust listener plus local UTF-16LE pipe to
  unmodified `Runner.Worker`) and **E's rule** (any native worker must run `node24` JS actions in-sandbox rather than rejecting or handing off
  after `acquirejob`).
- **Test skipping, slow suites, and `main` pushes.** `landlock(7)` (`stat(2)` gap) and runtime fixture reads ([cli.rs:214](file:///Users/donbeave/Projects/tailrocks/velnor-new/ci-helper/crates/velnor-ci/src/cli.rs#L214),
  [golden.rs:19-24](file:///Users/donbeave/Projects/tailrocks/velnor-new/generator/crates/velnor-gen/tests/it/golden.rs#L19-L24)) decisively support **E** over G and F: never skip selected test binaries across code or fixture edits, refactor the four
  `>250 s` suites (`unbound_retirement`, `runner_image_qualification_public`, `image_publication_public`, `maintenance-helpers`) before
  sharding, and emit a fast tree-verified `push` receipt on `main` ([gate.rs:137-155](file:///Users/donbeave/Projects/tailrocks/velnor-new/ci-helper/crates/velnor-ci/src/gate.rs#L137-L155)) rather than dropping the `push` trigger as G proposes.

### Hypotheses judged differently after reading G, E, and F

- **H2 (from-scratch Rust runner vs official runner and ARC).** Proposal G's verification of [JobRunner.cs:47-70](file:///Users/donbeave/.cache/ci-ambition/refs/actions-runner/src/Runner.Worker/JobRunner.cs#L47-L70) (`Runner.Worker` needs no
  runner RSA private key) and Proposal F's `>= 15%` Phase 4 entry gate narrow my judgment on the **executor half** of H2. Replacing ARC and
  `Runner.Listener` in Rust holds unconditionally because only a Velnor listener and node agent can advertise exact slot capacity, bind jobs to
  warm XFS nodes *after* `acquirejob`, and rewrite `CacheServerUrl` and `ResultsServiceUrl` to the local site CAS. Replacing `Runner.Worker`
  itself saves only ~3 to 8 s per consolidated run, so Velnor should ship the L1 `Runner.Worker` UTF-16LE pipe bridge first and build a native
  worker only if measured Worker startup still exceeds 15% of warm PR wall time.
- **H5 (workflow design and merge-queue latency).** Two findings revise my numbers. First, Proposal E's check of ruleset `24396608`
  (`min_entries_to_merge: 1`) and PR `#990` (`45 s` gap) shows `min_entries_to_merge_wait_minutes: 5` does not add a 5-minute pause to single
  PRs. The real queue bottlenecks are `max_entries_to_build: 2` (`2 -> 5`) and Proposal G's 10.4% contention-flake batch ejection rate. Second,
  Proposal E's [gate.rs:137-155](file:///Users/donbeave/Projects/tailrocks/velnor-new/ci-helper/crates/velnor-ci/src/gate.rs#L137-L155) audit and `rust-ci.yml`'s missing `local-mode` target prove H5 must keep a fast tree-verified `push` run on
  `main` for release gates and carry `velnor-runner-host --features local-mode` into `velnor-gen`.
- **H1, H3, and H4.** Unchanged (`H1` fails on JS and C# CPU and holds only for local data endpoints, `H3` holds for DaemonSets and fails for
  per-job pods, and `H4` holds via node-local XFS CoW `target/` clones, reinforced by all three proposals hitting `mbx` locking and freshness traps).

---

## Key (added by the manager after the review)

- Proposal G: Claude Fable 5.2
- Proposal E: GPT-6.2-Sol
- Proposal F: Muse Wonder Full
