use velnor_actions_contract::RustBinaryReleaseConfig;

const FILE: &str = "config.toml";

fn enabled() -> RustBinaryReleaseConfig {
    RustBinaryReleaseConfig {
        enabled: true,
        manifest_path: "crates/repo-scan/Cargo.toml".to_owned(),
        package: "repo-scan".to_owned(),
        binary: Some("repo-scan".to_owned()),
        source_commit_env: Some("REPO_SCAN_SOURCE_COMMIT".to_owned()),
    }
}

fn problem(config: &RustBinaryReleaseConfig) -> String {
    match config.validate(FILE) {
        Err(error) => error.to_string(),
        Ok(()) => String::new(),
    }
}

#[test]
fn binary_release_defaults_disabled_and_requires_a_package_when_enabled() {
    let default = RustBinaryReleaseConfig::default();
    assert!(!default.enabled);
    assert_eq!(default.manifest_path, "Cargo.toml");
    assert_eq!(default.binary_name(), "");
    assert_eq!(default.validate(FILE), Ok(()));

    let mut missing_package = default;
    missing_package.enabled = true;
    assert!(problem(&missing_package).contains("missing_package"));
}

#[test]
fn binary_release_accepts_exact_package_and_binary_names() {
    let config = enabled();
    assert_eq!(config.binary_name(), "repo-scan");
    assert_eq!(config.validate(FILE), Ok(()));

    let mut default_binary = config;
    default_binary.binary = None;
    assert_eq!(default_binary.binary_name(), "repo-scan");
    assert_eq!(default_binary.validate(FILE), Ok(()));
}

#[test]
fn binary_release_rejects_unsafe_paths_and_names() {
    for path in [
        "../Cargo.toml",
        "/tmp/Cargo.toml",
        "Cargo.toml;touch x",
        "Cargo.lock",
    ] {
        let mut config = enabled();
        config.manifest_path = path.to_owned();
        assert!(problem(&config).contains("binary_release.manifest_path"));
    }
    for package in [
        "../repo-scan",
        "-repo-scan",
        "repo scan",
        "${{ secrets.X }}",
    ] {
        let mut config = enabled();
        config.package = package.to_owned();
        assert!(problem(&config).contains("binary_release.package"));
    }
    for binary in ["../repo-scan", "--help", "repo scan", "$PATH"] {
        let mut config = enabled();
        config.binary = Some(binary.to_owned());
        assert!(problem(&config).contains("binary_release.binary"));
    }
    for name in [
        "",
        "1SOURCE",
        "SOURCE-COMMIT",
        "${{ GITHUB_SHA }}",
        "PATH",
        "GITHUB_TOKEN",
        "RUSTUP_TOOLCHAIN",
    ] {
        let mut config = enabled();
        config.source_commit_env = Some(name.to_owned());
        assert!(problem(&config).contains("binary_release.source_commit_env"));
    }
}

#[test]
fn binary_release_rejects_unknown_schema_fields() {
    let text = r#"{"enabled":true,"manifest_path":"Cargo.toml","package":"repo-scan","shell":"echo unsafe"}"#;
    assert!(serde_json::from_str::<RustBinaryReleaseConfig>(text).is_err());
}

#[test]
fn enabled_binary_release_requires_consumer_workflow_policy() {
    use velnor_actions_contract::{ContractError, RustStackConfig, WorkflowPolicy};

    let mut config = crate::impl_remed_contract::valid_config();
    let mut rust = RustStackConfig::default_config();
    rust.binary_release = enabled();
    config.stacks.rust = Some(rust);
    assert_eq!(config.validate(FILE), Ok(()));

    config.workflow.policy = WorkflowPolicy::VelnorRepositoryV1;
    let Err(ContractError::Config {
        key_path, problem, ..
    }) = config.validate(FILE)
    else {
        panic!("binary release under repository policy must fail");
    };
    assert_eq!(key_path, "stacks.rust.binary_release.enabled");
    assert_eq!(problem, "binary_release_requires_consumer_policy");
}
