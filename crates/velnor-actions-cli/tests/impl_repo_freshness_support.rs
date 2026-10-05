//! Repository-maintenance support stays outside the V1 product graph.

use std::error::Error;

use crate::impl_repo_policy::{manifest, read};

#[test]
fn freshness_support_is_private_and_separately_classified() -> Result<(), Box<dyn Error>> {
    let cli = manifest("crates/velnor-actions-cli")?;
    let orchestrator = manifest("crates/velnor-actions-orchestrator")?;
    let support = manifest("crates/velnor-actions-freshness")?;
    assert!(cli.contains("velnor-actions-freshness"));
    assert!(!orchestrator.contains("velnor-actions-freshness"));
    assert!(!support.contains("[[bin]]"));

    let architecture = read("docs/proposed/architecture.md")?;
    let quality = read("docs/proposed/rust-quality-contract.md")?;
    for contract in [architecture, quality] {
        assert!(contract.contains("velnor-actions-freshness"));
        assert!(contract.contains("velnor-archive-guard"));
        assert!(contract.contains("repository-only"));
        assert!(
            contract.contains("outside the eight-product V1 graph")
                || contract.contains("outside the eight-product V1 task graph")
                || contract.contains("outside the orchestrator, product task graph")
        );
    }
    let cli_contract = read("docs/proposed/cli-contract.md")?;
    assert!(cli_contract.contains("repo-policy-v1"));
    assert!(cli_contract.contains("trailer-policy"));
    Ok(())
}
