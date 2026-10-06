//! Intake dependency-direction case: adapters stay leaf-only.
//!
//! Split from `impl_orch_intake` to hold the 400-line size gate; wired
//! into `velnor_orchestrator` by the parent with one `mod` line. Uses
//! `crate::impl_common` fixtures.

use std::fs;
use std::path::PathBuf;

use crate::impl_common::TestResult;

#[test]
fn intake_adapter_dependency_direction() -> TestResult {
    let crates = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    for (group, adapter) in [
        ("adapters", "rust"),
        ("adapters", "mise"),
        ("adapters", "actionlint"),
        ("services", "workflow-renderer"),
    ] {
        let path = crates.join(format!("{group}/velnor-actions-{adapter}/Cargo.toml"));
        let text = fs::read_to_string(path)?;
        let manifest: toml::Table = toml::from_str(&text)?;
        if let Some(deps) = manifest.get("dependencies").and_then(toml::Value::as_table) {
            for name in deps.keys() {
                assert!(
                    !name.starts_with("velnor-actions-") || name == "velnor-actions-contract",
                    "{adapter} must not depend on {name}"
                );
            }
        }
    }
    let path = crates.join("services/velnor-actions-orchestrator/Cargo.toml");
    let text = fs::read_to_string(path)?;
    let manifest: toml::Table = toml::from_str(&text)?;
    let deps = manifest
        .get("dependencies")
        .and_then(toml::Value::as_table)
        .ok_or("orchestrator deps")?;
    for name in [
        "velnor-actions-contract",
        "velnor-actions-rust",
        "velnor-actions-mise",
        "velnor-actions-actionlint",
        "velnor-actions-workflow-renderer",
    ] {
        assert!(deps.contains_key(name), "orchestrator composes {name}");
    }
    Ok(())
}
