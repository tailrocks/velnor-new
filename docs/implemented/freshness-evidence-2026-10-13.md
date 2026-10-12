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

The exact candidate SHA `438bd90d2ef64515373f413b501b7ae01189a8e7` passed both
hosted jobs in the `mise-pin` qualification dispatch: [Linux x64 job
115050479288](https://github.com/tailrocks/velnor-new/actions/runs/38321521320/job/115050479288)
and [macOS x64 job
115050479238](https://github.com/tailrocks/velnor-new/actions/runs/38321521320/job/115050479238).
Both verified the exact Mise version and binary digest. An earlier API read
showed the enclosing dispatch `in_progress` with no conclusion despite all
jobs being terminal. It subsequently finalized at `2026-10-12T23:30:12Z` as
`completed/success`; the final API summary has 61 terminal jobs (2 success,
59 skipped, 0 unfinished). This workflow has no `Required` job. The PR's
`Required` result remains the final repository CI check.

The final-state full workspace Nextest run passed all 3,699 tests (2 skipped;
4 slow), run ID `d3d7207f-d1c7-40ba-bb72-4cd5f06adfb8`; its command and
summary are recorded in the [Nextest receipt](evidence/freshness-2026-10-13/workspace-nextest-run.json)
(SHA-256 `389a7cd6aa7cdea6d088bb6c9d8e9af29d177b96972f8411ed76bbabf99e4b87`).
It ran with Rust `1.98.1`, Mise `2026.10.8`, and MBX `1.21.1`. Workspace
doctests passed (0 doctests present). Both `scripts/check-freshness.sh` and
`scripts/check-freshness.sh --with-advisories` passed after qualification.
Other completed gates:
`cargo fmt --all -- --check`, Clippy with warnings denied, both Cargo Deny
workspaces, Actionlint, Alint `check --fail-on-warning` (three existing
informational notices), Zizmor with no findings, shell syntax and diff checks,
generated `.github` tree byte comparison against CLI output, and all five
OpenTofu workflow goldens. The updated inventory records the four non-held
pins as qualified/current against the successful bounded upstream
observations above. No release workflow was published or merged as part of
this update.
