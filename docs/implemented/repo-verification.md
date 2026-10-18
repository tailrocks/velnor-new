# Repository Verification Procedures

## Overview

All changes must pass the full repository verification suite from the repo root before commit and PR merge.

> [!IMPORTANT]
> **Test Runner Enforcement**: Always run tests with `cargo nextest`; never use `cargo test`.
> Nextest executes tests in isolated worker processes with parallel scheduling and structured telemetry.

---

## The Verification Suite

Run all six checks sequentially from the workspace root.

If the workspace `target` is a symlink into an MBX-managed cache, direct Cargo
commands do not hold an MBX cache lease. Use a separately owned absolute
`CARGO_TARGET_DIR` for the entire verification run so automatic cache collection
cannot remove active test binaries. After Nextest builds the CLI, set
`VELNOR_ACTIONS_BIN="$CARGO_TARGET_DIR/debug/velnor-actions"` for the freshness
script. Preserve the managed symlink and global cache settings. Bounded build/test
parallelism is permitted; it must not omit any required test or gate.

```bash
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo nextest run --locked --workspace
alint validate-config && alint check --fail-on-warning
cargo deny check
bash scripts/check-freshness.sh
```

---

## Command Reference & Usage Details

### 1. Code Formatting: `cargo fmt --all -- --check`
- **Purpose**: Validates that all Rust code across all workspace members strictly conforms to Rust 2024 edition formatting rules.
- **Why It Matters**: Enforces zero format variance and uniform code aesthetics without manual style debates.
- **Remediation**: If formatting fails, run `cargo fmt --all` to automatically format all files.

### 2. Static Analysis & Lints: `cargo clippy --locked --workspace --all-targets -- -D warnings`
- **Purpose**: Runs the Clippy linter across all targets (libraries, binaries, integration tests, examples, benchmarks) with warnings treated as hard errors.
- **Why It Matters**: Enforces proof invariants: `unsafe_code = "forbid"`, no `unwrap()`/`panic!` in production code, pedantic correctness checks, and API hygiene.
- **Flags**:
  - `--locked`: Guarantees reproducible builds against `Cargo.lock`.
  - `--all-targets`: Ensures tests and benchmarks meet the same lint standards as product crates.
  - `-D warnings`: Prevents warning accumulation.

### 3. Test Suite Execution: `cargo nextest run --locked --workspace`
- **Purpose**: Runs all unit, integration, and property tests across all workspace crates.
- **Strict Prohibition**: **Never use `cargo test`**. `cargo test` is forbidden because it lacks process isolation, nextest profile management, and retry handling.
- **Focused Execution**:
  - Run specific package: `cargo nextest run --locked -p <package_name>`
  - Run specific test: `cargo nextest run --locked -p <package_name> -E 'test(<test_filter>)'`

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
cargo clippy --manifest-path crates/velnor-runner/Cargo.toml --locked --all-targets -- -D warnings
cargo nextest run --manifest-path crates/velnor-runner/Cargo.toml --locked
cargo deny --manifest-path crates/velnor-runner/Cargo.toml check
```
