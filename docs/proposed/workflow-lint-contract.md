# Workflow lint contract

Velnor lints every GitHub Actions file it generates or allows with
[jactionlint](https://github.com/jdx/jactionlint), its only workflow linter
([hard decision 10](../jactionlint-decision.md)).
This page is the specification; the owning code is the workflow-lint adapter
`crates/velnor-actions-actionlint` (historical crate name), the orchestrator
(`workflow_jobs.rs`, `validate.rs`, `jactionlint_verify_step.rs`) and the
Mise catalog.

## Configuration

`.velnor/config.toml` has one knob:

```toml
[workflow.lint]
profile = "correctness"   # "correctness" (default) | "default" | "pedantic"
```

- `correctness` is actionlint's checks plus jactionlint's bug detectors. It
  is the default, so the `default` profile's security and policy rules and
  the `pedantic` profile's opinionated rules are opt-in.
- `strict` and `all` (jactionlint's config-only aliases of `pedantic`) and
  every other key are rejected. Rules, ignores, `paths`, baselines and
  online checks cannot be configured.

## Generated `.github/jactionlint.yaml`

Fixed keys, in this order: the Velnor marker, `profile`,
`generated-files: report`, sorted and deduplicated `config-variables`, and
`self-hosted-runner.labels` when there are labels to declare.

- `generated-files: report` is required. The Velnor marker matches
  jactionlint's generated-file heuristic, whose default `skip-policy` would
  silently turn the policy and style rules off on every file Velnor owns.
- `self-hosted-runner.labels` lists the configured Scale Set selector labels
  and any hosted label a generated job uses that the pinned jactionlint does
  not know. A GitHub-hosted label it knows (`ubuntu-26.04`, `macos-15`,
  `xcode-27`, ...) is never listed, because jactionlint would then treat jobs
  on GitHub's runners as self-hosted.
- The file has no `rules`, `ignores`, `paths`, `baseline`, `online` or
  `extends` key.
- A marked `.github/actionlint.yaml` left by an older generator is retired by
  the next generation; an unmarked one stays repository content.

## File set and flags

Every run checks the files `hk`'s `Builtins.jactionlint` checks:

```text
.github/workflows/*.y*ml
.github/actions/**/action.y*ml
.github/dependabot.y*ml
```

A bare repository run is never used: it would also lint composite actions
in golden trees such as `docs/proposed/opentofu-goldens/**`. Every run passes:

| Flag | Why |
|---|---|
| `--config-file .github/jactionlint.yaml` | One config for CI, release gates, staged validation and `hk` |
| `--strict-exit` | Any finding fails, whatever its level |
| `--no-online` | No network check, even if a pull request sets `online: true` in the config |
| `--shellcheck=shellcheck` | The pinned ShellCheck; a missing binary is exit 3, never a silent pass |
| `--pyflakes=` | No other `PATH` binary runs |
| `--no-color` | Plain logs |

CI and the release gates add `--format github` (workflow-command
annotations, no extra token scope; SARIF upload would need
`security-events: write` and is not used). Staged validation adds
`--format oneline`. `JACTIONLINT_NO_HINTS` is never set: it would hide the
"shellcheck was not found" note.

## Where it runs

| Place | What it lints |
|---|---|
| `jactionlint` job in `ci.yml` (always on, needed by `Required`) | The committed file set, listed by one credential-scrubbed `bash -c` that fails closed (exit 3) when it is empty |
| Release eligibility gates (`product-release.yml`, `product-release-generator.yml`) | The same script and argv as the CI job |
| Staged validation before `generate` writes | Every staged workflow and composite action |
| Candidate qualification (Linux, macOS, macOS Intel) | Through the staged validation of the qualified `generate` |

There is no separate zizmor job: jactionlint carries its audits, and honours
the `# zizmor: ignore[<audit>]` annotations Velnor emits
([coverage evidence](../jactionlint-decision.md#zizmor-is-retired)).

## Supply chain

- The Mise catalog pins jactionlint (`jactionlint@<exact>`, backend
  `github:jdx/jactionlint`, which checks the release-API digest and the
  GitHub artifact attestation on every install).
- `catalog_jactionlint_digests.rs` records, per Mise platform (`linux-x64`,
  `linux-arm64`, `macos-arm64`, `macos-x64`), the archive and the extracted
  executable SHA-256.
- A `Verify jactionlint` step follows every install. It resolves the
  executable with `mise which --tool jactionlint@<v> jactionlint` and checks
  it with `sha256sum -c`; a catalogued version without a digest for the
  runner platform fails. A project-declared version (hard decision 4) without
  a catalogued digest prints a notice and keeps Mise's checks. The lock audit
  treats a verified, catalogued install as covered without a `mise.lock` row.
- Provenance is checked when the version is bumped, never at generate time:
  `cosign verify-blob-attestation` on the release `packslip.sigstore.json`
  and `gh attestation verify --signer-workflow
  jdx/jactionlint/.github/workflows/release.yaml` must pass for every table
  platform; the evidence is recorded on `JACTIONLINT_VERSION`
  ([version policy](version-policy.md)).

## Local runs

The consumer's `mise.toml` declares `jactionlint` (and `shellcheck`), so CI
reads that version through hard decision 4 and `hk` runs the same binary
locally. The `hk` step is check-only (`Builtins.jactionlint` also ships `fix`
and `check_diff`, which would write `.github/`) and passes the same flags:

```pkl
["jactionlint"] = (Builtins.jactionlint) {
  fix = null
  check_diff = null
  check = "jactionlint --format gcc --strict-exit --no-online --shellcheck=shellcheck --pyflakes= {{files}}"
}
```
