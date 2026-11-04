# Native candidate receipt rehearsal

This fixture exercises the rendered candidate-receipt step through the native dispatcher with a local GitHub and registry. The registry serves a real Cargo archive for an existing crate. The runner must provide the qualified Linux Rust toolchain, mise, and cargo-semver-checks; the fixture does not replace these tools.

Run `drive.py` inside a network-disabled Linux container with the Velnor checkout at `/src`, this directory at `/fixture`, an empty writable directory at `/workspace`, and writable runner directories at `/runner/<mode>`. Each runner directory needs `velnor/cargo`, `velnor/rustup`, and `velnor-semver/cargo-semver-checks`. Install the qualified Rust toolchain in the Rustup home and the catalog-pinned checker at that path. Put the qualified mise and cargo-nextest binaries on `PATH` and populate Cargo's offline dependency cache before starting the container.

Run `python3 /fixture/drive.py success`, then `checksum` and `source`, each with an empty workspace and runner. The success case checks the generated receipt, baseline checksum, and checker identity. The other cases require the exact checksum and changed-source errors without a receipt. Every case checks that the fake services received no mutation.

The test-only entry point leaves the production authority guard enabled. This isolated rehearsal does not test protected GitHub jobs, real OIDC, or publication. Coordinator cases exercise qualified logic only after verifying that the production authority guard rejects the operation.

The `success` case also runs the rendered publisher admission step and requires `release_mode=publish`. The `approval` case gives that step the wrong approval comment; `verify-source` changes source after the receipt is made. Both require an error and no admission output.

The `journal` and `journal-bad-bundle` cases also need `/assets` containing an official Velnor 0.1.73 manifest, its Linux binary and both signed bundles, the catalog-pinned GitHub CLI archive contents, and a public trusted-root file. The test-only runner passes that root to the real `gh attestation verify` command so verification works offline. The positive case verifies the recorded started receipt; the altered bundle must fail before any journal write. These historical assets are test inputs and do not authorize a later generator release.

The `coordinate`, `coordinate-skip`, `coordinate-oidc-denied`, and `coordinate-crash` cases additionally need the catalog-pinned release-plz executable and Rust channel manifest in `/assets`. Run each in a fresh container with network disabled and fixture hostnames mapped to loopback. The Cargo boundary packages the actual source; the registry and OIDC services are local fixtures. The crash case restarts the coordinator within the same workflow attempt after the fixture receives the archive. It does not prove recovery in a new GitHub run.

The `coordinate-corrupt-git` case modifies the acquired Git bundle and requires rejection before any publication or journal mutation.

The coordinator rehearsal is in evidence quarantine because the 2026-11-04 `coordinate` case failed once with an unexplained HTTP transport error. Quarantine does not permit omitting cases or ignoring failures: a protected rehearsal must run the required cases explicitly and block on any failure. Later passes do not resolve the original failure. The upload fixture directly checks the approved source SHA and `src/lib.rs`; coordinator reconciliation performs the full archive comparison.

Supplied-input mode supports a single recovery package on the `main` branch, the `release` environment, crates.io, and a root `Cargo.toml`. Other configurations fail closed. Supplied-input mode does not support the `source`, `checksum`, or `verify-source` fault-injection cases; those cases use the synthetic source.

For offline supplied-source SemVer checks, set `VELNOR_NATIVE_REGISTRY_SNAPSHOT` to a read-only registry snapshot containing exactly `index/` and `cache/`. The test-only runner copies it into a fresh Cargo home with no-follow checks and runs offline. The cache must contain every required crate archive. This setting has no effect in production builds.
