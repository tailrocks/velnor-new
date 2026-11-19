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

`scripts/verify-local.sh` first installs the exact Bun version declared in the
root `mise.toml` and verifies that `mise.lock` declares the same version. It
checks the resolved Bun binary with `bun --version` and fails if installation
changes the committed lock. This Bun prerequisite is separate from the Rust
policy `SPECS` comparison below. The repository-owned orchestrator test suite
uses that same project Bun pin in its preparation step and tools-cache identity.

`scripts/verify-local.sh` uses offline Cargo metadata to identify packages
with Nextest-applicable targets (`test = true`). A successful empty result
skips Nextest only for that workspace; metadata errors fail verification. The
root workspace runs per-package and workspace-wide Nextest scopes with
`--no-tests fail` when applicable targets exist. The required
`test-maintenance-helpers` task applies the same metadata rule to its two named
package scopes and first runs the `pin-mirror` repository-policy action against
the PR head with `refs/remotes/origin/main` as the base ref. Each invoked
Nextest scope fails when its selected suite is empty. The task generator
separately uses `--no-tests warn` for hash-partitioned selections so an
individual empty shard is allowed; that does not prove complete suite coverage.
As suites return, generated verification must enforce fail-on-empty at each
declared required-suite scope, while intentional filtered or partitioned subruns
may allow an empty selection only when the generated plan selects the complete
suite and execution evidence confirms its tasks ran. V1 has no permanent empty-suite exception.

Before either root verification entry point runs `velnor-archive-guard` tests,
it builds the `qualification-diagnostics` CLI example and resolves its Cargo
compiler artifact through the repository-policy executable selector with kind
`example`. It exports the absolute executable path in
`VELNOR_QUALIFICATION_DIAGNOSTICS_FIXTURE`. A missing or invalid artifact fails
the test scope; tests do not invoke Cargo to build it.

---

## The Verification Suite

For complete Rust verification, run `bash scripts/verify-local.sh` from the repo
root. It checks the root workspace, including formatting, Clippy,
metadata-gated Nextest, docs, and freshness. Run the independent repository
checks alongside it. For each change, run narrow checks that exercise it and
complete the full relevant verification scope before finishing.

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
bash scripts/verify-local.sh
alint validate-config && alint check --fail-on-warning
cargo deny check
```

The local script also runs `scripts/check-freshness.sh`.

Tests that need a fixture or an environment variable that only the verification
entry points set are run with the recipe in
[Local recipe for fixture-dependent tests](#local-recipe-for-fixture-dependent-tests).

## Local recipe for fixture-dependent tests

A plain `cargo nextest run --workspace` fails the tests of `velnor-actions-cli`,
`velnor-actions-mise` and `velnor-archive-guard` when their fixture variable is
missing. Each such failure names this section. Run a crate with its recipe
below, or run `bash scripts/verify-local.sh`, which sets every variable and runs
the whole suite.

| Variable | Value | Set by | Read by |
|---|---|---|---|
| `VELNOR_IMAGE_PUBLICATION_FIXTURE` | Absolute path to the `image-publication-fixture` example executable | `scripts/verify-local.sh`, CI | `velnor-actions-cli` `image_publication_public` |
| `VELNOR_QUALIFICATION_DIAGNOSTICS_FIXTURE` | Absolute path to the `qualification-diagnostics` example executable | `scripts/verify-local.sh` | `velnor-archive-guard` `golden_collector_public` |
| `VELNOR_QUALIFICATION_CLOCK_DRIVER` | Absolute path to the `golden_collector_clock` example executable | `scripts/verify-local.sh` | `velnor-archive-guard` `golden_collector_public` |
| `VELNOR_GROUP_PROBE_RECEIPT` | Receipt written by `scripts/provision-group-signal-interposer.sh` (macOS) | `scripts/verify-local.sh` on macOS | `velnor-actions-mise` `permission_retry_darwin` |

`velnor-actions-cli` `check_runtime_identity_public` also needs the `mise` on
`PATH` to be the exact pinned release (`MISE_VERSION` in
`crates/velnor-actions-mise/src/catalog.rs`). A different mise build fails with
`mise_binary_unreadable`, because the orchestrator reads at most the pinned byte
count; the progress test states this precondition before it runs. `mise.toml`
does not pin mise itself, so install the release with
`mise install aqua:jdx/mise@2026.11.1` and put its binary first on `PATH`:

```sh
PATH="$HOME/.local/share/mise/installs/aqua-jdx-mise/2026.11.1/mise/bin:$PATH" \
  "$HOME/.cargo/bin/cargo" nextest run -p velnor-actions-cli --test check_runtime_identity_public progress
```

The `~/.local/bin/mise` shim does not satisfy this check, so the pinned binary
must come first on `PATH` for the run. Call `$HOME/.cargo/bin/cargo` directly:
the MBX `cargo` wrapper on the default `PATH` rebuilds `PATH` before it runs
the test, which drops the pinned binary from the front.

The example paths must be canonical, so each recipe resolves the examples
directory with `cd -P`. It uses `CARGO_TARGET_DIR` when it is set, and `target`
otherwise.

### velnor-actions-cli

```bash
cargo build --locked -p velnor-actions-cli --example image-publication-fixture && \
VELNOR_IMAGE_PUBLICATION_FIXTURE="$(cd -P "${CARGO_TARGET_DIR:-target}/debug/examples" && pwd)/image-publication-fixture" \
  cargo nextest run --locked --no-fail-fast -p velnor-actions-cli
```

### velnor-archive-guard

```bash
cargo build --locked -p velnor-actions-cli --example qualification-diagnostics && \
cargo build --locked -p velnor-archive-guard --example golden_collector_clock && \
examples="$(cd -P "${CARGO_TARGET_DIR:-target}/debug/examples" && pwd)" && \
VELNOR_QUALIFICATION_DIAGNOSTICS_FIXTURE="$examples/qualification-diagnostics" \
VELNOR_QUALIFICATION_CLOCK_DRIVER="$examples/golden_collector_clock" \
  cargo nextest run --locked --no-fail-fast -p velnor-archive-guard
```

### velnor-actions-mise (macOS)

The receipt names a private interposer library. The recipe provisions it in a
temporary directory, resolved to its canonical path, that is removed when the
command exits:

```bash
set -eu
probe_root="$(cd -P "$(mktemp -d)" && pwd)"
trap 'rm -rf "$probe_root"' EXIT
bash scripts/provision-group-signal-interposer.sh "$probe_root/fixture"
VELNOR_GROUP_PROBE_RECEIPT="$probe_root/fixture/group_signal_zero_interpose.json" \
  cargo nextest run --locked --no-fail-fast -p velnor-actions-mise
```

The tests verify the receipt, fixture source, and library before use.

---

## Command Reference & Usage Details

### 1. Code Formatting: `cargo fmt --all -- --check`

- **Purpose**: Validates Rust formatting in the root Cargo workspace, which holds every crate.
- **Why It Matters**: Enforces zero format variance and uniform code aesthetics without manual style debates.
- **Remediation**: Run `cargo fmt --all`.

### 2. Static Analysis & Lints: `cargo clippy --locked --workspace --all-targets -- -D warnings`

- **Purpose**: Runs the Clippy linter across all targets (libraries, binaries, integration tests, examples, benchmarks) with warnings treated as hard errors.
- **Why It Matters**: Enforces proof invariants: `unsafe_code = "forbid"`, no `unwrap()`/`panic!` in production code, pedantic correctness checks, and API hygiene.
- **Flags**:
  - `--locked`: Guarantees reproducible builds against `Cargo.lock`.
  - `--all-targets`: Ensures tests and benchmarks meet the same lint standards as product crates.
  - `-D warnings`: Prevents warning accumulation.

### 3. Test Suite Execution: `bash scripts/verify-local.sh`

- **Purpose**: Runs root per-package and workspace Nextest scopes. Cargo metadata decides whether each workspace or named package scope has a `test = true` target; only a successful empty result skips Nextest. Metadata errors fail, and each invoked Nextest scope uses `--no-tests fail`.
- **Strict Prohibition**: **Never use `cargo test`**. `cargo test` is forbidden because it lacks process isolation, nextest profile management, and retry handling.
- **Focused Execution**:
  - Run a package whose Cargo metadata reports a test-enabled target: `cargo nextest run --locked -p <package_name> --no-tests fail`
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
- **Checks**:
  - `local-pin`: Compiled-in constants match reviewed inventory pins, including
    the policy mirror and cargo-mutants pin.
  - `effective-identity`: Declared requirements match every Cargo.lock
    resolution across discovered workspaces and dependency scopes.
  - `upstream-freshness`: Reviewed pins have fresh upstream evidence; stale pins,
    stale evidence, and lookup failures fail closed.
  - `advisories`: Deny policy and optional live scans cover every Cargo
    workspace, with no ignored advisories.
- **Output**: Each `row: {...}` line is a compact JSON object with `check`,
  `subject`, `status`, and `detail` keys. A row with `status: fail` makes the
  command exit nonzero.
- **Options**: `--root DIR` validates a fixture tree instead of the repository.
  `--check-upstream` performs a bounded, read-only latest-stable lookup with a
  10-second timeout and 512 KiB encoded/decoded cap; it writes nothing and runs
  weekly in CI outside build gates. `--with-advisories` runs the live Cargo Deny
  scan for every workspace, with a 180-second timeout per workspace, and the
  local deny-policy checks.

---

## Runner Crates (`crates/velnor-runner-*`)

The runner controller and supervisor crates (`crates/velnor-runner-*` and `crates/velnor-resource-probe`) are members of the root workspace. They use the root `Cargo.toml`, `Cargo.lock` and `deny.toml`, and `scripts/verify-local.sh` verifies them with the root stages.

The Docker-readiness fixture rewrites the entrypoint to no-op its absolute-path
`wait-docker-sock` and `clear-dockerenv` commands because the fixture stub root
does not provide those image paths. It verifies the Docker API-readiness gate
only; it does not exercise those entrypoint preflights.

## OpenTofu Release Goldens

Golden capture and check require the checkout's independently built
`velnor-archive-guard`. Build it with the current Rust build guard and MBX,
then copy the executable into a user-owned directory with mode `0700`. Record
its SHA-256 from the build output. The executable must be a regular file owned
by the current user, have one link, and must not be writable by group or other
users. The parent directory must be canonical and private.

Pass both guard values to every collector command:

For the full shell harness, also build the CLI package's
`qualification-diagnostics` Cargo example in release mode through the same
Rust build guard and MBX. Record its SHA-256 independently and pass its path
and digest. This native executable shares the production CLI entry and adds
controlled plan diagnostics without changing release manifests.

```bash
/private/path/velnor-golden-collector check \
  --root "$PWD" --cli /private/path/velnor-actions \
  --archive-guard /private/path/velnor-archive-guard \
  --archive-guard-sha256 <independent-64-character-sha256>
scripts/test-capture-opentofu-goldens-bin.sh \
  --archive-guard /private/path/velnor-archive-guard \
  --archive-guard-sha256 <independent-64-character-sha256> \
  --diagnostics-fixture /private/path/qualification-diagnostics \
  --diagnostics-fixture-sha256 <independent-fixture-64-character-sha256> \
  /private/path/velnor-actions
```

The collector checks file ownership, permissions, link count, canonical path,
and the supplied digest before it runs the guard. It then verifies the guard's
compiled source fingerprint against the current checkout. It checks the file
identity, digest, and source fingerprint again during candidate execution. A
missing or invalid explicit guard fails. The collector does not select another
guard or run the builder after that failure. Both options are required.

Candidate commands use bounded process-group cleanup. This is not an operating
system sandbox: repository code running as the same user can access that user's
files, and a process that changes its session or process group can escape the
original group. Qualification snapshots detect observed changes; they do not
isolate hostile code. Pull-request candidate evidence does not authorize image
publication, which separately requires the protected-main source and authenticated
producer and attestor identities.

Use `check` to compare the five CLI goldens without changing checked-in files.
Use `capture` only after a known-good local result. The separate shell harness
continues to exercise release-candidate checks with its external oracle.
