# Freshness evidence — 2026-11-03

This set refreshes all `checked_at` values in `.velnor/freshness-inventory.json`. The 2026-11-02 evidence aged past the 24 h interval at `2026-11-03T11:28:49Z`. No pin changes.

## Capture time

- `capture-1` ran from `11:37:13Z` to `11:37:59Z`. `capture-2` ran from `11:37:59Z` to `11:38:44Z`.
- The runner documentation was fetched at `11:38:54Z`.
- The oldest `checked_at` in the inventory is `2026-11-03T11:38:00.345Z`. The 24 h freshness window ends at `2026-11-04T11:38:00Z`.
- Release dispatch must start before the window ends. If it cannot, capture again in a small PR that merges before dispatch.

## Captures

Two independent authenticated captures are in [the evidence directory](evidence/freshness-2026-11-03/): `capture-1/` and `capture-2/`.

- Each capture queries all 25 tool and action subjects (12 tools, 13 actions).
- GitHub requests use `gh api` with the session credentials. The crates.io requests use a plain `curl` with a user agent. No token is stored in any file.
- Each capture holds 61 receipts, `comparison.json`, `observed-at.json` and `receipts.json`. `receipts.json` binds every response body by SHA-256.
- Each capture reads the Rust manifest with the byte range `0-131071`. The response is `206` with `bytes 0-131071/943486`.
- Each capture records, for every action, one release receipt and two tag receipts (`pinned-*` and `latest-*`). Every pinned action tag resolves to the SHA in the inventory. `mr-boxington` also has both tag receipts, and the commit `b0b67184…` equals its `latest_sha`.
- Annotated tags are resolved to the commit. The raw ref and tag-object responses are kept as `*-ref-raw.body` and `*-tagobj-raw.body`.
- `capture-agreement.json` binds both captures. The `comparison.json` hash is `3f3432e0…` in both. The `observed` fields of all 61 receipts are identical. The result is `agree: true`.

## Rows that differ from the pin

Two rows have `match: false` in both captures. Both are held.

| Subject | Pinned | Upstream latest | Upstream published |
|---|---|---|---|
| `nextest` | `0.9.150` | `0.9.151` | `2026-11-03T00:02:40Z` |
| `jdx/mise-action` | `v5.2.5` | `v5.2.6` | `2026-11-02T19:54:50Z` |

`jdx/mise-action` tag `v5.2.5` still resolves to `6612f0ef07ed2a736cd1e00a94a065522dd41195`, the inventory SHA. Tag `v5.2.6` resolves to `f5be966e4a917d54244728e6a5fdcd1d93a35e6c`.

Both pins are held because this refresh must not change pins. Two temporary holds cover them. Both are granted `2026-11-03` and expire `2026-11-17`. The `issue` field of both holds cites PR #467, the update set that set the current pins. The inventory rows for both subjects have `status: held`. Their `latest` values match the captures. Bumps for `nextest 0.9.151` and `jdx/mise-action v5.2.6` land in a separate update set that removes both holds.

The other 23 rows match their pins. This includes the six pins bumped by #467 (`mise`, `mr-boxington`, `jactionlint`, `release-plz`, `asamarts/alint` and `jdx/mise-action` at `v5.2.5`).

Pins, `qualified` values, the repository Mise mirrors, the compiled catalogs and the generated workflows do not change.

## Runner

The official hosted-runner reference still lists `ubuntu-latest`, `ubuntu-24.04`, `ubuntu-22.04` and `ubuntu-26.04` for the standard Linux runner. The inventory keeps `ubuntu-26.04` as default.

Successful main CI run `41626261741` (job `125884265888` `Plan`, source `2a53f0d28311105b5322924eeb10fe7121da4149`) records:

- image `ubuntu-26.04`, version `20261025.179.1`
- runner `2.338.2`

This is a normal CI runtime observation, not formal qualification. The files are in `evidence/freshness-2026-11-03/runner/`: `runner-doc.html`, `ci-job.log` (the `Set up job` step only) and `runner-observation.json`.

## Gates

- `scripts/check-freshness.sh` exits 0: `check-freshness: PASS`, 315 pass rows, 0 fail rows. Two `upstream-freshness` rows pass as `held`.
- `scripts/check-freshness.sh --check-upstream` exits 1 by design. Its live probe, run at `11:39:17Z`, reports exactly the two held rows as stale pins, with the values in the table above. All other rows pass (338 pass rows, 2 fail rows).
- Both runs used `MISE_LOCKED=1`, `MISE_ENV=` and `VELNOR_ACTIONS_BIN` unset.
