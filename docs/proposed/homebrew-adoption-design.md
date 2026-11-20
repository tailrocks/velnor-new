# Homebrew adoption workflow: design proposal

This page proposes a `velnor-gen` feature that renders the `homebrew-adoption.yml` workflow. It is a design, not a contract. No code implements it yet, and the owner must approve the open choice below before code starts.

## Proposed shape

- A new opt-in file `.velnor/homebrew-adoption.toml`, the same pattern as `.velnor/release.toml`. Without the file the generator emits no new output, so the default output stays byte-identical.
- Keys: `repository` (the owner and name that publishes the SDK release), `signer_workflow`, `runner`, `timeout_minutes`, `matrix_max_parallel` and `artifact_retention_days`. The file holds no tap and no secret. The tap comes from the transition file that is passed in at run time.
- A new `velnor-render::homebrew_adoption` module renders a `workflow_call` workflow with four jobs: `plan`, `native-checks` (a matrix taken from the plan output), `required` and `required-gate`.
- The 40-line bootstrap `run:` step (download the manifest, check its sha256, `gh attestation verify`, check the jq assertions, download the asset and install it) breaks design rule 9. It becomes one helper command, for example `velnor-ci stage-sdk`, which reads `repository`, `signer-workflow`, the version, the source sha and the manifest sha from the environment. The helper lives in `ci-helper/` and needs a helper release before the generator can pin it.
- `Setup Mise` and `Install pinned GitHub CLI` need a pin source for `gh`. `mise.toml` has no `gh` entry; the pin lives in `.velnor/version-policy.toml` (`[tools]`) and `.velnor/controller-approvals.json`. The generator must either read that policy file or require and validate a `gh` pin in the project's `mise.toml`. It must not fall back to an unpinned default. This choice is open for the owner.

## What the current workflow contains

- Trigger: `workflow_call` only, with four required string inputs: `transition_id`, `sdk_version`, `sdk_source_sha` and `sdk_manifest_sha256`. It is not triggered by push or schedule.
- Permissions (workflow level and each job except the gate): `contents: read`, `actions: read` and `attestations: read`. `required-gate` sets no `permissions` key of its own, so it inherits the workflow-level permissions. Dropping them would change behavior and must be proposed as an explicit hardening. The workflow also has a top-level `cache-mode: read` key that actionlint does not know.
- Secrets: none. Only `github.token`, exposed as `GH_TOKEN`. The gate job blanks every token variable and runs `env -u ... false`.
- Target tap: none in this file. The tap lives in `.velnor/qualification/homebrew/jackin/transition.json` and reaches the workflow through `transition_id`. The only hard-coded repository is `tailrocks/velnor-new`, the SDK release source, which becomes the `repository` config key.
- Jobs: `plan` (stage the SDK, checkout, `write-request-v1`, `plan-v1`, upload the plan artifact), `native-checks` (matrix, `fail-fast: false`, `max-parallel: 2`, download the plan, `execute-native-homebrew-check-v1`, upload the report), `required` (`fetch-reports-v1`, `write-request-v1`, `merge-v1`, upload the final report) and `required-gate` (fail unless the verdict passed).
- The file is gone. #1529 (owner decision, 2026-11-19) deleted `.github/workflows/homebrew-adoption.yml` and removed the Homebrew formula from the pipelines. Its non-bootstrap steps ran the staged old-engine binary `velnor-actions-$VERSION`, selected by `VELNOR_INTERNAL_OP`. #1747 deleted that engine, so a revival would be rendered by `velnor-gen` and would call `velnor-ci`.

## Why the change is large and risky

1. Byte-identical output is impossible by design. The baseline it was compared against, `fixtures/opentofu-goldens/cases/dogfood/preview/.github/workflows/homebrew-adoption.yml`, was deleted by #1747 with the old engine. Moving the bootstrap into a helper changes the file anyway, so a revival needs a new baseline and a reviewed semantic diff, not an empty diff.
2. The generator already models job `outputs` (`RenderedJob.outputs`), per-step `continue-on-error` (`RenderedStep.continue_on_error`), the `workflow_call` trigger (`velnor-render/src/reusable_on.rs`), a matrix taken from a job output (`RenderedMatrixSource::Output`, with `max-parallel` and `fail-fast`) and the job `cache-mode` key. A revival needs no new model types for these.
3. The helper command needs its own tests and a published helper release, which the generator then pins. That is two releases in sequence.
4. The old-engine coupling no longer exists, so it does not block the design. The open choice on the `gh` pin source above still does.

## Suggested split

1. `velnor-ci`: the `stage-sdk` command, with tests against recorded release data, then a helper release.
2. `velnor-render::homebrew_adoption`: a renderer that uses the existing `workflow_call`, matrix-from-output and `cache-mode` support, behind the new config only.
3. `homebrew-adoption.toml` config, a sample `homebrew-adoption`, a `generator/CONFIG.md` entry, and a golden test that fails before the renderer exists.
4. Proof: generate the workflow and diff it against a new baseline, because the old one was deleted with #1747. No copy exists under `.github/workflows/`, so the baseline must be added with the change. The expected diff is the header, the bootstrap step and `cache-mode`, plus any permission change proposed explicitly.
