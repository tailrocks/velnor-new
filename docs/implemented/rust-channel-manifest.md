# Verified Rust installs (channel-manifest digest)

- State: implemented (this pull request)
- Specification: [hard decision 4](../hard-decisions.md#4-tool-versions-come-from-the-projects-mise-config)
  (Trade-offs: `core:rust` lock entries carry no checksum),
  [consumer Rust setup](consumer-rust-setup.md)
- Delivered: `RUST_CHANNEL_MANIFESTS` in `velnor-actions-mise`
  (`catalog_rust_manifests.rs`), the generated `Verify Rust channel manifest`
  step in every plan, crate and dependency-seed job that installs Rust
  (`rust_manifest_step.rs`), and the lock-audit change that treats such an
  install as verified.
- Acceptance evidence: `rust_manifest_step::tests`,
  `catalog::rust_manifests::tests`, `impl_orch_lock_audit`
  (`full_lock_leaves_nothing_unverifiable`), `lock_audit::rust::tests`.
- Deviations: none
- Follow-up: add each new stable Rust release (and its digest) to the
  catalog when `RUST_VERSION` moves; the catalog test requires the newest row
  to be the pin.

## The gap

The Mise core Rust backend installs through rustup. Its `mise.lock` entry has no checksum.
See `supports_lockfile_url() == false` in the Mise file `src/plugins/core/rust.rs`.
Each generation therefore reported `tool_install_unverified: rust@X(no_checksums)`.

Rustup checks each downloaded component archive against the SHA-256 value in the downloaded channel manifest.
The missing control was a pin for the channel manifest itself.

## What Velnor does

1. The catalog records, per Rust release, the SHA-256 of
   `https://static.rust-lang.org/dist/channel-rust-<version>.toml`. Every row
   was produced by hashing the downloaded manifest and comparing it with the
   published `channel-rust-<version>.toml.sha256`; the rows cover every
   stable release from 1.85.0 to the catalog pin.
2. After `Prepare pinned tools` (and `Prepare Rust components`), the
   generated `Verify Rust channel manifest` step:
   - looks up the run-time `${VELNOR_TOOL_RUST}` in the digest table embedded
     in the step (the generated text stays version-free, hard decision 4);
   - fetches the manifest over HTTPS (`--proto =https --tlsv1.2`) and checks
     it with `sha256sum -c` (`shasum -a 256 -c` on macOS);
   - requires every SHA-256 recorded in the installed toolchain's
     `lib/rustlib/multirust-channel-manifest.toml` to appear in the verified
     manifest, and the installed manifest to record at least one.
   The step clears credentials from its environment and needs no token.
3. The step fails the job with a stable code:
   `rust_channel_manifest_not_catalogued` (a Rust version the catalog does
   not know), `rust_channel_manifest_digest_mismatch`,
   `rust_installed_manifest_missing`, `rust_installed_manifest_empty`, or
   `rust_component_hash_not_in_manifest` (the toolchain was installed from a
   different manifest).
4. The generate-time lock audit no longer lists `rust@X(no_checksums)` for a
   job that runs the step. A declared Rust version without a catalog digest
   fails generation with `rust_channel_manifest_not_catalogued:rust@X`
   instead of producing a workflow that would fail at run time.

The check compares the installed manifest metadata with the pinned channel manifest.
Rustup performs component verification during installation.
The same metadata check also applies to a toolchain restored from the tools cache.
This check does not hash unpacked files or prove that those files have not changed.

Local checks used real rustup installations. Versions 1.99.1 and 1.98.1 passed.
Each negative case failed with its diagnostic code: a wrong pinned digest, an altered installed-manifest component hash, and version 1.84.1 without a catalog entry.