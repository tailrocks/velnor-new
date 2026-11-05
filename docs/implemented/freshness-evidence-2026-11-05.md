# Freshness evidence — 2026-11-05

This set refreshes all `checked_at` values in `.velnor/freshness-inventory.json` and moves three pins to the newest stable releases: `mise` `2026.10.25` to `2026.11.1`, `mr-boxington` `1.29.1` to `1.29.2`, and `opentofu` `1.13.2` to `1.13.8`. The 2026-11-03 evidence aged past the 24 h interval at `2026-11-04T11:38:00Z`, so `scripts/check-freshness.sh` failed on main before this change.

## Capture time

- `capture-1` ran from `04:32:57Z` to `04:33:40Z`. `capture-2` ran from `04:33:43Z` to `04:34:27Z`.
- The runner documentation was fetched at `04:35:18Z`.
- The oldest `checked_at` in the inventory is `2026-11-05T04:33:43.627Z`. The 24 h freshness window ends at `2026-11-06T04:33:43Z`.
- `product-release.yml` and `product-release-generator.yml` run `scripts/check-freshness.sh` offline. A release dispatch must start before the window ends. If it cannot, capture again in a small PR that merges before dispatch (`python3 -I docs/implemented/evidence/freshness-2026-11-05/capture.py capture|agree|apply`, see Captures).

## Capture conditions

- The GitHub requests ran through `gh api` with the session credentials (authenticated rate limit). The crates.io and Rust manifest requests use `curl` with the user agent `velnor-freshness-capture`. No token is stored in any file.
- `scripts/check-freshness.sh --check-upstream` is unauthenticated. It ran at `04:37:36Z` and exited 0: 348 pass rows, 0 fail rows, 6 info rows, 25 `upstream-probe` rows all `pinned==latest`.

## Captures

Two independent captures are in [the evidence directory](evidence/freshness-2026-11-05/): `capture-1/` and `capture-2/`. [`capture.py`](evidence/freshness-2026-11-05/capture.py) produced both and the inventory update.

- Each capture queries all 25 tool and action subjects (12 tools, 13 actions) and holds 85 receipts, `comparison.json`, `observed-at.json` and `receipts.json`. `receipts.json` binds each response body by SHA-256. `rust.headers.json` records the ranged read of the Rust manifest.
- For every action and for `mr-boxington`, each capture records the pinned and the latest tag as the raw ref, the tag object (when the tag is annotated) and the peeled commit. Every pinned action tag resolves to the SHA in the inventory, and the capture stops on a mismatch.
- `capture-agreement.json` binds both captures. The `comparison.json` hashes and the `observed` fields of all receipts are identical, so the result is `agree: true`.
- The `apply` step reads `capture-2`, writes `latest`, `checked_at`, `status` and `latest_sha` for each row, and sets `qualified` equal to `pinned` only for the three bumped tools. All 25 rows are `current`.

## Rows that differed from the pin

Before the bump, three rows were behind upstream. This set bumps all three.

| Subject | Pinned before | Upstream latest | Upstream published |
|---|---|---|---|
| `mise` | `2026.10.25` | `v2026.11.1` | `2026-11-04T19:13:36Z` |
| `mr-boxington` | `1.29.1` | `v1.29.2` | `2026-11-03T18:02:27Z` |
| `opentofu` | `1.13.2` | `v1.13.8` | `2026-11-04T21:45:55Z` |

The other 22 rows match their pins. All three releases are stable and not drafts (`bump/*-release.json`).

## Why the pin records change together

The pin mirror (`VELNOR_REPO_POLICY_ACTION=pin-mirror`, required task `test-maintenance-helpers`) compares `.velnor/version-policy.toml`, the compiled catalog and `.velnor/freshness-inventory.json`. A policy or catalog change without the inventory fails it (`local-pin` and `policy-mirror` rows). The mise bump must also move the Mise bootstrap triple in `.velnor/generator.lock` and in the active entry of `.velnor/controller-approvals.json`. The candidate leg compares the lock with the catalog (`lock_mismatch:mise_bootstrap`), and the lock and the approval record must name the same triple (`verify_approved_mise_bootstrap`). The approval `manifest_digest` covers only the controller version, repository, commit and target binaries (`canonical_manifest_digest` in `controller_manifest_digest.rs`), so the bootstrap triple changes without a new digest. `mise_runtime` (the controller's own Mise, `2026.10.19`) and `gh` are controller-visible and do not change. No velnor-actions release is needed first.

## Bumps

`mise` `2026.11.1`:

- Tag `v2026.11.1` is the annotated tag object `80b7205c…`. It resolves to commit `0fa55804…` and tree `865bec85…`, which are the `source_commit` and `source_tree` of the source-build bootstrap asset.
- The standalone `linux-x64` (`7129dbab…`, 177907728 B) and `macos-x64` (`ead24142…`, 172294368 B) binaries and the `linux-arm64` and `macos-arm64` archives match the GitHub release API digest, the vendor `SHASUMS256.txt` and the SHA-256 of the downloaded file. The inner `mise/bin/mise` of the `linux-arm64` archive is `ed5dc8d5…` and of the `macos-arm64` archive `9ca10059…`.
- Changed: `.mise-version`, the version policy, the catalog constants (`MISE_VERSION`, the Linux x64 digest, the macOS digests and byte counts), the source-build bootstrap assets, `scripts/owned_tool_source.py`, the `source-qualification.sh` expected value, the qualification pin constants, the Mise bootstrap triple in the lock and the approval record, the README and the contract docs.
- `src/config/command_wrapper.rs` is identical at `v2026.10.25` and `v2026.11.1`, and `docs/lang/rust.md` gained only a `RUSTUP_DIST_SERVER` paragraph, so [consumer-rust-setup.md](consumer-rust-setup.md) stays accurate.

`mr-boxington` `1.29.2`:

- Tag `v1.29.2` is the annotated tag object `b3c50e5f…`. It resolves to commit `7fa83b28…` and tree `f0bcb989…`.
- The `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu` and `aarch64-apple-darwin` archives match the release API digest, the vendor `SHA256SUMS` and the downloaded file. The inner `mbx` binaries are `03337a94…`, `31c0dd83…` and `c3f14e1e…`. The same method reproduces the 1.29.1 pins, so the values are comparable.
- The compare `v1.29.1...v1.29.2` has 7 commits. It changes `mbx` rustc dep-info and out-dir handling and the changelogs, and none of the files that [consumer-rust-setup.md](consumer-rust-setup.md) cites except the changelogs.
- Changed: the version policy, the catalog constant and bootstrap assets, `scripts/owned_tool_source.py`, `source-qualification.sh`, `mise.toml`, `.velnor/mise.toml`, both `mise.lock` files (from `mise lock`), the README and three fixture versions.

`opentofu` `1.13.8`:

- Tag `v1.13.8` is the annotated tag object `8f9577e1…`.
- The four catalog archives (`linux_amd64` `dab8135d…`, `linux_arm64` `fb4a3cba…`, `darwin_amd64` `1e829e67…`, `darwin_arm64` `dc53f799…`) match the release API digest and `tofu_1.13.8_SHA256SUMS`.
- `cosign verify-blob` of `tofu_1.13.8_SHA256SUMS` with its `.sig` and `.pem` passes for the identity `https://github.com/opentofu/opentofu/.github/workflows/release.yml@refs/heads/v1.13` and the issuer `https://token.actions.githubusercontent.com`.
- The `darwin_arm64` binary from the archive runs and prints `OpenTofu v1.13.8`.
- Changed: the version policy, the catalog constant and four digests, `mise.toml` and `mise.lock` (from `mise lock`).

The `mise lock` output for both lock files changed only the two re-resolved tools. Every new checksum equals an upstream asset digest (`bump/qualification.json`). The new `checksum_source = "mise-versions"` lines on some OpenTofu platforms record where Mise took the checksum, and the values equal the release digests.

The evidence files are in `evidence/freshness-2026-11-05/bump/`: the three release responses, the vendor checksum files, the OpenTofu signature files and `qualification.json`. The `qualified` values of the three rows equal the new pins on this evidence. The hosted `Source qualification` and `Plan` jobs of this pull request install and run the pinned binaries on the CI runner.

## Runner

The official hosted-runner reference still lists `ubuntu-26.04`, `ubuntu-24.04` and `ubuntu-22.04` (`runner/runner-doc.html`). The inventory keeps `ubuntu-26.04` as default.

Successful main CI run `41954899634` (job `127007350554` `Plan`, source `102936d506c5f7187cada60bb7065781b018c2a2`) records image `ubuntu-26.04`, version `20261101.186.1`, and runner `2.338.2`. This is a normal CI runtime observation, not formal qualification. The files are in `evidence/freshness-2026-11-05/runner/`.

## Gates

- `scripts/check-freshness.sh` exits 0: `check-freshness: PASS`, 323 pass rows, 0 fail rows, 5 info rows.
- `scripts/check-freshness.sh --check-upstream` exits 0 (see Capture conditions).
- The pin-mirror action passes against `refs/remotes/origin/main` and reports the three tool changes as `pin-diff` info rows.
