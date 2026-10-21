# Repository Verification Procedures

## Git hooks

Install the declared tools with `mise install`.
Enable the Git hooks with `mise x -- hk install --global --mise`.
Use one global installation on Git 2.54 or later.
The configuration uses hk 2.7.0.
The Rust steps use the nearest Cargo manifest for each changed file.
The formatter uses `--all` for each selected Cargo manifest.
The formatter includes the packages that the root Cargo manifest lists.
`crates/velnor-runner` uses its own Cargo manifest.
The steps exclude test fixtures and generated golden files.
### Check Git state after hook fixes

The Cargo formatter and Clippy fix commands run through
`scripts/hk-fix-write-guard.sh`.
The script runs each command with its selected Cargo manifest and arguments.
It checks the Git index and file state before and after each command.
It checks tracked files and untracked files that Git does not ignore.
If this state changes after a successful command, the script returns a failure.
This check includes files outside hk's selected input set.

Existing staged, unstaged, and untracked changes can remain if the command does not change their content or index entries.
The state check does not print file content or a diff.
It does not add changes to the Git index.
If a Cargo command fails, the script returns the same exit status.
Keep the full verification procedure below.

## Full verification

Run `bash scripts/verify-local.sh` from the repository root.
The script reads exact tool versions from `.velnor/version-policy.toml`.
It loads no project Mise configuration.
The script does a check of the root and nested runner workspaces.

The script resolves Rust components with the selected `rustup` toolchain.
It puts that toolchain's `bin` directory first in `PATH`.
It compares paths and versions for `rustc`, Cargo, Clippy, and rustfmt.
It resolves Nextest with `mise --no-config which`.
The script resolves the pinned `cargo-nextest` executable with Mise.
For the Nextest stages, it gives MBX a private Cargo dispatcher through `CARGO`.
The dispatcher runs the pinned executable for Cargo's `nextest` command.
It sends all other Cargo commands to the selected Rust toolchain's Cargo executable.
Cargo can search `CARGO_HOME/bin` before `PATH` for external commands.
The dispatcher selects the pinned Nextest executable directly.
The script keeps the caller's normal `CARGO_HOME`, configuration, and credentials.
MBX and Nextest send metadata and build commands through the dispatcher.
The local Mise executable's version is reported; a policy mismatch warns and
continues. The script compares Rust component paths and versions.
It also compares the MBX version, Cargo and MBX Clippy versions, the Nextest executable path and version, and the cargo-deny version.

Nextest runs ordinary unit and integration tests.
Nextest does not run doctests.
MBX `test --doc` runs library doctests in a separate stage.
Crates without library targets have no doctest stage.

The script runs formatting and strict Clippy checks in both workspaces.
It runs Nextest suites and doctests in both workspaces.
It builds documentation for each crate in the root workspace.
It runs cargo-deny 0.20.2 against the root and nested runner workspaces.
Each dependency audit uses its workspace manifest and deny policy.
It runs freshness, repository policy, generated-tree, and fixture checks.
Set `CARGO_TARGET_DIR` to keep build output outside the repository.

Use an absolute `CARGO_TARGET_DIR` outside the repository for all verification commands.
Keep an existing MBX target symlink unchanged.
Direct Cargo commands do not hold an MBX cache lease.
Automatic cache removal can remove their active test binaries.
Set `VELNOR_ACTIONS_BIN` to `$CARGO_TARGET_DIR/debug/velnor-actions` after the CLI build.
Use that path for the freshness script.
Keep every necessary test and gate when you limit parallel work.

Run the repository structure checks after the local verification script:

```bash
alint validate-config
alint check --fail-on-warning
```
