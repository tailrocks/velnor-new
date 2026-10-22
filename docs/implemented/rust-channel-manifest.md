# Verified Rust installs (channel-manifest digest)

- State: implemented (this pull request)
- Specification: [hard decision 4](../hard-decisions.md#4-tool-versions-come-from-the-projects-mise-config)
  (Trade-offs: `core:rust` lock entries carry no checksum),
  [consumer Rust setup](consumer-rust-setup.md)
- Delivered: `RUST_CHANNEL_MANIFESTS` in `velnor-actions-mise`
  (`catalog_rust_manifests.rs`); the file-by-file toolchain check
  (`rust_toolchain_script.rs`) behind the generated
  `Verify restored Rust toolchain` and `Verify Rust channel manifest` steps in
  every plan, crate and dependency-seed job that installs Rust
  (`rust_manifest_step.rs`); and the lock-audit change that treats such an
  install as verified.
- Acceptance evidence: `rust_toolchain_script::tests` (the script executed
  against a fixture release: a replaced `rustc`, an extra, missing or
  symlinked file, an unlisted component, a substituted tarball, a wrong
  manifest digest and a poisoned cache each fail),
  `rust_manifest_step::tests`, `catalog::rust_manifests::tests`,
  `impl_orch_lock_audit`
  (`full_lock_leaves_nothing_unverifiable`), `lock_audit::rust::tests`.
- Deviations: none
- Follow-up: add each new stable Rust release (and its digest) to the
  catalog when `RUST_VERSION` moves; the catalog test requires the newest row
  to be the pin.

## The gap

Mise's core Rust backend installs through rustup, and its `mise.lock` entry
has no checksum (`supports_lockfile_url() == false` in Mise's
`src/plugins/core/rust.rs`). Every generate therefore reported
`tool_install_unverified: rust@X(no_checksums)`.

rustup verifies each component tarball against the SHA-256 in the channel
manifest it downloads, but that proves nothing later: the tools cache restores
`${{ runner.temp }}/velnor/rustup` in every job, and any default-branch job
that runs code (CI running build scripts and tests, the upgrade job running
releases published minutes ago) holds the runner's cache credential and can
write the next entry. A check of rustup's own metadata
(`multirust-channel-manifest.toml`) passes with a replaced `bin/rustc`; the
first version of this step did exactly that.

## What Velnor does

1. The catalog records, per Rust release, the SHA-256 of
   `https://static.rust-lang.org/dist/channel-rust-<version>.toml`. Every row
   was produced by hashing the downloaded manifest and comparing it with the
   published `channel-rust-<version>.toml.sha256`; the rows cover every
   stable release from 1.85.0 to the catalog pin.
2. One script (`rust_toolchain_script.rs`) verifies a toolchain file by file:
   - it looks up the run-time `${VELNOR_TOOL_RUST}` in the digest table
     embedded in the step (the generated text stays version-free, hard
     decision 4), fetches the manifest over HTTPS (`--proto =https
     --tlsv1.2`) and checks its SHA-256 (`sha256sum`, `shasum -a 256` on
     macOS);
   - for every component the toolchain lists in `lib/rustlib/components` it
     takes the `xz_url` and `xz_hash` from the verified manifest, downloads
     the tarball (only from `https://static.rust-lang.org/dist/`), checks its
     SHA-256 and hashes every file the tarball installs;
   - it requires the toolchain directory to hold exactly those files with
     exactly those bytes, apart from rustup's own bookkeeping
     (`lib/rustlib/components`, `manifest-*`, `multirust-*`,
     `rust-installer-version`), and no symlink or special file. A replaced
     `rustc`, a planted or deleted file, or a component dropped from the list
     (its files become unexpected) all fail.
   Expected sums are kept per component tarball in
   `$RUNNER_TEMP/velnor/rust-verified` for the rest of the job, so the second
   run in a job downloads nothing it already checked. The step runs
   credential-scrubbed and needs no token.
3. Every owned-home job runs it twice:
   - `Verify restored Rust toolchain`, right before `Prepare pinned tools`
     and so after the tools cache restore and any seed copy, before Mise or
     rustup touches the toolchain (on a cache hit `mise install` already runs
     `rustup` and `rustc`). It deletes the restored
     `$CARGO_HOME/bin` (rustup and its proxies), so Mise reinstalls a fresh
     rustup instead of running a cached one, and verifies a restored
     toolchain; with nothing restored it does nothing;
   - `Verify Rust channel manifest`, after `Prepare pinned tools` and
     `Prepare Rust components`, on the toolchain the job will use.
   A poisoned cache entry therefore fails the job before any restored Rust
   byte runs; delete the entry (`gh cache delete <key>`) to recover.
4. The step fails the job with a stable code:
   `rust_channel_manifest_not_catalogued` (a Rust version the catalog does
   not know), `rust_channel_manifest_digest_mismatch`,
   `rust_toolchain_missing`, `rust_installed_manifest_missing`,
   `rust_installed_manifest_empty`, `rust_toolchain_file_unexpected`,
   `rust_component_not_in_manifest`, `rust_component_name_invalid`,
   `rust_component_url_untrusted`, `rust_component_tarball_digest_mismatch`
   or `rust_toolchain_bytes_mismatch`.
5. The generate-time lock audit no longer lists `rust@X(no_checksums)` for a
   job that runs the step. A declared Rust version without a catalog digest
   fails generation with `rust_channel_manifest_not_catalogued:rust@X`
   instead of producing a workflow that would fail at run time.

Checked locally against a real rustup install (1.99.1, default profile with
`rust-docs`, aarch64-apple-darwin) with the rendered `ci.yml` steps: the
restored-phase run verified it in 73 s (all tarballs downloaded), the
installed-phase run in 23 s (sums reused), and replacing `bin/rustc` fails.

## Remaining cache trust (named, not fixed here)

The same cache entry restores the other Mise installs (MBX, Nextest and the
rest of `~/.local/share/mise/installs`); only the Mise binary itself is
re-checked against its pinned digest on restore. Those installs carry
`mise.lock` checksums at install time but are not re-verified after a
restore, so a default-branch job that runs code could poison them the same
way. Closing that needs a post-restore check of every install against
`mise.lock`, which is a tools-cache contract change for the owner.
