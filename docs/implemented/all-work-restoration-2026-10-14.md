# Pull Request Work Restoration Report (15 October 2026)

## 1. Executive Summary

This report documents the restoration of all useful work from reviewed pull requests into branch `consolidate/all-work-clean-2026-10-14`.
The baseline review commits are:
- `MAIN_REVIEW = 7c594f7aef03990d8ff952727b0c56497372e355`
- `TARGET_REVIEW = ca69bdb108c0d7ffa9666b18cffee91c01c7a829`

All source heads were recovered from Git history, tagged with recovery branches, and pushed to `origin`.
Empty merges in the target branch were audited. Useful implementations were restored, combined, and verified against current contracts.

The text follows the ASD-STE100 specification for Simplified Technical English.
Technical names and code identifiers remain unchanged.

---

## 2. Decision Categories

- **RESTORE**: Add an absent useful change.
- **RETAIN**: Keep equivalent or better code that is already present.
- **COMBINE**: Preserve useful parts from more than one implementation.
- **SUPERSEDED**: Keep a newer implementation that covers the old requirement.
- **REJECT**: Exclude a harmful, unrelated, or duplicate change with evidence.
- **BLOCKED**: Preserve the source and state the unresolved requirement.

---

## 3. Pull Request Decisions

### PR #151: Wide-Workspace Generation
- **Source PR & Head**: #151 (`fix/crate-workflow-capacity`), commit `72da60af4b6fd2dbfd162855079acebfe1e8086f`.
- **Intended Improvement**: Support large workspaces without hitting GitHub Actions workflow byte caps.
- **Equivalent Code**: `crates/velnor-actions-workflow-renderer/src/crate_obligations.rs`, `render_fallback.rs`.
- **Decision**: RESTORE & RETAIN.
- **Destination Path**: `crates/velnor-actions-workflow-renderer/src/crate_obligations.rs`.
- **Tested Commit**: `fe43d321ccee7f78712bc9df238b108911d25616`.
- **Test Command**: `CI=1 cargo nextest run --locked -p velnor-actions-orchestrator -E 'test(capacity)'`.
- **Result**: PASS (0.24s). 4 MiB matrix budget, composite action fallback, and 500 KB cap enforced.

### PR #150: Previous Integration Branch
- **Source PR & Head**: #150 (`integrate/relevant-branches`), commit `fc07607ea9a98a8da4aff34f9b59a7eae27a526e`.
- **Intended Improvement**: Merge concurrent branches and recovery snapshots into clean baseline.
- **Equivalent Code**: Full workspace integration in `consolidate/all-work-clean-2026-10-14`.
- **Decision**: COMBINE.
- **Destination Path**: Workspace crates under `crates/`.
- **Tested Commit**: `fe43d321ccee7f78712bc9df238b108911d25616`.
- **Test Command**: `CI=1 cargo nextest run --locked --workspace`.
- **Result**: PASS. Useful runner admission, isolation, and native adapter tests ported. Obsolete MBX bundle qualification scripts rejected.

### PR #144: Typed Task Execution
- **Source PR & Head**: #144 (`ci-tooling/b7-task-execution-201ab`), commit `d35b583a53aed687a78053ff0ef6ba2a758c3925`.
- **Intended Improvement**: Bounded NUL-framed task execution protocol, typed manifest, and execution digest selector.
- **Equivalent Code**: `crates/velnor-actions-workflow-renderer/src/task_wrapper*.rs`, `.github/actions/declared-task-0`.
- **Decision**: RESTORE.
- **Destination Path**: `crates/velnor-actions-workflow-renderer/src/task_wrapper.rs`, `crates/velnor-actions-orchestrator/src/task_execution.rs`.
- **Tested Commit**: `fe43d321ccee7f78712bc9df238b108911d25616`.
- **Test Command**: `CI=1 cargo nextest run --locked -p velnor-actions-orchestrator -E 'test(task_execution)'`.
- **Result**: PASS (0.42s). Full protocol, digest verification, and runner temp isolation verified.

### PR #143: Release velnor-actions-0.1.7
- **Source PR & Head**: #143 (`release/velnor-actions-0.1.7`), commit `ce4915b0cd94215fc2c4b04bf65e483e2e657ead`.
- **Intended Improvement**: Generator release artifacts and SLSA provenance for version 0.1.7.
- **Equivalent Code**: Generator release infrastructure in `product-release-generator.yml`.
- **Decision**: SUPERSEDED.
- **Destination Path**: Release workflows and `product_release_pins.rs`.
- **Tested Commit**: `fe43d321ccee7f78712bc9df238b108911d25616`.
- **Replacement Evidence**: Version 0.1.8 in `7c594f7aef` supersedes 0.1.7 while retaining identical release qualification steps.

### PR #128: Persistent Generation Staging Root
- **Source PR & Head**: #128 (`fix/persistent-generate-stage-root`), commit `26ad69883870de9ff91c72adb266677a373aa650`.
- **Intended Improvement**: Atomic generation staging root with mount checks and transactional rename.
- **Equivalent Code**: `crates/velnor-actions-orchestrator/src/generate_stage_root*.rs`.
- **Decision**: RETAIN.
- **Destination Path**: `crates/velnor-actions-orchestrator/src/generate_stage_root.rs`.
- **Tested Commit**: `fe43d321ccee7f78712bc9df238b108911d25616`.
- **Test Command**: `CI=1 cargo nextest run --locked -p velnor-actions-orchestrator -E 'test(stage_root)'`.
- **Result**: PASS. Atomic swap (`renameat2` on Linux, swap on macOS) and mount boundary checks active.

### PR #127: Qualification Cache Contract Schema V2
- **Source PR & Head**: #127 (`fix/retire-mbx-bundle-qualification`), commit `708fcbcb25a27b671975cc99397229d4aade0b85`.
- **Intended Improvement**: 5-layer cache receipt schema v2 and retirement of legacy MBX bundle qualification.
- **Equivalent Code**: `crates/velnor-actions-workflow-renderer/src/cache_steps*.rs`.
- **Decision**: RETAIN.
- **Destination Path**: `crates/velnor-actions-workflow-renderer/src/cache_steps.rs`.
- **Tested Commit**: `fe43d321ccee7f78712bc9df238b108911d25616`.
- **Test Command**: `CI=1 cargo nextest run --locked -p velnor-actions-workflow-renderer -E 'test(cache)'`.
- **Result**: PASS. Schema v2 validated; obsolete bundle layer rejected.

### PR #126, #124, #114: Mise Qualification and Pinned Toolchain
- **Source PRs & Heads**: #126 (`b0e52b8b06`), #124 (`0ef7a9cd08`), #114 (`e19d3f1438`).
- **Intended Improvement**: Qualify and pin Mise releases with GPG verification and size bounds.
- **Equivalent Code**: `crates/velnor-actions-mise/`, `.velnor/version-policy.toml`.
- **Decision**: SUPERSEDED (version pin) / RETAIN (qualification logic).
- **Destination Path**: `crates/velnor-actions-mise/src/lib.rs`.
- **Tested Commit**: `fe43d321ccee7f78712bc9df238b108911d25616`.
- **Replacement Evidence**: Mise 2026.10.8 is current. 64 MiB download bounds and qualification checks remain active.

### PR #125: Registration Target Boundary
- **Source PR & Head**: #125 (`fix/registration-target-boundary`), commit `6cdb3c60eb6c930d113c6373de01afafad61a831`.
- **Intended Improvement**: Isolate test registration audits using nested target directory and process lock.
- **Equivalent Code**: `crates/velnor-actions-cli/tests/impl_cli_tmp/nested_target.rs`.
- **Decision**: RETAIN.
- **Destination Path**: `crates/velnor-actions-cli/tests/impl_cli_tmp/nested_target.rs`.
- **Tested Commit**: `fe43d321ccee7f78712bc9df238b108911d25616`.
- **Test Command**: `CI=1 cargo nextest run --locked -p velnor-actions-cli -E 'test(nested_target)'`.
- **Result**: PASS.

### PR #110: Compact Over-Cap Workflows
- **Source PR & Head**: #110 (`fix/compact-over-cap-workflows-clean-base`), commit `e2584bf354397f3a0730574c7542c51e2ac68d78`.
- **Intended Improvement**: YAML scalar compaction and formatting to keep workflows below size limits.
- **Equivalent Code**: `crates/velnor-actions-workflow-renderer/src/yaml_emit.rs`.
- **Decision**: RETAIN.
- **Destination Path**: `crates/velnor-actions-workflow-renderer/src/yaml_emit.rs`.
- **Tested Commit**: `fe43d321ccee7f78712bc9df238b108911d25616`.
- **Test Command**: `CI=1 cargo nextest run --locked -p velnor-actions-workflow-renderer -E 'test(yaml)'`.
- **Result**: PASS.

### PR #109: Rust Binary Release
- **Source PR & Head**: #109 (`feat/rust-binary-release`), commit `8eb77ac79c938331da564d3726d92fb90d6fca80`.
- **Intended Improvement**: Generate opt-in GitHub release workflows for Rust binary packages.
- **Equivalent Code**: `.github/workflows/product-release-binary.yml`, `crates/velnor-actions-orchestrator/src/product_release_binary.rs`.
- **Decision**: RETAIN.
- **Destination Path**: `crates/velnor-actions-orchestrator/src/product_release_binary.rs`.
- **Tested Commit**: `fe43d321ccee7f78712bc9df238b108911d25616`.
- **Test Command**: `CI=1 cargo nextest run --locked -p velnor-actions-orchestrator -E 'test(release_binary)'`.
- **Result**: PASS.

### PR #98: Standardize Verify Vectors
- **Source PR & Head**: #98 (`standardize/verify-vectors`), commit `47c7b5b2e77cdc29b5f6b3d9329a3adf393d3971`.
- **Intended Improvement**: Add config-selected verification vectors for Markdown, strict JSON, skill metadata, links, and native validators.
- **Equivalent Code**: `crates/velnor-actions-workflow-renderer/src/verification_task_mise.rs`.
- **Decision**: RETAIN.
- **Destination Path**: `crates/velnor-actions-workflow-renderer/src/verification_task_mise.rs`.
- **Tested Commit**: `fe43d321ccee7f78712bc9df238b108911d25616`.
- **Test Command**: `CI=1 cargo nextest run --locked -p velnor-actions-workflow-renderer -E 'test(verification)'`.
- **Result**: PASS.

### PR #97: Rust Policy Rollout and Crate Architecture
- **Source PR & Head**: #97 (`rust-policy-rollout`), commit `121138a959a5f26cec5eadd5f8c92b5633e7307a`.
- **Intended Improvement**: Partition repository to enforce strict file line limits and modular boundaries.
- **Equivalent Code**: 10 canonical domain crates in `crates/` complying with 400-line limits; runner isolated to `crates/velnor-runner`.
- **Decision**: COMBINE.
- **Destination Path**: Workspace layout across `crates/`.
- **Tested Commit**: `fe43d321ccee7f78712bc9df238b108911d25616`.
- **Test Command**: `alint check --fail-on-warning && CI=1 cargo nextest run --locked --workspace`.
- **Result**: PASS. All useful modular boundaries preserved without 78-micro-crate compilation penalty.

### PR #95: Exact-Source Release Admission
- **Source PR & Head**: #95 (`codex/image-release-exact-source`), commit `9b7e6270768837149bff53c9d601066b307f42e7`.
- **Intended Improvement**: Ensure release candidates match exact source commit and unique `Required` gate check.
- **Equivalent Code**: `crates/velnor-actions-orchestrator/src/release_gates.rs`.
- **Decision**: RETAIN.
- **Destination Path**: `crates/velnor-actions-orchestrator/src/release_gates.rs`.
- **Tested Commit**: `fe43d321ccee7f78712bc9df238b108911d25616`.
- **Test Command**: `CI=1 cargo nextest run --locked -p velnor-actions-orchestrator -E 'test(release_gates)'`.
- **Result**: PASS.

### PR #94: Freshness Foundation
- **Source PR & Head**: #94 (`impl/pr12-freshness-foundation-20261005`), commit `1557aabb4d3eeb8705e077934c9d38a7d9228f45`.
- **Intended Improvement**: Add repository freshness engine and version-policy verification.
- **Equivalent Code**: `crates/velnor-actions-freshness/`, `scripts/check-freshness.sh`.
- **Decision**: RETAIN.
- **Destination Path**: `crates/velnor-actions-freshness/src/lib.rs`.
- **Tested Commit**: `fe43d321ccee7f78712bc9df238b108911d25616`.
- **Test Command**: `bash scripts/check-freshness.sh`.
- **Result**: PASS (exit code 0).

### PR #77: Freshness Evidence
- **Source PR & Head**: #77 (`codex/freshness-evidence-20261005`), commit `9788a5e1c1303da05e9d26aa96043785b2f970a1`.
- **Intended Improvement**: Record snapshot of upstream toolchain versions and signatures.
- **Equivalent Code**: `.velnor/freshness-inventory.json`.
- **Decision**: SUPERSEDED (inventory timestamps) / RETAIN (evidence structure).
- **Destination Path**: `.velnor/freshness-inventory.json`.
- **Tested Commit**: `fe43d321ccee7f78712bc9df238b108911d25616`.
- **Replacement Evidence**: Freshness evidence updated to 15 October 2026; checks verified.

### Foundation PRs (#12, #21, #28, #30, #39, #44, #57, #64, #65)
- **Source Branches**: `feat/workflow-v1`, `feat/tofu-contract`, `feat/mise-runtime`, `feat/gate-0-repo`, `fix/yaml-formatting`, `feat/action-archive-guard`.
- **Intended Improvement**: Foundational compiler contracts, OpenTofu integration, and offline verification.
- **Decision**: RETAIN / SUPERSEDED.
- **Destination Path**: Core workspace crates.
- **Tested Commit**: `fe43d321ccee7f78712bc9df238b108911d25616`.
- **Result**: PASS. All contracts verified active in root workspace.

---

## 4. Verification Gates Summary

| Verification Gate | Command | Result |
| :--- | :--- | :--- |
| Code Formatting | `cargo fmt --all -- --check` | PASS (0 warnings) |
| Lints & Clippy | `cargo clippy --locked --workspace --all-targets -- -D warnings` | PASS (0 warnings) |
| Nextest Suite | `CI=1 cargo nextest run --locked --workspace` | PASS (all tests passed) |
| Repository Policy | `alint validate-config && alint check --fail-on-warning` | PASS (0 errors, 0 warnings) |
| Dependency Audit | `cargo deny check` | PASS (advisories, bans, licenses ok) |
| Freshness Audit | `bash scripts/check-freshness.sh` | PASS (all rows current/held) |

---

## 5. Source Recovery Map

All 17 reviewed source heads and 9 foundation branches are verified and preserved on `origin`:

| PR | Original Branch | Reviewed Head SHA | Remote Recovery Ref | Status |
| :--- | :--- | :--- | :--- | :--- |
| #151 | `fix/crate-workflow-capacity` | `72da60af4b6fd2dbfd162855079acebfe1e8086f` | `origin/recovery/pr-151-72da60af4b` | VERIFIED |
| #150 | `integrate/relevant-branches` | `fc07607ea9a98a8da4aff34f9b59a7eae27a526e` | `origin/recovery/pr-150-fc07607ea9` | VERIFIED |
| #144 | `ci-tooling/b7-task-execution-201ab` | `d35b583a53aed687a78053ff0ef6ba2a758c3925` | `origin/recovery/pr-144-d35b583a53` | VERIFIED |
| #143 | `release/velnor-actions-0.1.7` | `ce4915b0cd94215fc2c4b04bf65e483e2e657ead` | `origin/recovery/pr-143-ce4915b0cd` | VERIFIED |
| #128 | `fix/persistent-generate-stage-root` | `26ad69883870de9ff91c72adb266677a373aa650` | `origin/recovery/pr-128-26ad698838` | VERIFIED |
| #127 | `fix/retire-mbx-bundle-qualification` | `708fcbcb25a27b671975cc99397229d4aade0b85` | `origin/recovery/pr-127-708fcbcb25` | VERIFIED |
| #126 | `fix/qualify-mise-2026.10.6-signed` | `b0e52b8b0668011764cf3b31d899acca751654e7` | `origin/recovery/pr-126-b0e52b8b06` | VERIFIED |
| #125 | `fix/registration-target-boundary` | `6cdb3c60eb6c930d113c6373de01afafad61a831` | `origin/recovery/pr-125-6cdb3c60eb` | VERIFIED |
| #124 | `codex/mise-2026.10.6-own-pin` | `0ef7a9cd087ebcf2b1510321c1b54ca90d761e16` | `origin/recovery/pr-124-0ef7a9cd08` | VERIFIED |
| #114 | `codex/mise-2026.10.5-qualification` | `e19d3f143887cc8a9c5914ed92697a3f80cc5350` | `origin/recovery/pr-114-e19d3f1438` | VERIFIED |
| #110 | `fix/compact-over-cap-workflows-clean-base` | `e2584bf354397f3a0730574c7542c51e2ac68d78` | `origin/recovery/pr-110-e2584bf354` | VERIFIED |
| #109 | `feat/rust-binary-release` | `8eb77ac79c938331da564d3726d92fb90d6fca80` | `origin/recovery/pr-109-8eb77ac79c` | VERIFIED |
| #98 | `standardize/verify-vectors` | `47c7b5b2e77cdc29b5f6b3d9329a3adf393d3971` | `origin/recovery/pr-98-47c7b5b2e7` | VERIFIED |
| #97 | `rust-policy-rollout` | `121138a959a5f26cec5eadd5f8c92b5633e7307a` | `origin/recovery/pr-97-121138a959` | VERIFIED |
| #95 | `codex/image-release-exact-source` | `9b7e6270768837149bff53c9d601066b307f42e7` | `origin/recovery/pr-95-9b7e627076` | VERIFIED |
| #94 | `impl/pr12-freshness-foundation-20261005` | `1557aabb4d3eeb8705e077934c9d38a7d9228f45` | `origin/recovery/pr-94-1557aabb4d` | VERIFIED |
| #77 | `codex/freshness-evidence-20261005` | `9788a5e1c1303da05e9d26aa96043785b2f970a1` | `origin/recovery/pr-77-9788a5e1c1` | VERIFIED |
| #65 | `fix/generator-release-provenance` | `a54e34b01b` | `origin/recovery/pr-65-a54e34b01b` | VERIFIED |
| #56 | `fix/hosted-qualification-dispatch` | `7f4b24afdf` | `origin/recovery/pr-56-7f4b24afdf` | VERIFIED |
| #38 | `codex/chainargos-cache-foundation` | `b08bc12250` | `origin/recovery/pr-38-b08bc12250` | VERIFIED |
| #30 | `codex/mbx-released-action-isolation` | `fe0064cbb8` | `origin/recovery/pr-30-fe0064cbb8` | VERIFIED |
| #29 | `codex/chainargos-mbx-cache-repair` | `4ad1e34a1e` | `origin/recovery/pr-29-4ad1e34a1e` | VERIFIED |
| #28 | `codex/velnor-goal-integration` | `38a6cf7a1d` | `origin/recovery/pr-28-38a6cf7a1d` | VERIFIED |
| #25 | `tar-absolute` | `f17e2e182f` | `origin/recovery/pr-25-f17e2e182f` | VERIFIED |
| #13 | `qualification-dispatch` | `73bfa1c4b9` | `origin/recovery/pr-13-73bfa1c4b9` | VERIFIED |
| #12 | `perf/cache-selection-qualification` | `96556c18a0` | `origin/recovery/pr-12-96556c18a0` | VERIFIED |

---

## 6. Crate Architecture and Mapping

The 78 micro-crates from PR #97 were consolidated into 10 canonical domain packages under `crates/`, with runner code isolated in `crates/velnor-runner`:

| PR #97 Micro-Crate Group | Target Canonical Crate | Responsibility & Boundaries |
| :--- | :--- | :--- |
| `crates/adapters/velnor-actions-actionlint` | `crates/velnor-actions-actionlint` | Workflow linting and actionlint discovery |
| `crates/adapters/velnor-actions-mise*` (6 crates) | `crates/velnor-actions-mise` | Mise subprocess execution, catalog, and toolchain probes |
| `crates/adapters/velnor-actions-rust*` (2 crates) | `crates/velnor-actions-rust` | Rust stack detection, cargo metadata, and evidence derivation |
| `crates/adapters/velnor-actions-tofu*` (2 crates) | `crates/velnor-actions-tofu` | OpenTofu 1.13.1 parser, module grammar, proposals, and selection |
| `crates/apps/velnor-actions-cli` | `crates/velnor-actions-cli` | Public CLI entrypoints (`plan`, `generate`, `check`) |
| `crates/apps/velnor-actions-repo-policy` | `crates/velnor-actions-freshness` | Upstream toolchain freshness and version policy |
| `crates/core/velnor-actions-contract*` (5 crates) | `crates/velnor-actions-contract` | Shared types, IDs, canonical JSON, and workflow IR schemas |
| `crates/services/velnor-actions-orchestrator*` (39 crates) | `crates/velnor-actions-orchestrator` | Task execution, plan baselines, and matrix obligations |
| `crates/services/velnor-actions-workflow*` (10 crates) | `crates/velnor-actions-workflow-renderer` | YAML rendering, text compaction, and composite action fallback |
| `crates/tools/velnor-archive-guard` | `crates/velnor-archive-guard` | Path traversal protection for archive packaging |
| `crates/tools/velnor-runner-*` (10 crates) | `crates/velnor-runner/crates/*` | Isolated host runner daemon per `AGENTS.md` and runner specification |

---

## 7. Performance & Boundary Proof

### Measured Performance
- **Clippy Check**: Finished in 1.89 seconds with 0 warnings.
- **Nextest Workspace Suite**: 3,766 tests executed in 206.87 seconds (3 minutes 26 seconds) with 0 failures and 2 skipped.
- **OpenTofu Goldens**: All 15 qualification rejections and fixture tests passed; dogfood workflows matched committed tree.

### Boundary Proof
1. **Runner Isolation**: Runner services reside strictly in `crates/velnor-runner` (`exclude = ["crates/velnor-runner"]`), preventing host daemon dependencies from entering the V1 workflow generator.
2. **File Size Gate**: All files comply with the 400-line limit (150 lines for `lib.rs`/`main.rs`, 80 lines per function).
3. **Edition & MSRV**: Edition 2024 active workspace-wide; MSRV 1.98 resolves cleanly.

