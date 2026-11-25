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
`--no-tests fail` when applicable targets exist. The
`test-maintenance-helpers` task (a local full run: no generated workflow job runs it, so it is
not a required check) applies the same metadata rule to its two named
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

### 7. No Doctests: `bash scripts/check-no-doctests.sh`

- **Purpose**: Fails when rustdoc lists any doctest in the workspace (policy: `docs/proposed/rust-test-policy.md`). Crates that set `doctest = false` are listed too.
- **How**: Runs `cargo doc --workspace --no-deps --lib --bins --examples` with `RUSTDOCFLAGS="--test --test-args --list"` once for every combination of the workspace's features (each `package/feature` pair on or off, except `default`) with `--no-default-features` and, when a workspace member declares a `default` feature, once more with the default features on top. More than six features fail the check as too many to enumerate. The check reads Cargo's merged configuration (`cargo config get`) and fails when any source sets `rustdocflags`, whatever the key spelling. It matches only the `(line N): test` lines of rustdoc's standard output, so a Cargo or build-script warning cannot be taken for a doctest. A Cargo failure fails the check and prints Cargo's output.
- **Limits**: rustdoc sees the host target and the feature combinations above only. An item behind `cfg(windows)` or another operating system, is not listed. Run the script on that operating system to cover its items.
- **Where it runs**: `mise run check-no-doctests` locally. In CI it is a `[[test_setup]]` step with the default phase in `.velnor/rust-ci.toml`, so it runs before the tests of the root's jobs, the test steps wait for it, and a push that the merge group verified skips it like the other checks. It always lists the whole workspace: a setup step cannot read the planned package selection.
- **Fixtures**: `mise run test-no-doctests-listing` plants fixture workspaces (fence forms, binary and example targets, `cfg(not(feature))`, a build-script warning, a discovery failure) and compares each result with the expectation, printing the checker output on a mismatch. The generated lint job "No-doctests listing fixtures" runs it when either script, `mise.toml` or `mise.lock` changes.

### 8. Private Names: `python3 scripts/private-names-guard.py`

- **Purpose**: Keeps private downstream organization and repository names out of tracked files.
- **Authority**: `scripts/private-names-guard.py` alone decides whether a private name is present. It reads the bytes of every tracked path, file and symlink target, independent of file type, extension case or structure, and compares them with the salted digests in `scripts/private-names-hashes.txt`. Matching is case-insensitive and also runs on the URL-decoded, `\uXXXX`-unescaped, `\/`-unescaped and HTML-unescaped text, after Unicode NFKC normalization with zero-width characters and combining marks removed. It matches a name inside identifiers, an `org/repo` pair with spaces around the slash or a `.git` suffix, and a name written with spaces, tabs, hyphens, underscores, dots or nothing between its letters and words, such as `Title Case Spaced`, `hyphen-form`, `under_score` or `dot.form`. A private name that is also an ordinary phrase therefore flags that phrase; reword the prose.
- **Exemption**: `scripts/private-names-guard.py` skips exactly two paths (`EXEMPT_PATHS`). `docs/hard-decisions.md` is skipped because the owner's question about that document is recorded there and only the owner may change it. `.velnor/runner.lock` is skipped because it is a hashed input of the job-image candidate and must stay byte-identical. Each exemption is by exact path; a copy of either file under any other path is scanned. Changing either file requires owner approval.
- **Changed files**: A pull request scans only the files it changes. A push, a schedule and any change to the guard or its digest list scan the whole tree.

---

## Everything goes through mbx

Every Rust build and test runs through mise and mbx. `mise.toml` sets `[wrappers.cargo] command = "mbx"`, so `cargo` on `PATH` is the mise wrapper that runs mbx, and mbx keeps the compiler results in its store. Do not run `~/.cargo/bin/cargo` or the rustup toolchain directly.

Four exceptions are allowed. Nothing else may skip mbx.

1. **Token fetch.** The `velnor-ci fetch` step runs the real `cargo fetch --locked` for each Cargo root, with the private token in the environment of that one command. It never runs a mise shim or a command wrapper, so the token never reaches a wrapper from the repository or `PATH`. The fetch downloads and does not compile, so mbx has nothing to cache.
2. **Bootstrap of mbx and mise.** Building mise and mbx from source (`scripts/build-owned-tool.py`) uses the pinned official mbx release, checked by digest, as its cargo driver. A tool cannot be its own compiler wrapper while it is being built.
3. **Rustdoc and doctests.** `cargo doc` and doctests run without mbx, because mbx cannot cache them yet. This is accepted for now and is revisited when mbx caches them.
4. **Release-plz `cargo package` and `cargo publish`.** The `release_pr` and `tag` jobs run release-plz, which runs `cargo package` and `cargo publish` itself, without mbx and without `jdx/mise-action`. Both jobs hold the release App token, and the generator rule `MiseActionInTokenJob` forbids mise in a job that holds a token, so a repository-declared cargo wrapper must not run there. Token safety wins over mbx for these two jobs. Build jobs hold no token and do run through mise and mbx.

MBX is fail closed when the crates mirror runs. The mirror step reads the mise configuration in the workspace (`mise.toml`, `.mise.toml`, `mise/config.toml`, `.mise/config.toml`, `.config/mise.toml` or `.config/mise/config.toml`) with the same detector `velnor-ci report-waste` uses. If it declares MBX (a `[wrappers.cargo]` command that runs `mbx`, including a path such as `/opt/tools/bin/mbx`, or `mr_boxington = true` on a tool, including the array form), the step records `VELNOR_CARGO_MBX=required` for the rest of the job. From then on the wrapper refuses to run any `cargo` that is not the mise wrapper and fails with `mbx is declared but cargo on PATH isn't the mise mbx wrapper: <path>`. When mise's launch finishes without creating its cargo wrapper, the wrapper fails with `mbx is declared but mise's cargo wrapper is missing after activation` instead of running a plain `cargo`. It never falls back to a cargo found elsewhere on `PATH`. `VELNOR_CRATES_MIRROR=off` does not switch MBX off: with MBX declared, the step writes no mirror config but still exports `CARGO` and `VELNOR_CARGO_WRAPPER` pointing at velnor-ci's forwarding wrapper. Every run of the step exports its complete mode: `VELNOR_CARGO_CONFIG` is empty when the mirror is off, and `VELNOR_CARGO_MBX` is empty when nothing declares MBX, so a later run clears what an earlier run in the same job exported. The MBX check runs when each `cargo` is called, not at setup, because setup runs before the tools install and mise's wrapper may not exist yet. A call fails when the first `cargo` on `PATH` is not the mise wrapper. Repositories that do not declare MBX are unchanged. The mirror mode and the MBX requirement are not read from the job's environment alone. Setup writes a record, `velnor-cargo-mode`, in the `velnor-cargo` directory beside the wrapper, and the wrapper finds it from its own executable path, never from an environment variable. A step `env:` block cannot undo it: with the mirror on, a missing or empty `VELNOR_CARGO_CONFIG` fails, and so does a value that names a different file than the config path the record holds, and `VELNOR_CRATES_MIRROR=off` set after setup fails with a message instead of switching the mirror off; the only way to turn it off is to run the crates-mirror step with `VELNOR_CRATES_MIRROR=off`. The crates-mirror step reads `VELNOR_CRATES_MIRROR` from the environment it is started with, so an `off` that an earlier step exported through `GITHUB_ENV` is honored the same as a step-level `env:` value: the rerun stays off and writes no mirror config. Only an explicit empty `VELNOR_CRATES_MIRROR` on the step turns a later run back on, and that is the empty reset the mirror-on setup itself appends to `GITHUB_ENV`. `VELNOR_CARGO_MBX` can only add the MBX requirement, never remove it. `VELNOR_CARGO_MISE_PHASE` and `VELNOR_CARGO_REAL` are state that the wrapper passes to its own child cargo calls while a chain runs, not something setup writes, so they are trusted from the job environment: a step that sets `VELNOR_CARGO_MISE_PHASE=wrapped` already needs control of the step's environment, which this check does not claim to stop. The requirement applies only to a `cargo` that a step starts, not to one that a running Cargo or test starts (the environment has `CARGO_MANIFEST_DIR`, which Cargo and nextest set for build scripts and tests), and only when its project declares MBX: the project is the directory of its `--manifest-path`, or else the working directory, resolved after cargo's `-C <dir>` (`-C dir`, `-Cdir`, `-C=dir`, also inside a short-flag cluster such as `-vCdir`) changes into it, and a script or manifest named as the command is scoped by its own path, following a symlink to the file it names (cargo's own rule for a manifest command under `-Zscript`: a `.rs` file or any path of more than one component such as `a/b/c`, `./tool`, `../tool` or `a/Cargo.toml`; without `-Zscript` cargo runs a bare `foo.rs`, or a multi-component name such as `a/b`, as an external subcommand when an executable of that name is on `PATH` (`cargo-foo.rs`, `cargo-a/b`), so the directories of both the script and its `--manifest-path` count; a bare name such as `nextest` or `tool` is a subcommand even under `-Zscript` and is scoped like any other, by its `--manifest-path`; a path that cannot be resolved because of `..` components requires MBX); inside `GITHUB_WORKSPACE` the directories up to the workspace root count too. A `cargo` that a test starts for a scratch project outside the workspace therefore runs as before, while a build of the checkout still fails closed. The `-m` shorthand for `--manifest-path` is honored for the built-in manifest commands, their built-in aliases (`b`, `c`, `d`, `t`, `r`, `rm`) and aliases defined in `.cargo/config.toml`, `.cargo/config`, `CARGO_ALIAS_*` or `$CARGO_HOME`, which are resolved to their expansion first. A user alias may override only cargo's short aliases (`b`, `c`, `d`, `t`, `r`, `rm`) and `clippy`, so an alias with one of those names is expanded and scoped by its expansion; an alias named like any other built-in, such as `test`, `add`, `info`, `remove` or `report`, is ignored, so the scope follows the built-in. `.cargo/config` wins over `.cargo/config.toml` in the same directory. An alias definition that cannot be read, an `include` in a config file, or an alias given with `--config` makes a call require MBX when its subcommand is one an alias could expand to: `b`, `c`, `clippy`, `d`, `r`, `rm`, `t`, or a name that is not a cargo built-in, because the alias could expand to a manifest in the checkout. A `GITHUB_WORKSPACE` that is set but cannot be resolved to an existing directory, for example a missing path, a file or a path with an inaccessible component, also requires MBX. When it is unset or empty, the MBX declaration scan covers the project directory alone and reads no directory above it; the `-m` alias lookup above still reads `.cargo/config.toml` and `.cargo/config` in every ancestor directory. So does a call whose options before the subcommand cannot be classified the way cargo parses them: `-C` without a value or with an empty one, `-C` given twice, or an unknown option. This is a guard against a misconfigured job, not a sandbox: step code that sets `CARGO_MANIFEST_DIR` or runs its own `cargo` on a project outside the checkout is not stopped, as it could already run any command. velnor-ci releases before 0.1.110 do not have this check, and releases before 0.1.111 apply it to every `cargo` on the job's `PATH`. The `-C` rule (every spelling, a `-C` that names a symlink, and the fail-closed cases above) first ships in velnor-ci 0.1.112; 0.1.111 still chooses the project from the launch directory and ignores `-C`.

**Release builds always use a fresh private mbx store; never shared, never restored.** mbx's action records are unsigned: rewriting the records in a writable store makes a clean checkout serve the writer's output. A self-hosted runner's `~/.cache/mbx`, a shared store or a restored Actions cache would therefore be a poisoning path into release binaries. Every generated release job that runs cargo through mise (the `build-*` jobs and `family-versions`) has the step `Private mbx store` right after its checkout, before `jdx/mise-action` and any cargo: `echo "MBX_CACHE_DIR=$RUNNER_TEMP/mbx-release-<job>" >> "$GITHUB_ENV"`. The runner empties `$RUNNER_TEMP` for every job, so the store starts empty and no other job can write it. Release jobs never set `MBX_REMOTE` or mount the shared store, and never restore or save an mbx cache. The generator tests in `crates/velnor-gen/tests/it/release_mbx.rs` pin all of this.

No script may add a fifth exception. `scripts/build-owned-archive-guard.sh` builds its guard through the repository's mise and mbx, with a fresh private mbx store per run. `scripts/local-cargo.sh` was removed: it called rustup's cargo directly and only its own test used it. Exceptions 1 and 3 do not build mbx or mise, so the bootstrap rule does not cover them; the owner must decide whether they stay.

To verify a local checkout:

```sh
export MISE_TRUSTED_CONFIG_PATHS=$PWD
mise install
type -a cargo | head -1   # must show a path under mise/command-wrappers/bin/
```

Without `MISE_TRUSTED_CONFIG_PATHS`, mise falls back to plain cargo without a warning, so the `type -a` check is the test. A build through mbx prints `mbx[cache]: object cache: H hits, M misses, ...` lines in its output. If they are missing, the build did not go through mbx.

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
