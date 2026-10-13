//! Intra-workspace dependency edges allowed per member package.

/// Contract-family edges by leaf dir name.
fn expected_contract_family(leaf: &str) -> Option<Vec<&str>> {
    match leaf {
        "velnor-actions-contract" => Some(vec![]),
        "velnor-actions-contract-config" => Some(vec![
            "velnor-actions-contract",
            "velnor-actions-contract-release",
        ]),
        "velnor-actions-contract-planning" => Some(vec![
            "velnor-actions-contract",
            "velnor-actions-contract-config",
        ]),
        "velnor-actions-contract-release" => Some(vec!["velnor-actions-contract"]),
        "velnor-actions-contract-workflow" => Some(vec![
            "velnor-actions-contract",
            "velnor-actions-contract-config",
            "velnor-actions-contract-planning",
            "velnor-actions-contract-release",
        ]),
        _ => None,
    }
}

/// Adapter edges by leaf dir name.
fn expected_adapter(leaf: &str) -> Option<Vec<&str>> {
    match leaf {
        "velnor-actions-actionlint" => Some(vec![
            "velnor-actions-contract",
            "velnor-actions-contract-config",
            "velnor-actions-contract-release",
        ]),
        "velnor-actions-tofu" => Some(vec![
            "velnor-actions-contract",
            "velnor-actions-contract-planning",
            "velnor-actions-tofu-core",
        ]),
        "velnor-actions-tofu-core" => Some(vec![
            "velnor-actions-contract",
            "velnor-actions-contract-config",
            "velnor-actions-contract-planning",
            "velnor-actions-contract-release",
        ]),
        "velnor-actions-rust" => Some(vec![
            "velnor-actions-contract",
            "velnor-actions-contract-config",
            "velnor-actions-contract-planning",
            "velnor-actions-rust-core",
        ]),
        "velnor-actions-rust-core" => Some(vec![
            "velnor-actions-contract",
            "velnor-actions-contract-planning",
        ]),
        _ => None,
    }
}

/// Mise-family edges by leaf dir name.
fn expected_mise_family(leaf: &str) -> Option<Vec<&str>> {
    match leaf {
        "velnor-actions-mise-cache" => Some(vec![
            "velnor-actions-contract",
            "velnor-actions-contract-config",
            "velnor-actions-contract-workflow",
            "velnor-actions-mise-core",
        ]),
        "velnor-actions-mise-catalog" => Some(vec![
            "velnor-actions-contract",
            "velnor-actions-contract-config",
            "velnor-actions-contract-planning",
            "velnor-actions-contract-release",
            "velnor-actions-mise-core",
        ]),
        "velnor-actions-mise-core" => Some(vec![
            "velnor-actions-contract",
            "velnor-actions-contract-config",
            "velnor-actions-contract-planning",
        ]),
        "velnor-actions-mise-nextest" => Some(vec![
            "velnor-actions-contract",
            "velnor-actions-contract-config",
            "velnor-actions-contract-release",
            "velnor-actions-mise-core",
            "velnor-actions-mise-catalog",
        ]),
        "velnor-actions-mise-probes" => Some(vec![
            "velnor-actions-contract",
            "velnor-actions-contract-config",
            "velnor-actions-mise-core",
        ]),
        "velnor-actions-mise" => Some(vec![
            "velnor-actions-mise-cache",
            "velnor-actions-mise-catalog",
            "velnor-actions-mise-core",
            "velnor-actions-mise-nextest",
            "velnor-actions-mise-probes",
        ]),
        _ => None,
    }
}

/// Service/app edges by leaf dir name.
fn expected_service(leaf: &str) -> Option<Vec<&str>> {
    match leaf {
        "velnor-actions-orchestrator" => Some(vec![
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
        ]),
        "velnor-actions-workflow-renderer" => Some(vec![
            "velnor-actions-contract",
            "velnor-actions-contract-config",
            "velnor-actions-contract-release",
            "velnor-actions-contract-workflow",
        ]),
        "velnor-actions-cli" => Some(vec!["velnor-actions-orchestrator"]),
        "velnor-actions-repo-policy" => Some(vec![]),
        _ => None,
    }
}

/// Allowed `velnor-actions-*` dependencies per member (matched on leaf dir name).
pub(crate) fn expected_internal(dir: &str) -> Vec<&str> {
    let leaf = dir.rsplit('/').next().unwrap_or("");
    expected_contract_family(leaf)
        .or_else(|| expected_adapter(leaf))
        .or_else(|| expected_mise_family(leaf))
        .or_else(|| expected_service(leaf))
        .unwrap_or_else(|| vec!["velnor-actions-contract"])
}
