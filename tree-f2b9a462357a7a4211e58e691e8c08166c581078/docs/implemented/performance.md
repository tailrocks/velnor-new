# V1 performance acceptance — local + CI-observable measurements

Commit measured: `75f150a` (branch `docs/velnor-actions-spec`).
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

## 1. Empty-cache cold run — 56.06 s

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

- Concurrent: contract lane (88 tests) + cli lane (110 tests) → 7.86 s
  wall, both exit 0. MBX lane1 `36 hits, 3 misses`, lane2 `70 hits, 9
  misses`, `0 B` transferred.
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

Latest completed run 36540401062 (failure, 91 s created→updated):

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
No green run exists on this branch, so every CI budget verdict below is
UNMEASURED pending a green run on the named runner (and a merge-queue
run for the merge path, which has no samples at all).

## Budget verdicts

- Provisioned structural preflight < 2 s: local `./target/debug/
  velnor-actions plan` (warm, no compilation, analysis + report only)
  → 0.87 s wall. Supports the budget on fast hardware; UNMEASURED on
  the named hosted runner after tool provisioning.
- Warm leaf validation in seconds: compile 0.51 s (case 4) fits;
  whole-suite validation ~25 s does not — per-crate lanes (case 6,
  ~9 s for two small crates sequentially) are the conforming scope.
  Hosted-runner value UNMEASURED.
- 2-minute warm dogfood path on named hardware: local warm full runs
  are 11 s (nextest) to 56 s (cold compile+test), so the budget fits
  locally; the CI warm path is UNMEASURED (no green run; observed
  red runs span 1.5–11 min and are setup-dominated).
- Doctest gate: UNMEASURED — `cargo test --doc --workspace` fails
  locally with pre-existing E0063 at
  `crates/velnor-actions-orchestrator/src/config.rs:249` under
  `--cfg doctest` (missing `compile_driver`/`test_runner` fields).
  Not touched: outside this task's write allowance.
