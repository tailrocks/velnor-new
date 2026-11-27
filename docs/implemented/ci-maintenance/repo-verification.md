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

---

## The Verification Suite

No single script runs the full gate. The full local gate is the `run:` steps of the jobs in `.github/workflows/ci.yml` and `.github/workflows/repository-checks.yml` that the changed paths select. Read the steps from the workflow files: they are the source of truth, and `velnor-gen` generates `ci.yml`. Run them from the repo root, in a checkout where `cargo` resolves to the mbx wrapper (see [Everything goes through mbx](#everything-goes-through-mbx)). Skip a job only when the changed paths do not select it, and run every job when you are unsure.

Run the steps with the shell the workflow sets: `bash -p {0}` for `ci.yml`, `bash -e {0}` for the jobs of `repository-checks.yml`.

**The translation rule.** A `ci.yml` step runs `$VELNOR_EXEC`, the released helper that only the runner has. Translate each `run:` line once, in this order:

1. `. "$VELNOR_EXEC" run -- <command>` becomes `<command>`.
2. `. "$VELNOR_EXEC" <subcommand>` becomes `target/debug/velnor-ci <subcommand>`.
3. A `<command>` that starts with `velnor-ci`, such as `velnor-ci inputs build --` or `velnor-ci lane-guard`, runs as `target/debug/velnor-ci ...`, because `velnor-ci` is not on `PATH` in a checkout.
4. Drop output-format flags (`--message-format=json-render-diagnostics` and `--cargo-message-format=json-render-diagnostics`) and keep every other argument.

Build the binary first with `cargo build --locked -p velnor-ci --bin velnor-ci`. In a checkout that uses mbx, `target` is a symlink into the mbx store, and `target/debug/velnor-ci` works through it. Do not use `cargo run` for it: `cargo run` sets `CARGO` to the toolchain's cargo, so the nested builds of `lane-guard` skip mbx.

Run the checking steps. `velnor-ci inputs build --` runs its own checks before the command starts, such as the `outside_selection` check and the build-directory declarations. `check-metadata` runs in every `rust` job. CI runs `inputs audit --root .` on a schedule, on a manual dispatch and when the restored build directory has expired. Run it for the same audit after a full `target/debug/velnor-ci inputs build -- cargo clippy --workspace --all-targets` and `target/debug/velnor-ci inputs build -- cargo build --workspace --all-targets` in the checkout. On a partial build directory it fails with `found no current dep-info`. Skip the steps that only prepare the runner: the `uses:` steps, the launcher install, and the subcommands that restore, save or report runner state (`plan` is covered below, and the others are `prepare`, `delta`, `inputs restore-all`, `restored`, `inputs save`, `cache`, `report`, `tools-verify`, `tools-local`, `crates-mirror`, `mbx-store`, `sources`, `merge-queue-check`, `merge-group-passed`, `bench` and `gate`). Skip the plain `rustup` steps that install components and targets, and the `echo` and `printf` steps that write job environment or outputs. Also skip the two `repository-checks.yml` steps that run `scripts/test-install-secret-scanners.sh` and `scripts/install-secret-scanners.sh`. Both handle Linux binaries only and fail on macOS with `no pinned secret scanner assets for Darwin/arm64`. Run the `git fetch --no-tags --unshallow` step of the `secret-scan` job only in a shallow clone, because it fails in a full clone.

CI runs the helper that the `Install velnor-ci <version>` step of `ci.yml` pins, and it publishes Linux binaries only, so the pinned helper cannot run on macOS. The binary you build from the checkout reports its own version with `target/debug/velnor-ci --version`, and that version can differ from the pin. When the two differ, build the pinned release in its own worktree through mbx and run that binary from your change's own checkout:

```sh
git worktree add --detach <path> tools-v<pinned version>
( cd <path> && export MISE_TRUSTED_CONFIG_PATHS=$PWD && mise install && git checkout -- mise.lock && mbx doctor && cargo build --locked -p velnor-ci --bin velnor-ci )
<path>/target/debug/velnor-ci <subcommand>
```

The worktree supplies only the binary. The binary reads the checkout it runs in, so do not run it from `<path>`.

A nextest step reads `-E "$TEST_FILTER"`. The skipped `velnor-ci plan` step computes that value, so take it from the local plan. Set `GITHUB_OUTPUT` to a file and run `target/debug/velnor-ci plan --explain --changed <path>` on one changed path. Then copy the `rust_root_test_filter` line from that file into `TEST_FILTER`. The plan includes the packages that depend on a changed crate, so this value can be wider than the crate you changed. Use `all()` to run the whole workspace when you are unsure. Unset or empty, nextest fails with a filterset parse error.

The table applies the translation rule to the steps of the `rust` job and lists the commands of the other jobs. It is not the gate and it can fall behind the workflows. When the table and a workflow step differ, the workflow step wins.

| CI job | Local command |
|---|---|
| `rust`, metadata | `target/debug/velnor-ci check-metadata` |
| `rust`, format | `cargo-fmt --all --check` |
| `rust`, lint | `target/debug/velnor-ci inputs build -- cargo-clippy clippy --workspace --all-targets --locked -- -D warnings` |
| `rust`, docs | `target/debug/velnor-ci inputs build -- cargo doc --workspace --no-deps --locked --config 'build.rustdocflags=["-D","warnings"]'` |
| `rust`, per-package check | `target/debug/velnor-ci inputs build -- cargo check -p <pkg> --locked`, once for each selected package. CI runs these steps on a push, a schedule or a manual dispatch |
| `rust`, test tool setup | `mise --no-config --no-env --no-hooks install aqua:sigstore/cosign@3.1.4` and `mise --no-config --no-env --no-hooks install gh@2.104.0` |
| `rust`, tests | `target/debug/velnor-ci inputs build -- cargo-nextest nextest run --workspace --locked --profile ci --no-fail-fast --ignore-default-filter --status-level pass -E "$TEST_FILTER" --no-tests=pass` |
| `rust`, tests with the `test-support` feature | `target/debug/velnor-ci inputs build -- cargo-nextest nextest run --workspace --locked --features 'test-support' --profile ci --no-fail-fast --ignore-default-filter --status-level pass -E "($TEST_FILTER) and (package(velnor-runner))" --no-tests=fail` |
| `rust`, test lane guard with the `test-support` feature | `target/debug/velnor-ci lane-guard --features 'test-support' --filter 'package(velnor-runner)'` |
| `rust`, lint with the `test-support` feature | `target/debug/velnor-ci inputs build -- cargo-clippy clippy --workspace --all-targets --locked --features 'test-support' -- -D warnings` |
| `rust`, tests with the `local-mode` feature | `target/debug/velnor-ci inputs build -- cargo-nextest nextest run --workspace --locked --features 'local-mode' --profile ci --no-fail-fast --ignore-default-filter --status-level pass -E "($TEST_FILTER) and (package(velnor-runner-host) and (test(~local::) or binary(local_host_public) or binary(local_pair_public)))" --no-tests=fail` |
| `rust`, test lane guard with the `local-mode` feature | `target/debug/velnor-ci lane-guard --features 'local-mode' --filter 'package(velnor-runner-host) and (test(~local::) or binary(local_host_public) or binary(local_pair_public))'` |
| `rust`, lint with the `local-mode` feature | `target/debug/velnor-ci inputs build -- cargo-clippy clippy --workspace --all-targets --locked --features 'local-mode' -- -D warnings` |
| `rust`, docs with the `local-mode` feature | `target/debug/velnor-ci inputs build -- cargo doc --workspace --no-deps --locked --config 'build.rustdocflags=["-D","warnings"]' --features 'local-mode'` |
| `rust`, input audit | `target/debug/velnor-ci inputs audit --root .` |
| `policy`, dependencies | `cargo deny --config deny.toml --locked check` |
| `policy`, unused dependencies | `cargo machete` |
| `lint-alint` | `alint validate-config && alint check --fail-on-warning`, then `bash scripts/alint-line-cap-probe.sh` |
| `lint-jactionlint` | `jactionlint --config-file .github/jactionlint.yaml --format github --strict-exit --no-online --no-color --pyflakes= --shellcheck=shellcheck` |
| `lint-generator-samples` | `mise run lint-generator-samples` |
| `lint-hosted-phase-measurement` | `mise run lint-hosted-phase-measurement` |
| `lint-mise-binary-digests` | `mise run test-mise-binary-digests` |
| `lint-validate-hk` | `mise run validate-hk` |
| `lint-generated-check` | `cargo run --locked -p velnor-gen --bin velnor-gen -- --root . check --verify-release` |
| `helper-gates`, fetch and check the added gates | `git fetch --force --tags --quiet origin`, then `bash scripts/check-helper-gates.sh origin/<base>`, where `<base>` is the branch your pull request targets |
| `helper-gates`, self-test the gate check | The `run:` block of the self-test step of the `helper-gates` job in `repository-checks.yml`, run verbatim under `bash -e` |
| `helper-gates`, merge-queue test | `bash scripts/test-helper-gates-merge-queue.sh` |
| `secret-scan`, scans | Every step of the job in `repository-checks.yml` except the two installers, run in this order with the scanners that `mise install` installs from `mise.toml`: `bash scripts/test-secret-scan-worktree.sh`, the `Print scanner versions` step (`gitleaks version`, `betterleaks version` and `kingfisher --version`), `bash scripts/test-secret-scan-config.sh`, `gitleaks git . --redact --no-banner --config .gitleaks.toml --log-opts="--full-history --diff-merges=first-parent HEAD"`, `betterleaks git . --redact -c .betterleaks.toml --log-opts="--full-history --diff-merges=first-parent HEAD"` and `bash scripts/secret-scan-worktree.sh`. The unshallow step is handled above |

`cargo doc` is the third exception in [Everything goes through mbx](#everything-goes-through-mbx).

The nextest rows read `$TEST_FILTER`, which `velnor-ci plan` computes from the diff in CI, so set it as described above. The `test-support` and `local-mode` rows narrow it with their own `-E` condition and enable their feature. CI skips such a lane, with a `Skip test lane` step, when the plan selects none of its packages. The tests row passes `--ignore-default-filter`, as the generated `ci.yml` step does, so the default filter of the nextest profile skips no test. It passes `--no-tests=pass` because an empty selection is valid there. Both feature lanes pass `--no-tests=fail`.

The `test-maintenance-helpers` mise task runs the repository-only maintenance helper tests. No generated workflow job runs it, so it is a local full run and not a required check. It first runs `scripts/test-setup-velnor-action.py`, the mise binary digest tests, the tag ruleset drift test and `cargo fetch --locked`, then runs the `velnor-archive-guard` tests with `--no-tests fail`.

`hk.pkl` defines the other local checks: `rumdl` (pinned in `mise.toml`), `tofu`, the toolchain mirror check (`scripts/check-rust-toolchain-mirror.sh`), the `cargo test` ban check (`scripts/check-test-cargo.sh`) and the secret scans. The `pre-commit` and `pre-push` hooks run the secret scans.

For any change, run the jobs that `velnor-ci plan` selects, plus the always-on jobs. CI computes the selection in its `rust-plan` job with `. "$VELNOR_EXEC" plan --explain`. In CI the plan lists the changed files with git and reads the old text of each changed `Cargo.lock`, root `Cargo.toml`, workflow file, file of a lint task and file that holds a setting weakening from the first parent of the merge commit. Locally, build the binary, then pass the changed paths, relative to the repository root, and the old text of each changed file that the plan compares with `--base-file <path>=<file>`. `<file>` holds the text from the base branch, and a process substitution works:

```sh
cargo build --locked -p velnor-ci --bin velnor-ci
target/debug/velnor-ci plan --explain --changed Cargo.lock --base-file Cargo.lock=<(git show origin/<base>:Cargo.lock)
```

Give `--base-file` once for each changed `Cargo.lock`, `Cargo.toml` or configuration file the plan reads, such as `.velnor/rust-ci.toml` or a cargo, mise or nextest configuration. Without it, a changed `Cargo.lock` or root `Cargo.toml` selects everything, which is more than CI selects. For a changed configuration file, `--base-file` does not change the selection. It decides which `check does less` lines the explanation marks `(new in this change)`, and which settings the plan reports as `::warning` annotations: without it, the plan treats every setting in the file as new and annotates each one. Without `--changed` and without a pull request event, the plan selects everything.

Set `GITHUB_OUTPUT` to a file to read the `rust_*` and `lint_*` values that the plan writes. The plan reads the files in the working tree and the paths you pass. It also reads the `origin` remote to place the hashed `full_run` schedule, and it fails with `origin must identify a hosted repository to place a hashed full_run schedule` when `origin` is a local path or a `file://` URL. It works when `origin` is absent or a hosted URL, over SSH or HTTPS.

The always-on jobs run for every change:

- `Jactionlint` (`lint-jactionlint` job in `ci.yml`). Its `if:` checks the event, the draft state, the labels and the merge-queue skip output, and no path.
- `Alint` (`lint-alint` job in `ci.yml`). Its `if:` is the same as the `Jactionlint` job.
- `Helper gates` (`helper-gates` job in `repository-checks.yml`). It runs for a pull request from the same repository and for a merge group.
- `Secret scanners` (`secret-scan` job in `repository-checks.yml`). It runs for a pull request from the same repository, for a push and for a merge group.

The `required` job in `ci.yml` collects the results of the other jobs and runs no check. Run the table rows for the always-on jobs locally: the `alint` and `jactionlint` rows, the three `helper-gates` rows and the `secret-scan` row. `rumdl` is not a CI job. `mise.toml` pins it and `hk.pkl` runs it, so run `rumdl check` on the changed Markdown files as well.

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

### 3. Test Suite Execution: `cargo nextest run --workspace --locked --profile ci --no-fail-fast --ignore-default-filter --no-tests=fail`

- **Purpose**: Runs the workspace Nextest suite with the `ci` profile from `.config/nextest.toml`. `--no-tests=fail` makes an empty selection fail.
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

### 6. Private Names: `python3 scripts/private-names-guard.py`

- **Purpose**: Keeps private downstream organization and repository names out of tracked files.
- **Authority**: `scripts/private-names-guard.py` alone decides whether a private name is present. It reads the bytes of every tracked path, file and symlink target, independent of file type, extension case or structure, and compares them with the salted digests in `scripts/private-names-hashes.txt`. Matching is case-insensitive and also runs on the URL-decoded, `\uXXXX`-unescaped, `\/`-unescaped and HTML-unescaped text, after Unicode NFKC normalization with zero-width characters and combining marks removed. It matches a name inside identifiers, an `org/repo` pair with spaces around the slash or a `.git` suffix, and a name written with spaces, tabs, hyphens, underscores, dots or nothing between its letters and words, such as `Title Case Spaced`, `hyphen-form`, `under_score` or `dot.form`. A private name that is also an ordinary phrase therefore flags that phrase; reword the prose.
- **Exemption**: `scripts/private-names-guard.py` skips exactly two paths (`EXEMPT_PATHS`). `docs/hard-decisions.md` is skipped because the owner's question about that document is recorded there and only the owner may change it. `.velnor/runner.lock` is skipped because it is a hashed input of the job-image candidate and must stay byte-identical. Each exemption is by exact path; a copy of either file under any other path is scanned. Changing either file requires owner approval.
- **Changed files**: A pull request scans only the files it changes. A push, a schedule and any change to the guard or its digest list scan the whole tree.

---

## Everything goes through mbx

Every Rust build and test runs through mise and mbx. `mise.toml` sets `[wrappers.cargo] command = "mbx"`, so `cargo` on `PATH` is the mise wrapper that runs mbx, and mbx keeps the compiler results in its store. Do not run `~/.cargo/bin/cargo` or the rustup toolchain directly.

Three exceptions are allowed. Nothing else may skip mbx.

1. **Token fetch.** The `velnor-ci fetch` subcommand, which a generated workflow runs when `[private_sources]` is set and which no workflow of this repository runs, runs the real `cargo fetch --locked` for each Cargo root, with the private token in the environment of that one command. It never runs a mise shim or a command wrapper, so the token never reaches a wrapper from the repository or `PATH`. The fetch downloads and does not compile, so mbx has nothing to cache.
2. **Bootstrap of mbx and mise.** Building mise and mbx from source (`scripts/build-owned-tool.py`) uses the pinned official mbx release, checked by digest, as its cargo driver. A tool cannot be its own compiler wrapper while it is being built.
3. **Rustdoc and doctests.** `cargo doc` and doctests run without mbx, because mbx cannot cache them yet. This is accepted for now and is revisited when mbx caches them.

The release workflow `release.yml` runs no `cargo package`, `cargo publish` or release-plz. Its `family` job holds the release App token and runs only `velnor-ci` subcommands. The generator rule `MiseActionInTokenJob` (`crates/velnor-render/src/rules.rs`) forbids `jdx/mise-action` in a job that holds the release App token, so no repository-declared cargo wrapper runs there. The release build jobs hold no token and run through mise and mbx with a private store.

MBX is fail closed when the crates mirror runs. The mirror step reads the mise configuration in the workspace (`mise.toml`, `.mise.toml`, `mise/config.toml`, `.mise/config.toml`, `.config/mise.toml` or `.config/mise/config.toml`) with the same detector `velnor-ci report-waste` uses. If it declares MBX (a `[wrappers.cargo]` command that runs `mbx`, including a path such as `/opt/tools/bin/mbx`, or `mr_boxington = true` on a tool, including the array form), the step records `VELNOR_CARGO_MBX=required` for the rest of the job. From then on the wrapper refuses to run any `cargo` that is not the mise wrapper and fails with `mbx is declared but cargo on PATH isn't the mise mbx wrapper: <path>`. When mise's launch finishes without creating its cargo wrapper, the wrapper fails with `mbx is declared but mise's cargo wrapper is missing after activation` instead of running a plain `cargo`. It never falls back to a cargo found elsewhere on `PATH`. `VELNOR_CRATES_MIRROR=off` does not switch MBX off: with MBX declared, the step writes no mirror config but still exports `CARGO` and `VELNOR_CARGO_WRAPPER` pointing at velnor-ci's forwarding wrapper. Every run of the step exports its complete mode: `VELNOR_CARGO_CONFIG` is empty when the mirror is off, and `VELNOR_CARGO_MBX` is empty when nothing declares MBX, so a later run clears what an earlier run in the same job exported. The MBX check runs when each `cargo` is called, not at setup, because setup runs before the tools install and mise's wrapper may not exist yet. A call fails when the first `cargo` on `PATH` is not the mise wrapper. Repositories that do not declare MBX are unchanged. The mirror mode and the MBX requirement are not read from the job's environment alone. Setup writes a record, `velnor-cargo-mode`, in the `velnor-cargo` directory beside the wrapper, and the wrapper finds it from its own executable path, never from an environment variable. A step `env:` block cannot undo it: with the mirror on, a missing or empty `VELNOR_CARGO_CONFIG` fails, and so does a value that names a different file than the config path the record holds, and `VELNOR_CRATES_MIRROR=off` set after setup fails with a message instead of switching the mirror off; the only way to turn it off is to run the crates-mirror step with `VELNOR_CRATES_MIRROR=off`. The crates-mirror step reads `VELNOR_CRATES_MIRROR` from the environment it is started with, so an `off` that an earlier step exported through `GITHUB_ENV` is honored the same as a step-level `env:` value: the rerun stays off and writes no mirror config. Only an explicit empty `VELNOR_CRATES_MIRROR` on the step turns a later run back on, and that is the empty reset the mirror-on setup itself appends to `GITHUB_ENV`. `VELNOR_CARGO_MBX` can only add the MBX requirement, never remove it. `VELNOR_CARGO_MISE_PHASE` and `VELNOR_CARGO_REAL` are state that the wrapper passes to its own child cargo calls while a chain runs, not something setup writes, so they are trusted from the job environment: a step that sets `VELNOR_CARGO_MISE_PHASE=wrapped` already needs control of the step's environment, which this check does not claim to stop. The requirement applies only to a `cargo` that a step starts, not to one that a running Cargo or test starts (the environment has `CARGO_MANIFEST_DIR`, which Cargo and nextest set for build scripts and tests), and only when its project declares MBX: the project is the directory of its `--manifest-path`, or else the working directory, resolved after cargo's `-C <dir>` (`-C dir`, `-Cdir`, `-C=dir`, also inside a short-flag cluster such as `-vCdir`) changes into it, and a script or manifest named as the command is scoped by its own path, following a symlink to the file it names (cargo's own rule for a manifest command under `-Zscript`: a `.rs` file or any path of more than one component such as `a/b/c`, `./tool`, `../tool` or `a/Cargo.toml`; without `-Zscript` cargo runs a bare `foo.rs`, or a multi-component name such as `a/b`, as an external subcommand when an executable of that name is on `PATH` (`cargo-foo.rs`, `cargo-a/b`), so the directories of both the script and its `--manifest-path` count; a bare name such as `nextest` or `tool` is a subcommand even under `-Zscript` and is scoped like any other, by its `--manifest-path`; a path that cannot be resolved because of `..` components requires MBX); inside `GITHUB_WORKSPACE` the directories up to the workspace root count too. A `cargo` that a test starts for a scratch project outside the workspace therefore runs as before, while a build of the checkout still fails closed. The `-m` shorthand for `--manifest-path` is honored for the built-in manifest commands, their built-in aliases (`b`, `c`, `d`, `t`, `r`, `rm`) and aliases defined in `.cargo/config.toml`, `.cargo/config`, `CARGO_ALIAS_*` or `$CARGO_HOME`, which are resolved to their expansion first. A user alias may override only cargo's short aliases (`b`, `c`, `d`, `t`, `r`, `rm`) and `clippy`, so an alias with one of those names is expanded and scoped by its expansion; an alias named like any other built-in, such as `test`, `add`, `info`, `remove` or `report`, is ignored, so the scope follows the built-in. `.cargo/config` wins over `.cargo/config.toml` in the same directory. An alias definition that cannot be read, an `include` in a config file, or an alias given with `--config` makes a call require MBX when its subcommand is one an alias could expand to: `b`, `c`, `clippy`, `d`, `r`, `rm`, `t`, or a name that is not a cargo built-in, because the alias could expand to a manifest in the checkout. A `GITHUB_WORKSPACE` that is set but cannot be resolved to an existing directory, for example a missing path, a file or a path with an inaccessible component, also requires MBX. When it is unset or empty, the MBX declaration scan covers the project directory alone and reads no directory above it; the `-m` alias lookup above still reads `.cargo/config.toml` and `.cargo/config` in every ancestor directory. So does a call whose options before the subcommand cannot be classified the way cargo parses them: `-C` without a value or with an empty one, `-C` given twice, or an unknown option. This is a guard against a misconfigured job, not a sandbox: step code that sets `CARGO_MANIFEST_DIR` or runs its own `cargo` on a project outside the checkout is not stopped, as it could already run any command. velnor-ci releases before 0.1.110 do not have this check, and releases before 0.1.111 apply it to every `cargo` on the job's `PATH`. The `-C` rule (every spelling, a `-C` that names a symlink, and the fail-closed cases above) first ships in velnor-ci 0.1.112; 0.1.111 still chooses the project from the launch directory and ignores `-C`.

**Release builds always use a fresh private mbx store; never shared, never restored.** mbx's action records are unsigned: rewriting the records in a writable store makes a clean checkout serve the writer's output. A self-hosted runner's `~/.cache/mbx`, a shared store or a restored Actions cache would therefore be a poisoning path into release binaries. Every generated release job that runs cargo through mise (the `build-*` jobs and `family-versions`) has the step `Private mbx store` right after its checkout, before `jdx/mise-action` and any cargo: `echo "MBX_CACHE_DIR=$RUNNER_TEMP/mbx-release-<job>" >> "$GITHUB_ENV"`. The runner empties `$RUNNER_TEMP` for every job, so the store starts empty and no other job can write it. Release jobs never set `MBX_REMOTE` or mount the shared store, and never restore or save an mbx cache. The generator tests in `crates/velnor-gen/tests/it/release_mbx.rs` pin all of this.

No script may add a fourth exception. `scripts/build-owned-archive-guard.sh` builds its guard through the repository's mise and mbx, with a fresh private mbx store per run. It sets its own `CARGO_TARGET_DIR` inside that hermetic build so the guard binary lands in a path that depends on the source fingerprint. This is not an exception to the mbx rule and is no template for other scripts or for your shell: do not set `CARGO_TARGET_DIR` yourself. `scripts/local-cargo.sh` was removed: it called rustup's cargo directly and only its own test used it. Exceptions 1 and 3 do not build mbx or mise, so the bootstrap rule does not cover them; the owner must decide whether they stay.

To verify a local checkout:

```sh
export MISE_TRUSTED_CONFIG_PATHS=$PWD
mise install
type -a cargo | head -1   # must show a path under mise/command-wrappers/bin/
mbx doctor               # must report 0 failures
```

Without `MISE_TRUSTED_CONFIG_PATHS`, mise falls back to plain cargo without a warning, so the `type -a` check is the test. A build through mbx prints `mbx[cache]` lines in its output. The counts change from build to build, for example `mbx[cache]: 8 hits, 0 misses; 0 B downloaded, 0 B uploaded, 0 B stored locally`. If they are missing, the build did not go through mbx.

## Runner Crates (`crates/velnor-runner-*`)

The runner controller and supervisor crates (`crates/velnor-runner-*` and `crates/velnor-resource-probe`) are members of the root workspace. They use the root `Cargo.toml`, `Cargo.lock` and `deny.toml`, and the root verification commands in [The Verification Suite](#the-verification-suite) cover them.

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
