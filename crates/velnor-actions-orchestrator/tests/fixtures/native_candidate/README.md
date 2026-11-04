# Native candidate receipt rehearsal

This fixture exercises the rendered candidate-receipt step through the native dispatcher with a local GitHub and registry. The registry serves a real Cargo archive for an existing crate. The runner must provide the qualified Linux Rust toolchain, mise, and cargo-semver-checks; the fixture does not replace these tools.

Run `drive.py` inside a network-disabled Linux container with the Velnor checkout at `/src`, this directory at `/fixture`, an empty writable directory at `/workspace`, and writable runner directories at `/runner/<mode>`. Each runner directory needs `velnor/cargo`, `velnor/rustup`, and `velnor-semver/cargo-semver-checks`. Install the qualified Rust toolchain in the Rustup home and the catalog-pinned checker at that path. Put the qualified mise and cargo-nextest binaries on `PATH` and populate Cargo's offline dependency cache before starting the container.

Run `python3 /fixture/drive.py success`, then `checksum` and `source`, each with an empty workspace and runner. The success case checks the generated receipt, baseline checksum, and checker identity. The other cases require the exact checksum and changed-source errors without a receipt. Every case checks that the fake services received no mutation.

The test-only entry point leaves the production authority guard enabled. This rehearsal does not test protected GitHub jobs, real OIDC, publication, or the release coordinator.

The `success` case also runs the rendered publisher admission step and requires `release_mode=publish`. The `approval` case gives that step the wrong approval comment; `verify-source` changes source after the receipt is made. Both require an error and no admission output.
