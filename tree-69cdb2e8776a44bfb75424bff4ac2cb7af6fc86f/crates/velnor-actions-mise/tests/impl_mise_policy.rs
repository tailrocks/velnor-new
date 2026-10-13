//! Version-policy mirror equality cases.
use velnor_actions_mise::ToolCatalog;
use velnor_actions_mise::catalog::lock::{LockError, verify_version_policy};

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
    let missing = text.replace("mise = \"2026.10.4\"\n", "");
    assert!(verify_version_policy(&missing, &ToolCatalog::pinned()).is_err());
}
