//! Offline Cargo source closure admission; no compiler or network execution.
use super::*;
use std::fs;
use std::time::Duration;
use velnor_actions_contract::config::{
    QualifiedToolArtifact, QualifiedToolExecutable, QualifiedToolProbe,
};

struct Fixture {
    _temporary: tempfile::TempDir,
    home: PathBuf,
    primary: PathBuf,
    dependency: PathBuf,
    tool: QualifiedTool,
}

fn fixture() -> Fixture {
    let temporary = tempfile::tempdir().expect("temp root");
    let home = temporary
        .path()
        .canonicalize()
        .expect("canonical root")
        .join("tool");
    let primary = home.join("unpacked/primary/source");
    let dependency = home.join("unpacked/dependency/dependency");
    fs::create_dir_all(primary.join("src")).expect("primary dirs");
    fs::create_dir_all(dependency.join("src")).expect("dependency dirs");
    fs::create_dir_all(home.join("cargo")).expect("cargo home");
    fs::write(
        primary.join("Cargo.toml"),
        "[package]\nname = \"source\"\nversion = \"0.1.0\"\n",
    )
    .expect("manifest");
    fs::write(
        dependency.join("Cargo.toml"),
        "[package]\nname = \"dependency\"\nversion = \"1.2.3\"\n",
    )
    .expect("dependency manifest");
    fs::write(primary.join("src/main.rs"), "fn main() {}\n").expect("source");
    fs::write(dependency.join("src/lib.rs"), "pub fn dependency() {}\n")
        .expect("dependency source");
    let checksum = "d".repeat(64);
    let lock = format!(
        "version = 4\n[[package]]\nname = \"source\"\nversion = \"0.1.0\"\n[[package]]\nname = \"dependency\"\nversion = \"1.2.3\"\nsource = \"registry+https://github.com/rust-lang/crates.io-index\"\nchecksum = \"{checksum}\"\n"
    );
    fs::write(primary.join("Cargo.lock"), &lock).expect("source lock");
    let tool = QualifiedTool {
        id: "source".to_owned(),
        backend: QualifiedToolBackend::Cargo {
            crate_name: "source".to_owned(),
        },
        version: "0.1.0".to_owned(),
        options: QualifiedToolOptions::Cargo {
            default_features: false,
            features: Vec::new(),
            installation: QualifiedCargoInstallation::Source {
                source_lock_sha256: crate::cover_identity::generator::sha256_hex(lock.as_bytes()),
            },
        },
        depends_on: vec!["rust".to_owned()],
        platforms: vec![QualifiedToolPlatform {
            platform: CheckPlatform::LinuxX64,
            artifacts: vec![QualifiedToolArtifact {
                url: "https://static.crates.io/crates/source/source-0.1.0.crate".to_owned(),
                sha256: "a".repeat(64),
            }],
            dependency_artifacts: vec![QualifiedToolArtifact {
                url: "https://static.crates.io/crates/dependency/dependency-1.2.3.crate".to_owned(),
                sha256: checksum,
            }],
            install_tree_sha256: "b".repeat(64),
            executables: vec![QualifiedToolExecutable {
                name: "source".to_owned(),
                path: "bin/source".to_owned(),
                sha256: "c".repeat(64),
                probe: QualifiedToolProbe::Version {
                    expected: "source 0.1.0".to_owned(),
                },
            }],
        }],
    };
    Fixture {
        _temporary: temporary,
        home,
        primary,
        dependency,
        tool,
    }
}

fn prepare(fixture: &Fixture) -> Result<(), OrchestratorError> {
    prepare_cargo_source(
        &fixture.tool,
        CheckPlatform::LinuxX64,
        &fixture.home,
        std::slice::from_ref(&fixture.primary),
        std::slice::from_ref(&fixture.dependency),
        velnor_actions_mise::CheckDeadline::after(Duration::from_secs(60)).expect("deadline"),
    )
}

mod cargo_tests;
