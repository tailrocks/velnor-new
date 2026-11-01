# Freshness evidence — 2026-11-01

VER-3.2 probe of the stale pins found on 2026-11-01: mise
`2026.10.21 -> 2026.10.25`, jactionlint `2.3.0 -> 2.4.0`, and
`jdx/mise-action` `v5.2.4 -> v5.2.5` (tag commit
`6612f0ef07ed2a736cd1e00a94a065522dd41195`). All three bumps are held
(see below); no pin changes in this set.

Two independent authenticated captures (`capture-1/`, `capture-2/` under
[the evidence directory](evidence/freshness-2026-11-01/)), recaptured live
on 2026-11-01 against the held pins, agree on all 25 tool/action rows;
`capture-agreement.json` binds both by SHA-256. Captures were regenerated
after the pin bumps were reverted to temporary holds, so each held
subject records its held version as `pinned` and upstream latest
separately as `observed`: mise pinned `2026.10.21` / observed
`v2026.10.25`, mr-boxington pinned `1.28.0` / observed `v1.29.1`,
jactionlint pinned `2.3.0` / observed `v2.4.0`, and `jdx/mise-action`
pinned `v5.2.4` / observed `v5.2.5` (these four rows have `match: false`
by design — that difference is what the holds cover). For every held
tool both captures contain a `pinned-<tool>` release receipt and a
`pinned-<tool>-tag-0` tag receipt resolving the held tag to its SHA
(mise `v2026.10.21` → `61eb04dbc806cb646821e342b7abbbdaf1581e84`,
mr-boxington `v1.28.0` → `44f50c329a83dedd012e1e49cf9f265f79226a83`,
jactionlint `v2.3.0` → `a65ab044e916760429068fe5794b8306abdfcbb9`). For
`jdx/mise-action` the `pinned` tag receipt requests
`git/ref/tags/v5.2.4` and resolves to the inventory SHA
`6be3c7ed4c21d4f76b208d351d95aac273ebb76d`; the `latest` receipt
separately requests `git/ref/tags/v5.2.5` →
`6612f0ef07ed2a736cd1e00a94a065522dd41195`. Every pinned action tag
resolves to the inventory SHA in both captures. No rows change
pin/qualified; only `latest` reflects upstream.
mr-boxington (`1.28.0`), mise (`2026.10.21`), jactionlint (`2.3.0`) and
`jdx/mise-action` (`v5.2.4`) are held (temporary holds in the inventory,
granted 2026-11-01, expires 2026-11-15) because the adopted controller
0.1.66 catalogues those versions; their bumps land with controller
adoption after the SDK 0.1.70 release. All 25 `checked_at`
timestamps refresh from the capture-2 observations.

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

Gates: `scripts/check-freshness.sh` PASS (recapture re-run
2026-11-01T16:50Z against the held tree; output in
`evidence/freshness-2026-11-01/refreshed-native-freshness.log`, showing
the held pins and the four temporary holds expiring 2026-11-15). `scripts/check-freshness.sh --check-upstream` exits 1
by design: its live probe reports exactly the four held rows as stale
pins (mise `2026.10.21` vs `v2026.10.25`, mr-boxington `1.28.0` vs
`v1.29.1`, jactionlint `2.3.0` vs `v2.4.0`, `jdx/mise-action` `v5.2.4`
vs `v5.2.5`), while the inventory `upstream-freshness` rows pass as
`held`. Earlier gates from the original capture: `cargo fmt --all --
--check` PASS; focused nextest for mise/contract/actionlint (14),
orchestrator + workflow-renderer (336), and cli + freshness (68) PASS.
Full workspace nextest/clippy were not run locally; CI covers them.
