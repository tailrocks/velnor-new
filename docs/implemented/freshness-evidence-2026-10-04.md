# Freshness evidence snapshot — 2026-10-04

This record summarizes the `origin/main` evidence refresh from commit `2e3fc0218d1c0de4f57cfcc3b7e8d93d3696c069`. Its probe completed at `2026-10-04T11:28:54Z`. The merged `.velnor/freshness-inventory.json` retains those newer upstream observations alongside the integration's selected pins and dated holds; the integration disposition below separates these from main's historical pins.

## Tool and action sources

The bounded read-only `bash scripts/check-freshness.sh --check-upstream` probe completed at `2026-10-04T11:28:54Z` using Python 3.14.7 (`tomllib` is required by the script). Its per-row source URLs are recorded in the origin/main inventory. The probe found no lookup failures. It returned nonzero for six newer upstream releases whose main-tree pins remain held; that result is expected and is not represented as a passing latest-version check. Python 3.14.7 is the probe runtime only; it is not a catalog tool.

| Subject | Origin/main pin (historical snapshot) | Latest stable observed | Disposition |
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

The `mr-boxington` `v1.22.0` tag resolved to commit `10474d43342ad65df3b02323dd8092d18ab38101` when checked at `2026-10-04T11:30:21Z`. The released action tag `v1.7.1` resolved to commit `d0825fbaf3cc36ca2609aa38e71046265a1f1e37` at the same time. [Velnor PR #29](https://github.com/tailrocks/velnor-new/pull/29) carries a separate integration candidate, `commit-ec3ebbf` at `ec3ebbfbc1fdaffa59d476e87e4f386fdc60d533`, from [upstream action PR #62](https://github.com/jdx/mr-boxington-action/pull/62). That candidate is not the published `v1.7.1` tag SHA; PR #29 marks the candidate for replacement with a released SHA before merge. In this origin/main snapshot, the two 14-day holds granted `2026-10-04` retain the selected `1.21.1` and `v1.6.0` pins while PR #29 completes current-source cache lifecycle, disk, and input qualification and resolves the immutable action source.

The four existing `#6` holds remain in force. The refreshed inventory now records the observed latest values, including `mise v2026.10.1`, without converting those pins to current or extending their existing expiries.

## Integration pin and qualification disposition

The merged catalog selects `mr-boxington` `1.22.0` in `.velnor/version-policy.toml` and `crates/velnor-actions-mise/src/catalog.rs`. The selected source is the exact [v1.22.0 release tag](https://api.github.com/repos/jdx/mr-boxington/releases/tags/v1.22.0), which resolves to `10474d43342ad65df3b02323dd8092d18ab38101`; the local stock identity is verified. The merged inventory keeps that selected version and the newer main observation (`v1.22.0`, SHA `10474d43342ad65df3b02323dd8092d18ab38101`, checked at `2026-10-04T11:28:54Z`). The main snapshot's `1.21.1` pin is historical evidence, not the current catalog selection. Hosted production qualification for the selected candidate remains `PARTIAL`: #29's current-source cache lifecycle, disk, and input proof is still open. Keep the #29 hold, and do not describe the hosted candidate as formally qualified.

The MBX action remains pinned to `v1.6.0` at `1687e54eb349cadf61fa38b5813a77875489e8e6`. The goal-specific #28 hold remains owned by `@donbeave` through `2026-10-11`, pending hosted proof of the selected local-backend explicit-bundle lifecycle, export-copy peak, and failed-import cold fallback. Main's generic #29 action hold through `2026-10-18` is additional; it does not replace the earlier goal checkpoint. The latest released `v1.7.1` action SHA remains `d0825fbaf3cc36ca2609aa38e71046265a1f1e37`; the separate PR #29 integration candidate is not that published tag.

The Mise pin remains `2026.9.18`; main's `2026-10-04T11:28:54Z` check still reports `v2026.10.1`. Official fix #13926 (`dfe74a90b41603625ee6aabecb42f14a1f5eb0f6`) remains unreleased: the accepted fix was eight commits ahead of the immutable release tag, and published Linux/macOS assets retained pre-fix digests. The goal-specific #6 hold remains owned by `@donbeave` through `2026-10-11`; resolve it only after a fixed stable release and supported-platform qualification are recorded. See the [PR #12 disposition](../reviews/pr-12-disposition.md) for the release and asset evidence.

## Hosted runner evidence

The official [hosted-runner reference](https://docs.github.com/en/actions/reference/runners/github-hosted-runners) was read with a one-page, read-only Firecrawl scrape (cache max age `0`). The request started at `2026-10-04T16:00:53Z` and completed at `2026-10-04T16:00:55Z`; this retrieval cutoff is `runner.checked_at` in the inventory. The page returned HTTP `200`. Its public and private Linux x64 tables list `ubuntu-26.04`, `ubuntu-24.04`, and `ubuntu-22.04`; the inventory default remains `ubuntu-26.04`. Receipt: Firecrawl scrape `01a107a5-941f-7129-afe5-e3aa8e782d21`; extracted Markdown SHA-256 `b6313bff55ebf73cf6c71b4009c19843c167115878217291e08e36f80727b98e`.

The successful Plan job in [CI run 37198168960](https://github.com/tailrocks/velnor-new/actions/runs/37198168960/job/111424357156) started at `2026-10-04T11:17:14Z`. Its log records Ubuntu `26.04.1`, runner image `ubuntu-26.04`, image version `20260927.149.1`, and [image release `ubuntu26/20260927.149`](https://github.com/actions/runner-images/releases/tag/ubuntu26%2F20260927.149). This is a runtime image observation, separate from the later hosted-runner documentation retrieval cutoff; the Plan job is not a formal Qualification run.

For comparison, the earlier successful [Qualification run 37109854949](https://github.com/tailrocks/velnor-new/actions/runs/37109854949/job/111165489915) ran its hosted Composite qualification on the same Ubuntu version and runner image at `2026-10-03T08:29:17Z`. That prior Qualification result remains the recorded behavior qualification; the newer Plan observation refreshes image identity only and does not claim the Qualification matrix ran again.
