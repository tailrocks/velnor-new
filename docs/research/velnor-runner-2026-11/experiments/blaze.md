# blaze on velnor-new: compatibility, speed, fidelity

Measured 2026-11-19 on macOS 27 arm64 (18 cores, 128 GB). Machine load average was 40-110 throughout (other agents), so times are indicative. No swap measurement was taken.
blaze tested: canary 0.20261105.4 (v2 rewrite, the newest release; matches the docs at the cloned HEAD 6e3d4da7). Stable channel is 0.20261027.0 (v1, `ra-jit` era); only the root `check` was run on it (failed: build script `velnor-runner-host` hit "unsupported: writable static MIN without a rustc symbol", 83 s).
velnor-new commit: HEAD moved during the run (other agents committing). B1/B2 extractions came from 09f030b542 or d6be276a6c (diff between them: 31 files, 572 lines, mostly generator crates); all B3-B6 trees are d6be276a6c. The exact commit per extraction was not logged.
Comparisons use the same trees and scripts in scripts/; raw logs in logs/ (timings.tsv has every timing with load average).

## Setup and isolation
- Installer read first (dl/installer.sh); docs install.md, toolchain.md, usage.md read before running.
- Run as `sh installer.sh --no-toolchain` (and `--channel canary`), then `blaze toolchain`, with HOME, BLAZE_HOME, XDG_{CACHE,CONFIG,DATA,RUNTIME}_HOME, CARGO_HOME, TMPDIR all under blaze-exp/ (scripts/env.sh; canary under blaze-exp/canary/). Installer puts binaries in $CARGO_HOME/bin and writes no rc file; it only prints a PATH hint.
- This blaze ignored BLAZE_TOOLCHAINS (toolchain landed in $HOME/.blaze/toolchains, i.e. inside scratch).
- Proof: real ~/.blaze absent after the run; mtimes of ~/.zshrc, .zprofile, .zshenv, .bashrc, fish config identical before/after (logs/rc-mtimes-before.txt vs after; final diff clean). ~/.local/share/mise and the repo were not touched (sources via `git archive`).
- Cargo baseline: ~/.cargo/bin cargo, RUSTUP_TOOLCHAIN=1.99.1, scratch CARGO_HOME, `cargo fetch --locked` first; `nice -n 10`, `-j 8`. blaze: BLAZE_CORES=8 (background nice 10). Equal CPU limit. cargo-nextest was not used (only in mise's dir); `cargo test` used.
- All daemons stopped; no blaze/ra-jit process remains.

## Research facts (docs at ~/.cache/ci-ambition/refs/blaze, HEAD 6e3d4da7, 2026-11-18)
- Command surface: build/check/run/test/bench; docs/usage.md:29-51 (usage.md:44 says fmt/clippy/doc are "no such command", stale: the 0.20261105.4 help lists check with lints, doc, fmt). `blaze check` runs rustc, clippy and blaze lints (docs/cargo.md:1883-1895); clippy lints are blaze's own ports of a chosen subset (docs/cargo.md:1947-1975 "A lint blaze has not ported is not run").
- Not supported: staticlib/dylib crate types, artifact deps; failures stop, nothing falls back to cargo (usage.md:44-51; cargo.md:1773-1781). Test check target fails for baselines (cargo.md "Tests are not counted yet").
- Backends: dev/test profile = copy-and-patch stencils, not Cranelift (cargo.md:1734-1752); release = LLVM. Cranelift only compiles proc-macro wasm.
- Borrow checker: own, Polonius-alpha by default (more permissive than NLL); 99.8% of rustc's borrowck tests in scope (borrowck.md:5-12; usage.md:122-124 still says "two thirds"); admits it accepts some code rustc rejects (borrowck.md:25-30).
- Design stance: "superset-of-rust: every program nightly accepts builds; rustc and cargo are references, not specs" (AGENTS.md:79-80).
- Benchmarks: all published numbers are v1, 2026-10-20..25; "No run of v2 has been published" (benchmarks.md:15-17). Method: harness/bench.py, dev profile, warm incremental `cargo build` vs `blaze build`, same linker, dedicated machine, median of 3-5 edits (benchmarks.md:31-70, 348-386, 485-500). Claimed edit-rebuild: ripgrep macOS 993 ms -> 126 ms (7.9x); geometric mean ~10x macOS, 11x Linux x86-64. Memory claim: 3.2x (Linux x86-64) to 12x (macOS) cargo; bun 26 GB, nushell 12.5 GB on macOS (benchmarks.md:108-130). "cargo is the speed baseline, not the answer (no harness compares blaze's output with cargo's)" (benchmarks.md:~365).
- Memory/disk: install.md:39-40 "a few GB for a small project, tens of GB for the largest"; remote-cache.md:25 a cold build of blaze itself needs >16 GB (OOM at 14.8 GB on 4 CPUs; 28.3 GB peak); memory-footprint.md:1-30: a stage5 self-build hit 75.7 GiB against a 47 GiB limit and the Mac swapped; memory-budget.md:14-30: CI "tests" job on a 15.6 GB runner was lost to shutdown/stuck since 2026-10-29. Both are plans (status: plan).
- Self-check vs rustc: no differential harness since 2026-10-30; "the bar is real crates' own tests" (support.md:14-16; v2/plan.md:178-182). Store check BLAZE_VERIFY_STORE recomputes a sampled fraction of store hits and compares bytes; the oracle compares incremental builds with clean blaze builds, not with rustc (v2/plan/design/verify.md:1-40).
- CI/remote cache: warm daemon on a long-lived machine, plan "check in under 20 s" (v2/plan/design/ci.md:1-40, plan); shared remote cache gateway/CAS, partly built (remote-cache.md:1-60); install.md:31-38 sandbox needs user namespaces or Landlock, containers need seccomp=unconfined or BLAZE_SANDBOX=off.
- Platform: Linux x86-64 listed "yes" (support.md:52-66, but numbers from CI run 39057174862 at 888e87df74, v1), "v2 has not been run on any platform in CI" at 2026-10-30 (platforms.md:5-9). Needs glibc >= 2.34 (install.md:26). Open issue #1855 (2026-11-06): canary daemon exits on every command on Ubuntu 24.04 stock kernel (io_uring PBUF_RING EINVAL). I could not test Linux.
- Pinning: `[workspace.metadata.blaze] version = "0.YYYYMMDD.N"` (or "stable"/"canary"); blaze ignores rust-toolchain and RUSTUP_TOOLCHAIN; always builds with blaze-2026-10-27 nightly 1.101.0 (cargo.md:1609-1680; toolchain.md:7-21, 257-268). toolchain.md:267 itself says keep cargo build/test in CI: "the project's toolchain decides".
- Stability record (gh, read-only, 2026-11-19): 432 open / 283 closed issues. Open wrong-result/false-verdict issues include #1522 (accepts test code rustc rejects: E0063, E0277 surface only as codegen refusals), #1604 (`blaze test` accepts crate-level compile_error!, exit 0), #1618 (wrong code: specializing impl missed through macro-only dependency), #1636 (false E0515 and minutes-long checks), #1600 (false E0311). Workflow history: blaze.yaml (merge gate) 0 successes in the 300 runs the API lists (since 2026-11-12); promote.yaml (canary->stable) 0 successes (30 failure, 8 cancelled of last 40) and stable is still the 2026-10-27 v1 build; canary.yaml 36/40 success; release.yaml 38/40 failed. No release since 2026-11-05 while main gained 2745 commits; #1780 (no_std proc-macro crate, hit below) was closed 2026-11-10 but is in no release, so results here may be fixed on main.

## B1 compatibility (canary 0.20261105.4; `--workspace --all-targets`)
| workspace | command | result | first error (category) | small patch? |
|---|---|---|---|---|
| `.` (root) | check, 12m32s; build started, killed unfinished | FAIL | `multiversion_no_op v1.0.0` "reaches 3 functions blaze could not compile" (generated wasm entry names std in a `#![no_std]` proc-macro crate; blaze bug, #1780 fixed on main only). Also `velnor-runner-host` panic: query "held 394335251 reads ... past 268435456" in `coroutine_obligation_errors` (blaze bug), and 19 false `env!("CARGO") not defined at compile time` in velnor-actions-rust tests (blaze bug). 49 units not checked | not tried |
| `ci-helper` (+generator path deps) | build, 2m27s | FAIL | same multiversion_no_op (via serde-saphyr) | patch (remove `#![no_std]` from a scratch copy via `[patch.crates-io]`): lib and bin build; test "it" (all velnor-ci integration tests, ~781) still fails: 897 errors "unresolved import `velnor_ci::io` ... could not find `io`" (blaze bug, below) + env!("CARGO") |
| `generator` | check 3m06s, build 3m10s | FAIL | same multiversion_no_op blocks velnor-gen and 8 units. velnor-model/rust/render/release build and pass | patch not run here (same as ci-helper patch) |
| `local` | check 3m24s, build (cold) 4m06s | FAIL | check: 142 false lint errors `allow(clippy::expect_used, ...) on this expression gives no reason` on every `#[tokio::test]` (clippy ignores macro-generated code; blaze lint false positive). build: velnor-local test "it" 702 errors `unresolved import velnor_local::io` | none |
| `crates/velnor-runner` | build (cold) 19m49s | FAIL | `turso_sdk_kit` via `icu_collator`: "can't normalize alias ... DataStruct" const-eval MirLowerError (unsupported feature in front end), blocks 25 units incl. velnor-runner-cli; plus velnor-resource-probe test "job_hook_public" 122 errors, same lib/bin issue | none |

Root cause of the "unresolved import" class, reproduced on a 3-file crate (work/repro-*): a package whose lib and bin share a name (the default `src/lib.rs` + `src/main.rs` layout) makes `blaze test` fail its integration tests ("could not find `io` in `repro`"); renaming the bin makes the same test pass. It is independent of `test = false`. This hits velnor-ci, velnor-gen, velnor-local, velnor-resource-probe.
Buildable subset used for B2-B6: generator `-p velnor-model -p velnor-release -p velnor-render -p velnor-rust --all-targets` ("S-gen"; 83 units; velnor-release has 149 tests).

## B2 cold build (same 8-core cap, nice 10)
| scope | blaze | cargo 1.99.1 (INCR=1) | load |
|---|---|---|---|
| S-gen (blaze: store and build dir wiped, std included) | 215.5 s, rc 0, daemon peak RSS 17.7 GB | 17.9 s (INCR=0: 13.1 s) | 50-71 / 63 |
| local whole workspace | 246.0 s, rc 101 (one test target failed), peak 18.9 GB | 26.8 s, rc 0 | 51-61 / 66 |
| generator whole | not completed (check 186 s rc 101) | 36.4 s | / 55 |
| runner whole | 1188.9 s, rc 101 (25+ units unbuilt), peak 34.3 GB | 265.4 s, rc 0 | 62-45 / 60 |
Cargo's RSS column in timings.tsv is only the largest single process (≈0.7-4.7 GB), not a tree total. Single runs, no repetition.

## B3 edit-rebuild, S-gen, warm (3 reps; median; edit adds `let _x = black_box(N);` to a fn body)
| edit | blaze | cargo INCR=1 | cargo INCR=0 | blaze peak RSS |
|---|---|---|---|---|
| (a) leaf fn, velnor-rust base_cache.rs | 21.3 s (34.1, 21.3, 10.1) | 1.5 s (1.2, 2.5, 1.5) | 9.2 s | ~20 GB |
| (b) widely used fn, velnor-model ids.rs `child` | 22.9 s (29.3, 22.9, 14.0) | 2.8 s (4.7, 2.8, 2.0) | 12.2 s (18.4, 12.2, 11.6) | ~21 GB |
| nothing changed | 2.2 s (after first, 47.9 and 7.5) | 0.2 s | - | up to 32 GB (two checkouts loaded) |
Load average 40-60. blaze did splice links ("spliced"), but the time is dominated by re-analysis plus its lints; blaze was 5-15x slower than cargo incremental and 1.5-2.5x slower than cargo non-incremental here. Not reproduced: the published 8-10x speedups (those are v1 numbers).

## B4 tests (velnor-release: 149 tests = 1 unit + 148 `it`)
- Pass/fail/skip sets by name: identical (149 ok, 0 failed, 0 ignored) between `blaze test -p velnor-release` and `cargo test` (logs/b4-names-*.txt, diff clean).
- First run after cold build: blaze 158.2 s vs cargo 13.5 s. Second run: blaze 45.5 s (not served from cache) vs cargo 0.2 s. After a body edit in velnor-release (3 reps): blaze 14.9/16.7/14.6 s (median 14.9) vs cargo 2.3/2.1/2.3 s. Load 63-108.
- velnor-ci (~781) and velnor-gen suites: could not run (B1: lib/bin-name bug, plus multiversion_no_op).

## B5 fidelity (velnor-release; blaze vs cargo 1.99.1 / clippy 0.1.99)
| fault | blaze | cargo | verdict |
|---|---|---|---|
| F1 type error | rc 101, E0308 (build and check) | rc 101, E0308 | match |
| F2 borrow E0597 dangling | rc 101, E0597 | same | match |
| F2b E0382 use after move | rc 101, E0382 | same | match |
| extra borrowck battery (E0502, E0499, E0505, E0506, E0716, E0373, E0507, E0596) | 8/8 rejected | 8/8 | match (E0507 message text differs: "move out of a shared reference" vs "out of index of Vec") |
| F3 failing assertion in one test | `blaze test` rc 101, FAILED on that test | same | match |
| F4a rustc deny lint (`unused_must_use = deny`) | rc 101 | rc 101 | match |
| F4b clippy deny lint (`unwrap_used = "deny"`, `Some(1).unwrap()`) | `blaze check` rc 0, `blaze build` rc 0, nothing reported | `cargo build` rc 0; `cargo clippy` rc 101 | FALSE GREEN vs clippy (blaze ports only a subset of clippy lints) |
Also false reds / noise: the clean control tree (F0) gave 0 clippy warnings but 18 blaze warnings (e.g. `map_unwrap_or`, `unused import MISE_LOCK`, `blaze::frozen_collection`); 142 false lint errors in `local` (tokio::test); false `env!("CARGO")` errors. Only a handful of borrowck shapes were tried; #1522/#1604 (open) show accepts-what-rustc-rejects cases exist (E0063, E0277 in tests; compile_error!).

## B6 resources
- Daemon RSS: 17.7-21 GB after S-gen cold/edit builds; 29-32 GB with a second checkout loaded; 34-44 GB on runner/ci-helper builds; ~20-34 GB for the root check. `blaze status` reported 16.8 GiB (RSS 22 GB) idle after S-gen. Daemon memory limit default: machine memory less 1 GB (127 GiB here).
- Disk after S-gen only: build dir 3.9 GB + state/store 5.3 GB (store 4.0 GB) + sources 0.65 GB + std library 79 MB + toolchain 559 MB (installed blaze binaries 172 MB for stable ra-jit / ~120 MB canary), vs cargo target 887 MB (INCR=1) / 435 MB (INCR=0) for the same scope. Total scratch peaked at 54 GB across all workspaces.
- Second checkout of the same S-gen tree (different path, store warm): 178 s (cold was 215.5 s; load 86-109): little sharing in wall time. cargo has no sharing across checkouts (~18 s cold anyway).

## Caveats
- Load average 40-110 on 18 cores all session; background-priority blaze vs equally niced cargo; single measurements for B2/B6; no swap numbers collected; macOS only (no Linux x86-64 run).
- Tested the newest release, which is 14 days and ~2700 commits behind main; several found bugs (#1780) are fixed on main but unreleased. HEAD of velnor-new moved mid-run.
- Edits/faults were in the S-gen subset because the full workspaces do not build. cargo-nextest not used. Patch experiment applied to scratch copies only.
- Root build was killed unfinished by the coordinator; B1 root result rests on the check run.

## Implications for Velnor (facts only)
- None of the 5 workspaces builds fully with the newest blaze release; the blockers are a no_std proc-macro dependency (multiversion_no_op), lib+bin-same-name integration tests, an unsupported const-eval case (icu_collator), a front-end query blow-up (runner-host) and missing `CARGO` env. velnor-ci and velnor-gen suites cannot run under it.
- On the one buildable subset, build/type/borrow/test-failure/rustc-lint verdicts matched cargo 1.99.1 in 14 of 14 faults, but clippy deny lints (unwrap_used) were not flagged: a false green if used in place of clippy; and it adds false-positive lint errors.
- Speed: cold 12x slower and edit-rebuild 5-15x slower than cargo incremental in these runs; the docs' speedups are v1 numbers and the docs say no v2 numbers are published.
- Memory per node: 18-21 GB for 4 crates, 30-44 GB for larger workspaces, vs cargo under 5 GB; docs cite a 15.6 GB CI runner failing under it. Ubuntu 24.04 daemon start bug #1855 is open.
- It always uses its own nightly (1.101.0) instead of the 1.99.1 pin, and blaze's own docs say CI should stay on the project toolchain. Merge gate and stable promotion of blaze were red throughout the observed window.
