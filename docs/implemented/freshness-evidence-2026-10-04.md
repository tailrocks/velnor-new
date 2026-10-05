# Freshness evidence snapshot — 2026-10-04

This record documents the evidence refresh in `.velnor/freshness-inventory.json`. It preserves existing qualified pins and records upstream moves as explicit temporary holds; it does not approve a version or source change.

## Tool and action sources

The bounded read-only `bash scripts/check-freshness.sh --check-upstream` probe completed at `2026-10-04T11:28:54Z` using Python 3.14.7 (`tomllib` is required by the script). Its per-row source URLs are recorded in the inventory. The probe found no lookup failures. It returned nonzero for six newer upstream releases whose current pins remain held; that result is expected and is not represented as a passing latest-version check.

| Subject | Current pin | Latest stable observed | Disposition |
| --- | --- | --- | --- |
| `mise` | `2026.9.18` | `v2026.10.1` | Existing hold `#6` |
| `rust` | `1.98.1` | `1.99.0` | Existing hold `#6` |
| `mr-boxington` | `1.21.1` | `v1.22.0`, commit [`10474d43`](https://github.com/jdx/mr-boxington/commit/10474d43342ad65df3b02323dd8092d18ab38101) | Hold `#29` pending cache lifecycle, disk, and input qualification |
| `gh` | `2.102.0` | `v2.102.0` | Current |
| `actionlint` | `1.7.12` | `v1.7.12` | Current |
| `shellcheck` | `0.11.0` | `v0.11.0` | Current |
| `zizmor` | `1.30.1` | `v1.30.1` | Current |
| `nextest` | `0.9.146` | `0.9.146` | Current |
| `opentofu` | `1.13.1` | `v1.13.1` | Current |
| `release-plz` | `0.3.169` | `0.3.169` | Current |
| `jdx/mise-action` | `v5.0.0` | `v5.1.0` | Existing hold `#6` |
| `actions/checkout` | `v7.0.1` | `v7.0.1` | Current |
| `actions/download-artifact` | `v8.0.1` | `v8.0.1` | Current |
| `actions/upload-artifact` | `v7.0.1` | `v7.0.1` | Current |
| `actions/cache/restore`, `actions/cache/save` | `v6.1.0` | `v6.1.0` | Current |
| `jdx/mr-boxington-action` | `v1.6.0` at `1687e54` | `v1.7.1`, tag resolves to [`d0825fba`](https://github.com/jdx/mr-boxington-action/commit/d0825fbaf3cc36ca2609aa38e71046265a1f1e37) | Hold `#29`; the integration candidate is recorded below |
| `asamarts/alint` | `v0.16.1` | `v0.17.0` | Existing hold `#6` |
| `Swatinem/rust-cache` | `v2.9.2` | `v2.9.2` | Current |

The `mr-boxington` `v1.22.0` tag resolved to commit `10474d43342ad65df3b02323dd8092d18ab38101` when checked at `2026-10-04T11:30:21Z`. The released action tag `v1.7.1` resolved to commit `d0825fbaf3cc36ca2609aa38e71046265a1f1e37` at the same time. [Velnor PR #29](https://github.com/tailrocks/velnor-new/pull/29) carries a separate integration candidate, `commit-ec3ebbf` at `ec3ebbfbc1fdaffa59d476e87e4f386fdc60d533`, from [upstream action PR #62](https://github.com/jdx/mr-boxington-action/pull/62). That candidate is not the published `v1.7.1` tag SHA; PR #29 marks the candidate for replacement with a released SHA before merge. The two 14-day holds granted `2026-10-04` retain the existing `1.21.1` and `v1.6.0` pins while PR #29 completes current-source cache lifecycle, disk, and input qualification and resolves the immutable action source.

The four existing `#6` holds remain in force. The refreshed inventory now records the observed latest values, including `mise v2026.10.1`, without converting those pins to current or extending their existing expiries.

## Hosted runner evidence

GitHub's [hosted-runner reference](https://docs.github.com/en/actions/reference/runners/github-hosted-runners) was rechecked on `2026-10-04`; its public and private Linux x64 tables still list `ubuntu-26.04`, `ubuntu-24.04`, and `ubuntu-22.04`. No supported label or default changed.

The successful Plan job in [CI run 37198168960](https://github.com/tailrocks/velnor-new/actions/runs/37198168960/job/111424357156) started at `2026-10-04T11:17:14Z`. Its log records Ubuntu `26.04.1`, runner image `ubuntu-26.04`, image version `20260927.149.1`, and [image release `ubuntu26/20260927.149`](https://github.com/actions/runner-images/releases/tag/ubuntu26%2F20260927.149). This is a fresh runtime image observation for the inventory's unchanged default label; the Plan job is not a formal Qualification run.

For comparison, the earlier successful [Qualification run 37109854949](https://github.com/tailrocks/velnor-new/actions/runs/37109854949/job/111165489915) ran its hosted Composite qualification on the same Ubuntu version and runner image at `2026-10-03T08:29:17Z`. That prior Qualification result remains the recorded behavior qualification; the newer Plan observation refreshes image identity only and does not claim the Qualification matrix ran again.
