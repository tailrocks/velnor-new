# Freshness evidence — 2026-10-05

This page records two separate observations: the hosted-runner identity check
and the tool/action release-metadata check. The latter updates observation
metadata only; it does not qualify a new tool, action, or cache route.

## Hosted runner

The [GitHub-hosted runners reference](https://docs.github.com/en/actions/reference/runners/github-hosted-runners)
was rechecked on 2026-10-05. Its public and private standard-runner tables
still list `ubuntu-26.04`, `ubuntu-24.04`, and `ubuntu-22.04`, matching the
inventory's supported labels and unchanged `ubuntu-26.04` default.

Protected-main [CI run 37300188152](https://github.com/tailrocks/velnor-new/actions/runs/37300188152)
completed successfully at head `2d9bca8a37b0440e29a3520aa03e77752079f400`.
Its successful [Rust / velnor-actions-cli job](https://github.com/tailrocks/velnor-new/actions/runs/37300188152/job/111731481419)
started at `2026-10-05T11:02:45Z`. The runner log records Ubuntu `26.04.1`,
image `ubuntu-26.04`, image version `20260927.149.1`, runner version `2.337.0`,
and [image release `ubuntu26/20260927.149`](https://github.com/actions/runner-images/releases/tag/ubuntu26%2F20260927.149).
This is a runtime image observation only; it is not a formal Qualification run.

## Tool and action releases

Two bounded, read-only source captures completed on 2026-10-05. Capture A ran
from `11:38:00Z` through `11:38:10Z`; capture B ran from `11:40:22Z` through
`11:41:06Z`. Each captured all 18 distinct source URLs represented by the 19
tool/action inventory rows. GitHub release/tag metadata came from GitHub's
API, the Rust version from the stable-channel TOML, and Cargo tool versions
from crates.io. The raw response sizes and SHA-256 digests were verified
against both capture manifests. The parsed A and B latest-version tables are
identical (SHA-256 `f33dbac44fc52cf8693091a2986cc5b3c91f898ba30b506524e5750080aa3df8`);
the manifest SHA-256 values are `3676fbb2f984bcc07fc5389809df80bb3d4d64c9ea8645b7e3aad1a347a2bf00`
for A and `3ddb82ab9cf05596cf77c4d090e2612c1604df06263f18f6eef3f8e142deef29`
for B. `checked_at` records the end of capture B.

| Inventory row | Latest observed | Disposition |
| --- | --- | --- |
| tool `mise` | `v2026.10.3` | held at `2026.9.18` under #6 |
| tool `rust` | `1.99.0` | held at `1.98.1` under #6 |
| tool `mr-boxington` | `v1.22.0` | held at `1.21.1` under #29 |
| tool `gh` | `v2.102.0` | matches pin |
| tool `actionlint` | `v1.7.12` | matches pin |
| tool `shellcheck` | `v0.11.0` | matches pin |
| tool `zizmor` | `v1.30.1` | matches pin |
| tool `nextest` | `0.9.146` | matches pin |
| tool `opentofu` | `v1.13.1` | matches pin |
| tool `release-plz` | `0.3.169` | matches pin |
| action `jdx/mise-action` | `v5.1.1` | held at `v5.0.0` under #6 |
| action `actions/checkout` | `v7.0.1` | matches pin |
| action `actions/download-artifact` | `v8.0.1` | matches pin |
| action `actions/upload-artifact` | `v7.0.1` | matches pin |
| action `actions/cache/restore` | `v6.1.0` | matches pin |
| action `actions/cache/save` | `v6.1.0` | matches pin |
| action `jdx/mr-boxington-action` | `v1.7.1` | held at `v1.6.0` under #29 |
| action `asamarts/alint` | `v0.17.0` | held at `v0.16.1` under #6 |
| action `Swatinem/rust-cache` | `v2.9.2` | matches pin |

All 13 rows marked `current` still match their pins. The six existing held rows
remain held with their pin, qualified version/SHA, exception owner, issue, and
expiry unchanged. Only the latest-observed values for `mise` and
`jdx/mise-action` changed from the preceding inventory; no release was promoted.
The separately evidenced runner row is unchanged by this release-metadata check.
