//! Intra-workspace dependency edges allowed per member package.

/// Allowed `velnor-actions-*` dependencies per member (matched on leaf dir name).
pub(crate) fn expected_internal(dir: &str) -> Vec<&str> {
    match dir.rsplit('/').next().unwrap_or("") {
        "velnor-actions-contract" | "velnor-actions-repo-policy" => vec![],
        "velnor-actions-orchestrator" => vec![
            "velnor-actions-actionlint",
            "velnor-actions-contract",
            "velnor-actions-mise",
            "velnor-actions-rust",
            "velnor-actions-rust-core",
            "velnor-actions-tofu",
            "velnor-actions-tofu-core",
            "velnor-actions-workflow-renderer",
        ],
        "velnor-actions-tofu" => vec!["velnor-actions-contract", "velnor-actions-tofu-core"],
        "velnor-actions-rust" => vec!["velnor-actions-contract", "velnor-actions-rust-core"],
        "velnor-actions-cli" => vec!["velnor-actions-orchestrator"],
        "velnor-actions-mise-cache" | "velnor-actions-mise-catalog" => {
            vec!["velnor-actions-contract", "velnor-actions-mise-core"]
        }
        "velnor-actions-mise-nextest" => {
            vec![
                "velnor-actions-contract",
                "velnor-actions-mise-core",
                "velnor-actions-mise-catalog",
            ]
        }
        "velnor-actions-mise-probes" => vec!["velnor-actions-contract", "velnor-actions-mise-core"],
        "velnor-actions-mise" => {
            vec![
                "velnor-actions-mise-cache",
                "velnor-actions-mise-catalog",
                "velnor-actions-mise-core",
                "velnor-actions-mise-nextest",
                "velnor-actions-mise-probes",
            ]
        }
        _ => vec!["velnor-actions-contract"],
    }
}
