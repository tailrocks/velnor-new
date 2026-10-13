//! Expected workspace member directories (package name is the leaf).

/// Expected member directories (package name is the leaf).
pub(crate) const MEMBERS: [&str; 25] = [
    "crates/adapters/velnor-actions-actionlint",
    "crates/apps/velnor-actions-cli",
    "crates/apps/velnor-actions-repo-policy",
    "crates/core/velnor-actions-contract",
    "crates/core/velnor-actions-contract-config",
    "crates/core/velnor-actions-contract-planning",
    "crates/core/velnor-actions-contract-release",
    "crates/core/velnor-actions-contract-workflow",
    "crates/adapters/velnor-actions-mise",
    "crates/adapters/velnor-actions-mise-cache",
    "crates/adapters/velnor-actions-mise-catalog",
    "crates/adapters/velnor-actions-mise-core",
    "crates/adapters/velnor-actions-mise-nextest",
    "crates/adapters/velnor-actions-mise-probes",
    "crates/services/velnor-actions-orchestrator",
    "crates/adapters/velnor-actions-rust",
    "crates/adapters/velnor-actions-rust-core",
    "crates/adapters/velnor-actions-tofu",
    "crates/adapters/velnor-actions-tofu-core",
    "crates/services/velnor-actions-workflow-cache",
    "crates/services/velnor-actions-workflow-generator",
    "crates/services/velnor-actions-workflow-release",
    "crates/services/velnor-actions-workflow-renderer",
    "crates/services/velnor-actions-workflow-steps",
    "crates/services/velnor-actions-workflow-tree",
];

/// Package name for a member dir (the leaf segment).
pub(crate) fn package(dir: &str) -> &str {
    dir.rsplit('/').next().unwrap_or("")
}
