# Verified Rust installs (channel-manifest digest)

- State: implemented (this pull request); the pinned tree digest is implemented in fci/p3-tree-digest; the channel-manifest normal form was removed in favour of the overwrite (owner standing grant 2026-11-02, velnor-fast-ci GOAL.md; decisions.tsv row X44)
- Specification: [hard decision 4](../hard-decisions.md#4-tool-versions-come-from-the-projects-mise-config)
  (Trade-offs: `core:rust` lock entries carry no checksum),
  [consumer Rust setup](consumer-rust-setup.md)
- Delivered: `RUST_CHANNEL_MANIFESTS` in `velnor-actions-mise`
  (`catalog_rust_manifests.rs`); the file-by-file toolchain check
  (`toolchain_script` in `native_verifiers.rs` of
  `velnor-actions-workflow-renderer`) behind the generated
  `Verify restored Rust toolchain` and `Verify Rust channel manifest` steps in
  every plan, crate and dependency-seed job that installs Rust
  (`rust_manifest_step.rs`); and the lock-audit change that treats such an
  install as verified.
- Acceptance evidence: `rust_install_audit::tests` (the all-workflow
  coverage audit), `lock_audit_tests`
  (`a_bare_rust_install_is_a_blocking_lock_audit_finding`), and the
  `rust_tree_digest_pin` target of `velnor-actions-workflow-renderer`, which
  runs the rendered script against a fixture release: a replaced `rustc`, a
  planted or missing file, a file below a `manifest-*` directory and a
  tampered foreign-triple toolchain each fail with
  `rust_toolchain_bytes_mismatch` (`bookkeeping::b5`, `b6`, `b7`); component
  sums forged in the cache, also through a symlinked `rust-verified`, are
  discarded and the check fails (`bookkeeping::c1`); the installed phase
  reuses what the restored phase verified without the network
  (`bookkeeping::c2`); a substituted tarball, an off-site component URL, an
  uncatalogued version and a component the channel does not ship each fail
  with their code (`bookkeeping::r1`); a wrong channel digest is refused and
  not kept (`manifest::w1`), a poisoned job-local channel copy is never
  installed (`manifest::px3`); a renamed toolchain directory and every
  mutation under a matching pin fail (`hard_fail::n1a`, `n1b`, `n1c`).
- Deviations: none
- Follow-up: when `RUST_VERSION` moves, add a row for the new stable Rust
  release to `RUST_CHANNEL_MANIFESTS` in
  `crates/velnor-actions-mise/src/catalog_rust_manifests.rs`. Then run
  `scripts/derive-rust-tree-digest` for that release and add its tree pin to
  `RUST_TREE_DIGESTS` in
  `crates/velnor-actions-workflow-renderer/src/native_verifiers_tree_digest.rs`.

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
2. One script (`toolchain_script` in `native_verifiers.rs`) verifies a toolchain file by file.
   Its first command sets `PATH=/usr/bin:/bin:/usr/sbin:/sbin`, so every
   helper (`sha256sum`, `curl`, `tar`, `find`) comes from root-owned system
   directories and never from the restored Mise `bin/` or `shims/` that Setup
   Mise put on `PATH` first
   ([hermetic checks](../proposed/tools-cache-v2.md#the-checks-run-hermetically)).
   Every helper this script runs also runs under `/usr/bin/env -i` with an
   allowlist (`PATH`, `LC_ALL=C`, `HOME`, `RUNNER_TEMP`, `RUSTUP_HOME`, `MISE_RUSTUP_HOME`,
   `CARGO_HOME`, `VELNOR_TOOL_RUST`, `HTTPS_PROXY`, `HTTP_PROXY`, `NO_PROXY`,
   `ALL_PROXY` and their lowercase forms, `SSL_CERT_FILE`, `SSL_CERT_DIR`,
   `CURL_CA_BUNDLE`), so caller variables such as `TAR_OPTIONS`,
   `XZ_DEFAULTS` or `QUOTING_STYLE` change no verdict, and `curl` runs with
   `-q`:
   - it looks up the run-time `${VELNOR_TOOL_RUST}` in the digest table
     embedded in the step (the generated text stays version-free, hard
     decision 4), fetches the manifest over HTTPS (`--proto =https
     --tlsv1.2`) and checks its SHA-256 (`sha256sum`, `shasum -a 256` on
     macOS);
   - for every component the toolchain lists in `lib/rustlib/components` it
     takes the `xz_url` and `xz_hash` from the verified manifest, downloads
     the tarball (only from `https://static.rust-lang.org/dist/`), checks its
     SHA-256 and hashes every file the tarball installs;
   - it replaces `lib/rustlib/multirust-channel-manifest.toml`, which
     `rustup component add` and `rustup target add` read to pick download
     URLs, hashes, renames and profiles, with the verified
     `channel-rust-<version>.toml` bytes. rustup rewrites the file on install,
     so its bytes differ, and no parser compares the two forms. The bytes
     come from `$RUNNER_TEMP/velnor/rust-verified/channel-rust-<version>.toml`
     when its SHA-256 equals the catalog row, else from the download above
     (a download that is not the catalogued SHA-256 fails the step before
     anything is written and is not kept). The write goes to a temporary file
     in the same directory, mode 0644, then a rename, so a hard link is never
     written through; the symlink and special-file scan runs first, and a
     directory at that path fails the step. A missing manifest is recreated.
     The verified bytes also bind the downloads the job's later
     `rustup component add` and `rustup target add` take from the installed
     manifest (decisions.tsv X44, SEC-6);
   - it requires the toolchain directory to hold exactly those files with
     exactly those bytes, apart from rustup's own bookkeeping regular files
     directly under `lib/rustlib` (`components`, `manifest-*`, `multirust-*`,
     `rust-installer-version`), and no symlink or special file; the channel
     manifest is replaced as above in both phases of this fallback. A file below
     a `manifest-*` directory is not bookkeeping and is compared. A replaced
     `rustc`, a planted or deleted file, or a component dropped from the list
     (its files become unexpected) all fail.
   Expected sums are kept per component tarball in
   `$RUNNER_TEMP/velnor/rust-verified` after the restored phase has wiped the
   directory and recomputed them, so the installed phase downloads nothing the
   restored phase already checked. The step runs
   credential-scrubbed and needs no token.
3. Every owned-home job runs it twice:
   - `Verify restored Rust toolchain`, right before `Prepare pinned tools`
     and so after the tools cache restore and any seed copy, before Mise or
     rustup touches the toolchain (on a cache hit `mise install` already runs
     `rustup` and `rustc`). It deletes the restored
     `$CARGO_HOME/bin` (rustup and its proxies), so Mise reinstalls a fresh
     rustup instead of running a cached one. It also deletes
     `$RUNNER_TEMP/velnor/rust-verified`, so a sums file planted by a
     poisoned cache or a left-over earlier step cannot stand in for the
     tarball check. It then compares a restored
     toolchain with a pinned installed-tree digest (type, path, mode and
     SHA-256 of every entry, derived from the component tarballs of the
     pinned channel manifest; the stored `multirust-channel-manifest.toml` is
     an ordinary entry bound by its SHA-256, and this phase never repairs it:
     a changed or missing manifest is a `rust_tree_digest_mismatch`) before
     any network access. A match copies the verified manifest into
     `$RUNNER_TEMP/velnor/rust-verified`, so the installed phase below stays
     offline. The pin is selected by the job's expected
     Rust version and target, never by the restored `components` file or the
     toolchain directory name. The owned-home steps render the target
     `x86_64-unknown-linux-gnu`. Other host triples have no pin row yet (owner
     standing grant 2026-11-02, velnor-fast-ci GOAL.md; decisions.tsv X35): a
     `toolchains/` directory holding exactly one entry named
     `<version>-<a>-<b>-<c>` that is not `<version>-<target>` (for example
     `aarch64-unknown-linux-gnu`, `aarch64-apple-darwin`,
     `x86_64-apple-darwin` or `x86_64-unknown-linux-musl`) is treated as
     unpinned and takes the file-by-file check in both phases, which is blind
     to modes and bookkeeping bytes but still fails a tampered tree. Any other entry, a lone entry that is not
     triple-shaped (`<version>-evil`), or a foreign entry beside another,
     fails. A match passes. If a pin row exists for that
     version and target, only the pinned path runs: a components set no row
     pins, a `toolchains/` entry other than exactly `<version>-<target>`, a
     tree or listing difference, or a bookkeeping file over the size cap
     fails the job with `rust_tree_digest_mismatch` and the
     `pin_mismatch` notice names the check (`components_not_pinned`,
     `toolchains_differ`, `tree_differs`, and so on). Only if no pin row
     exists for that version and target does the step print
     `pin_mismatch: no_pin` and fall back to the file-by-file check in step 2,
     which does not see modes or bookkeeping bytes. The pin covers
     `manifest-<component>` and `rust-installer-version` byte for byte;
     `multirust-config.toml` is a size-capped regular-file entry. The cache
     payload holds no memo of a passed check. With nothing restored the step
     does nothing;
   - `Verify Rust channel manifest`, after `Prepare pinned tools` and
     `Prepare Rust components`, on the toolchain the job will use. It first
     replaces the manifest rustup rewrote with the verified channel bytes
     (step 2), then runs the same pinned comparison, and falls back to the
     file-by-file check only when no pin row exists.
   The pinned listing is built with `find -printf` (GNU `find`), which the
   file-by-file path does not need; a toolchain directory of another host
   triple (macOS, ARM or musl Linux) never reaches the pinned listing.
   `scripts/derive-rust-tree-digest` derives a pin row, sets
   `PATH=/usr/bin:/bin:/usr/sbin:/sbin` itself before anything else (so no
   helper comes from the caller's `PATH`),
   always re-executes itself once under `env -i` with the same allowlist minus
   `PATH` (it trusts no environment marker: recursion is detected by an
   argument the re-execution adds and the script then consumes, so a caller's
   `TAR_OPTIONS`, `XZ_DEFAULTS`, `QUOTING_STYLE` or `LC_ALL` never reaches
   `tar`, `xz` or `find`), refuses a
   blank or malformed version or target before any download, writes only to stdout (digests, the pin row and
   the listing) and stages the downloaded
   channel manifest as the manifest entry. A manifest-form token in the
   tools-cache static digest keeps entries stored before this change, whose
   manifest holds rustup's rewritten bytes, from being restored.
   Residual risks (cache decision 10, decisions.tsv X35 and X44): the content
   of `multirust-config.toml` is unbound (denial of service only); for a
   version and target with no pin row, and for a toolchain directory of
   another host triple,
   the file-by-file fallback overwrites the channel manifest but leaves the
   `manifest-*` bytes, the `multirust-config.toml` content and all file modes
   unbound, which affects consumer repos that pin a Rust other than the
   pinned 1.99.1 in owned-home jobs (the default-home jobs always use that
   fallback); the pin binds only `toolchains/`, while the tools cache also
   restores the rest of `$RUNNER_TEMP/velnor/rustup` (`settings.toml`,
   `update-hashes`, `downloads`), which no check binds (info); a reordered
   `components` file is accepted, because it is hashed sorted.
   A poisoned cache entry therefore fails the job before any restored Rust
   byte runs; delete the entry (`gh cache delete <key>`) to recover.
4. The step fails the job with a stable code:
   `rust_channel_manifest_not_catalogued` (a Rust version the catalog does
   not know), `rust_channel_manifest_digest_mismatch`,
   `rust_installed_manifest_unwritable`,
   `rust_toolchain_missing`, `rust_installed_manifest_missing`,
   `rust_installed_manifest_empty`, `rust_toolchain_file_unexpected`,
   `rust_tree_digest_mismatch`,
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

## Coverage: every job that installs Rust

The same script, in its installed phase on the runner's default rustup home
(`${RUSTUP_HOME:-$HOME/.rustup}`), runs as `Verify Rust channel manifest`
directly after the Rust install in every other generated job:

| File | Job or action |
|---|---|
| `product-release-binary.yml` | `build-binary` (the macOS `velnor-host` build) |
| `product-release-images.yml` | the resource-probe image build, after the musl target |
| `.github/actions/generator-release-build-*` | the `velnor-actions` release builds, after the Intel target on the cross leg |
| `.github/actions/generator-release-qualify-*` | release qualification |
| `qualification.yml` | `mbx-cache-write-hosted`, `mbx-cache-read-hosted` (the toolchain is put on `PATH` only after the check) |
| `binary-release.yml`, `binary-preview.yml` | every payload producer, after `Install pinned Cargo metadata toolchain` and before `Fetch Cargo sources` |
| `upgrades.yml` | `prepare` and `verify`, inside the regeneration prerequisites: right after the fresh `rust@<pin>` install and before `cargo fetch` (`VELNOR_VERIFY_RUST_SCRIPT`) |

These default-home steps carry no tree pin. They restore no cache and install
the default profile, so they always run the file-by-file check.

`freshness.yml` is not among them. Its `freshness` job installs into owned,
job-local homes and runs the pinned installed-phase verifier only (no restore
phase, no tools cache). It installs `rust[profile=minimal]@<pin>` and adds no
component: its probe runs `cargo run --locked -p velnor-actions-cli` and
`cargo metadata` only. The pinned verifier requires the installed component
set to equal a pinned row, and a bare `rust@<pin>` install yields the default
profile, which no row pins, so the job would exit with
`pin_mismatch: components_not_pinned`. The two pinned sets are the minimal
profile (`cargo`, `rust-std`, `rustc`) and the minimal profile plus
`clippy-preview` and `rustfmt-preview`.

Two more job kinds run on the owned homes with the tools cache, so they also
get `Verify restored Rust toolchain` before their install:

| File | Job |
|---|---|
| `ci.yml` | every `image-*` job that builds a Cargo binary into an image |
| `suites.yml` and the suite jobs in `ci.yml` | every declared Rust test suite job (build, run, sharded) |

The default-home jobs resolve the rustup home as `RUSTUP_HOME`, then
`MISE_RUSTUP_HOME`, then `$HOME/.rustup`, which is where Mise installs it.

`ProductReleasePins`, `MbxQualificationPins` and `FreshnessSpec` carry the
argv (`verify_rust_argv`), built by the orchestrator from the catalog.

Generation enforces the coverage (`rust_install_audit.rs`): after rendering,
it reads every generated `.github/**/*.yml` (workflows and composite actions,
`ci.yml` step anchors resolved) and fails with
`rust_install_unverified:<file>:<step>:<why>` when a run of steps that
installs or changes Rust (`install rust@`, `install rust[`, `rustup toolchain install`,
`rustup component add`, `rustup target add`, `rustup-init`) is not followed
by the verification before a step runs Rust, Cargo or MBX, or already runs
`rustc`, Cargo or MBX itself. A step may verify its own install only by
calling `$VELNOR_VERIFY_RUST_SCRIPT` after every install and before every use
in its text (the `upgrades.yml` scripts). The same audit fails the step when any
`rust[` option is not exactly `profile=minimal]@`; the bare `rust@` form stays
allowed for release installs. Removing
the step from the resource-probe build makes `generate` fail with
`rust_install_unverified:.github/workflows/product-release-images.yml:Install pinned Rust for the resource probe:no verification directly follows the install`.

## The other cached tools

The same cache entry restores the other Mise installs (MBX, Nextest and the
rest of `~/.local/share/mise/installs`). Since cache decision 10 the restore
composite re-verifies each of them against its `mise.lock` or catalog
archive pin and discards any that does not match; see
[tools cache V2](../proposed/tools-cache-v2.md#re-verification-after-restore).
