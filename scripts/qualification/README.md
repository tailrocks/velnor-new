# Hosted Rust toolchain qualification

Dispatch the generated `qualification.yml` workflow with `mode=rust-toolchain` at a reviewed source ref to measure the official Rust 1.99.0 component projection on hosted Linux x64 and macOS ARM64:

```sh
gh workflow run qualification.yml --ref <reviewed-source-sha> -f mode=rust-toolchain
```

Each job verifies the checked-out SHA, downloads the pinned official channel manifest and only its `cargo`, `rustc`, and platform `rust-std` archives, validates every SHA-256 before extraction, probes the installed executables, and uploads a receipt artifact. The receipt binds the manifest, component archives, executable hashes, canonical install-tree digest, workflow SHA, source SHA, and run attempt.

The workflow does not modify the tool catalog or treat configured hashes as qualification results. A hosted run must complete on both platforms, and its uploaded receipts must be reviewed before any qualification row or generated consumer workflow is changed.
