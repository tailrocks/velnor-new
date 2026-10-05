# Freshness evidence snapshot — 2026-10-05

This evidence-only update refreshes 13 `status=current` upstream rows in `.velnor/freshness-inventory.json` from successful read-only lookups and refreshes the runner row from a successful hosted Plan job. Pins, qualified values, statuses, temporary holds, and hold expiries are unchanged. The previous main run [37303474086](https://github.com/tailrocks/velnor-new/actions/runs/37303474086), job 111741861131, failed its repository-policy test because those 13 rows and the runner row had crossed the 24-hour evidence interval.

## Tool and action observations

The bounded `bash scripts/check-freshness.sh --check-upstream` probe ran with Python 3.14.7 at `2026-10-05T11:54:17Z`. It returned 13 `pinned==latest` rows, six newer-version findings on already-held pins, no lookup failures, and exit 1. The nonzero result is expected: it does not approve or qualify the newer versions. Only `checked_at` on the 13 matching `status=current` rows was refreshed; pins and qualified values were preserved.

The complete retained probe log is `/tmp/velnor-freshness-refresh-d435-r1.log` (SHA-256 `b4579e0c2392b4592432cb4ce8dc2b23602b054b8fc0eeb23bb8489bf3e8d92e`). Its 13 passing upstream rows include cache restore and save separately. The six newer-version findings are held pins, not lookup failures.

| Subject | Pinned and latest observed |
| --- | --- |
| `gh` | `2.102.0` |
| `actionlint` | `1.7.12` |
| `shellcheck` | `0.11.0` |
| `zizmor` | `1.30.1` |
| `nextest` | `0.9.146` |
| `opentofu` | `1.13.1` |
| `release-plz` | `0.3.169` |
| `Swatinem/rust-cache` | `v2.9.2` |
| `actions/cache/restore` and `actions/cache/save` | `v6.1.0` |
| `actions/checkout` | `v7.0.1` |
| `actions/download-artifact` | `v8.0.1` |
| `actions/upload-artifact` | `v7.0.1` |

The same probe observed newer releases for existing held pins: `mise` `v2026.10.3`, Rust `1.99.0`, `mr-boxington` `v1.22.0`, `asamarts/alint` `v0.17.0`, `jdx/mise-action` `v5.1.1`, and `jdx/mr-boxington-action` `v1.7.1`. The `jdx/mise-action` observation is newer than the `v5.1.0` recorded in the 2026-10-04 snapshot. This update leaves all six held pins and hold records unchanged; the newer releases remain subject to their existing update and qualification decisions.

For `Swatinem/rust-cache`, the probe found `v2.9.2`. A separate HTTPS `git ls-remote` check at `2026-10-05T12:04:01Z` resolved `refs/tags/v2.9.2` to annotated tag object `63fed3e2fecf6f7b51dc6f043341b79ef82a9ae7` and peeled commit `6323deb102c322ba6fcbdcafc7e3dddab59af2b6`, matching the pinned and qualified SHA. The retained command output is `/tmp/velnor-swatinem-v2.9.2-tag-resolution-20261005.log` (SHA-256 `e5113a7637ec19bf7255f0301e297bf3c8ff4d0161fb1f01caddc19275ec69be`).

## Hosted runner observation

The successful [CI run 37301567773](https://github.com/tailrocks/velnor-new/actions/runs/37301567773), head `0bd6447a8e29d4722ce2ebb78bfcd90fa35f2b18`, ran the Plan job 111735628499 on the hosted `ubuntu-26.04` runner. Its job log records Ubuntu `26.04.1`, image version `20260927.149.1`, and release [`ubuntu26/20260927.149`](https://github.com/actions/runner-images/releases/tag/ubuntu26%2F20260927.149). The runner row timestamp is set to the job start, `2026-10-05T11:14:42Z`; the retained job log is `/tmp/velnor-runner-freshness-plan-37301567773.log` (SHA-256 `57cec84df187357446eedf09a148386d870a1ae8727c3fac2e7a02b33a040b2d`). The [GitHub-hosted runners reference](https://docs.github.com/en/actions/reference/runners/github-hosted-runners), checked on 2026-10-05, lists `ubuntu-26.04` for both public and private standard runners.

This Plan job refreshes the observed image identity and supported label only. It is not a new Qualification run and does not change the recorded platform qualification result.

## Validation

With the explicit Python 3.14.7 executable directory first in `PATH`, `bash scripts/check-freshness.sh` passed at `2026-10-05T12:07:05Z`; its retained log is `/tmp/velnor-freshness-evidence-20261005-offline-gate-r2.log` (SHA-256 `a9f6ad45460d5edd5f17505b2b51ac8cb9b30a3b9240c5c4d51454c06ffefa89`). The previously failing CLI policy regression `impl_cli_verify_local::verify_local_repo_policy_stage_executes` passed under locked Nextest with Rust 1.98.1 and that same Python path (run `a3e92c21-3ad1-437e-8bec-080d95f5055a`, 1 passed, 236 filtered); log `/tmp/velnor-freshness-evidence-20261005-cli-policy-nextest.log`, SHA-256 `bdbdd76c61f8025553436aaf5dae702f32b7e174f9d455d479e22801ef804f4a`.

An initial wrapper invocation (`mise exec python@3.14.7 -- bash scripts/check-freshness.sh`) failed because the child shell selected `/usr/bin/python3` 3.9.6, which lacks `tomllib`; this was an environment-invocation failure, not a gate result. The passing retry put the pinned Python 3.14.7 directory directly first in `PATH`. The first failure is retained at `/tmp/velnor-freshness-evidence-20261005-offline-gate.log` (SHA-256 `7388cffd1c41600cc08a5f040f44203005c50e495d3ff52985f087dc1390892a`).
