# Freshness evidence — 2026-10-24

This record supports the reviewed default-pin refresh. It preserves the
upstream observations and does not claim that the updated pins have completed
the repository qualification gates.

## Upstream probes

The retained invocation was `bash scripts/check-freshness.sh --check-upstream`
with `VELNOR_ACTIONS_BIN` bound to this checkout's candidate binary. It ran
from the repository root on 2026-10-24 and exited 1 because three existing
pins were behind the observed latest releases. The run made 25 tool/action
lookups, with 22 matching the previously recorded pin and no lookup failures.
The local stdout capture (74,243 bytes; SHA-256
`6b9516b5c434c3758ba69955540a4a74f73e8495d4df14ae74ae5fc747060f22`) and
receipt (exit 1, 3,424-byte stderr; SHA-256
`b8ebb53ba0f8b0e824ba0cff96210b8970a5ca21dbafef01100a6d2f8015dc27`) are
retained for this review. Their `/tmp` paths are session-local capture context,
not sources available to repository readers. All probe rows carry their own
observed timestamp below. The pre-update full freshness gate also exited 1
because the 26 inventory evidence rows were older than the 24-hour interval.
Its local receipt/output capture has stdout SHA-256
`2ed1276156ac9f398a622bcbe711c69c80a3fdbbffec9517d17fc963a37023db` and
stderr SHA-256
`1bad2d75fbe85359df900251a64f4cc293d83513f2cb3ec5abbe4e9d37edbb7d`; the
capture directory is session-local context, not a reader-accessible source.

| Type | Subject | Latest observed | Probe outcome | Source | Checked at |
|---|---|---|---|---|---|
| tool | `bun` | `bun-v1.4.4` | matched recorded pin | <https://api.github.com/repos/oven-sh/bun/releases/latest> | `2026-10-24T14:59:01Z` |
| tool | `cosign` | `v3.1.4` | matched recorded pin | <https://api.github.com/repos/sigstore/cosign/releases/latest> | `2026-10-24T14:59:01Z` |
| tool | `crane` | `v0.22.1` | matched recorded pin | <https://api.github.com/repos/google/go-containerregistry/releases/latest> | `2026-10-24T14:59:01Z` |
| tool | `gh` | `v2.103.2` | new release found; pin refreshed | <https://api.github.com/repos/cli/cli/releases/latest> | `2026-10-24T14:59:01Z` |
| tool | `jactionlint` | `v2.3.0` | matched recorded pin | <https://api.github.com/repos/jdx/jactionlint/releases/latest> | `2026-10-24T14:59:01Z` |
| tool | `mise` | `v2026.10.18` | matched recorded pin | <https://api.github.com/repos/jdx/mise/releases/latest> | `2026-10-24T14:59:01Z` |
| tool | `mr-boxington` | `v1.27.0` | matched recorded pin | <https://api.github.com/repos/jdx/mr-boxington/releases/latest> | `2026-10-24T14:59:01Z` |
| tool | `nextest` | `0.9.149` | matched recorded pin | <https://crates.io/api/v1/crates/cargo-nextest> | `2026-10-24T14:59:01Z` |
| tool | `opentofu` | `v1.13.2` | matched recorded pin | <https://api.github.com/repos/opentofu/opentofu/releases/latest> | `2026-10-24T14:59:01Z` |
| tool | `release-plz` | `0.4.0` | matched recorded pin | <https://crates.io/api/v1/crates/release-plz> | `2026-10-24T14:59:01Z` |
| tool | `rust` | `1.99.1` | matched recorded pin | <https://static.rust-lang.org/dist/channel-rust-stable.toml> | `2026-10-24T14:59:01Z` |
| tool | `shellcheck` | `v0.11.0` | matched recorded pin | <https://api.github.com/repos/koalaman/shellcheck/releases/latest> | `2026-10-24T14:59:01Z` |
| action | `actions/attest` | `v4.2.2` | matched recorded pin | <https://api.github.com/repos/actions/attest/releases/latest> | `2026-10-24T14:59:01Z` |
| action | `actions/attest-build-provenance` | `v4.2.2` | matched recorded pin | <https://api.github.com/repos/actions/attest-build-provenance/releases/latest> | `2026-10-24T14:59:01Z` |
| action | `actions/cache/restore` | `v6.1.0` | matched recorded pin | <https://api.github.com/repos/actions/cache/releases/latest> | `2026-10-24T14:59:01Z` |
| action | `actions/cache/save` | `v6.1.0` | matched recorded pin | <https://api.github.com/repos/actions/cache/releases/latest> | `2026-10-24T14:59:01Z` |
| action | `actions/checkout` | `v7.0.1` | matched recorded pin | <https://api.github.com/repos/actions/checkout/releases/latest> | `2026-10-24T14:59:01Z` |
| action | `actions/create-github-app-token` | `v3.3.0` | matched recorded pin | <https://api.github.com/repos/actions/create-github-app-token/releases/latest> | `2026-10-24T14:59:01Z` |
| action | `actions/download-artifact` | `v8.0.2` | matched recorded pin | <https://api.github.com/repos/actions/download-artifact/releases/latest> | `2026-10-24T14:59:01Z` |
| action | `actions/github-script` | `v9.0.0` | matched recorded pin | <https://api.github.com/repos/actions/github-script/releases/latest> | `2026-10-24T14:59:01Z` |
| action | `actions/upload-artifact` | `v7.0.2` | matched recorded pin | <https://api.github.com/repos/actions/upload-artifact/releases/latest> | `2026-10-24T14:59:01Z` |
| action | `asamarts/alint` | `v0.20.1` | new release found; pin refreshed | <https://api.github.com/repos/asamarts/alint/releases/latest> | `2026-10-24T14:59:01Z` |
| action | `aws-actions/configure-aws-credentials` | `v6.3.0` | matched recorded pin | <https://api.github.com/repos/aws-actions/configure-aws-credentials/releases/latest> | `2026-10-24T14:59:01Z` |
| action | `jdx/mise-action` | `v5.2.3` | new release found; pin refreshed | <https://api.github.com/repos/jdx/mise-action/releases/latest> | `2026-10-24T14:59:01Z` |
| action | `jdx/mr-boxington-action` | `v1.7.2` | matched recorded pin | <https://api.github.com/repos/jdx/mr-boxington-action/releases> | `2026-10-24T14:59:01Z` |

The resulting inventory refresh records the three newly observed pins and
sets all 25 row timestamps from the probe output. It does not update unrelated
project toolchain or lock files.

## Immutable release and compatibility evidence

- GitHub CLI `v2.103.2` resolves to verified commit
  `4f2cfe92e3499f8581bf39db77e0a1431fa7b55c`. The immutable-source comparison
  for the Velnor API and release-asset paths is documented in
  [`immutable-preview-api-qualification.md`](immutable-preview-api-qualification.md).
  The local comparison packet (SHA-256
  `d965788f535ebd7f7beba43e31ecbdc0f80536652d7788de0229f8b332068386`) and
  independent readback (SHA-256
  `cf100de6c4a0998f34f0ad684088a204f776937fbff08ccef3d82697906a2b9a`) are
  session-local receipts, not reader-accessible sources. The original printed
  v2.103.2 upload-source digest had 61 hex characters (three short). The
  readback fetched the immutable `upload.go` path at both CLI commits and
  verified byte-identical 4,100-byte files, SHA-256
  `8a276af9cba13142c6481b68f82f245b489137fe11a2281108d0849b0d41e15e`.
- Alint `v0.20.1` resolves to tag target
  `265d9fdc720bb8f59d00b87e33e4149869f77327`. Its action manifest retains
  Velnor's `version`, `path`, `config`, `format`, and `fail-on-warning` inputs.
  See the [immutable v0.20.1 action manifest](https://raw.githubusercontent.com/asamarts/alint/v0.20.1/action.yml).
  The release page does not establish a signed tag; no signature claim is made.
- `jdx/mise-action` `v5.2.3` resolves to verified commit
  `82a612c5ab8bcda7cc5db6b7e81a8e7584c3dc6e`. Its manifest retains the
  explicit version and SHA inputs Velnor uses. Velnor disables action caching,
  so the release's cache-key search change does not affect this use.
  See the [immutable v5.2.3 action manifest](https://raw.githubusercontent.com/jdx/mise-action/v5.2.3/action.yml).
- These immutable release and manifest sources are independent of the
  25-row latest-release probe. They establish the pin identities and inspected
  input compatibility, not official artifact installation or hosted action
  qualification.

## Hosted runner observation

The runner row is not a release API probe. Supported labels were checked at
[GitHub's hosted-runner reference](https://docs.github.com/en/actions/reference/runners/github-hosted-runners):
`ubuntu-26.04`, `ubuntu-24.04`, and `ubuntu-22.04`. A successful
[Jackin main CI run](https://github.com/jackin-project/jackin/actions/runs/40105861104),
attempt 1, Plan job `120907292037`, observed `Ubuntu 26.04.1
LTS`, image label `ubuntu-26.04`, image version `20261012.163.1`, and runner
`2.338.0` at `2026-10-24T15:11:52Z`. The source-bound raw log was retained
locally (267,730 bytes; SHA-256
`59a40a81e32b12259b592fd3341e6bee44e4ccc570507d02b3acdcb8ef86b46a`); the
parsed source-bound evidence record was also retained locally (SHA-256
`7b1b915da2fdb7c495689704ed4440203f16500c554b6a6d516e0c70d429f077`). These
local capture paths are session context; the linked run and official runner
references above are the portable sources.
This is a CI runtime observation, not a new formal platform qualification.

## Qualification status

The pin and inventory source refresh is recorded above. Qualification of this
updated source set is pending. This document will be extended only with results
that are actually observed after the source readback and bounded test/build
runs; no pass is inferred from the baseline probe or runner observation.
