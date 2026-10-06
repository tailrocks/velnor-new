# Velnor V1 Rust Verification Contract

This is a binding extension of the [Rust quality contract](rust-quality-contract.md).

## Required verification

If structure blocks a clean change, land the smallest
behavior-preserving refactor first (verified with golden parity and
committed), preserving error outcomes, ordering, defaults, serialization,
IDs, hashes, and process behavior; the behavior change gets its own commit
and tests. A red baseline is reproduced and classified first, never waived.

Mise MUST expose these focused task templates; each invocation MUST target one
package/configuration and preserve the real exit status. `dependencies` is
repository-wide because it inspects manifests without compiling every crate.
Velnor's repository policy runs Alint in its own job; ordinary consumer CI
does not require it. The generated Velnor CI MUST use one matrix
entry per selected crate as specified in the workflow contract; it MUST NOT
replace those entries with workspace-wide Clippy or test runs.

The following are logical task definitions, not shell command templates.
`velnor-actions-mise` emits and executes them through pinned Mise. Exact
executable and argument vectors for generated Rust tasks are defined by the
[task execution contract](task-execution-contract.md); each task uses the
workspace's detected Cargo or MBX compile driver and Cargo-test or Nextest test
runner. Generated workflow YAML does not invoke Rust commands outside Mise.

```text
fmt-check          Formatting validation once for the selected source tree
dependencies       Dependency policy and unused direct dependency checks
actionlint         Generated workflow syntax validation
zizmor             Generated workflow security validation
clippy <pkg>       Configured Clippy validation for one package
test-build <pkg>   Build once for one package when selected runner needs an archive
test <pkg>         Run Cargo test or the prepared Nextest configuration
doctest <pkg>      Run documentation tests for one package
doc <pkg>          Build package documentation with warnings denied
msrv <pkg>         Check one package on the exact declared minimum Rust version
```

Every command above runs through Mise. Rust compiler invocations pass through
MBX only for workspaces whose detected profile selects it. A full-workspace
compile/test MAY run as an explicit release or diagnostic audit, but MUST NOT
be the default PR task.

The default feature/target matrix MUST be explicit. Do not use `--all-features`
as a substitute for a supported matrix. `cargo-hack` MAY enumerate feature
combinations in an isolated checkout. Each product crate MUST be checked on
MSRV in the toolchain-update qualification workflow using the Mise-managed
tool version equal to workspace `rust-version`; it MUST use `--locked`. Action
workflows MUST also pass `actionlint` and `zizmor`, use full commit-SHA pins,
least-privilege permissions, and avoid privileged execution of untrusted
pull-request code.

Risk-triggered verification MUST add `cargo-mutants` for important behavior,
`cargo-fuzz` for untrusted parsers, Miri for unsafe/low-level code, Loom for
custom synchronization, and `cargo-semver-checks` for published APIs. Coverage
does not replace behavioral or mutation evidence. Retries are disabled by
default; a retry requires a reviewed reason.

Agent instructions, performance measurements, acceptance budgets, and
readiness evidence are specified in the [agent and performance
contract](agent-and-performance-contract.md).
