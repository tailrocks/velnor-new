# PR notes: velnor-gen and source-preview-publication.yml (#1290, P1)

## Step 1 decision (design only, no code yet)

### What the current workflow does

`.github/workflows/source-preview-publication.yml` is a reusable workflow
(`on: workflow_call`, top-level `permissions: contents: read`). It has four
jobs, each gated by the same `if` (push event, this repository, tag
`sdk-preview-*`) and each on `ubuntu-26.04`:

| Job | Needs | Permissions | Environment | Outputs |
| --- | --- | --- | --- | --- |
| `source-preview-source` (5 min) | none | contents read, actions read | none | three artifact ids (source, linux payload, darwin payload) from three `upload-artifact` steps |
| `source-preview-draft` (10 min) | source | actions read, contents write | `generator-source-preview` | artifact_id, the three ids, sign, pair_origin, pair_id, receipt_sha256, bundle_sha256 (from step `draft`) |
| `source-preview-attest` (10 min) | source, draft | actions read, contents read, id-token/attestations/artifact-metadata write | `generator-source-preview` | pair_origin, pair_id, receipt_sha256, bundle_sha256 (expressions with `||` fallbacks) |
| `source-preview-publish` (10 min) | source, draft, attest | actions read, contents write | `generator-source-preview` | none |

Each job runs the same shapes of step: validate an artifact id, download
the artifact by exact id (`download-artifact`), set up mise, install a
pinned `gh`, then materialize a "fixed script" (about 90 KB of base64 in
`run:` steps, sha256-checked) and execute it with job-context env
(`job.workflow_repository`, `job.workflow_sha`, `needs.*.outputs.*`).
There is no checkout: the scripts come from the workflow bytes themselves,
so the authority is the pinned workflow ref. `attest` adds
`attest-build-provenance` and uploads, all with step-level `if`.

### Why this is not a small change

The velnor-gen model today generates Rust CI workflows from analysed
repository facts. It has no way to express an arbitrary multi-job
workflow (needs, environments, job outputs, per-step `if`, per-job
permissions). Beyond the model, the current file breaks design rule 9 in
four places: multi-line shell in "Validate exact artifact ID", "Install
pinned GitHub CLI" and "Retain exact preview bundle"; the script
materializer (`set -euo pipefail`, `mktemp`, `printf` into `$GITHUB_ENV`);
the sha-check plus `exec bash` launcher; and about 270 KB of embedded
Python/shell. A byte-identical regeneration is therefore impossible by
design. The rule-9 version needs new `velnor-ci` helper commands and a
helper release (the generator pins a helper version, see CONFIG.md), plus
the embedded authority scripts moving into the helper or into committed
files. That spans generator model, renderer, helper, sample, goldens and a
release, which is over the 90 minute budget. Per the task, stopping here.

### Smallest generic feature (proposed)

An opt-in `[[workflow]]` table in `.velnor/rust-ci.toml`: a declarative
authority workflow, absent by default so default output stays
byte-identical. It is not specific to velnor-new; any repository can
declare a fixed-shape workflow.

```toml
[[workflow]]
name = "Source preview publication authority"
file = "source-preview-publication.yml"
on = "workflow_call"
permissions = { contents = "read" }
if = "github.event_name == 'push' && startsWith(github.ref, 'refs/tags/sdk-preview-')"

[[workflow.job]]
id = "source-preview-draft"
needs = ["source-preview-source"]
environment = "generator-source-preview"
permissions = { actions = "read", contents = "write" }
timeout_minutes = 10
outputs = { sign = "steps.draft.outputs.sign" }

[[workflow.job.step]]
uses = "download-artifact"        # only from the fixed pinned action list
with = { artifact-ids = "needs.source-preview-source.outputs.source_artifact_id" }
[[workflow.job.step]]
helper = "validate-artifact-id"   # expands to one `velnor-ci ...` command
```

Rules for the feature:

1. Actions come only from the existing fixed, SHA-pinned list; the config
   names an action, never a SHA or a free `uses`.
2. A step is either an action or a `helper` command. Free `run` strings
   are refused (rule 9). Env values are context expressions, validated
   against an allow-list of contexts (`needs.*.outputs.*`, `job.*`,
   `github.token`, `steps.*.outputs.*`).
3. `needs`, `if`, `environment`, `permissions`, `outputs` map one-to-one
   to GitHub keys; the renderer validates that every `needs.X` names a
   declared job and every output names a declared step id.

Helper commands needed (new, in `velnor-ci`): `validate-artifact-id`
(replaces the shell loop), `install-gh` (replaces the mise install step),
`stage-bundle` (replaces the bundle copy), and `run-authority <script>`
which carries the embedded, digest-pinned script and replaces the
materialize and launcher steps. The authority scripts themselves move
into the helper release so the workflow carries only digests.

### Proposed split

1. Model and renderer for `[[workflow]]` (jobs, needs, outputs, env,
   `if`), with a red-first test (a config with two jobs renders needs and
   outputs) and a `workflow-declared` sample. About 400 lines.
2. Helper commands and the helper release bump, with CONFIG.md entry.
3. Move the authority scripts into the helper; the `velnor-new` sample
   regenerates the file and the diff against the current one is recorded
   here.

### Proof status

Not done: no code was written, so there is no generated output to diff
against the current file. Expected diff once done: every shell `run:`
body and the 270 KB script payload change (helper command or digest
instead); triggers, jobs, permissions, needs, environments, `if` gates
and outputs should be identical.
