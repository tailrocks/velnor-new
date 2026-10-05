# Freshness evidence snapshot — 2026-10-05

This update records a bounded upstream probe and one GitHub-hosted runner
image observation in `.velnor/freshness-inventory.json`. It does not change
pins, holds, runner labels, or platform qualification.

## Tool and action sources

`bash scripts/check-freshness.sh --check-upstream` completed its read-only
probe at `2026-10-05T11:40:12Z`. All 13 inventory rows with `status: current`
matched their pinned and qualified versions. Their evidence timestamps now
record that probe time. The script writes no inventory changes; this reviewed
update records only those successful results.

| Subject | Pinned and observed latest |
| --- | --- |
| `gh` | `2.102.0` |
| `actionlint` | `1.7.12` |
| `shellcheck` | `0.11.0` |
| `zizmor` | `1.30.1` |
| `nextest` | `0.9.146` |
| `opentofu` | `1.13.1` |
| `release-plz` | `0.3.169` |
| `Swatinem/rust-cache` | `v2.9.2` |
| `actions/checkout` | `v7.0.1` |
| `actions/download-artifact` | `v8.0.1` |
| `actions/upload-artifact` | `v7.0.1` |
| `actions/cache/restore`, `actions/cache/save` | `v6.1.0` |

The probe returned nonzero because the previous timestamps for the 13 current
rows were stale and six held pins differ from the latest upstream values. It
observed `mise` `v2026.10.3`, Rust `1.99.0`, `mr-boxington` `v1.22.0`,
`asamarts/alint` `v0.17.0`, `jdx/mise-action` `v5.1.1`, and
`jdx/mr-boxington-action` `v1.7.1`. Existing holds still cover these changes.
No held pin, expiry, or qualification was changed.

## Hosted runner evidence

GitHub's [hosted-runner reference](https://docs.github.com/en/actions/reference/runners/github-hosted-runners)
was checked on `2026-10-05`. The public and private Linux x64 tables list
`ubuntu-26.04`, `ubuntu-24.04`, and `ubuntu-22.04`, matching the inventory.

The successful Plan job in [PR 74 CI run 37302665873](https://github.com/tailrocks/velnor-new/actions/runs/37302665873/job/111738856781)
logged the runner image at `2026-10-05T11:23:58.752Z`: Ubuntu `26.04.1`,
image `ubuntu-26.04`, version `20260927.149.1`. The inventory records this
observed time. The corresponding
[runner-images release](https://github.com/actions/runner-images/releases/tag/ubuntu26%2F20260927.149)
identifies the image build as `ubuntu26/20260927.149`.

The `actions/runner-images` repository advanced from
`6d942e630479cd99a93dadfc766af11242bfa402` to
`381c440f34711afd641805c7ec44a1a2549d5f16` at `2026-10-05T09:26:22Z`.
That commit only renames a variable in its SBOM report workflow. The Ubuntu
26 release and README version remain `20260927.149` and `20260927.149.1`.
The separate runner lock still matches `actions/runner` `2.337.0` and digest
`sha256:70920811a4f8ad4328818682bca5c6469c1c942fab52448868071d0063816613`.

The overall PR run failed because its freshness check found the prior runner
observation older than 24 hours. The Plan job itself succeeded. This refresh
records a real runtime observation; it is not a Qualification result or a pass
for the failed PR run.
