**Status:** Proposed specification. These rules apply to the Velnor Actions generator workspace.

# Velnor Actions agent and performance contract

`AGENTS.md` MUST stay short. It MUST point to this contract, architecture,
task commands, generated-file rules, and invariants. It MUST require regression
tests, independent boundary assertions, reported checks, and review of snapshot
changes. It MUST forbid suppressing diagnostics, deleting tests, hand-editing
generated workflows, or weakening checks for a green result.

The repository MUST measure cold build, warm build, unchanged run, leaf edit,
shared-API edit, concurrent validation lanes, and toolchain update separately.
Record wall time, queue time, peak memory, compiled targets, tool installation,
cache transfer, MBX hits/misses, and task reuse. Initial acceptance budgets are
structure preflight under two seconds after tools are provisioned, warm leaf
validation in seconds, and a two-minute warm path for the small fixture on a
named runner including setup/cache transfer. Queue time MUST be reported
separately. Baselines MUST NOT update automatically to hide regressions.

`velnor-actions plan` MUST complete one analysis pass and reuse the same
detector, graph, workflow-IR, and renderer code as `generate`. Its report MUST
be deterministic, count detected stacks/workspaces/crates and planned matrix
entries/jobs, and match generated workflow facts. Measure its runtime with
structure preflight; it MUST NOT trigger Rust compilation or cache transfers.

## Acceptance table

| Requirement | Acceptance evidence |
| --- | --- |
| Repository policy | Velnor's `velnor-repository-v1` Alint job passes configured repository rules |
| Toolchain | Exact Rust/MBX/Nextest pins in version policy; optional project tool files are checked read-only; the pinned update workflow checks every package on declared MSRV |
| Workspace | Cargo metadata succeeds; Alint checks only its configured path/file rules |
| Tests | Required unit, integration, and doctest suites pass; no custom Rust AST or repository-policy linter is assumed |
| Limits | Oversized source, entrypoint, function, and document fixtures fail |
| Lints | Required Clippy/rustdoc output fails CI; unsafe policy is enforced |
| Tool path | Preflight proves exact Velnor pins ran through Mise and MBX handled compilation; all three project tool files remain byte-identical |
| Dependencies | cargo-deny and cargo-machete pass; Git sources are pinned |
| Workflows | Generated YAML is deterministic, action-pinned, actionlint/zizmor clean |
| Plan report | Stable text, no YAML/JSON, parity with generated jobs and matrix, no repository writes |
| Agent integrity | Policy files are protected; feature PR cannot remove its own gate |
| Performance | Named-runner measurements meet budgets; cache misses have reasons |

Velnor V1 implementation is ready to leave the proposal phase only when every
row passes from a clean checkout and the negative fixture suite demonstrates
that each bypass is rejected.
