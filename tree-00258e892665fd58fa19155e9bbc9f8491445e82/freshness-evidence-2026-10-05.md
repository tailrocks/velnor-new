# Freshness evidence snapshot — 2026-10-05

This record documents the evidence refresh in `.velnor/freshness-inventory.json`. It preserves existing qualified pins and records upstream moves as explicit temporary holds; it does not approve a version or source change.

## Tool and action sources

The bounded read-only `bash scripts/check-freshness.sh --check-upstream` probe completed at `2026-10-05T11:47:13Z`. It wrote nothing by itself. The per-row `checked_at` values below are that probe timestamp. The probe found no lookup failures. It returned nonzero for six newer upstream releases whose current pins remain held; that result is expected and is not represented as a passing latest-version check.

Rows whose pin still matched latest, and whose `checked_at` is now `2026-10-05T11:47:13Z`: `gh` `v2.102.0`, `actionlint` `v1.7.12`, `shellcheck` `v0.11.0`, `zizmor` `v1.30.1`, `nextest` `0.9.146`, `opentofu` `v1.13.1`, `release-plz` `0.3.169`, `Swatinem/rust-cache` `v2.9.2`, `actions/cache/restore` `v6.1.0`, `actions/cache/save` `v6.1.0`, `actions/checkout` `v7.0.1`, `actions/download-artifact` `v8.0.1`, `actions/upload-artifact` `v7.0.1`.

Held pins were not converted to current and their expiries were not extended.

| Subject | Current pin | Latest stable observed | Disposition |
| --- | --- | --- | --- |
| `mise` | `2026.9.18` | `v2026.10.3` | Existing hold `#6`; previous observation was `v2026.10.1` |
| `rust` | `1.98.1` | `1.99.0` | Existing hold `#6` |
| `mr-boxington` | `1.21.1` | `v1.22.0` | Hold `#29` |
| `jdx/mise-action` | `v5.0.0` | `v5.1.1` | Existing hold `#6`; previous observation was `v5.1.0` |
| `asamarts/alint` | `v0.16.1` | `v0.17.0` | Existing hold `#6` |
| `jdx/mr-boxington-action` | `v1.6.0` | `v1.7.1` | Hold `#29`; recorded `latest_sha` unchanged |

## Hosted runner evidence

GitHub's hosted-runner reference was rechecked on 2026-10-05. Its public and private Linux x64 tables still list `ubuntu-26.04`, `ubuntu-24.04`, and `ubuntu-22.04`. No supported label or default changed.

The successful `Cache / GitHub hosted` job [111673185494](https://github.com/tailrocks/velnor-new/actions/runs/37282302785/job/111673185494) in [Qualification run 37282302785](https://github.com/tailrocks/velnor-new/actions/runs/37282302785) started at `2026-10-05T08:14:18Z` and concluded `success` on label `ubuntu-26.04`. Its captured log records Ubuntu `26.04.1`, runner image `ubuntu-26.04`, image version `20260927.149.1`, runner version `2.337.0`, and [image release `ubuntu26/20260927.149`](https://github.com/actions/runner-images/releases/tag/ubuntu26%2F20260927.149). This refreshes image identity only. It does not claim the Qualification matrix ran again.

A later successful Plan job [111730803454](https://github.com/tailrocks/velnor-new/actions/runs/37300188152/job/111730803454) in CI run `37300188152` started at `2026-10-05T11:00:45Z` on label `ubuntu-26.04`. Downloading that job log returned HTTP 403, so its image version string is not the source of this stamp.
