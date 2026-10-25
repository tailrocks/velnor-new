# jactionlint in velnor-new

This records how velnor-new lints its own GitHub Actions files with
jactionlint. The rule is [hard decision 10](../jactionlint-decision.md)
and the specification is the [workflow lint contract](../proposed/workflow-lint-contract.md).

## Configuration

- `.velnor/config.toml` sets `[workflow.lint] profile = "default"`, which
  turns on the security and policy rules. velnor-new is the first repository
  on that profile.
- `mise.toml` declares `jactionlint = "2.3.0"` and `shellcheck = "0.11.0"`.
  Generated CI reads both at run time (hard decision 4), so CI, the release
  gates and a local `hk` run use one binary. `mise.lock` carries their
  checksums for `linux-x64`, `macos-arm64` and `macos-x64` (from
  `mise lock jactionlint shellcheck --platform linux-x64,macos-arm64,macos-x64`).
- `.github/jactionlint.yaml` is generated: `profile: default`,
  `generated-files: report`, `config-variables: []` and the scale-set labels
  `velnor` and `ubuntu-26.04-scale-set`.

## Pinned release provenance

The pinned release is `jdx/jactionlint` tag `v2.3.0`, published as a stable
release on 2026-10-21. The latest-release API was checked on 2026-10-22. At
this bump, both provenance checks below passed on 2026-10-22 for the four
archives covered by the current
[`JACTIONLINT_DIGESTS` table](../../crates/velnor-actions-contract/src/config/jactionlint_verifier.rs#L18):

- `cosign verify-blob-attestation` on `packslip.sigstore.json`, with type
  `https://packslip.dev/release/v1`, signer identity
  `https://github.com/jdx/jactionlint/.github/workflows/release.yaml@refs/tags/v2.3.0`,
  and issuer `https://token.actions.githubusercontent.com`;
- `gh attestation verify -R jdx/jactionlint --signer-workflow
  jdx/jactionlint/.github/workflows/release.yaml`.

The checks covered the Linux amd64 and arm64 archives and the Darwin amd64 and
arm64 archives. This records the v2.3.0 bump only. For each future version
bump, repeat both checks for every table platform and record that version's
source tag, check date, and outcomes here; the current release's evidence does
not qualify a later version. The bump-time procedure is specified in the
[jactionlint subsection of the version policy](../proposed/version-policy.md#2-authoritative-version-inventory).

## Measurements

These were taken with jactionlint 2.3.0, ShellCheck 0.11.0 and the generated
flags (`--strict-exit --no-online --shellcheck=shellcheck --pyflakes=`) over
the hk file set: 10 workflows and 16 composite actions.

| Tree | correctness | default |
|---|---|---|
| `main` before the redesign (actionlint era), `generated-files: report` | exit 1, 10 findings | exit 1, 15 findings |
| After the generator fixes (#302) | exit 0 | exit 0 |
| This tree, at `default` with `--no-ignores` | | exit 0 |

The 15 findings were all fixed in generator source, not ignored: see #302.

- 3 × `local-action-checkout`
- 2 × SC2218, plus SC2329
- SC2221/SC2222
- 2 × SC2015
- 4 × `unused-job-output`
- 1 × `concurrency-limits`

## Local runs

`hk` is pinned in `mise.toml`. The check-only step from the
[contract](../proposed/workflow-lint-contract.md#local-runs) goes into
`hk.pkl` with the hk rollout. It was checked with hk 2.8.0: it lints the
same files as CI, reports a template injection in a composite action, and
skips a golden composite action under `docs/`.
