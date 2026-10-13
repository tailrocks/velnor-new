# C08 / T25: test preparation and minimal tools

Qualification: local command semantics and focused regression tests only.
Fresh hosted-runner performance, cohort comparisons and archive-transfer costs
remain unmeasured.

## Root causes and generator fixes

`workflow.rs::plan_uses_rust` previously installed Rust for every non-Tofu
consumer, including tools-only repositories with no Rust evidence. It now
requires detected Rust workspaces/proposals or generator repository policy.
Generator policy retains the current-source helper bootstrap. Validators remain.

The Nextest-only `Build` group advertised test executables but emitted ordinary
`build`. It now emits `nextest list --list-type binaries-only`, using the same
profile, package, manifest, features and target as the test runner. This builds
test binaries without executing or discovering tests. The subsequent test
obligation still executes; doctests remain separate. Synthetic Cargo-test Build
groups use `test --no-run`.

## Exact tool evidence

Pinned Nextest 0.9.146 source `src/dispatch/core/list.rs:86–118` loads the selected
profile, invokes the shared binary builder, and returns binaries without harness
listing. `src/dispatch/core/base.rs:550` prepares `cargo test --no-run`.
Sources inspected in the local registry package for this exact version.

Exact Mise-installed MBX 1.21.0 and Nextest 0.9.146 executed an isolated
dependency-free fixture containing an intentionally panicking test. Separate
`MBX_CACHE_DIR` and `CARGO_TARGET_DIR` prevented cache/target contention.

1. Version probe confirmed Nextest 0.9.146.
2. Binary preparation succeeded, emitted no injected panic: 0.624 s wall.
3. Nextest run executed the test and failed with exit 100 / injected panic:
   1.296 s wall.

Raw commands, durations and logs: `ci-performance-nextest-evidence/`. These local
durations prove command behavior; they establish no hosted performance claim.
An earlier ambient Nextest 0.9.143 probe was excluded from pinned qualification.

## Regression and independent review

- Rust argv tests: 13 passed.
- Rust task derivation tests: 11 passed.
- Orchestrator bootstrap-role tests: 3 passed.
- Rust all-target Clippy with warnings denied: passed.
- Independent read-only reviewer inspected actual diff, dispatch/tool selection,
  and pinned Nextest source; no actionable findings.

## Scheduling and archive qualification remains open

Historical telemetry in `docs/implemented/performance.md` records contract test
body 0.211 s, Mise 1.262 s, Rust 0.323 s, renderer 0.298 s, CLI 23.910 s, and
orchestrator 679.316 s. Earlier build timings measured ordinary builds, so they
cannot qualify corrected test preparation. No paired Nextest archive creation,
upload/download and direct execution measurement exists. Cohort/archive changes
require new measurements and feature/runtime/fixture equivalence proof.

Report retrieval currently deduplicates job artifacts but downloads exact-name
artifacts sequentially. Bounded concurrency needs measured transfer evidence and
run/attempt/provenance regression tests before adoption.
