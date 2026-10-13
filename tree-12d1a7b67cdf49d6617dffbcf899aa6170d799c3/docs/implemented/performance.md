# V1 performance acceptance — local + CI-observable measurements

Commit measured: `75f150a` (branch `docs/velnor-actions-spec`).
Refreshed at `bdfffb9`: suite is now 1048 pass/0 fail (978 integration +
70 src-unit, 21 binaries; nextest ci profile 1048/1048 in 10.77 s);
cases 1–7 figures below were measured at `75f150a` except where noted.
Machine: `arm64`, macOS 27.0 (26A428), Apple M5 Max.
Toolchain: rustc/cargo 1.98.1, MBX 1.19.0 via mise cargo-wrapper shim,
nextest 0.9.143 (mise.toml pins aqua nextest 0.9.146; PATH resolved 0.9.143).
`cargo` below = MBX shim unless marked "direct" (rustup cargo, MBX bypassed).
Mutation scenarios ran in pristine `git clone`s at 75f150a under `/tmp/vperf/`
so the working tree was never modified for measurement; each clone was
`mise trust`ed (untrusted clones fail `plan_*` tests at `cargo init zebra`).
The working tree received unrelated sibling edits at 15:40 local during
measurement; repo-tree numbers below predate that; clone numbers are unaffected.
Doc-length budget: rust-quality §5 (400 lines) — this file complies.

Required-metrics reference: `docs/proposed/agent-and-performance-contract.md`
(wall, queue, peak RSS, compiled targets, tool install, cache transfer,
MBX hits/misses, task reuse) and implementation-plan §"Performance
qualification" cases 1–7. Every figure is a measured wall time (`time -p`
`real`); anything else is labeled UNMEASURED with the missing setup.

## 1. Empty-cache cold run — 56.06 s (historical-partial: warm shared registry)

Command (fresh clone, empty `target/`, direct cargo, warm shared registry):

```sh
cargo test --workspace --locked --offline   # rustup 1.98.1 binary, MBX bypassed
```

Wall 56.06 s, exit 0, 21/21 test binaries pass. Cargo reports 28.61 s
compile (65 locked packages + 7 workspace crates); the rest is test
execution. Warm shared registry (4.2 GiB) — no downloads occurred.
Proves: a from-zero local compile+test fits in ~1 min on this hardware.
Does not prove: hosted-runner cold time (other CPU/arch), cold-registry
download, or empty-MBX-store compile (the shared MBX store cannot be
emptied non-destructively).
CI datapoint: run 36540401062 `velnor-plan` "Build helper" step took
50 s on `ubuntu-26.04` (cold runner, fetch + full helper compile).
UNMEASURED: cold compile on the named CI runner as a controlled case;
cold registry/origin download time (needs `cargo fetch` with empty
`CARGO_HOME`).

## 2. Warm MBX rebuild (fresh checkout, warm store) — 3.30 s

```sh
cargo build --workspace --locked --offline   # fresh clone, empty target/
```

Wall 3.30 s (cargo: 3.10 s), exit 0.
`mbx[cache]: 76 hits, 1 miss; 0 B downloaded, 0 B uploaded`.
Proves: MBX object reuse across checkouts — a fresh tree materializes
without compiling. Does not prove the CI MBX path: no remote is
configured, so cache transfer/invalidation over the network is unexercised.
UNMEASURED: CI cache-transfer time and MBX miss reasons on hosted runners.

## 3. Unchanged rerun — 0.09 s build / 11–27 s test

Repo tree, fully warm, all green:

| Command | Wall |
| --- | --- |
| `cargo build --workspace --locked` | 0.09 s |
| `cargo test --workspace --locked` (twice) | 14.71 s, then 13.37 s |
| `cargo nextest run --workspace --locked --profile ci` | 11.21 s, 1016/1016 pass |

Clone reruns (nothing changed, exit 0): 23.79 s, 27.13 s, 27.16 s.
Compile fingerprint alone is 0.27–0.28 s (`Finished test profile`);
everything else is test execution. Per-binary slowest is ~19–20 s
(orchestrator subprocess suite) in every tree, but total wall varies
13–27 s run to run — timing-sensitive suites, not tree location.
MBX reports `0 hits, 0 misses, 6 bypassed` on reruns: test execution is
never served from the compiler cache, per contract.
Peak RSS: cargo supervisor max 126 MiB during a warm full test
(`/usr/bin/time -l`); whole-tree peak UNMEASURED (needs tree-wide RSS
sampling, e.g. sampled `ps`/`powermetrics` during a run).
Proves: no-op compile check is millisecond-scale; full-suite execution
dominates all warm scenarios. Says nothing about the 2 s preflight
budget (different operation) or CI wall times.

## 4. Leaf-crate one-file change — 25.18 s (compile 0.51 s)

Clone + comment appended to `crates/velnor-actions-cli/src/main.rs`
(cli is depended on by nothing — rebuild-set probe):

```sh
cargo test --workspace --locked --offline
```

Wall 25.18 s, exit 0. Recompiled crates: `velnor-actions-cli` only,
0.51 s; MBX `0 hits, 2 misses, 2 incremental, 6 bypassed`.
Proves: rebuild set is minimal (1 crate, sub-second compile).
The ~25 s total is full-suite re-execution, so "warm leaf validation in
seconds" holds for compile but not for a whole-workspace `cargo test`;
per-crate validation (case 6) is the path that fits the budget.

## 5. Shared-crate (contract) API change — 50.92 s (compile 17.43 s)

Clone + trivial `pub fn perf_probe_marker() -> u32` added to
`velnor-actions-contract/src/lib.rs` (real public-API addition):

```sh
cargo test --workspace --locked --offline
```

Wall 50.92 s, exit 0. Recompiled: all 7 workspace crates, 17.43 s;
MBX `0 hits, 2 misses, 22 incremental, 6 bypassed`.
Proves: a contract change rebuilds the whole workspace, as expected;
~2x the leaf total, compile 34x the leaf compile.

## 6. Budgeted concurrency — code bounds + one 2-lane run

Bounds from code: `.github/workflows/velnor.yml` matrix strategy
`max-parallel: 2`, `fail-fast: false`; `.config/nextest.toml`
`test-threads = "num-cpus"`; CI profile `--no-tests fail` via CLI.
Local 2-lane run (separate `CARGO_TARGET_DIR`s, cold targets, warm MBX):

- Concurrent: contract lane (88 tests at measure time; 89 at `bdfffb9`)
  + cli lane (110 tests) → 7.86 s wall, both exit 0. MBX lane1 `36
  hits, 3 misses`, lane2 `70 hits, 9 misses`, `0 B` transferred.
- Sequential same lanes, cold targets → 9.28 s wall.

Speedup 1.18x: with a warm MBX store each lane's compile is seconds and
the suites are tiny, so lanes are setup-bound. Proves lane isolation
works (no shared-target contention, both green).
CI evidence: run 36528572043 serialized ~48 matrix entries (7 crates ×
fmt/build/clippy/nextest/doctest/doc tasks) 2-at-a-time at ~20–30 s per
entry (setup-dominated) → 11 m 10 s total. The bound caps cost but wall
scales with entry count.
UNMEASURED: 4-lane local run (not attempted; CI bound is 2, so a 4-lane
number would not qualify any shipped path).

## 7. Dependency/lockfile touch — 27.16 s, zero recompiles

Clone + `touch Cargo.lock` (content byte-identical):

```sh
cargo test --workspace --locked --offline
```

Wall 27.16 s, exit 0. Recompiled crates: none (`Finished` in 0.27 s;
cargo keys on content, mtime-only touch is free). Total is pure test
execution (matches the 27.13 s steady-state rerun).
UNMEASURED: content-changing dependency bump and MSRV/toolchain-update
qualification (missing setup: version bump in a scratch clone +
Mise-managed `rust-version` toolchain run of every package; plus
`cargo-machete`/deny timing on the changed graph).

## CI runs (branch `docs/velnor-actions-spec`, `ubuntu-26.04`)

Green run 36569723507 at `bdfffb9` (success 47/47, 21 m 19 s
created→updated —
`https://github.com/tailrocks/velnor-new/actions/runs/36569723507`):

| Job | Wall | Notes |
| --- | --- | --- |
| Queue (created → first job start) | ~4 s | fast pickup |
| Velnor Plan | 81 s | 15 s tool prep + 48 s cold helper build; `Plan` step itself 1 s; `Check generated files` 1 s |
| Velnor Task × 42 | 18 m 52 s span (12:42:17→13:01:09Z) | `max-parallel: 2` serialized, setup-dominated per entry |
| Velnor Policy / Alint / Workflow Lint | ≤ 20 s each | deny+machete+zizmor / alint binary / actionlint |
| Velnor / Required | 52 s | downloaded + validated 42/42 matrix reports; `final-report.json` `status: passed`, 0 failed |

Full-matrix wall scales with entry count under the `max-parallel: 2`
bound; per-entry cost is tool provisioning, not Velnor analysis (1 s).

Earlier red run 36540401062 (failure, 91 s created→updated):

| Job | Wall | Notes |
| --- | --- | --- |
| Queue (created → first job start) | ~3–4 s | fast pickup, all jobs |
| Velnor Plan (failed) | 79 s | 15 s tool prep + 50 s cold "Build helper" compile; failed at "Check generated files" |
| Velnor Policy | 15 s | deny + machete + zizmor steps |
| Velnor Alint | 18 s | |
| Velnor Workflow Lint | 10 s | actionlint |
| Velnor / Required | 5 s | failed, missing plan artifact |
| Velnor Task | skipped | plan never published |

Outlier run 36528572043: 11 m 10 s, all-red matrix fan-out (see case 6).
A merge-queue run has no samples at all (merge path timing UNMEASURED).

## Budget verdicts

- Provisioned structural preflight < 2 s: PASSED. Local
  `./target/debug/velnor-actions plan` (warm, analysis + report only)
  → 0.87 s; hosted `Plan` step post-provisioning → 1 s (green run).
- Warm leaf validation in seconds: compile 0.51 s (case 4) fits;
  whole-suite validation ~25 s does not — per-crate lanes (case 6,
  ~9 s for two small crates sequentially) are the conforming scope.
  Hosted single-leg warm value UNMEASURED (legs provision tools per
  entry; no isolated warm-leg sample).
- Two-minute warm SMALL-FIXTURE path on a named runner: explicitly
  UNPASSED — no small-fixture CI run exists. (Full-dogfood green run
  is 21 m 19 s for 47 jobs; the contract budgets the small fixture,
  not the full matrix.)
- Doctest gate: green — `cargo test --doc --workspace --locked`
  exits 0 (6 crates, 0 doctests; the former E0063 at
  `orchestrator/src/config.rs` no longer reproduces). Vacuous: no
  doctests exist yet.

## P13 inventory reuse + perf foundations

Commit measured: `c938a8c` (branch `docs/velnor-actions-spec`;
working tree carried concurrent-builder uncommitted changes).
Machine: `arm64`, macOS 27.0, Apple M5 Max, 128 GiB RAM.
Toolchain: rustc/cargo 1.98.1, mise 2026.9.16, mbx 1.20.0,
nextest 0.9.143, git 2.56.0, python 3.14.7.
`CARGO_TARGET_DIR=/tmp/t-p13` for every build below.

Method: `run_inventories` now reuses one fetched `cargo metadata`
record for every member manifest after membership validation (a
manifest declaring its own `[workspace]` root is never reused);
lanes stay 1 by design (parallel metadata contends on Cargo's
global package-cache lock). Fixture generator
(`tests/perf_fixtures_p13.rs`) builds 1/10/100-crate workspaces;
the harness (`tests/perf_harness_p13.rs`) times `prepare` /
`plan` / `generate` and prints `perf:` lines. Subprocess counts
come from a unit counting harness where one stub load equals one
`cargo metadata` subprocess — exact, not sampled.

Subprocess counts (exact): 1 member → 2 legacy / 1 reused;
10 members → 11 / 1; 100 members → 101 / 1.
Reuse premise verified on real output: root- vs member-manifest
metadata differ only in `workspace_default_members`, which the
parser never reads (`member_metadata_parses_to_same_record`
asserts identical `WorkspaceRecord`s). Legacy-vs-reuse
(outcomes, inventories) are asserted identical including
nested/independent/malformed cases
(`reuse_matches_legacy_outcomes_and_inventories`).

Wall times, two runs each (`prepare`/`plan`/`generate` in ms;
generate split as prepare+render; obligations/matrix in parens):

| Crates | prepare | plan | generate |
| --- | --- | --- | --- |
| 1 | 52, 69 | 592, 611 (8 obl) | 50+776, 68+765 |
| 10 | 52, 62 | 686, 718 (44 obl) | 56+783, 61+767 |
| 40 | — | 1161, 1247 (164 obl) | — |
| 100 | 86, 104 | budget (see below) | 79+774, 95+764 |

Single direct-`cargo metadata --no-deps --offline` on the
100-crate fixture: 0.04, 0.03, 0.04 s — a lower bound per
eliminated subprocess; the mise-wrapped cost is UNMEASURED.
A 100-crate full plan fails closed with
`matrix_budget_exceeded:595321` (256 KiB cap, never truncates;
60-wide already exceeds it by design), so plan scales to 40
while `prepare` (discovery + graph construction) scales to 100.
Complexity: metadata subprocesses O(workspaces) after reuse vs
O(manifests) before; prepare sublinear (52→104 ms over 100x
crates); plan ~linear in obligations; generate flat ~770 ms
(validator-dominated, crate-count independent).

Budgets status: no P13 budget PASSED — legacy-vs-reuse wall
comparison explicitly UNPASSED (legacy path removed; counts
exact, walls optimized-path only); mise-wrapped subprocess
cost UNPASSED; CI-hosted numbers UNPASSED (no runs). Earlier
budget verdicts above are unchanged and were not re-measured.

`scripts/verify-local.sh` (new, sole P13 entrypoint; no new
`velnor-actions` command) runs fmt, repo policy, generated-tree
freshness, per-crate clippy/tests, and a nextest-`ci`
integration pass, exiting nonzero with every failure listed. A
full pass is currently blocked by concurrent-builder drift, not
by P13 code: unformatted P03 files, 49.6 h-stale freshness
evidence, generated-tree `MISE_*` drift, P03's clippy
`too_many_lines` plus the pre-existing `shard.rs` dead-code
warning, and one P03-domain test failure proven (reverted-tree
rerun) independent of the reuse change.

Deferred explicitly: end-to-end negative pipeline tests that
need P05's crate graph (no P13 code depends on it);
Nextest-archive/sharding stays off, unqualified.

## P13 benchmark cases 1–7 (bench harness)

Commit measured: P13 working tree atop `444c88e` (this commit).
Machine: `arm64`, macOS 27.0, Apple M5 Max, 128 GiB RAM.
Toolchain: rustc/cargo 1.98.1 (48a229cea/797e8a9bc), mise 2026.9.16,
mbx 1.19.0, nextest 0.9.143, git 2.56.0, python 3.14.7; case 7 second
toolchain: rustc 1.97.1 (8bab26f4f).

Method: `impl_bench_p13` plans the same 10-crate fixture shape in
every case (44 obligations, under the 256 KiB matrix budget);
`digest=` hashes the sorted obligation task IDs, so equal digests
prove the same obligation set. `setup_ms` builds the fixture,
`plan_ms` runs `plan_internal`, `metadata_ms` is a direct-`cargo
metadata` lower-bound proxy for the compiler-subprocess cost inside
`plan`; `rss_kb` is sampled test-process peak RSS (whole test
binary, not isolated plan RSS). Queue/transfer are local no-ops
(`queue=na transfer_b=0`); CI values need hosted runs. Raw lines:
`cargo test -p velnor-actions-orchestrator --test velnor_orchestrator
impl_bench_p13 -- --nocapture` (`bench:` lines on stderr). Two runs
each; figures are run1/run2.

| Case | plan_ms | setup_ms | metadata_ms | rss_kb | Digest |
| --- | --- | --- | --- | --- | --- |
| 1 cold (empty caches) | 1095, 1081 | 73, 40 | 16, 19 | 343520, 343904 | cd8b34ba18f0982d |
| 2 warm restore | 952, 969 (cold 996, 990) | 73, 40 | 15, 15 | 345312, 345728 | same |
| 3 unchanged repeat | 945, 961 | 72, 41 | 16, 16 | 345264, 345760 | same |
| 4 leaf edit (c009) | 1096, 1080 | 73, 41 | 18, 19 | 343232, 343216 | same |
| 5 API edit (c000) | 991, 987 | 74, 41 | 16, 16 | 345312, 345488 | same |
| 7 dep edge c009→c008 | 1079, 1079 | 72, 40 | 15, 16 | 345312, 345648 | same |

Case 6 lanes (sequential sum vs parallel wall):

| Lanes | sequential_ms | parallel_ms | lane_ms | contention |
| --- | --- | --- | --- | --- |
| 2 | 1931, 1949 | 944, 951 | [943,944], [950,951] | 0% |
| 4 | 3626, 3689 | 945, 957 | [945,943,942,942], [956,955,957,956] | 0% |

Speedup 2.0x/3.9x; every lane matches its sequential digest.

Case 7 toolchain: `cargo metadata` under 1.97.1 vs 1.98.1 parses to
identical `WorkspaceRecord`s (walls [42,14]/[16,16] ms, 11
packages). The dep half asserts 0→1 edges on a real manifest change.

Findings, not verdicts: selection marks all 44 obligations in every
mutation case (fail-open broad); plan walls cluster ~1 s regardless
of touch site, so plan cost is discovery-dominated.

Budgets:

- Same obligation set across all 7 cases: PASSED (digest
  `cd8b34ba18f0982d` in all 16 samples).
- Inventory reuse op counts: PASSED (exact: 1/10/100 members →
  2/11/101 legacy subprocesses vs 1 reused).
- Concurrent lanes without contention: PASSED (0% at 2 and 4 lanes;
  metadata lanes stay 1 by design, plans parallelize above that).
- Toolchain-change inventory stability: PASSED (identical records
  1.97.1 vs 1.98.1).
- Per-stage wall/memory split: PARTIAL — setup/plan/metadata-proxy
  recorded; RSS is test-process peak, not isolated plan RSS; the
  in-plan compiler cost is a proxy, not traced.
- CI-hosted walls, queue, cache transfer: UNMEASURED (no runs).
- Two-minute warm SMALL-FIXTURE path: still UNPASSED (unchanged).

Negative pipeline: `impl_neg_pipeline_p13` (9 tests) drives
config→discovery→plan→IR→workflow→reports→Required; every stage
fails closed (missing/invalid config, malformed member, empty head,
tampered IR, unwritable output, missing leg → NotRun, failed leg →
red verdict). Unknown/malformed bases broaden by design, never
error. The P05-gated deferral above is lifted: no P05 crate graph
was needed.

`scripts/verify-local.sh` now also pins per-crate `cargo test --doc`,
per-crate `cargo doc --no-deps`, and the named fixture suite; every
stage still runs and every failure is still listed.
