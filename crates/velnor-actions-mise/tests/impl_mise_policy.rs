//! Version-policy mirror equality cases.
use velnor_actions_mise::catalog::lock::{LockError, verify_version_policy};
use velnor_actions_mise::{PYTHON_BINARY_SHA256_LINUX_X64, PYTHON_PBS_SHA256, ToolCatalog};

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
    let missing = text.replace("mise = \"2026.9.18\"\n", "");
    assert!(verify_version_policy(&missing, &ToolCatalog::pinned()).is_err());
}

#[test]
fn python_artifact_policy_rejects_source_checksum_and_extraction_drift() {
    let text = std::fs::read_to_string(policy_path()).expect("repo version-policy exists");
    for (current, changed) in [
        ("release = \"20261001\"", "release = \"20261003\""),
        ("releases/download/20261001", "releases/download/20261003"),
        ("releases/tag/20261001", "releases/tag/20261003"),
        ("strip_components = 1", "strip_components = 2"),
        ("bin_path = \"bin\"", "bin_path = \"python/bin\""),
        ("size_bytes = 36292523", "size_bytes = 36292524"),
    ] {
        let drifted = text.replace(current, changed);
        assert_ne!(text, drifted, "fixture must contain {current}");
        assert!(
            verify_version_policy(&drifted, &ToolCatalog::pinned()).is_err(),
            "policy must reject {changed}"
        );
    }
    let checksum_drift = text.replace(PYTHON_PBS_SHA256, &"0".repeat(64));
    assert_ne!(text, checksum_drift);
    assert!(verify_version_policy(&checksum_drift, &ToolCatalog::pinned()).is_err());
    let binary_drift = text.replace(PYTHON_BINARY_SHA256_LINUX_X64, &"0".repeat(64));
    assert_ne!(text, binary_drift);
    assert!(verify_version_policy(&binary_drift, &ToolCatalog::pinned()).is_err());
    let missing = text.replace("[tool_artifacts.python]\n", "");
    assert!(verify_version_policy(&missing, &ToolCatalog::pinned()).is_err());
}
