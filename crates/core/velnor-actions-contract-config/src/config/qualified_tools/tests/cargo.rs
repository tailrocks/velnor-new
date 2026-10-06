use super::super::*;
use super::bun;

#[test]
fn source_and_prebuilt_are_distinct_closed_cargo_options() {
    let mut tool = bun();
    tool.id = "codebook".to_owned();
    tool.version = "0.3.42".to_owned();
    tool.backend = QualifiedToolBackend::Cargo {
        crate_name: "codebook-lsp".to_owned(),
    };
    tool.options = QualifiedToolOptions::Cargo {
        default_features: false,
        features: Vec::new(),
        installation: QualifiedCargoInstallation::Source {
            source_lock_sha256: "d".repeat(64),
        },
    };
    tool.platforms[0].artifacts[0].url =
        "https://static.crates.io/crates/codebook-lsp/codebook-lsp-0.3.42.crate".to_owned();
    tool.platforms[0].executables[0].name = "codebook-lsp".to_owned();
    tool.platforms[0].executables[0].probe = QualifiedToolProbe::Version {
        expected: "codebook-lsp 0.3.42".to_owned(),
    };
    assert!(tool.validate("config", "qualified_tools[0]").is_ok());
    assert!(
        validate_qualified_tools(&[tool.clone()], "config").is_err(),
        "compiler prerequisite required"
    );
    tool.options = QualifiedToolOptions::Cargo {
        default_features: false,
        features: Vec::new(),
        installation: QualifiedCargoInstallation::Prebuilt {
            repository: "codebook/codebook".to_owned(),
        },
    };
    assert!(
        tool.validate("config", "qualified_tools[0]").is_err(),
        "custom features require source build"
    );
}

#[test]
fn prebuilt_cargo_archive_requires_no_unused_installer_prerequisite() {
    let mut tool = bun();
    tool.id = "sccache".to_owned();
    tool.version = "0.16.0".to_owned();
    tool.backend = QualifiedToolBackend::Cargo {
        crate_name: "sccache".to_owned(),
    };
    tool.options = QualifiedToolOptions::Cargo {
        default_features: true,
        features: Vec::new(),
        installation: QualifiedCargoInstallation::Prebuilt {
            repository: "mozilla/sccache".to_owned(),
        },
    };
    tool.platforms[0].artifacts[0].url = "https://github.com/mozilla/sccache/releases/download/v0.16.0/sccache-v0.16.0-x86_64-unknown-linux-musl.tar.gz".to_owned();
    tool.platforms[0].executables[0].name = "sccache".to_owned();
    tool.platforms[0].executables[0].probe = QualifiedToolProbe::Version {
        expected: "sccache 0.16.0".to_owned(),
    };
    assert!(validate_qualified_tools(&[tool], "config").is_ok());
    assert!(
        serde_json::from_value::<QualifiedCargoInstallation>(serde_json::json!({
            "mode":"binstall", "repository":"mozilla/sccache"
        }))
        .is_err()
    );
}

#[test]
fn root_archive_cardinality_is_typed_by_backend() {
    let mut generic = bun();
    let mut second = generic.platforms[0].artifacts[0].clone();
    second.url = second
        .url
        .replace("bun-linux-x64.zip", "bun-linux-x64-z.zip");
    generic.platforms[0].artifacts.push(second);
    generic.platforms[0]
        .artifacts
        .sort_by(|left, right| left.url.cmp(&right.url));
    let error = generic
        .validate("config", "qualified_tools[0]")
        .expect_err("ambiguous generic root archives");
    assert!(
        error
            .to_string()
            .contains("qualified_tool_requires_single_root_artifact")
    );
    let mut rust = bun();
    rust.id = "rust".to_owned();
    rust.version = "1.97.1".to_owned();
    rust.backend = QualifiedToolBackend::Core {
        tool: "rust".to_owned(),
    };
    rust.options = QualifiedToolOptions::Rust {
        components: Vec::new(),
        targets: Vec::new(),
    };
    rust.platforms[0].artifacts = ["cargo", "rustc"].into_iter().map(|component| QualifiedToolArtifact {
        url: format!("https://static.rust-lang.org/dist/{component}-1.97.1-x86_64-unknown-linux-gnu.tar.xz"), sha256: "a".repeat(64),
    }).collect();
    rust.platforms[0].executables[0].name = "rustc".to_owned();
    rust.platforms[0].executables[0].probe = QualifiedToolProbe::RustcVerbose {
        expected: format!(
            "rustc 1.97.1\ncommit-hash: {}\nrelease: 1.97.1\nhost: x86_64-unknown-linux-gnu",
            "a".repeat(40)
        ),
    };
    assert!(rust.validate("config", "qualified_tools[0]").is_ok());
}
