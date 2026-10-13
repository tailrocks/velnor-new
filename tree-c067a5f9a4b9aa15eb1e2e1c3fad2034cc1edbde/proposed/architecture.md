# Velnor V1 architecture

**Status:** Proposed implementation specification. No implementation is claimed.

This document defines Velnor's V1 crates, repository inputs, data contracts,
and generated-file ownership. The exact executable interface is in the [CLI
contract](cli-contract.md); workflow details are in [the workflow
contract](workflow-contract.md); cache/report details are in [the cache
contract](cache-contract.md); Rust tooling rules are in [the quality
contract](rust-quality-contract.md).

Normative terms **MUST**, **MUST NOT**, **SHOULD**, and **MAY** have their usual requirement meanings. Missing or invalid input MUST fail with a diagnostic; it MUST NOT silently select a broader or narrower behavior except where this spec says to broaden verification.

## 1. Product boundary and dependency direction

Velnor Actions is a stack-generic GitHub Actions workflow generator. V1 registers
the `rust` detector; the architecture keeps stack analysis, Mise integration,
generic workflow rendering, and orchestration in separate packages.

Cargo dependency direction (`A -> B` means “A depends on B”):

```text
velnor-actions-cli -> velnor-actions-orchestrator
velnor-actions-orchestrator -> velnor-actions-contract
velnor-actions-orchestrator -> velnor-actions-rust
velnor-actions-orchestrator -> velnor-actions-mise
velnor-actions-orchestrator -> velnor-actions-actionlint
velnor-actions-orchestrator -> velnor-actions-workflow-renderer
velnor-actions-rust -> velnor-actions-contract
velnor-actions-mise -> velnor-actions-contract
velnor-actions-actionlint -> velnor-actions-contract
velnor-actions-workflow-renderer -> velnor-actions-contract
```

The V1 workspace MUST have exactly these seven product crates. Every Cargo
package MUST use `velnor-actions-<purpose>`. Generic names such as
`velnor-model`, `velnor-core`, `velnor-common`, or
`velnor-utils`, and `velnor-rust` are forbidden. The only binary-target exception is
`velnor-actions`, owned by package `velnor-actions-cli`. Future stack adapters
MUST use explicit stack names such as `velnor-actions-node`; each adapter
contains only its named stack. Alint is the sole repository-structure linter
and runs as a separate GitHub Actions job.

| Crate | Owns | Must not own |
|---|---|---|
| `velnor-actions-contract` | Stack-neutral generator contracts: stack/component IDs, task graph, workflow IR, cache identities, generated-file records, reports, recommendations | Rust/Cargo, Mise, process, filesystem, YAML implementation, CLI, or generic application models |
| `velnor-actions-rust` | All V1 Rust/Cargo behavior: manifest discovery inputs, metadata conversion, Rust targets/graph, reverse-dependency selection, Rust task proposals and requirements, read-only `rust-toolchain.toml` inspection | Mise command/config/task syntax, GitHub YAML, process execution, or non-Rust stack behavior |
| `velnor-actions-mise` | Mise version/tool selection, command construction, read-only `mise.toml`/`mise.lock` inspection, environment isolation, task-cache integration, execution of tool requests through a fixed Mise subprocess wrapper | Cargo metadata/graph rules, `rust-toolchain.toml`, stack discovery, GitHub YAML, or other stack semantics |
| `velnor-actions-actionlint` | Pinned actionlint capability/version metadata, generated `.github/actionlint.yaml`, invocation, and actionlint-specific pre-write validation | Rust/Cargo, Mise execution, generic YAML rendering, or workflow planning |
| `velnor-actions-workflow-renderer` | Stack-neutral GitHub Actions YAML from workflow IR: jobs, matrices, steps, triggers, permissions, and syntax supported by pinned actionlint | Rust/Cargo, Mise syntax, repository scanning, subprocesses, or stack-specific policy |
| `velnor-actions-orchestrator` | Compose stack adapter, Mise adapter, contract, and renderer; plan obligations, apply selection/cache evidence, schedule resource-safe tasks, coordinate generation and writes | Parsing Cargo/Mise inputs, YAML templates, CLI parsing, building shell commands, or launching processes |
| `velnor-actions-cli` | Clap parser, typed dispatch, concise human plan report, generation output, exit codes; declares binary `velnor-actions` | Planning rules, Cargo/Mise policies, YAML/Mise rendering, or orchestration algorithms |

`velnor-actions` is the executable declared by package `velnor-actions-cli`.
Clap help MUST describe a stack-generic GitHub Actions workflow generator.
The CLI converts arguments to typed requests and calls the orchestrator; it
contains no product planning or stack logic. The orchestrator asks the Rust
adapter for Rust inventory, `rust-toolchain.toml` findings, and stack-neutral
task proposals; asks the Mise adapter to resolve tool requests and inspect Mise
files; and asks the workflow renderer to render generic workflow IR. It asks
the actionlint crate to validate rendered output before any write. `plan` and
`generate` use the same validated analysis snapshot;
`plan` prints its concise text summary, while `generate` serializes the same
workflow IR to YAML and writes `.github`. Rust, Mise, and workflow-renderer crates MUST
depend only on `velnor-actions-contract`; they MUST NOT depend on each other.
Only the orchestrator composes them. The executable/package name `velnor`
remains reserved for a separate future product.

Adapters exchange typed requests and stack-neutral results through the contract
crate. The Rust adapter requests Cargo metadata; the orchestrator asks Mise to
execute it with exact pins, then passes the JSON back to Rust for validation.
The Rust adapter never launches processes or constructs Mise commands. Mise
owns the fixed subprocess wrapper for Git, Cargo/MBX, Nextest, GitHub CLI, and
policy tools. The orchestrator supplies requests and schedules calls; it never
builds shell commands. Rust alone inspects `rust-toolchain.toml`; Mise alone
inspects `mise.toml` and `mise.lock`. The renderer receives validated command
references and generic workflow IR; it never emits Mise syntax.

All Velnor product Rust package roots MUST live under `crates/`. The root
manifest MUST be a virtual workspace with explicit members.
`fixtures/rust-workspaces/**` contains independent Cargo projects used as test
inputs, not Velnor product packages. Alint rules MUST scope product source and
line-limit rules to Velnor-owned paths; fixture manifests remain inputs to
dedicated tests, not product packages.

## 2. Repository files

```text
Cargo.toml
Cargo.lock
rustfmt.toml
clippy.toml
deny.toml
.mise-version
.alint.yml
.config/nextest.toml
.velnor/config.toml
.velnor/generator.lock
.velnor/version-policy.toml
crates/velnor-actions-contract/
crates/velnor-actions-rust/
crates/velnor-actions-mise/
crates/velnor-actions-actionlint/
crates/velnor-actions-workflow-renderer/
crates/velnor-actions-orchestrator/
crates/velnor-actions-cli/
fixtures/rust-workspaces/
.github/actionlint.yaml
.github/workflows/velnor.yml
```

`rust-toolchain.toml`, `mise.toml`, and `mise.lock` are optional inputs, not
required files. Their read-only behavior and missing-file recommendations are
defined in the [tooling input contract](tooling-input-contract.md).

`.mise-version` is a Velnor-repository bootstrap pin, not a generated output
or consumer-project input. A reviewed Velnor version-policy change updates it
with `.velnor/version-policy.toml`. Velnor Actions never creates or updates a
consumer's `.mise-version`.

`Cargo.toml`, `rust-toolchain.toml`, `mise.toml`, `mise.lock`,
`.velnor/config.toml`, and `.velnor/version-policy.toml` are repository-owned
inputs. The three tool files are optional and read-only to Velnor. Velnor
reports missing, malformed, stale, or conflicting settings with manual
recommendations; it never creates or edits them. V1 generates only the
`.github` tree listed in the [generated-file contract](generated-file-contract.md).
Every generated text file has a first-line Velnor Actions version marker with
no date. Velnor uses its own exact tool pins through Mise; project Mise files
are advisory inputs and cannot change generated tool versions. The Mise
adapter constructs fixed, pinned tool invocations; it creates no task files
inside the repository. If Mise task-result caching is enabled after its gate,
the adapter writes task definitions only under the runner's temporary directory.

## 3. Configuration schema

`.velnor/config.toml` MUST use TOML, `schema = 1`, and reject unknown fields. Its initial supported shape is:

```toml
schema = 1

[workflow]
name = "CI"
policy = "consumer-v1"
# default_branch = "main" # Required only when origin/HEAD cannot be resolved.
generator_validation = "bootstrap"
max_parallel_jobs = 2
# runner_label = "ubuntu-24.04" # optional older pinned override; omit for latest

[resources]
compiler_process_budget = 2
test_process_budget = 2

[test_sharding]
default_shards = 1
by_manifest = { "crates/large/Cargo.toml" = 2 }

[stacks]
ignore = [] # example override: ["rust"] disables Rust planning/generation

[stacks.rust]
configurations = [{ name = "default", features = ["default"], target = "host" }]

[discovery]
exclude = ["vendor/**", "fixtures/**"]

```

`workflow.policy` MUST be `consumer-v1` or `velnor-repository-v1`.
`consumer-v1` is the default and MUST NOT assume Velnor-specific files.
`velnor-repository-v1` enables Velnor's separate Alint/dependency-security jobs
and candidate validation; accept it only for canonical `tailrocks/velnor-new`.
In CI check `GITHUB_REPOSITORY`; locally normalize `origin`. Missing or
mismatched identity fails validation. The policy does not select a stack.

`workflow.runner_label` is optional. If omitted, generation MUST use the latest pinned label in `.velnor/version-policy.toml`. An explicit value MUST exactly match that file's supported-label list; it is the only runner-version compatibility override and is recorded as `config_override`. The plan records `latest_default` when no override is present. `workflow.name` is the display name. If `workflow.default_branch` is omitted, resolve the branch named by local symbolic ref `refs/remotes/origin/HEAD`; if that ref is absent or invalid, generation fails and instructs the user to set `workflow.default_branch`. Never assume `main`, use the current feature branch, or fetch a remote to guess. `generator_validation` is `bootstrap` or `candidate`; Velnor uses `candidate`. `max_parallel_jobs`, compiler/test budgets, and shard settings bound execution.

`stacks.ignore` is a sorted, duplicate-free list of exact registered stack IDs. For example, `ignore = ["rust"]` disables Rust task planning and generation. Detection still runs, and matching detections appear with status `ignored`. Unknown IDs fail configuration validation. CLI flags cannot change this list. Stack-specific options belong under `[stacks.<id>]`; V1 supports `[stacks.rust].configurations` only. If that field is absent, use one documented default Rust configuration. `discovery.exclude` contains repository-relative POSIX path globs applied before detectors; it is not a stack selector. Invalid, absolute, parent-traversal, or malformed patterns fail validation. V1 registers only `rust`; no-Rust repositories produce an empty stack inventory. V1 MUST reject custom shell fragments, raw YAML, arbitrary `uses:` actions, and task definitions in this file.

Configuration validation MUST report file, key path, and problem. Unknown `schema` versions fail with `unsupported_schema`; unknown keys fail with `unknown_config_field`. Defaults MUST NOT be invented for required fields.

`.velnor/generator.lock` MUST contain:

```toml
schema = 1

[generator]
binary = "velnor-actions"
version = "<exact-semver>"

[[generator.binaries]]
target = "x86_64-unknown-linux-gnu"
artifact = "<immutable-release-artifact-url>"
sha256 = "<64-lowercase-hex>"

[[generator.binaries]]
target = "aarch64-apple-darwin"
artifact = "<immutable-release-artifact-url>"
sha256 = "<64-lowercase-hex>"

[[generator.binaries]]
target = "x86_64-apple-darwin"
artifact = "<immutable-release-artifact-url>"
sha256 = "<64-lowercase-hex>"

[mise-bootstrap]
version = "<exact-semver>"
artifact = "<immutable-release-artifact-url>"
sha256 = "<64-lowercase-hex>"

[[actions]]
name = "actions/checkout"
version = "<release-label-for-review>"
sha = "<40-lowercase-hex-commit>"
reviewed = "YYYY-MM-DD"
```

One `actions` record is required for each action Velnor itself emits. This is a dogfood mirror of the binary's compiled-in registry, not a required consumer file; consumer workflows work from the registry and optional `.velnor/config.toml` overrides. The artifact URL MUST refer to an immutable release asset named for the `velnor-actions` executable and target; Velnor verifies SHA-256 before execution. Action names MUST be from the allowlist in the workflow contract. Each default action SHA MUST correspond to its latest stable release under the [version policy](version-policy.md). Lock updates are reviewed toolchain changes, not automatic generation side effects.

`.mise-version` MUST contain the same exact semantic version as
`mise-bootstrap.version`. GitHub CI bootstraps Mise by downloading that locked
release binary and verifying SHA-256 before invoking it. Local development MUST
use the same pinned version. This is the only step allowed to obtain Mise
without Mise itself. Every tool after this bootstrap is installed or selected
by Mise.

The `generator.binaries` array MUST contain exactly one entry for every
supported execution target. The target is the Rust host triple reported by the
bootstrapped toolchain. A missing or duplicate target entry is an error; Velnor
MUST NOT run a binary built for another target. Each artifact URL MUST be
immutable and each digest MUST be verified before execution. The lock file's
single `generator.version` applies to every binary entry.

### 3.1 Bootstrap, candidate, and promotion lifecycle

The bootstrap binary is the only Velnor binary allowed to decide the current
workflow graph. A candidate binary MUST NOT generate the job graph that builds
or promotes that same candidate. This prevents a changed generator from
changing the work needed to prove itself.

The first release and every later release use this sequence:

1. A protected `generator.lock` points to an already published, immutable
   bootstrap release. The initial release is seeded once by a manually reviewed
   binary built through Mise/MBX using the exact tool pins below; no Velnor
   binary is required to build that seed.
2. The pinned `velnor-actions` binary is invoked to select the record whose
   `target` matches the runner, verify its version and SHA-256, and use that
   binary for `plan`, metadata, and
   matrix generation. The plan job MUST NOT wait for or invoke the candidate.
3. The candidate build job invokes the fixed Cargo/MBX argument vector through
   Mise with exact versions from `.velnor/version-policy.toml`. It MUST NOT
   depend on or modify project tool files.
4. The candidate build uploads one binary for the runner target, together with
   its SHA-256, source commit, target triple, and toolchain identity. Candidate
   qualification downloads that exact artifact; it MUST NOT rebuild it in a
   later job.
5. Candidate qualification runs the candidate against generated output,
   fixtures, policy checks, and the required V1 gates. It may validate a
   changed generator output; the old bootstrap binary MUST NOT be required to
   reproduce a new generator contract.
6. A protected release job promotes only a candidate that passed qualification.
   It publishes immutable per-target assets, verifies the published digests,
   then updates `generator.lock` to the new version and all target entries in a
   separate reviewed change. The lock update is the final promotion step.

CI invokes the candidate build as `mise exec --no-config rust@<exact>
mr-boxington@<exact> -- mbx build --release --locked --package
velnor-actions-cli --bin velnor-actions`. The package and binary names differ.
Explicit arguments select exact tools without loading project
tool files. The policy gate MUST verify MBX handled the compile. A pull-request
candidate is never promoted and never replaces the locked bootstrap binary.
Until promotion completes, ordinary CI runs continue using the previous lock
entry.

## 4. Cargo discovery and metadata

The orchestrator builds one sorted repository file index, applies built-in and
`discovery.exclude` path exclusions, then runs every registered detector in
fixed `stack_id` order. A detector receives tracked/untracked mode and returns
`DetectedProject { stack_id, project_root, inputs, components, diagnostics }`.
The registry rejects duplicate `(stack_id, project_root)` records. After
detection, the orchestrator marks every record whose `stack_id` is in
`stacks.ignore` as `ignored`; it records the reason and creates no tasks from
that record. V1 registers `rust`, whose detector finds Cargo manifests, nested
workspaces, and standalone packages. Future detectors add inventory and their
own stack configuration without changing the CLI command tree.

For each candidate manifest, invoke Cargo metadata through Mise with explicit
exact tool pins and project configuration disabled. Metadata discovery is not
a compilation task, so it always uses Cargo even when subsequent compilation
uses MBX:

```text
mise exec --no-config rust@<exact> -- cargo metadata --format-version 1 --no-deps --manifest-path <manifest>
```

The output is Cargo metadata JSON format version 1. `--no-deps` avoids fetching third-party dependencies but sets the resolved `resolve` graph to null. It still exposes each workspace package's manifest dependency declarations, including local `path` values. Velnor MUST build its conservative local-package graph from those declarations and connect each path to the package whose manifest Cargo reports. It MUST include every declared local path edge, including optional and target-specific edges, so feature differences cannot omit a reverse dependent. The command MUST use exact tool arguments and MUST NOT read or write project Mise files. Nonzero status, invalid JSON, or unsupported metadata version fails that candidate and the plan; Velnor MUST NOT parse Cargo manifests or Rust source as fallback.

For qualification of the selected feature/target resolution, after dependency sources have been prepared, run:

```text
mise exec --no-config rust@<exact> -- cargo metadata --format-version 1 --locked --offline --manifest-path <workspace-root>/Cargo.toml
```

This uses the same explicit tool pins and Cargo metadata command. Missing offline dependencies are `preparation_incomplete`, not a reason to fetch or produce a partial resolution. Velnor's own manifest-to-model adapter consumes the JSON; the `cargo_metadata` crate MAY deserialize it but MUST NOT execute Cargo. The plan job MUST NOT wait for full resolution merely to compute conservative local reverse dependencies.

The inventory MUST retain workspace root, Cargo package ID, repository-relative manifest path, package name/version, members/exclusions, target kind/name/test status, normal/build/dev/target-specific dependencies, local path edges, declared features, required features, doctests, build scripts, toolchain/config inputs, and declared external files.

Per-workspace profile evidence and rules are fixed in the [task execution contract](task-execution-contract.md); the profile enters task and cache identities.

The local affected graph contains only first-party workspace packages and local path dependencies. Registry dependencies affect task identity but are not reverse-selection nodes.

## 5. Affected plan contract

CI planning MUST receive explicit base and head commit IDs. Local planning compares `HEAD` to the working tree and includes staged, unstaged, and relevant untracked files. The intended pull-request merge candidate is the input to CI selection. If base/head cannot be resolved, Velnor selects all packages and emits `reason = "comparison_unavailable"`; it MUST NOT return an empty plan.

Changes to a package select that package and every reverse-dependent local package. Manifest changes are evaluated against both base and head graphs so removed or renamed edges do not hide consumers. Changes to root Cargo config, `Cargo.lock`, Velnor-owned generator config, shared build inputs, or an unclassified file select all packages. Changes to `rust-toolchain.toml`, `mise.toml`, or `mise.lock` update inspection findings only; they do not invalidate task results because execution uses Velnor's exact pins. Documentation is relevant when included by rustdoc, tests, fixtures, or declared task inputs.

Plan JSON MUST use schema version 1 and contain:

```json
{
  "schema": 1,
  "run_key": "r123-a1",
  "plan_id": "plan-r123-a1",
  "base": "<commit-or-null>",
  "head": "<commit>",
  "event": "pull_request", "runner": {"label": "ubuntu-26.04", "selection": "latest_default"},
  "trust": "pr",
  "baseline": {
    "status": "used",
    "base_commit": "<40 lowercase hex>",
    "run_id": 12345,
    "artifact_id": "<exact artifact ID>",
    "manifest_digest": "b3-<64 lowercase hex>",
    "reason": null
  },
  "generator": {
    "version": "<exact-semver>",
    "target": "x86_64-unknown-linux-gnu",
    "sha256": "<64 lowercase hex characters>"
  },
  "packages": [{
    "package_id": "<Cargo package ID>",
    "name": "velnor-actions-contract",
    "manifest": "crates/velnor-actions-contract/Cargo.toml",
    "selected": false,
    "reasons": ["exact_baseline_identity"],
    "tasks": ["stack/rust/crates/velnor-actions-contract/clippy/default"]
  }],
  "obligations": [{
    "task_id": "stack/rust/crates/velnor-actions-contract/clippy/default",
    "decision": "covered_by_trusted_baseline",
    "reason": "exact_baseline_identity",
    "task_digest": "b3-<64 lowercase hex>",
    "input_digest": "b3-<64 lowercase hex>",
    "baseline_proof": {
      "source_commit": "<base commit>",
      "run_id": 12345,
      "proof_run_id": 12345,
      "manifest_digest": "b3-<64 lowercase hex>"
    }
  }],
  "matrix": {"include": []},
  "task_ids": ["<sorted unique IDs>"],
  "warnings": []
}
```

`run_key` is `r<github.run_id>-a<github.run_attempt>` in CI and `local` for
local planning. `plan_id` is `plan-<run-key>`. `matrix.include` MUST use the
exact entry shape and derived ID rules in [the workflow contract](workflow-contract.md).
The plan lists every current obligation; each matrix entry lists only its
`execute_task_ids`. Covered and cached obligations remain in `obligations` and
must carry their evidence reference. `packages` contains the complete Rust
inventory; `selected` is true only when that package has at least one execute
obligation. Packages, reasons, obligations, matrix
entries, and task IDs MUST be sorted deterministically. Unknown plan schema
fails. Each omitted package/task MUST have an explanation available to
internal explanation records. The plan written to its run-scoped location and
the matrix sent through `GITHUB_OUTPUT` MUST have the same canonical matrix bytes.

Generated workflow and Mise task ownership, task naming, rendering, and atomic write rules are specified in the [generated-file contract](generated-file-contract.md).

## 7. Acceptance table

| Input case | Command | Required result |
|---|---|---|
| `init` from a nested directory | `velnor-actions init` | Writes only root `.velnor/config.toml`; existing config is unchanged and reported as an error |
| Plan from a nested directory | `velnor-actions plan` | Reports the Git root and full crate inventory; writes no repository files |
| Plan/generate parity | `velnor-actions plan`, then `generate --output-dir <unique-temp-dir>` | Same detections, task IDs, runner, workflow files, jobs, steps, and matrix entries |
| Concise plan output | `velnor-actions plan` | Deterministic text lists workspaces, crates, planned checks, job groups, findings, and recommendations; no YAML/JSON |
| Standalone, nested, or multiple Rust workspaces | `velnor-actions generate` | Each Cargo root appears once in generated workflows; Cargo exclusions are respected |
| Repository with no registered stack | `velnor-actions generate` | Valid no-work workflow; no validation claim for unregistered stacks |
| Rust detected but ignored | `velnor-actions generate` | Detection remains internally recorded as ignored; no Rust tasks are emitted |
| Invalid/unknown config key | `velnor-actions generate` | Exit 1; key path on stderr; no output is replaced |
| Missing Cargo metadata dependency after offline preparation | `velnor-actions generate` | Exit 1; `preparation_incomplete`; no network fetch |
| Generated output and preview | `velnor-actions generate --output-dir <unique-temp-dir>` | Preview contains the complete `.github` tree; repository is byte-identical |
| Generation or validation failure | `velnor-actions generate` | Previous `.github` tree remains byte-identical |
| Successful default generation | `velnor-actions generate` | Entire root `.github` tree is replaced with generated output |
