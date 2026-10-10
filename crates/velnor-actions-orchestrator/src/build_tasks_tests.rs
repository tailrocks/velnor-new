use std::collections::BTreeMap;

use crate::build_tasks::{
    validate_config_shape, validate_source_lock_subset, validate_source_task_lock_requests,
};
use crate::native_tool_input::{
    NativeMiseConfig, NativeMiseLock, NativeToolSource, native_mise_source,
};

const ROOT_CONFIG: &str = r#"
min_version = "2026.10.7"

[tools]
mr-boxington = "1.23.0"
rust = { version = "1.99.0", mr_boxington = true }

[settings]
lockfile = true
idiomatic_version_file_enable_tools = ["rust"]

[settings.cargo]
binstall = true

[tasks.native-ci]
run = "mbx +1.99.0 xtask desktop ci"
"#;

fn project(source: &str) -> NativeMiseConfig {
    let value = toml::from_str(source).expect("source TOML");
    let input = native_mise_source("mise.toml", source.as_bytes(), &value).expect("typed source");
    match input.source {
        NativeToolSource::MiseConfig(config) => config,
        NativeToolSource::RustToolchain | NativeToolSource::MiseLock(_) => panic!("wrong source"),
    }
}

fn lock(source: &str) -> NativeMiseLock {
    let value = toml::from_str(source).expect("lock TOML");
    let input = native_mise_source("mise.lock", source.as_bytes(), &value).expect("typed lock");
    match input.source {
        NativeToolSource::MiseLock(lock) => lock,
        NativeToolSource::RustToolchain | NativeToolSource::MiseConfig(_) => panic!("wrong source"),
    }
}

const SWIFTLINT_ROW: &str = r#"
version = "0.65.1"
backend = "aqua:realm/SwiftLint"
specifiers = ["0.65.1"]

[tools.swiftlint.platforms.macos-arm64]
checksum = "sha256:0123456789abcdef"
url = "https://example.invalid/swiftlint.zip"
"#;

fn swiftlint_lock(rows: &str) -> String {
    format!("lockfile_version = 3\n\n[[tools.swiftlint]]\n{rows}")
}

#[test]
fn current_root_mise_shape_accepts_pinned_mbx_rust_without_wrappers() {
    let config = project(ROOT_CONFIG);
    validate_config_shape(&config, "1.99.0").expect("current pinned source");
    assert!(!config.root_keys.iter().any(|key| key == "wrappers"));
    assert_eq!(config.tools["rust"].mr_boxington, Some(true));
}

#[test]
fn legacy_wrapper_and_selected_tool_drift_fail_closed() {
    let legacy = format!("{ROOT_CONFIG}\n[wrappers.cargo]\ncommand = \"mbx\"\n");
    assert!(validate_config_shape(&project(&legacy), "1.99.0").is_err());

    let wrong_mbx = ROOT_CONFIG.replace("1.23.0", "1.22.0");
    assert!(validate_config_shape(&project(&wrong_mbx), "1.99.0").is_err());

    let wrong_rust = ROOT_CONFIG.replace("1.99.0", "1.98.0");
    assert!(validate_config_shape(&project(&wrong_rust), "1.98.0").is_err());

    let missing_route = ROOT_CONFIG.replace(", mr_boxington = true", "");
    assert!(validate_config_shape(&project(&missing_route), "1.99.0").is_err());

    let malformed_route = ROOT_CONFIG.replace("mr_boxington = true", "mr_boxington = \"true\"");
    assert!(validate_config_shape(&project(&malformed_route), "1.99.0").is_err());
}

#[test]
fn accepts_task_lock_as_exact_supported_subset_of_root_lock() {
    let root = lock(&format!(
        "{}\n[[tools.mise]]\nversion = \"2026.10.7\"\nbackend = \"aqua:jdx/mise\"\nspecifiers = [\"2026.10.7\"]\n",
        swiftlint_lock(SWIFTLINT_ROW)
    ));
    let source = lock(&swiftlint_lock(SWIFTLINT_ROW));

    validate_source_lock_subset(&source, &root).expect("exact task-local row is in root lock");
}

#[test]
fn rejects_foreign_or_incompatible_task_lock_rows() {
    let root = lock(&swiftlint_lock(SWIFTLINT_ROW));
    let foreign = lock(&swiftlint_lock(SWIFTLINT_ROW).replace("tools.swiftlint", "tools.foreign"));
    assert!(validate_source_lock_subset(&foreign, &root).is_err());

    let incompatible = lock(&swiftlint_lock(
        &SWIFTLINT_ROW.replace("0123456789abcdef", "fedcba9876543210"),
    ));
    assert!(validate_source_lock_subset(&incompatible, &root).is_err());
}

#[test]
fn rejects_duplicate_task_rows_and_ambiguous_root_rows() {
    let single = swiftlint_lock(SWIFTLINT_ROW);
    let duplicated_source = lock(&format!(
        "lockfile_version = 3\n\n[[tools.swiftlint]]\n{SWIFTLINT_ROW}\n[[tools.swiftlint]]\n{SWIFTLINT_ROW}"
    ));
    let root = lock(&single);
    assert!(validate_source_lock_subset(&duplicated_source, &root).is_err());

    let duplicated_root = lock(&format!(
        "lockfile_version = 3\n\n[[tools.swiftlint]]\n{SWIFTLINT_ROW}\n[[tools.swiftlint]]\n{SWIFTLINT_ROW}"
    ));
    let source = lock(&single);
    assert!(validate_source_lock_subset(&source, &duplicated_root).is_err());
}

#[test]
fn task_local_lock_matches_only_declared_tools_and_root_authority() {
    let root_config = project(&format!(
        "{ROOT_CONFIG}\n[tools.swiftlint]\nversion = \"0.65.1\"\n"
    ));
    let source_config = project(
        r#"
[tasks.lint]
run = "swiftlint lint --strict"
tools = { swiftlint = "0.65.1" }
"#,
    );
    let root_lock = lock(&swiftlint_lock(SWIFTLINT_ROW));
    let source_lock = lock(&swiftlint_lock(SWIFTLINT_ROW));
    validate_source_task_lock_requests(
        &source_config,
        Some(&source_lock),
        false,
        &root_config,
        &root_lock,
        "1.99.0",
        &BTreeMap::new(),
    )
    .expect("task-local selector has the exact root-selected locked row");

    let unrelated_row = format!(
        "{}\n[[tools.foreign]]\nversion = \"0.1.0\"\nbackend = \"aqua:example/foreign\"\nspecifiers = [\"0.1.0\"]\n",
        swiftlint_lock(SWIFTLINT_ROW)
    );
    let unrelated_lock = lock(&unrelated_row);
    assert!(
        validate_source_task_lock_requests(
            &source_config,
            Some(&unrelated_lock),
            false,
            &root_config,
            &root_lock,
            "1.99.0",
            &BTreeMap::new(),
        )
        .is_err()
    );

    let wrong_version_config = project(&format!(
        "{ROOT_CONFIG}\n[tools.swiftlint]\nversion = \"0.64.0\"\n"
    ));
    assert!(
        validate_source_task_lock_requests(
            &source_config,
            Some(&source_lock),
            false,
            &wrong_version_config,
            &root_lock,
            "1.99.0",
            &BTreeMap::new(),
        )
        .is_err()
    );
}
