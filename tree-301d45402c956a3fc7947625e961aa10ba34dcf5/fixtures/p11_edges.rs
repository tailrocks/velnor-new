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

/// Orchestrator-family edges by leaf dir name.
const ORCHESTRATOR_FAMILY: &[(&str, &[&str])] = &[
    (
        "velnor-actions-orchestrator-check-acquisition",
        &[
            "velnor-actions-contract",
            "velnor-actions-contract-config",
            "velnor-actions-mise",
            "velnor-actions-orchestrator-core",
        ],
    ),
    (
        "velnor-actions-orchestrator-check-evidence",
        &[
            "velnor-actions-contract",
            "velnor-actions-contract-config",
            "velnor-actions-contract-workflow",
            "velnor-actions-mise",
            "velnor-actions-orchestrator-check-acquisition",
            "velnor-actions-orchestrator-check-preparation",
            "velnor-actions-orchestrator-core",
        ],
    ),
    (
        "velnor-actions-orchestrator-check-preparation",
        &[
            "velnor-actions-contract",
            "velnor-actions-contract-config",
            "velnor-actions-mise",
            "velnor-actions-orchestrator-check-acquisition",
            "velnor-actions-orchestrator-core",
            "velnor-actions-workflow-steps",
        ],
    ),
    (
        "velnor-actions-orchestrator-core",
        &[
            "velnor-actions-actionlint",
            "velnor-actions-contract",
            "velnor-actions-contract-config",
            "velnor-actions-contract-planning",
            "velnor-actions-contract-release",
            "velnor-actions-contract-workflow",
            "velnor-actions-mise",
            "velnor-actions-rust",
            "velnor-actions-rust-core",
            "velnor-actions-tofu-core",
            "velnor-actions-workflow-steps",
            "velnor-actions-workflow-tree",
        ],
    ),
    (
        "velnor-actions-orchestrator-discovery",
        &[
            "velnor-actions-contract",
            "velnor-actions-contract-config",
            "velnor-actions-contract-planning",
            "velnor-actions-contract-release",
            "velnor-actions-mise",
            "velnor-actions-orchestrator-core",
            "velnor-actions-rust",
            "velnor-actions-rust-core",
            "velnor-actions-tofu",
            "velnor-actions-tofu-core",
            "velnor-actions-workflow-renderer",
        ],
    ),
    (
        "velnor-actions-orchestrator-graph",
        &[
            "velnor-actions-contract",
            "velnor-actions-contract-config",
            "velnor-actions-contract-planning",
            "velnor-actions-contract-release",
            "velnor-actions-contract-workflow",
            "velnor-actions-mise",
            "velnor-actions-orchestrator-core",
            "velnor-actions-orchestrator-discovery",
            "velnor-actions-rust",
            "velnor-actions-rust-core",
            "velnor-actions-tofu",
            "velnor-actions-tofu-core",
            "velnor-actions-workflow-document",
            "velnor-actions-workflow-steps",
        ],
    ),
    (
        "velnor-actions-orchestrator-pins",
        &[
            "velnor-actions-actionlint",
            "velnor-actions-contract-config",
            "velnor-actions-contract-release",
            "velnor-actions-contract-workflow",
            "velnor-actions-mise",
            "velnor-actions-orchestrator-core",
            "velnor-actions-orchestrator-discovery",
            "velnor-actions-workflow-jobs",
            "velnor-actions-workflow-steps",
        ],
    ),
    (
        "velnor-actions-orchestrator-provisioning",
        &[
            "velnor-actions-actionlint",
            "velnor-actions-contract",
            "velnor-actions-contract-config",
            "velnor-actions-contract-planning",
            "velnor-actions-contract-release",
            "velnor-actions-contract-workflow",
            "velnor-actions-mise",
            "velnor-actions-orchestrator-core",
            "velnor-actions-orchestrator-discovery",
            "velnor-actions-rust",
            "velnor-actions-tofu-core",
            "velnor-actions-workflow-cache",
            "velnor-actions-workflow-jobs",
            "velnor-actions-workflow-renderer",
            "velnor-actions-workflow-steps",
        ],
    ),
    (
        "velnor-actions-orchestrator-selection",
        &[
            "velnor-actions-contract-planning",
            "velnor-actions-contract-workflow",
            "velnor-actions-mise",
            "velnor-actions-orchestrator-core",
            "velnor-actions-orchestrator-discovery",
            "velnor-actions-rust-core",
        ],
    ),
];

fn expected_orchestrator_family(leaf: &str) -> Option<Vec<&str>> {
    ORCHESTRATOR_FAMILY
        .iter()
        .find(|(name, _)| *name == leaf)
        .map(|(_, edges)| edges.to_vec())
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
            "velnor-actions-orchestrator-check-acquisition",
            "velnor-actions-orchestrator-check-evidence",
            "velnor-actions-orchestrator-check-preparation",
            "velnor-actions-orchestrator-core",
            "velnor-actions-orchestrator-discovery",
            "velnor-actions-orchestrator-graph",
            "velnor-actions-orchestrator-pins",
            "velnor-actions-orchestrator-provisioning",
            "velnor-actions-orchestrator-selection",
            "velnor-actions-rust",
            "velnor-actions-rust-core",
            "velnor-actions-tofu",
            "velnor-actions-tofu-core",
            "velnor-actions-workflow-cache",
            "velnor-actions-workflow-document",
            "velnor-actions-workflow-generator",
            "velnor-actions-workflow-jobs",
            "velnor-actions-workflow-release",
            "velnor-actions-workflow-render-strict",
            "velnor-actions-workflow-renderer",
            "velnor-actions-workflow-schema2",
            "velnor-actions-workflow-steps",
            "velnor-actions-workflow-tree",
        ]),
        "velnor-actions-workflow-renderer" => Some(vec![
            "velnor-actions-contract",
            "velnor-actions-contract-config",
            "velnor-actions-contract-release",
            "velnor-actions-contract-workflow",
            "velnor-actions-workflow-cache",
            "velnor-actions-workflow-document",
            "velnor-actions-workflow-jobs",
            "velnor-actions-workflow-render-strict",
            "velnor-actions-workflow-steps",
            "velnor-actions-workflow-tree",
        ]),
        "velnor-actions-cli" => Some(vec![
            "velnor-actions-orchestrator",
            "velnor-actions-orchestrator-core",
        ]),
        "velnor-actions-repo-policy" => Some(vec![]),
        _ => expected_workflow_family(leaf),
    }
}

/// Workflow-family edges by leaf dir name.
fn expected_workflow_family(leaf: &str) -> Option<Vec<&str>> {
    match leaf {
        "velnor-actions-workflow-cache" => Some(vec![
            "velnor-actions-contract",
            "velnor-actions-contract-config",
            "velnor-actions-contract-release",
            "velnor-actions-contract-workflow",
            "velnor-actions-workflow-steps",
            "velnor-actions-workflow-tree",
        ]),
        "velnor-actions-workflow-jobs" => Some(vec![
            "velnor-actions-contract",
            "velnor-actions-contract-config",
            "velnor-actions-contract-release",
            "velnor-actions-contract-workflow",
            "velnor-actions-workflow-cache",
            "velnor-actions-workflow-steps",
            "velnor-actions-workflow-tree",
        ]),
        "velnor-actions-workflow-document" => Some(vec![
            "velnor-actions-contract-config",
            "velnor-actions-contract-workflow",
            "velnor-actions-workflow-cache",
            "velnor-actions-workflow-jobs",
            "velnor-actions-workflow-steps",
            "velnor-actions-workflow-tree",
        ]),
        "velnor-actions-workflow-generator" => Some(vec![
            "velnor-actions-contract-config",
            "velnor-actions-contract-release",
            "velnor-actions-workflow-steps",
            "velnor-actions-workflow-tree",
        ]),
        "velnor-actions-workflow-render-strict" => Some(vec![
            "velnor-actions-contract-config",
            "velnor-actions-contract-workflow",
            "velnor-actions-workflow-cache",
            "velnor-actions-workflow-document",
            "velnor-actions-workflow-jobs",
            "velnor-actions-workflow-steps",
            "velnor-actions-workflow-tree",
        ]),
        "velnor-actions-workflow-release" => Some(vec![
            "velnor-actions-contract",
            "velnor-actions-contract-workflow",
            "velnor-actions-workflow-steps",
            "velnor-actions-workflow-tree",
        ]),
        "velnor-actions-workflow-schema2" => Some(vec![
            "velnor-actions-contract-config",
            "velnor-actions-workflow-cache",
            "velnor-actions-workflow-generator",
            "velnor-actions-workflow-steps",
            "velnor-actions-workflow-tree",
        ]),
        "velnor-actions-workflow-steps" => Some(vec![
            "velnor-actions-contract",
            "velnor-actions-contract-workflow",
        ]),
        "velnor-actions-workflow-tree" => Some(vec![
            "velnor-actions-contract",
            "velnor-actions-contract-config",
            "velnor-actions-contract-release",
            "velnor-actions-contract-workflow",
            "velnor-actions-workflow-steps",
        ]),
        _ => None,
    }
}

/// Allowed `velnor-actions-*` dependencies per member (matched on leaf dir name).
pub(crate) fn expected_internal(dir: &str) -> Vec<&str> {
    let leaf = dir.rsplit('/').next().unwrap_or("");
    expected_contract_family(leaf)
        .or_else(|| expected_adapter(leaf))
        .or_else(|| expected_mise_family(leaf))
        .or_else(|| expected_orchestrator_family(leaf))
        .or_else(|| expected_service(leaf))
        .unwrap_or_else(|| vec!["velnor-actions-contract"])
}
