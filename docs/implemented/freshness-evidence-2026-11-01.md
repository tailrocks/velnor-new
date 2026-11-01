# Freshness evidence — 2026-11-01

VER-3.2 probe of the stale pins found on 2026-11-01: mise
`2026.10.21 -> 2026.10.25`, jactionlint `2.3.0 -> 2.4.0`, and
`jdx/mise-action` `v5.2.4 -> v5.2.5` (tag commit
`6612f0ef07ed2a736cd1e00a94a065522dd41195`). All three bumps are held
(see below); no pin changes in this set.

Two independent authenticated captures (`capture-1/`, `capture-2/` under
[the evidence directory](evidence/freshness-2026-11-01/)) agree on all 25
tool/action rows; `capture-agreement.json` binds both by SHA-256. Every
pinned action tag resolves to the inventory SHA in both captures. No rows change pin/qualified; only `latest` reflects upstream.
mr-boxington (`1.28.0`), mise (`2026.10.21`), jactionlint (`2.3.0`) and
`jdx/mise-action` (`v5.2.4`) are held (temporary holds in the inventory,
granted 2026-11-01, expires 2026-11-15) because the adopted controller
0.1.66 catalogues those versions; their bumps land with controller
adoption after the SDK 0.1.70 release. All 25 `checked_at`
timestamps refresh from the capture-2 observations.

Digest sources (all official upstream release assets, cross-checked two
ways): mise `SHASUMS256.txt` entries equal the GitHub release API asset
digests for linux-x64 (`5d362ecc…`, 176351376 bytes), macos-arm64
(`8a3ef007…`, 140146464 bytes) and macos-x64 (`e06eb3f0…`, 171256016
bytes). jactionlint archive SHA-256 values equal both the release API
digests and the published `jactionlint_2.4.0_checksums.txt`; executable
SHA-256 values are computed from the extracted official tarballs
(darwin-arm64 binary reports `2.4.0`). mr-boxington and mise-action need
no executable digest catalog entries; their lock entries use the release
API asset digests.

Runner evidence is separate: the official hosted-runner reference still
lists `ubuntu-26.04/24.04/22.04`, and successful main CI run `41318649363`
(job `124883983115`, source `7bbb80f0…`) records image `ubuntu-26.04`
version `20261025.179.1`, runner `2.338.2`. This is a normal CI runtime
observation, not formal qualification. See
`evidence/freshness-2026-11-01/runner/runner-observation.json`.

Repo-owned Mise mirrors (`mise.toml`, `mise.lock`, `.velnor/mise.toml`,
`.velnor/mise.lock`), the compiled catalogs and generated workflows stay
at the main pins for all held tools; no toolchain or other tool pin
changes.

Gates: `scripts/check-freshness.sh` PASS; `cargo fmt --all -- --check`
PASS; focused nextest for mise/contract/actionlint (14), orchestrator +
workflow-renderer (336), and cli + freshness (68) PASS. The live
`--check-upstream` probe passed at 2026-11-01T15:23Z; a later re-run hit
the unauthenticated GitHub API rate limit (HTTP 403 on two rows) — see PR
notes. Full workspace nextest/clippy were not run locally; CI covers them.
