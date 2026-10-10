**Status:** Proposed specification. The orchestrator coordinates writes; named
adapters own each generated format. The current generator follows the managed
output and unmanaged-content preservation rules in §3.

# Velnor Actions generated-file contract

Velnor V1 generate produces these managed outputs under `.github` and
preserves other repository-owned entries as specified in §3:

```text
.github/
├── AGENTS.md
├── CLAUDE.md                # regular file; single `@AGENTS.md` import line
├── actionlint.yaml
├── release-plz-bootstrap.toml   # consumer-v1 only, release enabled
├── release-plz.toml             # consumer-v1 only, release enabled
└── workflows/
    ├── binary-release.yml         # consumer-v1 only, Rust binary release enabled
    ├── ci.yml
    ├── velnor-qualification.yml  # velnor-repository-v1 only
    └── release.yml               # velnor-repository-v1, or consumer-v1 with release enabled
```

The qualification workflow is emitted only for
`workflow.policy = "velnor-repository-v1"`; `consumer-v1` MUST NOT emit
it. `release.yml` is emitted for `velnor-repository-v1` (keeping
its existing Velnor-internal meaning) and for `consumer-v1` when
`[stacks.rust.release].enabled = true`, together with the two effective
release-plz configs; with release disabled, `consumer-v1` emits none of
the three. All workflow files are rendered from typed workflow IR by
`velnor-actions-workflow-renderer`, each with an explicit `permissions:`
block (least privilege for its role); see [workflow
§3](workflow-contract.md).

> Amendment 2026-09-30: the consumer release scope ([release
> §11](release-contract.md)) overrides the earlier blanket
> prohibition on consumer release emission. `consumer-v1` MUST emit
> `release.yml` plus `.github/release-plz.toml` and
> `.github/release-plz-bootstrap.toml` when
> `[stacks.rust.release].enabled = true`, and MUST emit none of them
> otherwise.

The consumer binary workflow is emitted only when
`[stacks.rust.binary_release].enabled = true`; it is a separately generated,
single-package workflow described in [binary release
contract](binary-release-contract.md). With the option absent or disabled,
`binary-release.yml` is not emitted and any prior generated copy is retired on
successful generation.

| Output | Produces format | Write coordinator | Ownership rule |
|---|---|---|---|
| .github/actionlint.yaml | velnor-actions-actionlint | velnor-actions-orchestrator | Exact generated path; replaced on generation |
| .github/AGENTS.md | velnor-actions-workflow-renderer | velnor-actions-orchestrator | Exact generated path; version marker; replaced on generation |
| .github/CLAUDE.md | velnor-actions-workflow-renderer | velnor-actions-orchestrator | Exact generated pointer file with the single `@AGENTS.md` import line; replaced on generation |
| .github/workflows/** | velnor-actions-workflow-renderer | velnor-actions-orchestrator | Reserved generator-owned namespace; every path is replaced or retired on successful generation |
| .github/release-plz*.toml | velnor-actions-workflow-renderer | velnor-actions-orchestrator | Exact generated release-plz config paths; retired when no longer emitted |
| .github/actions/<logical>/action.yml | velnor-actions-workflow-renderer | velnor-actions-orchestrator | Generated shared action path is owned when its first line has a Velnor generated marker; replaced or retired with dispatch output |
| mise.toml, mise.lock, rust-toolchain.toml | None | None | Repository-owned read-only inputs; never create or modify |

Velnor V1 MUST NOT create .mise/tasks files or any other generated task
directory. Mise remains the tool and command execution authority, but the
workflow renderer emits fixed, validated Mise invocations directly in the
workflow. Future adapters MUST declare additional .github output formats
before they are supported.

init has one separate write: it creates the missing .velnor/config.toml sample
at the repository root. The sample is user-owned after creation; init MUST
refuse to overwrite it and generate MUST leave it unchanged. Its first line is
the same versioned generated marker, followed by comments for every optional
setting and uncommented values only for settings required to generate output.

`velnor-actions plan` writes no generated file. Its concise stdout report is
not a repository artifact and needs no generated-file marker. It may validate
the same renderer's output bytes in memory, then discard them. Only `generate`
writes the `.github` tree. `generate --output-dir PATH` prints its absolute
destination and generated-file list to stderr; `plan` keeps stdout for its
human report.

## 1. Internal task identities

The generator assigns every product obligation a stable internal task ID:
stack/<stack-id>/<component-key>/<task-kind>/<configuration>. Internal
orchestration obligations use internal/<task-kind>/<configuration>. The Rust
adapter uses a normalized repository-relative Cargo manifest path without
/Cargo.toml, with root for the root manifest. A test shard appends
/shard-<index>-of-<count>.

These IDs are metadata only. They are not public CLI commands, Mise task
names, generated files, or executable names. The workflow and reports preserve
them so selection, parallel scheduling, cache identity, and failure reporting
remain deterministic. A stack adapter owns its task-ID namespace; the
stack-neutral orchestrator only validates the grammar and ordering.

## 2. Generated workflow format

Every generated text file begins with:

```text
# Generated by Velnor Actions <version>; edit .velnor/config.toml and regenerate.
```

The marker MUST contain the exact Velnor Actions version used to render the
file. It MUST NOT contain a generation date. The YAML renderer MUST emit the
marker as a comment before the workflow document and MUST use stable key,
job, step, matrix, and array ordering. `.github/CLAUDE.md` is exempt from
the marker: it carries only the single `@AGENTS.md` import line so Claude
resolves the sibling instructions without symlink resolution.

Generated workflow shell steps MUST contain fixed argument vectors produced by
typed adapters. Repository configuration MUST NOT supply shell text, arbitrary
YAML, arbitrary uses actions, or unchecked environment interpolation.
Rust commands use exact Mise-managed tools and the detected compile driver.
This is the MBX form when repository evidence selected MBX:

```yaml
- name: Clippy
  run: mise --no-config exec mbx@<exact-version> -- mbx clippy --package <package> --all-targets --locked -- -D warnings
```

Rust command selection is per detected Rust workspace. The Rust adapter records
whether the repository uses MBX and whether it uses Nextest; the generator uses
the matching fixed command family. Without MBX, invoke Cargo through Mise.
Without Nextest, use `cargo test`; do not add Nextest merely because Velnor
supports it. If Nextest is selected, doctests still use `cargo test --doc`.

`.github/actionlint.yaml` MUST be rendered deterministically by
`velnor-actions-actionlint`; generation MUST NOT call `actionlint -init-config`
or fetch an unpinned template. It MUST contain the Velnor version header and
must configure no ignored diagnostics. Its default content is:

```yaml
# Generated by Velnor Actions <version>; edit .velnor/config.toml and regenerate.

config-variables: []

self-hosted-runner:
  labels:
    - ubuntu-26.04
```

`ubuntu-26.04` is listed only as an actionlint compatibility bridge while the
pinned actionlint release's built-in GitHub-hosted label table lags the
versioned GitHub runner catalog. Velnor's own runner registry validates that
it is an exact GitHub-hosted label; it is not emitted in `runs-on` as a
self-hosted runner. The actionlint crate MUST remove this bridge once its
pinned version recognizes the hosted label natively. When a generated workflow
references repository configuration variables, the renderer adds their exact
declared names to `config-variables`. V1 generates no self-hosted runners. Path-specific
ignore entries are forbidden unless a narrowly matched diagnostic is approved
in Velnor's own protected policy; consumer config cannot add ignores.
Actionlint config is part of the generated tree and is linted with the same
pinned actionlint binary as every generated workflow.

The renderer MUST quote all repository-derived values safely. It MUST reject
commands that include an absolute Cargo path, cargo install, an unpinned tool,
or an unvalidated shell fragment.

## 3. Replacement and preview

generate MUST render into a staging tree and validate every output before
publishing. For in-place generation only, the orchestrator may retain the
private, self-ignored `.github.velnor-stage/` runtime container at the Git root.
It contains a root-bound owner record and one persistent same-filesystem spare
directory; generation clears spare children but never removes or recreates
either root. This container is runtime state, not generated output. `plan`
and preview generation never create or modify it. The orchestrator owns only
the declared generated paths,
the reserved `.github/workflows/**` namespace, and shared action definitions
whose exact `.github/actions/<logical>/action.yml` path begins with a Velnor
generated marker. Those paths are replaced when emitted and retired when a
successful generation no longer emits them. The workflows namespace is fully
generator-owned; manually maintained workflows placed there are retired.

All other entries under `.github` are unmanaged repository content. Successful
generation MUST preserve their file bytes, hidden names, symbolic links and
targets, pre-existing empty directories, and permissions. A directory emptied
solely by retiring its managed generated outputs may be pruned. If an unmanaged
entry collides with a generated output path, generation MUST fail before
publishing rather than overwrite the entry. Unsupported filesystem entry
types and unsafe symlink ancestors also fail closed. Unmanaged files that need
to participate in workflow generation must be represented through supported
Velnor configuration or another generated input, because their contents are
not merged into generated files.

For in-place generation, an existing `.github` root and every real directory
below it MUST be owned by the effective user running Velnor. The orchestrator
checks directory ownership without following symbolic links before staging and
again immediately before exchange. A foreign-owned directory fails before
publication and leaves `.github` unchanged. File ownership is not preserved;
their bytes, permissions, names, and link targets remain the preservation
contract. An out-of-band ownership change after the final scan can make retired
tree cleanup fail after publication; the command reports that cleanup failure,
leaves the published output in place, and requires safe operator recovery before
the next in-place generation.

With no --output-dir, the orchestrator MUST stage the generated tree together
with preserved unmanaged content in the persistent spare, then replace the
repository `.github` directory only after staging and validation succeed. If
`.github` exists, its root mode MUST be preserved. If generation fails before
publication, the existing `.github` directory remains unchanged. After
publication, cleanup removes children from the retired root while keeping that
spare path present; cleanup failure is reported with the published output left
in place.

With --output-dir PATH, PATH is the exact fresh preview root. It MUST be absent
or empty; the command stages output beside PATH/.github and publishes the
completed directory only after preservation, collision checks, writes, and
permission restoration succeed. A failed preview MUST remove its reserved
PATH/.github so the empty preview root can be retried. The command prints its
absolute path and modifies no repository file. Callers MUST choose a unique
directory under /tmp or runner temporary storage. The preview tree MUST have
the same `.github` contents as a real generation, including preserved
unmanaged entries.

The generated workflow MUST preserve the repository-owned
rust-toolchain.toml, mise.toml, and mise.lock byte-for-byte because generation
never writes those paths. Missing or conflicting tool files produce
recommendations in the generated plan/report but do not cause Velnor to create
or rewrite them.

Generation MUST be deterministic for identical repository inputs, configuration,
tool-version policy, and Velnor version. It MUST reject an invalid or unknown
configuration before removing the existing .github tree. All generated output
paths, task IDs, matrix entries, and workflow steps MUST be sorted by their
documented canonical identity.
