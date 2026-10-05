The archive-guard builder used a shared Cargo target directory, so two checkouts with different source bytes could reuse one another’s compiled executable. This change namespaces the build by the admitted source fingerprint and platform/compiler/Cargo identity, verifies the compiled and installed executable reports the admitted fingerprint, and rechecks source bytes after compilation and before installation. It also creates and validates the private temporary directory before the first Mise command.

The fingerprint helper now includes `scripts/owned_archive_preflight.py` in the source manifest. It opens the checkout and `scripts` directories without following symlinks, verifies ownership and mode, safely reads the admitted source with identity checks, then compiles and executes those exact bytes under isolated Python with bytecode disabled. It no longer imports a module that could execute an unbound `.pyc`.

The regression harness plants a valid timestamp-based trap `.pyc` whose mtime and source-size header match. It proves ordinary Python import executes the trap, while the source-only prehash and builder ignore it. It then changes checkout B’s source bytes while restoring its mtime and verifies B gets a distinct target namespace. A separate direct run of the real pinned builder compiled and verified the installed binary; the regression does not claim two native Cargo builds.

Current publication head `d3ed26f1a47aa67c8296d00607472f5c63116365` normally merges main `86f864aabc2192a9f8ccd3f01f2e25b7858c3f20`. That docs-only carry-forward is disjoint from the archive-guard source paths. Exact source at `4c9afe5a58c95c2cef8d25946ecf3d15f2a3c806` received independent source PASS; the current merge changes no reviewed source paths.

All local checks pass on the corrected publication tree:

- `cargo nextest run --locked --workspace`: 3,093 passed, 1 skipped (run `7969023c-3b19-4e80-8efb-8e31e96e6341`).
- `cargo clippy --locked --workspace --all-targets -- -D warnings` and `cargo fmt --all -- --check`.
- Alint, root and nested-runner `cargo deny check`, and `bash scripts/check-freshness.sh`.
- Fake-Cargo preserved-mtime/PYC regression, registered test, direct pinned builder, ShellCheck, and `bash -n`.

Hosted CI is [run 37340683421](https://github.com/tailrocks/velnor-new/actions/runs/37340683421) at head d3ed26f. Plan, Actionlint, Alint, Cargo Deny, Cargo Machete, DCO, and Zizmor pass; Rust jobs are pending. Earlier run 37338754001 is retained as pre-correction evidence. No PR merge or runtime qualification is claimed.
