# Freshness and tool qualification evidence — 2026-10-13

This update responds to the stale-evidence release block. The failed release
workflow [38317853839](https://github.com/tailrocks/velnor-new/actions/runs/38317853839)
stopped at freshness validation before publication; it created no tag or
release artifacts. This change updates the four non-held stale pins and their
generated workflows, records current upstream observations, and preserves all
valid holds. It does not change `mise.toml`, `mise.lock`,
`rust-toolchain.toml`, or `Cargo.lock`.

## Upstream observations

Two successful bounded `scripts/check-freshness.sh --check-upstream` runs
reported the same 20 source rows at `2026-10-12T22:43:28Z` and
`2026-10-12T22:59:12Z`. The full second receipt, source rows, failed attempt
receipts, and raw log hashes are in
[`freshness-upstream-probe-2026-10-13.json`](freshness-upstream-probe-2026-10-13.json).
The command exited 1 because it was run against the old inventory: the old
timestamps were stale and upstream had released newer versions. The probe is
not represented as a passing freshness gate. Two attempts at `22:52:46Z`
timed out; they are preserved separately and excluded from the refreshed
inventory.

| Pin | Observed latest | Update and compatibility evidence |
| --- | --- | --- |
| Mise | `2026.10.8` | Official release tag `v2026.10.8`, peeled commit `b671d356aaf33b87e48e7469a77abb40c3553c68`; raw binary digests are in [Mise platform pins](mise-platform-pins.md). The Linux ARM64 source-build archive hash was checked against the official manifest and downloaded bytes; a targeted assertion protects it. |
| `release-plz` | `0.3.172` | The crates.io crate checksum is `ca9813f5aff1fbfb88d016a5b6685257caad6270d6a667a9ea23c99d749ab81e`; embedded VCS commit `8b523fa6c4eb4992f18365ac50a196b4dc6a1013`. Its release removes the old `release-plz/` branch fallback. Velnor does not set `pr_branch_prefix`, and no active release PR depends on that fallback. Existing explicit config and phase-separated argv remain covered by tests. |
| `asamarts/alint` action | `v0.18.1` | Upstream tag resolves to `f558d76d076710312c54723d18e44d8cd95b3977`. The generated repository-policy job keeps the explicit `.alint.yml` config and existing checks. |
| `jdx/mise-action` | `v5.1.2` | Upstream tag resolves to `08ae2a091c038e1e05ec6f5115dc5c69d5a2a6b8`. Its new `mise_dir` export and Windows fallback do not change Velnor's explicit version/digest setup, cache-disabled mode, or unset `mise_dir`. |

Rust `1.99.0`, `mr-boxington` `v1.23.0`, `actions/download-artifact`
`v8.0.2`, `actions/upload-artifact` `v7.0.2`, and
`mr-boxington-action` `v1.7.2` remain held by their existing valid temporary
holds. Their pinned versions, technical reasons, owners, issues, grant dates,
and expiry dates are unchanged. The latest `mr-boxington-action` source
commit observed was `47ce49b32025d93a7a2f4570815afaa2b7092240`; the inventory
refreshes this upstream fact without altering the hold.

## Runner observation

The GitHub-hosted-runner reference was fetched at `2026-10-12T23:00:50Z` and
listed `ubuntu-26.04`, `ubuntu-24.04`, and `ubuntu-22.04`. A separate
successful exact-main CI run at base commit
`b3b5005830ce442907f36c5e0c815b2890e701af` recorded Ubuntu image
`ubuntu-26.04`, version `20260927.149.1`. The workflow log and documentation
capture hashes are recorded in the JSON receipt. This is a runtime image
observation, not a new formal platform-qualification result; the supported
runner labels remain unchanged.

## Qualification and local gates

The candidate Mise pin still requires the `mise-pin` workflow on this exact
source head to pass both Linux x64 and macOS x64 jobs. The inventory keeps
Mise, `release-plz`, Alint, and `mise-action` in `pending` status until their
required qualification completes. Do not interpret changed evidence
timestamps as qualification.

Workspace Clippy passed with Rust `1.98.1`, MBX `1.21.1`, and warnings denied.
The full Nextest run is being repeated from the signed candidate commit because
two repository harnesses intentionally build their committed input with
`git archive HEAD`; running them before committing tested the old base tree.
Formatting, Alint, Cargo Deny, workflow validation, and the offline freshness
gate are also to be recorded against the final candidate head before this
record is complete. No release workflow was published or merged as part of
this update.
