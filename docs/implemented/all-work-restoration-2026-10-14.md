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
