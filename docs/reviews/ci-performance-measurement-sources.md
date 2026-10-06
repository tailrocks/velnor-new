# CI performance measurement sources

Status: source qualification only, 2026-10-03. No hosted performance claim.
Read the complete performance specification, including T01–T26 and reporting
requirements. Inspect MBX 1.21.0 source at `/tmp/velnor-mbx-source-1.21.0`.
This review defines the narrow supported reporting integration and identifies
upstream requirements before Velnor can attribute measurements correctly.

## Existing Velnor hooks

| Owner | Hook | Required integration |
|---|---|---|
| Orchestrator | `matrix_step.rs::obligation_step` | Derive an isolated report directory from the validated obligation matrix key; pass the supported upstream measurement setting through existing validated task env. |
| Orchestrator | `task_report.rs::write_task_report_to` | Read bounded completed reports after the real task exits; bind them to the validated plan identity; missing/cancelled/malformed telemetry stays unknown. |
| Contract | `workflow/report.rs::TaskReport` | Add optional supported measurement evidence, separate from task verdict, cache-result reuse and baseline coverage. |
| Renderer | Existing crate report upload | Include reviewed raw owning-tool reports with task evidence; keep upload on failure and preserve private evidence boundaries. |
| Collector | Existing attempt/log/report collector | Combine task evidence with actual job/step timestamps and archive-transfer observations. |

`TaskReport.duration_ms` currently records elapsed task-wrapper wall time.
The audited `schedule.rs::measured_timing` put that wall time in `task_ms` and
filled all other slots with zero. Those zeros never established measured compiler,
cache, download, test-only or queue durations. The structural timing fix replaces
unmeasured slots with absent optional values, retains the measured task wall with
`TaskWrapperWall` provenance and rejects unsupported category observations.
New process measurements must not be
put into additive `TaskTiming` slots: concurrent process durations can exceed
task wall time, and subprocess intervals may overlap their parents.

## Supported MBX 1.21.0 interface

`MBX_STATS_REPORT` is supported in `config.rs:137`, documented in
`docs/cli/configuration.md:493`, and written by
`session/stats.rs::write_stats_report` at line 509. The writer emits a versioned
JSON document with `version = 5` and atomically replaces one configured file.
`docs/stability.md:36` explicitly supports this interface; the session JSONL
streams are explicitly implementation details. Do not parse those streams.

| Supported field | Interpretation |
|---|---|
| `session_duration_ns` | Owning cache-session wall interval; excludes some later application execution. |
| `compiler[outcome].invocations` / `.duration_ns` | Actual instrumented process work, grouped by miss, unconsulted, bypass or verification. Cumulative process durations, not job wall time. |
| `wrapper_phases_ns` | Exclusive cumulative wrapper phase durations; parallel wrappers do not sum to build wall time. |
| `hits`, `misses`, `unconsulted`, `bypasses`, `verifications` | Distinct owning-tool outcomes; retain every category and bypass reason. |
| `predictions_loaded`, `prefetched_actions` | Prediction/prefetch activity; neither proves task-result reuse. |
| `estimated_compiler_duration_avoided_ns` | Historical estimated avoided work; never actual elapsed savings. |
| `downloaded_bytes`, `uploaded_bytes` | MBX remote object payload traffic; excludes GitHub archive transfer. |

Use the emitted `misses` field. Do not derive misses by subtracting hits from
lookups: `session/stats.rs:528` explains why repeated key probes invalidated
that earlier approach. Actual failed processes remain part of measured work;
for example `rustdoc.rs:152` records before checking process success.

### Attribution limits

1. The `compiler` outcome map mixes Rust compiler, C/C++ compiler, supported
   build-script execution (`build_script.rs:361`) and rustdoc
   (`rustdoc.rs:152`). It cannot establish separate compiler/link/build-script/
   rustdoc totals or identify third-party-only compilation.
2. `slow_compilations` retains only the five slowest labels. It is not a
   complete package/process ledger. Unknown third-party work cannot become zero.
3. Wrapper phases cover instrumented Rustc and C/C++ attempts. Upstream
   `docs/tui.md:208` records incomplete fallback coverage and uninstrumented
   rustdoc. Separate build-script/rustdoc durations remain unknown.
4. The stable JSON report has no session ID, parent ID, command identity,
   complete per-adapter identity, start/end envelope or completion status.
   Velnor cannot independently bind its totals to one outer task session.

### Nested overwrite is a structural blocker

The Cargo shim reuses an enclosing session when a socket is present
(`cli/shim.rs:83,168`). This covers ordinary nested Cargo calls during an
active build. It does not make one fixed report filename safe for all tasks.

`cli/cargo.rs:509–529` finishes and writes the outer build report before a
captured application/test launches. `cli/launch.rs:32,67,221` restores the
caller environment, including its `MBX_STATS_REPORT`. An explicit MBX command
inside that application/test can start another session and overwrite the
outer file. Multiple inner processes can collide. A unique filename per
obligation still permits those collisions inside the obligation.

Do not accept the last completed session as the obligation's compiler total.
Do not disable nested caching, mutate test behavior, add a compiler wrapper,
watcher, FIFO collector or second execution engine to work around this.

## Narrow upstream extension required

The following names describe a proposed API, not existing supported settings:

1. Add a documented report-directory setting. Each owning MBX session writes
   one immutable versioned report with an unpredictable unique session ID;
   atomic publication must never overwrite another session's report.
2. Include session ID, optional parent session ID, start/end interval,
   completion outcome and bounded command/workspace identity. Honor an opaque
   caller correlation value so Velnor can bind nested work to one obligation.
3. Expose actual process records or cumulative counters by adapter and
   package/unit identity, with exit outcomes and subprocess overlap explicit.
   Retain compiler-query and unsupported bypass categories. Separate measured
   work from estimates and historical cache metadata.
4. Include supported build-script and rustdoc timing; record linker child
   process work separately only where the owning tool can observe it without
   changing command semantics. Otherwise expose explicit unavailable reasons.
5. Report abandoned/incomplete session envelopes where possible. Cancellation
   and absent reports must not fabricate complete telemetry. Telemetry failure
   must preserve the task's real exit status.

Implement this in MBX's existing report writer, adapters and session ownership.
Then qualify the exact released interface/pin. Velnor should only configure
that interface, decode its documented version, attach evidence to existing
reports and upload it. No session-internal parser or additional task runner.

## Cargo and Rustc stable interface limits

The installed Rust 1.98.0 official docs agree with the current official
[Cargo build documentation](https://doc.rust-lang.org/cargo/commands/cargo-build.html):
stable `--timings` emits a human HTML report. It does not offer a supported
machine timing schema. The
[timing guide](https://doc.rust-lang.org/cargo/reference/timings.html) describes
unit/concurrency intervals and distinguishes custom build units. Under MBX a
Cargo unit interval includes wrapper/cache work, so it cannot independently
prove a real compiler process ran. Do not scrape embedded HTML JavaScript.

Cargo's supported
[JSON messages](https://doc.rust-lang.org/cargo/reference/external-tools.html)
can identify package/target/profile and `compiler-artifact.fresh`. A nonfresh
artifact can still be served by MBX after Cargo invokes the wrapper. A
`build-script-executed` message can contain cached output even when no script
ran. Neither gives a supported process/link/build-script duration. Collecting
these messages also needs owning-tool support to preserve existing stdout and
diagnostics; it is not permission to change task payload semantics.

Rustc's documented
[JSON section timings](https://doc.rust-lang.org/rustc/json.html#timings) are
unstable. A direct Rust 1.98.0 compilation probe, using an empty `/dev/null`
library and only a `/tmp` output, rejected `--json=timings` with the diagnostic
that it requires `-Zunstable-options`. The version-only invocation accepted
the flags without compilation, so `--version` is insufficient qualification.
No nightly flag, `RUSTC_BOOTSTRAP`, profiling flag or linker override was added.

## Remaining qualification limits

T01–T03 still need fresh hosted runners, immutable candidate/configuration and
an isolated namespace. Aggregate v5 counters alone cannot prove zero avoidable
eligible third-party compilation. Full obligation correctness, task-result
reuse, baseline coverage, GitHub archive bytes, tool downloads, queue/provision
time and test-only time need their respective existing owning sources.
Missing data stays unknown. This document does not waive those requirements.
