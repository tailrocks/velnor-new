# Velnor 86f Cargo seed capsule

This directory holds a lock-bound seed plan and candidate tooling for the exact
Velnor source commit `86f864aabc2192a9f8ccd3f01f2e25b7858c3f20`, tree
`1e16a9270f3ea99b04e68521cfc876fa19db67ac`. The pin is repeated in
`seed_candidate.py`; preparation refuses a different commit, tree, dirty source
worktree, or `Cargo.lock` SHA-256. The expected lock digest is
`95d251ccedbcc12095cecb89c9c08b14d72a5f0774d252029a559d45bb557b1a`.

The root lockfile is version 4 with 93 package instances: 84 crates.io
versions covering 83 names and 9 path packages. There are no Git dependencies.
The workspace contains the nine path packages and excludes
`crates/velnor-runner`; separate runner and fixture lockfiles are outside this
seed scope. Preparation includes every locked target and dependency kind,
including target-specific, build, and dev dependencies.

The portable capsule has exactly three data groups:

- `index-records/<crates.io-index-path>/<version>.json` contains only the
  matching index row for one locked package version. It omits other published
  versions and all registry configuration.
- `crates/<name>-<version>.crate` contains the archive whose SHA-256 equals the
  checksum in `Cargo.lock`.
- `sources/<name>-<version>/` contains regular files extracted from that
  verified archive. The manifest records each file's path, size, normalized
  mode, and SHA-256, including the extracted `Cargo.toml` identity.

`manifest.json` binds those rows and files to the source commit, tree, and lock
digest. `manifest.sha256` binds the manifest itself. The verifier compares the
capsule package list against the pinned source's lockfile, validates every
index checksum and archive checksum, compares extracted files to the archive,
and rejects symlinks, extra files, and configuration or credential files.
Neither script reads Cargo configuration, credential stores, environment
registry settings, or package code. The scripts do not invoke Cargo, Rust,
build scripts, or network clients.

## Review boundary

The read-only inventory found one ambient crates.io cache at
`/root/.cargo/registry/index.crates.io-1949cf8c6b5b557f`: all 84 locked versions
had matching sparse index rows, checksum-valid `.crate` archives, and extracted
source `Cargo.toml` files. The archives total 11,182,614 bytes compressed and
66,131,200 bytes unpacked; the largest archive is 3,006,116 bytes, the largest
crate is 18,183,406 bytes unpacked, and the largest archive has 473 members.
These measurements fit the script's explicit limits with room for the exact
lock: 64 MiB per archive, 128 MiB unpacked per crate, 512 MiB total, and 20,000
members per crate. That cache has no reviewed seed manifest binding it to this
source lock. It was inspected only; this task did not copy from it or write to
the shared cache or rootfs.

After independent review of this boundary and the offline fixtures, the
candidate can be prepared into a new directory under this private task
directory with explicit inputs:

```sh
python3 prepare_candidate.py \
  --source-root /root/.velnor-generator-preview-20261005/main-source-86f \
  --registry-root /root/.cargo/registry \
  --output /root/.velnor-generator-preview-20261005/cargo-seed-86f/candidates/86f-95d251cc
```

That command is documented for a later reviewed step and was not run here. It
copies only the exact lock subset and creates extracted source files from the
verified archives; it does not copy Cargo index caches, registry config,
credentials, a whole `CARGO_HOME`, or rootfs files. A resulting capsule can be
checked without network access:

```sh
python3 verify_candidate.py \
  /root/.velnor-generator-preview-20261005/cargo-seed-86f/candidates/86f-95d251cc \
  --source-root /root/.velnor-generator-preview-20261005/main-source-86f
```

The capsule is a review artifact, not a ready-made Cargo home. Any adapter that
places its rows and archives into a rootfs or shared Cargo cache remains a
separate, reviewed step.

The lock-only plan can be regenerated into this private directory without
reading any Cargo cache:

```sh
python3 write_plan.py \
  --source-root /root/.velnor-generator-preview-20261005/main-source-86f \
  --output /root/.velnor-generator-preview-20261005/cargo-seed-86f/seed-plan.json
```

The offline fixtures construct a tiny local crates.io cache with two locked
versions of one crate. They cover index selection, archive checksum checking,
safe source extraction, source pinning, exact capsule membership, and rejection
of unexpected configuration data. They use only local temporary files and
Git; they do not contact a registry or execute crate code.
