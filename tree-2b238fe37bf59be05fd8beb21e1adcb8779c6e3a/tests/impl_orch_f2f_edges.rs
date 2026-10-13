//! F2F edge table: allowed intra-workspace edges per member package.

/// Contract-family edges per member directory.
fn expected_contract_family(dir: &str) -> Option<Vec<&str>> {
    match dir {
        "crates/core/velnor-actions-contract" => Some(vec![]),
        "crates/core/velnor-actions-contract-config" => Some(vec![
            "velnor-actions-contract",
            "velnor-actions-contract-release",
        ]),
        "crates/core/velnor-actions-contract-planning" => Some(vec![
            "velnor-actions-contract",
            "velnor-actions-contract-config",
        ]),
        "crates/core/velnor-actions-contract-release" => Some(vec!["velnor-actions-contract"]),
        "crates/core/velnor-actions-contract-workflow" => Some(vec![
            "velnor-actions-contract",
            "velnor-actions-contract-config",
            "velnor-actions-contract-planning",
            "velnor-actions-contract-release",
        ]),
        _ => None,
    }
}

/// Adapter edges per member directory.
fn expected_adapter(dir: &str) -> Option<Vec<&str>> {
    match dir {
        "crates/adapters/velnor-actions-actionlint" => Some(vec![
            "velnor-actions-contract",
            "velnor-actions-contract-config",
            "velnor-actions-contract-release",
        ]),
        "crates/adapters/velnor-actions-tofu" => Some(vec![
            "velnor-actions-contract",
            "velnor-actions-contract-planning",
            "velnor-actions-tofu-core",
        ]),
        "crates/adapters/velnor-actions-tofu-core" => Some(vec![
            "velnor-actions-contract",
            "velnor-actions-contract-config",
            "velnor-actions-contract-planning",
            "velnor-actions-contract-release",
        ]),
        "crates/adapters/velnor-actions-rust" => Some(vec![
            "velnor-actions-contract",
            "velnor-actions-contract-config",
            "velnor-actions-contract-planning",
            "velnor-actions-rust-core",
        ]),
        "crates/adapters/velnor-actions-rust-core" => Some(vec![
            "velnor-actions-contract",
            "velnor-actions-contract-planning",
        ]),
        _ => None,
    }
}

/// Mise-family edges per member directory.
fn expected_mise_family(dir: &str) -> Option<Vec<&str>> {
    match dir {
        "crates/adapters/velnor-actions-mise" => Some(vec![
            "velnor-actions-mise-cache",
            "velnor-actions-mise-catalog",
            "velnor-actions-mise-core",
            "velnor-actions-mise-nextest",
            "velnor-actions-mise-probes",
        ]),
        "crates/adapters/velnor-actions-mise-cache" => Some(vec![
            "velnor-actions-contract",
            "velnor-actions-contract-config",
            "velnor-actions-contract-workflow",
            "velnor-actions-mise-core",
        ]),
        "crates/adapters/velnor-actions-mise-catalog" => Some(vec![
            "velnor-actions-contract",
            "velnor-actions-contract-config",
            "velnor-actions-contract-planning",
            "velnor-actions-contract-release",
            "velnor-actions-mise-core",
        ]),
        "crates/adapters/velnor-actions-mise-core" => Some(vec![
            "velnor-actions-contract",
            "velnor-actions-contract-config",
            "velnor-actions-contract-planning",
        ]),
        "crates/adapters/velnor-actions-mise-probes" => Some(vec![
            "velnor-actions-contract",
            "velnor-actions-contract-config",
            "velnor-actions-mise-core",
        ]),
        "crates/adapters/velnor-actions-mise-nextest" => Some(vec![
            "velnor-actions-contract",
            "velnor-actions-contract-config",
            "velnor-actions-contract-release",
            "velnor-actions-mise-catalog",
            "velnor-actions-mise-core",
        ]),
        _ => None,
    }
}

/// Service/app edges per member directory.
fn expected_service(dir: &str) -> Option<Vec<&str>> {
    match dir {
        "crates/services/velnor-actions-orchestrator" => Some(vec![
            "velnor-actions-actionlint",
            "velnor-actions-contract",
            "velnor-actions-contract-config",
            "velnor-actions-contract-planning",
            "velnor-actions-contract-release",
            "velnor-actions-contract-workflow",
            "velnor-actions-mise",
            "velnor-actions-rust",
            "velnor-actions-rust-core",
            "velnor-actions-tofu",
            "velnor-actions-tofu-core",
            "velnor-actions-workflow-renderer",
            "velnor-actions-workflow-steps",
            "velnor-actions-workflow-tree",
        ]),
        "crates/services/velnor-actions-workflow-renderer" => Some(vec![
            "velnor-actions-contract",
            "velnor-actions-contract-config",
            "velnor-actions-contract-release",
            "velnor-actions-contract-workflow",
            "velnor-actions-workflow-steps",
            "velnor-actions-workflow-tree",
        ]),
        "crates/services/velnor-actions-workflow-steps" => Some(vec![
            "velnor-actions-contract",
            "velnor-actions-contract-workflow",
        ]),
        "crates/services/velnor-actions-workflow-tree" => Some(vec![
            "velnor-actions-contract",
            "velnor-actions-contract-release",
            "velnor-actions-contract-workflow",
            "velnor-actions-workflow-steps",
        ]),
        "crates/apps/velnor-actions-cli" => Some(vec!["velnor-actions-orchestrator"]),
        "crates/apps/velnor-actions-repo-policy" => Some(vec![]),
        _ => None,
    }
}

/// Allowed intra-workspace edges per member package.
pub(crate) fn expected_internal(dir: &str) -> Vec<&str> {
    expected_contract_family(dir)
        .or_else(|| expected_adapter(dir))
        .or_else(|| expected_mise_family(dir))
        .or_else(|| expected_service(dir))
        .unwrap_or_else(|| vec!["velnor-actions-contract"])
}
