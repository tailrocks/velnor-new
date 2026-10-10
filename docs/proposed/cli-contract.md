# Velnor Actions CLI Contract

**Status:** Proposed. The `velnor-actions` binary is not implemented yet.

This document defines the complete public command line for Velnor Actions V1.
It has three commands. `plan` exposes a concise human summary from the same
analysis used by `generate`; detailed task selection, cache decisions, report
aggregation, and workflow rendering remain internal.

## 1. Executable and parser

The Cargo package MUST be named `velnor-actions-cli` and declare the executable
name `velnor-actions`. The bare name `velnor` is reserved for a future product.
V1 MUST NOT publish a `velnor` executable, package, or command alias.

The CLI MUST use Clap's derive API. Clap's built-in `--help` and `--version`
options are the only global options. The exact user-facing command tree is:

```text
velnor-actions init
velnor-actions plan
velnor-actions generate [--output-dir PATH]
```

No command may accept `--root`, a stack or profile selector, a format selector,
an ignore flag, a base/head selector, `--check`, a task ID, a report path, or
any other option not listed above. Unknown flags and extra positional arguments
MUST fail with Clap's usage diagnostic.

`crates/velnor-actions-cli/src/main.rs` parses arguments and dispatches typed
requests to `velnor-actions-orchestrator`. The CLI MUST NOT invoke Git, Cargo,
Mise, MBX, Nextest, or GitHub CLI directly. Command parsing tests MUST live in
separate test files. The orchestrator owns internal plan/task/report types and
coordinates typed process requests. The Mise adapter owns process creation,
argument validation, environment construction, and result capture for those
requests; the orchestrator MUST NOT launch processes or construct shell text.

Internal workflow steps MUST NOT be exposed as subcommands. Product workflow
operations use a bare invocation (no CLI arguments)
with `VELNOR_INTERNAL_OP` naming a versioned typed operation
(`write-request-v1`, `plan-v1`, `merge-v1`, `fetch-reports-v1`, or
`write-task-report-v1`, or `resolve-qualification-v1`) plus its gate inputs.
`resolve-qualification-v1` is restricted to a Velnor `workflow_dispatch`
plan with a runner-temp request file and a read-only GitHub token; it resolves
and verifies predecessor receipt lineage before `plan-v1` emits typed cache
directives. The complete protocol is in
[`hosted-cache-qualification-contract.md`](hosted-cache-qualification-contract.md).
`plan-v1` and `merge-v1`
read schema-1 JSON from the existing request file at
`VELNOR_REQUEST_FILE` and write the schema-1 JSON result to the sibling
`<op>-response.json` derived from the `<op>-request.json` file name;
`plan-v1` preserves a valid planner result there before checking whether
its matrix and identity fit the selected workflow output policy. A rejected
output policy fails before plan artifacts or `$GITHUB_OUTPUT` are written;
an unknown output-mode marker is rejected before planning and produces no
result file.
`write-request-v1` requires that path to be absent plus the GitHub event
environment and the runner-temp anchor (`RUNNER_TEMP`, which the request
path must sit under), and materializes the request file; `fetch-reports-v1`
takes no request file and instead requires the runner-temp velnor
directory plus the numeric run ID; `write-task-report-v1` takes no
request file and instead requires the runner-temp velnor directory,
the run environment, and the obligation env (`VELNOR_TASK_ID`,
`VELNOR_EXIT_CODE`, optional `VELNOR_DOWNSTREAM_TASK_IDS`), resolving
the obligation against the downloaded plan and writing the validated
task plus single-task matrix reports.
Versioned tags replace the earlier unversioned op vocabulary, which had
no names for request materialization or report retrieval. Any CLI
argument — including `--help`, `--version`, and public commands —
routes to the public Clap tree with the internal environment ignored,
so every public surface stays byte-identical with or without the
environment set; generated steps always invoke the helper bare, and
stray arguments fail closed through Clap's usage diagnostic (exit 2).
An unsatisfiable gate (unknown op, missing inputs) on a bare invocation
likewise falls through to Clap and fails with the usage diagnostic
(exit 2). A satisfied gate that fails operationally exits 1 with empty
stdout and a one-line stderr diagnostic. Helper staging and version
rules are in §6 and [workflow §3](workflow-contract.md).

Repository-maintenance checks use the separate private operation
`VELNOR_INTERNAL_OP=repo-policy-v1` and the allowlisted
`VELNOR_REPO_POLICY_ACTION` values `freshness`, `toolchain-specs`,
`mise-version`, `workspace-members`, `library-members`, and `trailer-policy`.
The `VELNOR_REPO_POLICY_ROOT` variable must name an existing directory.
Optional upstream, advisory, and trailer-identity flags accept only absent,
`0`, or `1`; trailer validation also requires an absolute existing message
file. Unknown actions and malformed flags fail closed. The gate transports
read-only operations to the repository-only `velnor-actions-freshness`
support crate; the orchestrator, public command tree, V1 product task graph,
and generated workflows MUST NOT depend on it. This adds no public command or
executable.

## 2. Repository root

All commands resolve the root by running `git rev-parse --show-toplevel` from
the process current working directory and require `git rev-parse
--is-inside-work-tree` to return `true`. This supports worktrees and nested
invocation without mistaking a `.git` file for a missing repository. If no
working-tree root exists, the command fails before reading configuration or
writing anything. There is no CLI root override.

The resolved root is the only source-tree location Velnor may inspect. `init`
always writes under that root. `plan` and `generate` always scan that root,
even when an output directory is supplied. Running any command from a child
directory never creates configuration or generated files in that child.

The root-discovery result MUST include the absolute root path and the original
working directory in internal diagnostics. Symlinked paths MUST be normalized
before comparing the working directory, repository root, or output directory.

## 3. Automatic stack detection

Velnor Actions is stack-generic and always invokes every registered detector in
stable order. V1 registers only the Rust/Cargo detector. Future TypeScript,
Bun, and other adapters add detectors without changing this command tree.
Stack-specific settings belong under `[stacks.<name>]`; stack selection never
appears in a command or flag.

`plan` and `generate` MUST call the same orchestrator preparation function and
consume the same validated `GenerationPreparation` and workflow IR. There MUST
NOT be a second discovery or planning implementation for `plan`. The shared
pipeline is:

```text
GenerationPreparation {
    repository_root,
    validated_config,
    detections_and_inventory,
    recommendations,
    obligations_and_task_graph,
    workflow_ir,
    rendered_files: [{ relative_path, bytes, digest }],
}
```

The renderer produces this deterministic in-memory file set once. `plan`
formats a summary from the preparation and file manifest, then discards file
bytes. `generate` writes the same bytes to its selected destination.

1. Resolve the Git root.
2. Read and validate `.velnor/config.toml`.
3. Build the repository file index and apply `[discovery].exclude`.
4. Run every registered detector in stable order.
5. Mark detections named by `[stacks].ignore` as `ignored`.
6. Build the internal plan for remaining detections.
7. Render the workflow through the same renderer and validate its complete
   in-memory file set and manifest.
8. `plan` summarizes the validated file set and discards rendered bytes;
   `generate` writes those same bytes to `.github` at the selected destination.

A stack ignore suppresses task planning, not detection or diagnostics. A
malformed discovered Cargo manifest therefore fails generation even when Rust
is listed in `stacks.ignore`. Unregistered technology stacks receive no
validation claim. A repository with no selected detection produces a valid
no-work workflow with the required final-result path.

## 4. `init`

`velnor-actions init` MUST:

- resolve the Git root from the current working directory;
- create the root `.velnor` directory when it does not exist;
- create exactly `.velnor/config.toml` when it is missing;
- refuse to overwrite or merge an existing `.velnor/config.toml`; and
- write no other file or directory.

The command MUST fail if `.velnor` is an ordinary file, if the config already
exists, or if the root cannot be found. There is no force or overwrite flag.

The created file MUST contain one required active value, `schema = 1`, and a
fully commented sample for every currently supported configuration section and
option. Optional values use the hardcoded Velnor default when omitted. The
sample MUST document these sections:

```toml
schema = 1

# Optional repository-owned named Mise checks, independent of language stacks.
# checks = []
# See docs/implemented/named-mise-checks.md for task, platform, tool, and evidence pins.

# Optional workflow display and policy settings. Omitted values use Velnor defaults.
# [workflow]
# name = "CI"                         # Workflow display name.
# policy = "consumer-v1"              # Only consumer policy; Velnor's reserved policy works only in tailrocks/velnor-new.
# default_branch = "<branch>"         # Push branch override; omit to use origin/HEAD. Required if origin/HEAD is unavailable.
# runner_label = "<latest pinned label>" # Exact latest pinned runner; omit for the default.
# generator_validation = "bootstrap"  # Generator validation mode.
# max_parallel_jobs = 2                # Maximum generated matrix concurrency.

# Optional resource limits for generated jobs.
# [resources]
# compiler_process_budget = 2           # MBX/Cargo compiler process budget.
# test_process_budget = 2               # Test process budget per generated lane.

# Optional test partitioning. Keep one shard unless measurement justifies more.
# [test_sharding]
# default_shards = 1                    # Default number of test partitions.
# by_manifest = {}                      # Manifest path to shard-count overrides.

# Optional exact registered stack IDs to suppress after detection.
# [stacks]
# ignore = []                           # Example: ["rust"].

# Optional Rust task configuration. The Rust detector is automatic in V1.
# [stacks.rust]
# configurations = [{ name = "default", features = ["default"], target = "host" }]
# compile_driver = "cargo"         # Sticky override: "cargo" or "mbx"; conflicts with durable evidence fail closed.
# test_runner = "cargo_test"       # Sticky override: "cargo_test" or "cargo_nextest".
# [[workflow.tasks]]               # Optional isolated, non-Rust verification job.
# id = "native-format"
# kind = "verification"
# mise_task = "desktop-format-check"
# runner = "macos-arm64"           # Or "linux-x64".
# timeout_minutes = 10              # Required, bounded 1..=360.

# Optional repository-relative POSIX globs excluded before detector input.
# [discovery]
# exclude = []

# Optional exact action-pin overrides. Omitted names use Velnor's bundled latest pins.
# [actions.overrides]
# "jdx/mise-action" = { version = "v5.0.0", sha = "9149ea85001c7435d5a66bb127d6a1b6227cb0a5" }
# "actions/checkout" = { version = "v7.0.1", sha = "3d3c42e5aac5ba805825da76410c181273ba90b1" }
# "actions/cache/restore" = { version = "v6.1.0", sha = "55cc8345863c7cc4c66a329aec7e433d2d1c52a9" }
# "actions/cache/save" = { version = "v6.1.0", sha = "55cc8345863c7cc4c66a329aec7e433d2d1c52a9" }
# "actions/upload-artifact" = { version = "v7.0.1", sha = "043fb46d1a93c77aae656e7c1c64a875d1fc6a0a" }
# "actions/download-artifact" = { version = "v8.0.1", sha = "3e5f45b2cfb9172054b4087a40e8e0b5a5461e7c" }
# "jdx/mr-boxington-action" = { version = "v1.6.0", sha = "1687e54eb349cadf61fa38b5813a77875489e8e6" }
# Values must be an allowlisted action's matching release version and full SHA.
```

The sample's comments MUST describe valid values, defaults, and the effect of
each option. `rust-toolchain.toml`, `mise.toml`, `mise.lock`, `.mise/tasks`,
`.github`, and any source file MUST remain untouched by `init`.

## 5. `plan`

`velnor-actions plan` MUST resolve the Git root, require the same valid
`.velnor/config.toml` as `generate`, and run the same repository analysis,
tool-file inspection, task selection, and workflow-IR planning. It MUST print a
concise, deterministic, human-readable report to stdout. It MUST NOT print
YAML, JSON, shell scripts, full command arguments, cache keys, task reports,
timestamps, or volatile run IDs. Findings and actionable recommendations go
to stderr.

The report MUST list detected stacks and their selected/ignored state; Rust
workspace roots; each crate's package name, relative manifest path, targets,
and local dependency names; planned workflow file paths; job groups and their
purpose; matrix entry count; focused step names; parallel groups; runner label;
planned cache layers; actionlint version/configuration; selected action pins;
detected Rust test runner and compile driver with evidence; and concise reasons
for ignored or ineligible work. It
MUST list every discovered crate in stable package/path order. Tool-file
recommendations MUST match `generate`.

The report describes the full configured workflow template and MUST explain
that pull-request execution can narrow crate obligations through its event-time
affected-work plan. `plan` MUST NOT claim cache hits, baseline coverage, or
exact PR selections. A repository with no supported stack still reports the
no-work workflow and final required check; an ignored Rust detection remains
visible.

Example:

```text
Velnor Actions plan 1.2.3
Repository: /work/project

Detected stacks
  Rust: selected
  Workspace crates: 5
    - core (crates/core/Cargo.toml) [lib]
    - parser (crates/parser/Cargo.toml) [lib; depends on core]
    - cli (crates/cli/Cargo.toml) [bin; depends on parser]
    - storage (crates/storage/Cargo.toml) [lib]
    - service (crates/service/Cargo.toml) [lib; depends on core, storage]

Workflow to generate
  .github/actionlint.yaml
  .github/workflows/ci.yml
  Runner: ubuntu-26.04 (latest pinned default)
  Jobs:
    - plan and formatting
    - Rust crate matrix: 5 entries
      Each: Clippy → build tests → run tests → doctests
    - final required result
  Parallel: independent crate entries; eligible Clippy lanes
  Cache layers: Mise tools, Cargo sources, MBX compilation objects

Recommendations
  mise.toml not found; Velnor will use its pinned tools
  rust-toolchain.toml not found; Velnor will not create it
```

The example is illustrative. Values and ordering MUST come from the validated
workflow plan, not from hardcoded display text.

`plan` has no task-specific arguments or output modes. It MUST NOT execute Rust
tasks, restore or publish caches, or modify `.github`, `.velnor/config.toml`,
user tool files, source files, or other repository content. It MUST use the
same validated preparation object and renderer output as `generate`; rendered
bytes exist only in memory, or staged under an outside-root temporary
directory that is discarded, and identical validation runs over either
form. Staging MUST be outside the repository root and MUST NOT be an
ancestor of it, mirroring `generate --output-dir` rules; canonicalization,
escape-fail-closed, and no-repository-writes still apply.

## 6. `generate`

`velnor-actions generate` MUST resolve the Git root, require a valid
`.velnor/config.toml` with `schema = 1`, run the complete automatic detection
pipeline, and generate all Velnor-owned GitHub Actions files. Missing optional
configuration values use hardcoded defaults; unknown keys, invalid values, and
unsupported stack settings fail before any output replacement.

Without `--output-dir`, the destination is the repository root. Velnor MUST
replace the generated `.github` tree from scratch after rendering succeeds.
The replacement MUST be atomic at the directory level: a failed scan, plan,
render, or validation leaves the previous `.github` tree unchanged. Generated
output files are written only under `.github`. In-place generation may retain
the private, self-ignored `.github.velnor-stage/` runtime container at the Git
root, bound to that canonical worktree and containing one persistent same-
filesystem spare; it is staging state, not generated output. Generation clears
only spare children and never removes or recreates the container or spare root.
`plan` and preview generation do not create or modify this state. An existing
`.github` root and every real directory below it must be owned by the caller;
foreign-owned directories fail before publication as specified by the
generated-file contract. `rust-toolchain.toml`, `mise.toml`, and `mise.lock`
are read-only inputs and MUST remain byte-for-byte unchanged.

`--output-dir PATH` is preview mode. PATH is the exact preview root; Velnor
writes `PATH/.github`. PATH MUST be fresh and empty (or absent); Velnor MUST
refuse a non-empty destination rather than replace unrelated files. The caller
chooses a unique path for each preview. Velnor MUST NOT modify the repository,
including `.github` or `.velnor/config.toml`. The canonical preview invocation
is:

```text
velnor-actions generate --output-dir "$(mktemp -d "/tmp/velnor-actions-preview.XXXXXX")"
```

The destination MUST be outside the repository root and MUST NOT be an ancestor
of it. The command prints the absolute preview root and generated file list to
stderr. There is no
`--check` mode; preview output and ordinary file comparison provide that
workflow.

Every generated text file MUST begin with a comment containing the exact
Velnor Actions version, for example:

```text
# Generated by Velnor Actions <exact-semver>; edit .velnor/config.toml and regenerate.
```

The marker MUST contain no generation date. The version is the generator's
exact release version and is part of the generated-file identity. Generated
workflows MUST be deterministic for identical repository inputs, configuration,
tool policy, and generator version.

Internal plans, task definitions, cache decisions, and reports MAY exist in
memory or in a run-scoped temporary directory while generation or a generated
workflow executes. They MUST NOT become additional public CLI commands or
additional repository-owned generated files.

A workflow helper is the SHA-256-verified generating-release binary
staged at `$RUNNER_TEMP`, never a repository path or a second
executable. The helper version MUST equal the bootstrap descriptor
selected in [workflow §3](workflow-contract.md), EXCEPT the enumerated
candidate-qualification steps in candidate mode (`Check generated
files` and the `candidate` qualification checks), which use the
verified candidate artifact. No other step may invoke the candidate.

## 7. Output and exit status

`plan` writes its report to stdout. `generate` writes human-readable
recommendations about optional tooling files and detection findings to stderr.
When `generate` replaces the `.github` tree, it MUST also list every
removed path that was not Velnor-generated output on stderr (see
[generated-file §3](generated-file-contract.md)); previews report the
same list without modifying the repository.
Neither `plan` nor `generate` has JSON or format-selection options. The
generated workflow is the machine-readable product; detailed plans and reports
remain internal and are validated by the orchestrator and workflow's final
required gate.

Exit statuses are intentionally small:

| Code | Meaning |
|---:|---|
| 0 | Command completed successfully. A valid no-work plan or generation is success. |
| 1 | Repository discovery, configuration, detection, planning, rendering, validation, or output replacement failed. |
| 2 | Clap usage error or unsupported command/option. |

No command may report success when a required detector, renderer, workflow
validation, or output replacement failed.
