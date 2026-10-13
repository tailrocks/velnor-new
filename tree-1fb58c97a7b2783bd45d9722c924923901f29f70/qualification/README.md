# Hosted Rust toolchain qualification

Dispatch the generated `qualification.yml` workflow with `mode=rust-toolchain` at a reviewed source ref to measure the official Rust 1.99.0 component projection on hosted Linux x64 and macOS ARM64:

```sh
gh workflow run qualification.yml --ref <reviewed-source-sha> -f mode=rust-toolchain
```

Each job verifies the checked-out SHA, installs the catalog-pinned MBX version through Mise, and runs the candidate `cargo --version` and `rustc -vV` probes through that MBX executable. It downloads the pinned official channel manifest and only its `cargo`, `rustc`, and platform `rust-std` archives, validating every SHA-256 before extraction. The schema-2 receipt binds the MBX version, canonical executable path and SHA-256, manifest, component archives, candidate executable hashes, canonical install-tree digest, workflow SHA, source SHA, and run attempt.

The workflow does not modify the tool catalog or treat configured hashes as qualification results. A hosted run must complete on both platforms, and its uploaded receipts must be reviewed before any qualification row or generated consumer workflow is changed.
