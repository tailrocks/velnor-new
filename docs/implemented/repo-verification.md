# Repository Verification Procedures

## Overview

Run narrow, change-relevant checks as work proceeds. Before completion, run the full verification scope relevant to the change from the repo root; PR merge gates remain those required by CI.

> [!IMPORTANT]
> **Test Runner Enforcement**: Always run tests with `cargo nextest`; never use `cargo test`.
> Nextest executes tests in isolated worker processes with parallel scheduling and structured telemetry.

## Current owner-approved test reset

PR #391 clears the previous unit, integration, and doctest suites so they can
be rebuilt integration-first. During this transition, Cargo manifests disable
empty targets with `test = false`, and generated CI omits their unit/integration
test steps. Where `autotests = false` or a target is disabled, restoring a test
file alone does not make Cargo discover or execute it: register or enable the
Cargo target, confirm the generated task plan selects it, and check execution
evidence to confirm that it ran.

The repository-owned `test-maintenance-helpers` task uses `--no-tests warn`. A
full-workspace or nested-workspace Nextest run also uses `--no-tests warn`
while the workspace has no tests. This accepts an empty discovery; it does not
prove that a required suite ran or enforce its completion criteria. The task
generator currently uses `--no-tests warn` for hash-partitioned selections so
an individual empty shard is allowed; that behavior does not prove complete
suite coverage. As suites return, generated verification must enforce
fail-on-empty at each declared required-suite scope, while intentional filtered
or partitioned subruns may allow an empty selection only when the generated
plan selects the complete suite and execution evidence confirms its tasks ran.
V1 has no permanent empty-suite exception.

---

## The Verification Suite

Use these six root-workspace checks for full verification; for each change,
run the narrow checks that exercise it and complete the full relevant
verification scope before finishing. The nested runner is a separate Cargo
workspace and needs its own formatting, lint, test, and dependency checks; root
`cargo --workspace` and root formatting do not include it.

If the workspace `target` is a symlink into an MBX-managed cache, direct Cargo
commands do not hold an MBX cache lease. Use a separately owned absolute
`CARGO_TARGET_DIR` for the entire verification run so automatic cache collection
cannot remove active test binaries. During the reset, Nextest has no test target
to build the CLI. Leave `VELNOR_ACTIONS_BIN` unset so the freshness script uses
its pinned-Mise `cargo run` fallback, or set it to an already built executable
such as `$CARGO_TARGET_DIR/debug/velnor-actions`. Preserve the managed symlink
and global cache settings. Bounded build/test parallelism is permitted; it must
not omit any required test or gate.

```bash
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo nextest run --locked --workspace --no-tests warn
alint validate-config && alint check --fail-on-warning
cargo deny check
bash scripts/check-freshness.sh
```

---

## Command Reference & Usage Details

### 1. Code Formatting: `cargo fmt --all -- --check`

- **Purpose**: Validates Rust formatting in the root Cargo workspace. The nested runner workspace is excluded and must also pass `cargo fmt --manifest-path crates/velnor-runner/Cargo.toml --all -- --check`.
- **Why It Matters**: Enforces zero format variance and uniform code aesthetics without manual style debates.
- **Remediation**: Run `cargo fmt --all` for root files and `cargo fmt --manifest-path crates/velnor-runner/Cargo.toml --all` for runner files.

### 2. Static Analysis & Lints: `cargo clippy --locked --workspace --all-targets -- -D warnings`

- **Purpose**: Runs the Clippy linter across all targets (libraries, binaries, integration tests, examples, benchmarks) with warnings treated as hard errors.
- **Why It Matters**: Enforces proof invariants: `unsafe_code = "forbid"`, no `unwrap()`/`panic!` in production code, pedantic correctness checks, and API hygiene.
- **Flags**:
  - `--locked`: Guarantees reproducible builds against `Cargo.lock`.
  - `--all-targets`: Ensures tests and benchmarks meet the same lint standards as product crates.
  - `-D warnings`: Prevents warning accumulation.

### 3. Test Suite Execution: `cargo nextest run --locked --workspace --no-tests warn`

- **Purpose**: Runs all discovered unit, integration, and property tests across all workspace crates. `--no-tests warn` permits the intentional empty suite during the reset above.
- **Strict Prohibition**: **Never use `cargo test`**. `cargo test` is forbidden because it lacks process isolation, nextest profile management, and retry handling.
- **Focused Execution**:
  - Run a package that may still be empty during the reset: `cargo nextest run --locked -p <package_name> --no-tests warn`
  - Run a specific test and fail if the filter selects none: `cargo nextest run --locked -p <package_name> -E 'test(<test_filter>)' --no-tests fail`

### 4. Repository Structure & Agent Instructions: `alint validate-config && alint check --fail-on-warning`

- **Purpose**: Validates repository structural rules, agent instruction budgets, licenses, and configuration integrity.
- **Why It Matters**: Enforces that `AGENTS.md` remains lean (<= 100 physical lines, <= 16 KiB), links are verified, and repository policies are preserved.

### 5. Dependency Audit & Compliance: `cargo deny check`

- **Purpose**: Checks crate dependencies against security advisories, approved licenses, duplicate versions, and source registries.
- **Why It Matters**: Prevents unapproved licenses, yanked dependencies, duplicate conflicting dependencies, and vulnerable crates from entering the dependency graph.

### 6. Toolchain & Pin Freshness: `bash scripts/check-freshness.sh`

- **Purpose**: Verifies that toolchain pins, GitHub Actions versions, and external dependencies adhere to `.velnor/version-policy.toml` and `.velnor/freshness-inventory.json`.
- **Why It Matters**: Prevents unrecorded drift from upstream tooling and validates active pin exceptions.

---

## Nested Runner Workspace (`crates/velnor-runner`)

The runner controller and supervisor live in the nested virtual workspace `crates/velnor-runner`, which has its own `Cargo.toml`, `Cargo.lock`, and `deny.toml`. To verify the runner workspace (always using `cargo nextest`):

```bash
cargo fmt --manifest-path crates/velnor-runner/Cargo.toml --all -- --check
cargo clippy --manifest-path crates/velnor-runner/Cargo.toml --locked --all-targets -- -D warnings
cargo nextest run --manifest-path crates/velnor-runner/Cargo.toml --locked --no-tests warn
cargo deny --manifest-path crates/velnor-runner/Cargo.toml check
```
