# Native adapter ownership migration

Status: architecture extension authorized by the goal coordinator on 2026-10-03; proposed implementation migration. Independent `ownership_boundary` review confirms the direction and the source-bound helper constraints below. No production source is moved by this document; shared compilation checkpoint remains frozen.

## Contract amendment

Replace the fixed seven/eight crate limits in `docs/proposed/architecture.md` and `docs/proposed/workflow-contract.md` with:

> V1 remains a workflow generator. `plan` and `generate` derive workflows from repository evidence; V1 is not a runner, interpreter, broker or distributed cache service. Common packages are contract, Mise, actionlint, workflow renderer, orchestrator and CLI. Rust and OpenTofu remain named stack packages. `velnor-actions-native` is a closed container of explicit isolated swift/node/java/oci/apt/homebrew/ruby/shell/reuse domain owners. Each exposes a typed facade and depends only on contract among product packages. Sibling references, reexports, shared semantic dispatchers, cross-domain includes/macros and helper semantics are forbidden and checked by a mandatory source boundary gate. Unrestricted native/delivery/application/utility catchalls and raw command/YAML configuration remain forbidden.

Independent `ownership_integration/boundary_review` approved this packaging amendment on 2026-10-03. Cargo packages enforce external edges; mandatory module gates enforce equivalent internal separation. One package per domain adds no required ownership property once these boundaries are enforced. This amendment does not approve current renderer/orchestrator leaks or claim extraction complete.

Retain the dependency direction:

```text
cli -> orchestrator
orchestrator -> contract, each registered stack adapter, mise, actionlint, renderer
each stack adapter -> contract
native -> contract
mise -> contract
actionlint -> contract
renderer -> contract
```

Stack adapters do not depend on each other or on Mise/renderer. An adapter requests a prerequisite through typed contract records; orchestrator resolves its owning adapter and composes execution. A Swift FFI obligation, for example, requests a Rust producer by validated package/features/target/profile; it does not construct Cargo/Mise commands itself or import the Rust crate.

## Ownership assignments

| Owner | Required audited semantics | Extraction source |
|---|---|---|
| `velnor-actions-rust` | Cargo/Rust discovery, compilation/FFI producer proposals, dependency policies, package/source/registry reconciliation | Rust-owned portions of workload security/desktop producers; renderer `release_source_validation.py`, `release_reconcile_*.py` and their source bundling |
| `velnor-actions-native::node` | Bun/npm package-manager inputs, workspace/scripts, public package source/cache qualification | `workloads_package*`, node/Bun cache eligibility and sanitizer policy |
| `velnor-actions-native::java` | Gradle/Maven detection, wrapper identity, module/database task semantics, native dependency/output qualification | `workloads_gradle*`, Gradle source/output cache policy |
| `velnor-actions-native::swift` | Swift package/Xcode project, native app packaging/signing/verification, Apple platform inputs | Desktop/Swift workload policy and renderer desktop policy/source helpers |
| `velnor-actions-native::oci` | Docker build contexts/platforms, OCI immutable tags/digests/index/provenance and BuildKit owner requirements | Workload Docker transport policy; renderer `oci_*` source helpers/policy |
| `velnor-actions-native::apt` | Debian/ELF packaging, GPG identity, APT indexes/feed admission/rollback/publication requirements | Renderer `delivery_apt_*.py` semantic helpers and closed policy |
| `velnor-actions-native::homebrew` | Formula/cask validation and source-bound package-update lifecycle fixtures | `workloads_homebrew*`, `workloads_tap*`, audited tap policy |
| `velnor-actions-native::ruby` | Explicit Ruby source syntax obligations | Closed Ruby workload semantic proposal |
| `velnor-actions-native::shell` | Explicit shell source validation obligations | Closed Shellcheck workload semantic proposal |
| `velnor-actions-native::reuse` | REUSE license metadata/input and validation obligations | Closed REUSE workload semantic proposal |
| `velnor-actions-mise` | Exact compiled tool profiles, isolated install/exec, fixed subprocess wrapper, generic source-bound helper preparation and execution argv | Renderer `delivery_tools` Mise construction; Rust preparation envelope decoder; catalog and environment authority remain here |
| `velnor-actions-orchestrator` | Adapter composition, exact source/trust/proof admission, selection, prerequisite/resource-safe job topology, generic IR and output writes | Family workflow policy currently in renderer; no moved Cargo/native parser or shell builder |
| `velnor-actions-workflow-renderer` | Validated common workflow IR serialization, generic GitHub action syntax and argument quoting | Remove direct family config APIs, policy helpers and stack/tool command inference |
| `velnor-actions-contract` | Stack-neutral request/result, workflow/action/cache/proof and source-bound helper descriptor shapes | No Cargo/Gradle/Swift/APT interpretation or helper source bodies |

The additional adapter count is evidence-driven, not a new fixed numerical gate. Ruby, shell, REUSE and Homebrew already have substantive retained obligations in this scope; removing their ownership merely to reduce the number of crates is not permitted. Alint remains the sole structure linter; generic repository-policy composition does not create another structure engine.

## Closed source-bound helper boundary

The coordinator approved a generic `SourceBoundHelper` descriptor and fixed auxiliary script lowering, **not** an internal runner-like PrepareTools operation. Adapter-owned fixed helper source remains part of the qualified generator source. Mise owns its fixed command/launcher construction and strict canonical argv validation; renderer does not decode or reconstruct Mise syntax.

1. Only registered compiled owners produce a helper descriptor. It binds a normalized fixed owned output path, exact source bytes/digest, executable/tool role, schema, declared input/output/credential policy and closed argument schema. Repository config cannot provide arbitrary helper source, digest, path, argv or interpreter. Plain JSON/hash agreement never grants authority.
2. Orchestrator accepts owner-produced records, checks helper/path collisions, emits exact support files and composes typed steps. Conflicting same-path helper bodies fail; identical shared helper records deduplicate deterministically.
3. Mise constructs the fixed launcher from the qualified source descriptor. Before executing, validate canonical root/path containment and all ancestors, regular-file/link restrictions, exact compiled source digest and expected interpreter identity. Protect verification-to-execution against mutable repository code. Linux-only `/usr/bin/sha256sum` must not be assumed on macOS; use a source-qualified supported verifier on every emitted runner class.
4. Renderer serializes the prepared generic argv/environment. No exception matching shell text, script-name guessing, Python-prefix stripping, selector parsing or alternate pin catalog. No public raw shell/YAML field is introduced.

This is generation and fixed adapter execution integration, not another workflow/task engine. MBX bundle semantics remain exclusively owned by MBX; native output reuse remains owned by the native tool. Source verification does not manufacture complete task-input or public cache eligibility proof.

## Migration sequence and acceptance

1. Freeze the current integrated compilation checkpoint; record source and deterministic gate results. Then apply the ownership contract amendment with the migration ledger.
2. Introduce closed contract request/results and registered owner factories. Register explicit domain modules as each audited family moves; enforce module isolation and contract-only product dependencies. Preserve edition, MSRV, pure workspace lints, exact locked dependencies and file/function limits.
3. Move complete semantic families, tests and fixed support sources to their assigned adapters. Move installation/launcher construction to Mise. Build common workflow IR in orchestrator. Delete old renderer implementations, exports, token decoders, duplicate catalogs and compatibility paths immediately after each family is integrated.
4. Regenerate deterministic fixture output once owners are stable. Compare full obligations, prerequisites, permissions, environments, source/tool identities, failure reports and release protection against the audited contracts. Correct changes must not disappear behind byte-equal old goldens.
5. Independent reviewers inspect actual APIs, dependency graph, generated source and negative cases. Two differently named native profiles must work without repository names in generator policy; non-main default branch, altered helper, unsafe path/link, injected arguments, unavailable verifier and failed producer must fail safely.
6. Run all authorized deterministic gates, then hosted cold/warm/third-run and applicable change/negative experiments. Publish and roll out only the source-bound qualified generator. No production benchmark publish/deploy and no performance claim from extraction alone.

Required completion evidence: adapter-to-contract dependency graph; no stack/Mise/native policy in renderer; no orchestration shell construction; owner-qualified support source/argv; no legacy aliases; full retained CI/CD parity; independent final-head review; applicable T01–T26 raw evidence. Until these exist, ARCH-01/02/06 remain open.
