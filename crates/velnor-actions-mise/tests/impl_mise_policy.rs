//! Version-policy mirror equality cases.
use velnor_actions_mise::catalog::lock::{LockError, verify_version_policy};
use velnor_actions_mise::{MISE_VERSION, PinnedTool, ToolCatalog};

fn policy_path() -> String {
    format!(
        "{}/../../.velnor/version-policy.toml",
        env!("CARGO_MANIFEST_DIR")
    )
}

#[test]
fn repo_policy_mirror_matches_compiled_catalog() -> Result<(), LockError> {
    let text = std::fs::read_to_string(policy_path()).expect("repo version-policy exists");
    verify_version_policy(&text, &ToolCatalog::pinned())
}

#[test]
fn policy_drift_fails_mirror_check() {
    let text = std::fs::read_to_string(policy_path()).expect("repo version-policy exists");
    let drifted = text.replace("rust = \"1.98.1\"", "rust = \"1.99.0\"");
    assert_ne!(text, drifted);
    let err = verify_version_policy(&drifted, &ToolCatalog::pinned());
    assert!(err.is_err_and(|err| err.to_string().contains("tool:rust")));
    let missing = text.replace(&format!("mise = \"{MISE_VERSION}\"\n"), "");
    assert!(verify_version_policy(&missing, &ToolCatalog::pinned()).is_err());
}

#[test]
fn every_catalog_tool_requires_an_exact_policy_mirror() {
    let text = std::fs::read_to_string(policy_path()).expect("repo version-policy exists");
    let catalog = ToolCatalog::pinned();
    for tool in PinnedTool::ALL {
        let pin = format!("{} = \"{}\"\n", tool.tool_name(), catalog.version(tool));
        assert!(text.contains(&pin), "missing policy pin: {pin}");
        let missing = text.replace(&pin, "");
        assert!(verify_version_policy(&missing, &catalog).is_err());
        let drifted = text.replace(&pin, &format!("{} = \"latest\"\n", tool.tool_name()));
        assert!(verify_version_policy(&drifted, &catalog).is_err());
    }
}

#[test]
fn policy_rejects_tools_outside_the_catalog() {
    let text = std::fs::read_to_string(policy_path()).expect("repo version-policy exists");
    let unknown = text.replace("[tools]\n", "[tools]\nambient = \"1.0.0\"\n");
    assert!(
        verify_version_policy(&unknown, &ToolCatalog::pinned())
            .is_err_and(|error| error.to_string().contains("tool_unknown:ambient"))
    );
}

#[test]
fn policy_rejects_duplicate_tools_sections() {
    let text = std::fs::read_to_string(policy_path()).expect("repo version-policy exists");
    let duplicate = format!("{text}\n[tools]\nambient = \"1.0.0\"\n");
    assert!(
        verify_version_policy(&duplicate, &ToolCatalog::pinned())
            .is_err_and(|error| error.to_string().contains("duplicate_section:tools"))
    );
}
