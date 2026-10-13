//! Compiler home follows the current tool's declared dependency, never a future sibling.
use super::*;
use velnor_actions_contract_config::config::{
    CheckPlatform, QualifiedToolExecutable, QualifiedToolPlatform, QualifiedToolProbe,
};

fn record(id: &str, backend: QualifiedToolBackend) -> QualifiedTool {
    QualifiedTool {
        id: id.into(),
        backend,
        version: "1.0.0".into(),
        options: QualifiedToolOptions::Default,
        depends_on: vec![],
        platforms: vec![],
    }
}

fn nextest_probe(tool: &mut QualifiedTool) {
    tool.platforms = vec![QualifiedToolPlatform {
        platform: CheckPlatform::LinuxX64,
        artifacts: vec![],
        dependency_artifacts: vec![],
        install_tree_sha256: "a".repeat(64),
        executables: vec![QualifiedToolExecutable {
            name: "cargo-nextest".into(),
            path: "bin/cargo-nextest".into(),
            sha256: "b".repeat(64),
            probe: QualifiedToolProbe::CargoNextestVersion {
                expected: "cargo-nextest 1.0.0".into(),
            },
        }],
    }];
}

#[test]
fn bun_before_rust_does_not_bind_nonexistent_future_compiler() {
    let home = tempfile::TempDir::new().expect("home");
    let bun = record("bun", QualifiedToolBackend::Core { tool: "bun".into() });
    let rust = record(
        "rust",
        QualifiedToolBackend::Core {
            tool: "rust".into(),
        },
    );
    let tools = [bun.clone(), rust.clone()];
    assert!(!home.path().join("tools/rust/prefix").exists());
    assert_eq!(compiler_prefix(&bun, &tools, home.path()), None);
    assert_eq!(
        compiler_prefix(&rust, &tools, home.path()),
        Some(home.path().join("tools/rust/prefix"))
    );
}

#[test]
fn cargo_selects_only_explicit_rust_prerequisite() {
    let home = tempfile::TempDir::new().expect("home");
    let rust = record(
        "compiler",
        QualifiedToolBackend::Core {
            tool: "rust".into(),
        },
    );
    let mut cargo = record(
        "nextest",
        QualifiedToolBackend::Cargo {
            crate_name: "cargo-nextest".into(),
        },
    );
    nextest_probe(&mut cargo);
    assert_eq!(
        compiler_prefix(&cargo, std::slice::from_ref(&rust), home.path()),
        None
    );
    cargo.depends_on = vec![rust.id.clone()];
    assert_eq!(
        compiler_prefix(&cargo, &[rust], home.path()),
        Some(home.path().join("tools/compiler/prefix"))
    );
}

#[test]
fn aqua_nextest_uses_its_explicit_rust_prerequisite() {
    let home = tempfile::TempDir::new().expect("home");
    let rust = record(
        "compiler",
        QualifiedToolBackend::Core {
            tool: "rust".into(),
        },
    );
    let mut nextest = record(
        "nextest",
        QualifiedToolBackend::Aqua {
            package: "nextest-rs/cargo-nextest".into(),
        },
    );
    nextest_probe(&mut nextest);
    nextest.depends_on = vec![rust.id.clone()];
    assert_eq!(
        compiler_prefix(&nextest, &[rust], home.path()),
        Some(home.path().join("tools/compiler/prefix"))
    );
}

#[test]
fn prebuilt_cargo_nextest_uses_its_explicit_rust_prerequisite() {
    let home = tempfile::TempDir::new().expect("home");
    let rust = record(
        "compiler",
        QualifiedToolBackend::Core {
            tool: "rust".into(),
        },
    );
    let mut nextest = record(
        "nextest",
        QualifiedToolBackend::Cargo {
            crate_name: "cargo-nextest".into(),
        },
    );
    nextest_probe(&mut nextest);
    nextest.options = QualifiedToolOptions::Cargo {
        default_features: true,
        features: vec![],
        installation: QualifiedCargoInstallation::Prebuilt {
            repository: "nextest-rs/nextest".into(),
        },
    };
    nextest.depends_on = vec![rust.id.clone()];
    assert_eq!(
        compiler_prefix(&nextest, &[rust], home.path()),
        Some(home.path().join("tools/compiler/prefix"))
    );
}
