# Repository Verification Procedures

Run `bash scripts/verify-local.sh` from the repository root.
The script reads exact tool versions from `.velnor/version-policy.toml`.
It loads no project Mise configuration.
The script does a check of the root and nested runner workspaces.

The script resolves Rust components with the selected `rustup` toolchain.
It puts that toolchain's `bin` directory first in its command path.
It compares paths and versions for `rustc`, Cargo, Clippy, and rustfmt.
It resolves Nextest with `mise --no-config which`.
MBX runs Nextest through the pinned executable.
The local Mise executable's version is reported; a policy mismatch warns and
continues. The script compares Rust component paths and versions.
It also compares the MBX version, Nextest executable and command path, and cargo-deny version.

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

Run the repository structure checks after the local verification script:

```bash
alint validate-config
alint check --fail-on-warning
```
