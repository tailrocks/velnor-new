# Freshness evidence — 2026-11-02

This set refreshes all `checked_at` values in `.velnor/freshness-inventory.json` for the SDK 0.1.72 preparation PR. No pin changes.

## Capture time

- T0 is `2026-11-02T11:27:43Z`. This is the start of `capture-1`.
- `capture-1` ran from `11:27:43Z` to `11:28:31Z`. `capture-2` ran from `11:28:48Z` to `11:29:37Z`.
- The runner documentation was fetched at `11:29:54Z`.
- The oldest `checked_at` in the inventory is `2026-11-02T11:28:49.589Z`. The 24 h freshness window ends at `2026-11-03T11:28:49Z`.
- Release dispatch must start before the window ends. If it cannot, capture again in a small PR that merges before dispatch.

## Captures

Two independent authenticated captures are in [the evidence directory](evidence/freshness-2026-11-02/): `capture-1/` and `capture-2/`.

- Each capture queries all 25 tool and action subjects (12 tools, 13 actions).
- GitHub requests use `gh api` with the session credentials. The crates.io requests use a plain `curl` with a user agent. No token is stored in any file.
- Each capture holds 72 receipts, `comparison.json`, `observed-at.json` and `receipts.json`. `receipts.json` binds every response body by SHA-256.
- Each capture reads the Rust manifest with the byte range `0-131071`. The response is `206` with `bytes 0-131071/943486`.
- Each capture records, for every action, one release receipt and two tag receipts (`pinned-*` and `latest-*`). Every pinned action tag resolves to the SHA in the inventory.
- Annotated tags are resolved to the commit. The raw ref and tag-object responses are kept as `*-ref-raw.body` and `*-tagobj-raw.body`.
- `capture-agreement.json` binds both captures. The `comparison.json` hash is `863f2c34…` in both. The `observed` fields of all 72 receipts are identical. The result is `agree: true`.

## Rows that differ from the pin

Six rows have `match: false` in both captures. All six are held.

| Subject | Pinned | Upstream latest |
|---|---|---|
| `mise` | `2026.10.21` | `v2026.10.25` |
| `mr-boxington` | `1.28.0` | `v1.29.1` |
| `jactionlint` | `2.3.0` | `v2.4.0` |
| `jdx/mise-action` | `v5.2.4` | `v5.2.5` |
| `release-plz` | `0.4.2` | `0.4.3` |
| `asamarts/alint` | `v0.20.1` | `v0.21.0` |

The first four holds are unchanged from 2026-11-01. The pinned tags of `mise`, `mr-boxington`, `jactionlint` and `jdx/mise-action` still resolve to the same SHAs as in `freshness-evidence-2026-11-01.md`.

`release-plz 0.4.3` and `asamarts/alint v0.21.0` are new. The `alint` tag `v0.21.0` resolves to `d61e4c802b02ac811224b35c59a62cfe5176f4d0`. Upstream published it at `2026-11-01T18:15:45Z`, after the last evidence set. Both captures see both releases.

Decision X32 holds these two pins. Bump PR #464 failed in the Plan job with `lock_mismatch:tool:release-plz:0.4.3:0.4.2`. The published controller 0.1.66 compares the policy with its compiled catalog and rejects a policy pin change. The bumps land in P1.5, after the 0.1.72 adoption (D19/DP0).

Two temporary holds cover them. Both are granted `2026-11-02` and expire `2026-11-15`. The `issue` field of both holds cites PR #464, the bump PR that records the failure. The four older holds cite PR #451. The inventory rows for both subjects have `status: held`. Their `latest` values match the captures.

Pins, `qualified` values, the repository Mise mirrors, the compiled catalogs and the generated workflows do not change.

## Runner

The official hosted-runner reference still lists `ubuntu-latest`, `ubuntu-24.04`, `ubuntu-22.04` and `ubuntu-26.04` for the standard Linux runner. The inventory keeps `ubuntu-26.04` as default.

Successful main CI run `41431792661` (job `125231898752` `Plan`, source `de71f1559260e35c4cfe1812daafc9ebce8b33aa`) records:

- image `ubuntu-26.04`, version `20261025.179.1`
- runner `2.338.2`

This is a normal CI runtime observation, not formal qualification. The files are in `evidence/freshness-2026-11-02/runner/`: `runner-doc.html`, `ci-job.log` (the `Set up job` step only) and `runner-observation.json`.

## Gates

- `scripts/check-freshness.sh` exits 0: `check-freshness: PASS`, 319 pass rows, 0 fail rows. Six `upstream-freshness` rows pass as `held`.
- `scripts/check-freshness.sh --check-upstream` exits 1 by design. Its live probe reports exactly the six held rows as stale pins, with the values in the table above. All other rows pass (338 pass rows, 6 fail rows).
- Both runs used `MISE_LOCKED=1`, `MISE_ENV=` and `VELNOR_ACTIONS_BIN` unset.
