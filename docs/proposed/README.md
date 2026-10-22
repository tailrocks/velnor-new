# Proposed implementation: Velnor V1

**Status:** Implemented specifications and contracts.

Velnor Actions is a stack-generic GitHub Actions workflow generator. The CLI
auto-detects every registered project stack. V1 registers Rust/Cargo and
OpenTofu detectors, so unregistered stacks receive no validation claim. Future
TypeScript and Bun detectors add coverage under the same command tree.
Configuration is in `.velnor/config.toml`: `[stacks].ignore` disables exact
registered stack IDs, `[stacks.<name>]` holds stack-specific settings, and
`[discovery].exclude` holds repository path globs. These are not CLI flags.

## Responsibility boundaries

| Component | Owns | Does not own |
|---|---|---|
| Rust adapter | Cargo inventory, local package graph, affected Rust task proposals | Mise config/commands, workflow YAML, process execution |
| Mise adapter | Tool selection, pinned command/environment construction, tool-file inspection, task-result cache integration | Rust package graph or GitHub YAML |
| Workflow-lint adapter (`velnor-actions-actionlint`) | Pinned jactionlint capability, generated `.github/jactionlint.yaml`, workflow-lint validation | Mise execution, Rust scanning, generic workflow rendering |
| Orchestrator | Combines stack proposals, Mise execution, selection, cache evidence, scheduling, and outputs | Stack/tool parsing or rendering implementation |
| Workflow renderer | Generic GitHub Actions YAML | Stack and Mise semantics |
| CLI | Clap arguments, typed dispatch, output and exit codes | Orchestration and stack/tool rules |
| MBX | Reusable Rust compilation work and compiler resource coordination | Test pass/fail results |
| GitHub Actions | Scheduling generated jobs and reporting required check status | Velnor's project discovery and cache validity rules |

`generate` replaces `.github/` as one complete tree. `init` separately creates
the user-owned `.velnor/config.toml` sample. The path is
`GitHub step → pinned tools → Mise command → stack task`; Velnor Actions scans
the repo and renders that workflow. `plan` shows a text summary; `generate
--output-dir PATH` writes the same `.github/` tree to a caller-selected fresh
preview directory.

The public command surface is deliberately small:

```text
velnor-actions init
velnor-actions plan
velnor-actions generate [--output-dir PATH]
```

`plan` prints a concise, human-readable summary from the same analysis and
workflow rendering path as `generate`. It prints no YAML and writes no files.

V1 contains eight purpose-scoped generator packages: `velnor-actions-contract`,
`velnor-actions-rust`, `velnor-actions-tofu`, `velnor-actions-mise`,
`velnor-actions-actionlint`, `velnor-actions-workflow-renderer`,
`velnor-actions-orchestrator`, and `velnor-actions-cli`, plus repository maintenance
packages `velnor-actions-freshness` and `velnor-archive-guard`. Rust, OpenTofu, and Mise
are separate crates; only the orchestrator composes them. The CLI package declares the
`velnor-actions` binary. Generic package names such as `velnor-model`, `velnor-core`,
and `velnor-rust` are forbidden.

The generator crates MUST NOT implement a runner, a Scale Set client, or an Actions interpreter. That work lives in the nested `crates/velnor-runner` workspace under the [macOS Scale Set runner](macos-scaleset-runner.md) specification. Kubernetes, a web server, a TUI, a remote cache service, and a deployment system remain out of scope. Generated workflows MAY use a small allowlist of full-SHA-pinned setup actions; Velnor does not execute arbitrary `uses:` actions itself.

## Normative specifications

1. [Architecture](architecture.md) defines workspace crates, configuration, discovery, data contracts, and generated-file ownership.
2. [Bootstrap and release contract](bootstrap-and-release-contract.md) defines consumer bootstrap assets, the compiled version catalog, Velnor's bootstrap lock, and release promotion.
3. [Generated-file contract](generated-file-contract.md) defines task names, file ownership, safe rendering, and atomic output replacement.
4. [CLI contract](cli-contract.md) fixes the stack-generic `velnor-actions` Clap command tree (`init`, `plan`, and `generate`), automatic detection, TOML ignore settings, and V1 Rust behavior.
5. [Workflow contract](workflow-contract.md) defines triggers, jobs, and matrix data.
6. [Task execution contract](task-execution-contract.md) defines task steps, Mise invocation, execution, and reports.
7. [Cache and report contract](cache-contract.md) defines task identities, cache ownership/trust, reports, and final status aggregation.
   See also [cache decisions](cache-decisions.md): the owner-adopted cache redesign (producer-scoped saves, dependency seed, opaque MBX bundles, family pruning, PR-scoped sources, no ImageVersion). Family specs, keys, writers and size ceilings: [cache families](cache-families.md).
8. [Host-image seed admission contract](host-seed-contract.md) defines trusted preinstalled seed inputs and fail-cold admission.
9. [Parallelism and affected-work contract](parallelism-and-selection-contract.md) defines dependency-aware selection, not-affected crate jobs (hard decision 4), native background/wait steps, bounded concurrency, and complete test partitioning.
10. [Rust quality contract](rust-quality-contract.md) defines crate ownership, lints, file limits, dependencies, and repository-policy protection.
11. [Agent and performance contract](agent-and-performance-contract.md) defines agent instructions, performance measurements, acceptance budgets, and readiness evidence.
12. [Tooling input contract](tooling-input-contract.md) defines read-only inspection and human recommendations for Rust/Mise tool files.
13. [Version policy](version-policy.md) defines latest-stable tool pins, freshness checks, exception expiry, and V2 version inventory.
14. [Implementation plan](implementation-plan.md) defines the ordered work packages and merge gates.
15. [Release contract](release-contract.md) defines consumer release-plz preparation, validation, protected publishing, and reconciliation.
16. [Release config schema](release-config-schema.md) defines the exact `[stacks.rust.release]` fields, defaults, and validation errors.
17. [Release coverage](release-coverage.md) maps release behaviors to their tests and snapshots.
18. [OpenTofu contract](opentofu-contract.md) defines the native `tofu` stack extension, tracked in `../reviews/opentofu-evidence.md`.
19. [Rust dependency policy](rust-dependency-policy.md) defines dependency narrowness, the initial crate set, and deny/machete supply-chain gates (companion to the quality contract §7).
20. [Rust test policy](rust-test-policy.md) defines test layout, integration entry points, doctest separation, fixture independence, and snapshot review (companion to the quality contract §4).
21. [Rust verification contract](rust-verification-contract.md) defines the required focused task templates, workflow verification, and risk-triggered checks (binding extension of the quality contract §9).
22. [macOS Scale Set runner](macos-scaleset-runner.md) is the active runner specification. It supersedes conflicting deferred roadmap clauses.
23. [Resource probe image artifact](resource-probe-artifact.md) defines the trusted probe image, archive, manifest, and publication contract.
24. [Resource probe controller](resource-probe-controller.md) defines controller-side guest probe execution and parsing.
25. [Runner resource admission](runner-resource-admission.md) defines candidate job admission and resource enforcement.
26. [GitHub Actions reference](github-actions-reference.md) defines source repository requirements and fidelity contracts.
27. [Image publication contract](image-publication-contract.md) defines `[[images]]`, Dockerfile pinning rules, and the build, publish and promote jobs.
28. [Private sources contract](private-sources-contract.md) defines private cross-repository dependency credentials (HD-7): the `[private_sources]` config, `.velnor/deps.lock`, the mint, fetch and revoke steps, and the credential boundary.
29. [Bun contract](bun-contract.md) defines the Bun stack (hard decision 6): `[stacks.bun]`, the `bun.lock`-only policy, workspace members, the Velnor-written `bunfig.toml`, affected-member claims, Playwright browsers and npm publication.
30. [dbt contract](dbt-contract.md) defines the dbt-fusion stack: `[stacks.dbt]`, the `X.Y.Z-preview.N` or stable `X.Y.Z` version grammar (HD-4a), the package-lock and profile policies, artifact admission and affected-project claims.
31. [Ansible contract](ansible-contract.md) defines the Ansible stack: `[stacks.ansible]`, hash-pinned Python requirements, offline collection pins, playbook discovery, vault refusal and affected-root claims; host deployment stays operator-run (HD-9).
32. [Workflow lint contract](workflow-lint-contract.md) defines jactionlint as the only workflow linter (hard decision 10): `[workflow.lint] profile`, the generated `.github/jactionlint.yaml`, the shared `hk` file set and flags, executable digest verification and the check-only `hk` step.

The listed documents are intended to be implementable without inventing behavior. If a behavior is not specified, fail with a clear error and add a decision to the spec before implementing it. Do not silently broaden V1.
