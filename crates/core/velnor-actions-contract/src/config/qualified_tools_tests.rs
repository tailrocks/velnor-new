//! Tool qualification rejects implicit backends, unsafe data and incomplete identity.
use super::*;

fn bun() -> QualifiedTool {
    QualifiedTool {
        id: "bun".to_owned(),
        backend: QualifiedToolBackend::Core {
            tool: "bun".to_owned(),
        },
        version: "1.3.14".to_owned(),
        options: QualifiedToolOptions::Default,
        depends_on: Vec::new(),
        platforms: vec![QualifiedToolPlatform {
            platform: CheckPlatform::LinuxX64,
            artifacts: vec![QualifiedToolArtifact {
                url:
                    "https://github.com/oven-sh/bun/releases/download/bun-v1.3.14/bun-linux-x64.zip"
                        .to_owned(),
                sha256: "a".repeat(64),
            }],
            dependency_artifacts: Vec::new(),
            install_tree_sha256: "b".repeat(64),
            executables: vec![QualifiedToolExecutable {
                name: "bun".to_owned(),
                path: "bin/bun".to_owned(),
                sha256: "c".repeat(64),
                probe: QualifiedToolProbe::Version {
                    expected: "1.3.14".to_owned(),
                },
            }],
        }],
    }
}

#[test]
fn declared_tool_requires_exact_source_tree_executable_and_probe_identity() {
    assert!(bun().validate("config", "qualified_tools[0]").is_ok());
    for version in ["latest", "v1.3.14", "1.3", "1.3.014", "1.3.14;id"] {
        let mut tool = bun();
        tool.version = version.to_owned();
        assert!(tool.validate("config", "qualified_tools[0]").is_err());
    }
    let mut tool = bun();
    tool.platforms[0].install_tree_sha256 = "0".repeat(64);
    assert!(tool.validate("config", "qualified_tools[0]").is_err());
    let mut tool = bun();
    tool.platforms[0].executables[0].path = "../outside/bun".to_owned();
    assert!(tool.validate("config", "qualified_tools[0]").is_err());
    let mut tool = bun();
    tool.platforms[0].executables[0].probe = QualifiedToolProbe::Version {
        expected: "bun 1.3.140".to_owned(),
    };
    assert!(tool.validate("config", "qualified_tools[0]").is_err());
}

#[test]
fn source_urls_cannot_escape_explicit_backend_provenance() {
    for url in [
        "http://github.com/oven-sh/bun/releases/download/bun-v1.3.14/bun.zip",
        "https://attacker.invalid/oven-sh/bun/releases/download/bun-v1.3.14/bun.zip",
        "https://github.com/attacker/bun/releases/download/bun-v1.3.14/bun.zip",
        "https://github.com/oven-sh/bun/releases/download/latest/bun.zip",
        "https://user@github.com/oven-sh/bun/releases/download/bun-v1.3.14/bun.zip",
        "https://github.com/oven-sh/bun/releases/download/bun-v1.3.14/bun.zip?token=x",
        "https://github.com/oven-sh/bun/releases/download/bun-v1.3.14/bun.zip#fragment",
        "https://github.com/oven-sh/bun/releases/download/bun-v1.3.14/../bun.zip",
    ] {
        let mut tool = bun();
        tool.platforms[0].artifacts[0].url = url.to_owned();
        assert!(
            tool.validate("config", "qualified_tools[0]").is_err(),
            "{url}"
        );
    }
}

#[test]
fn registry_proves_sorted_unique_dependency_graph() {
    assert!(validate_qualified_tools(&[bun()], "config").is_ok());
    assert!(validate_qualified_tools(&[bun(), bun()], "config").is_err());
    let mut unknown = bun();
    unknown.depends_on = vec!["missing".to_owned()];
    assert!(validate_qualified_tools(&[unknown], "config").is_err());
    let mut first = bun();
    first.id = "a".to_owned();
    first.depends_on = vec!["b".to_owned()];
    let mut second = bun();
    second.id = "b".to_owned();
    second.depends_on = vec!["a".to_owned()];
    assert!(validate_qualified_tools(&[first, second], "config").is_err());
    let mut duplicate_platform = bun();
    duplicate_platform
        .platforms
        .push(duplicate_platform.platforms[0].clone());
    assert!(
        duplicate_platform
            .validate("config", "qualified_tools[0]")
            .is_err()
    );
}

#[test]
fn raw_install_arguments_and_unknown_backends_are_not_schema_fields() {
    for key in ["env", "argv", "plugin_url", "install_args", "settings"] {
        let mut value = serde_json::to_value(bun()).expect("serialized declaration");
        value
            .as_object_mut()
            .expect("object")
            .insert(key.to_owned(), serde_json::json!("evil"));
        assert!(
            serde_json::from_value::<QualifiedTool>(value).is_err(),
            "{key}"
        );
    }
    let mut value = serde_json::to_value(bun()).expect("serialized declaration");
    value["backend"] = serde_json::json!({"kind":"github","repository":"oven-sh/bun"});
    assert!(serde_json::from_value::<QualifiedTool>(value).is_err());
}

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

fn rust_toolchain() -> QualifiedTool {
    let mut tool = bun();
    tool.id = "rust".to_owned();
    tool.version = "1.97.1".to_owned();
    tool.backend = QualifiedToolBackend::Core {
        tool: "rust".to_owned(),
    };
    tool.options = QualifiedToolOptions::Rust {
        components: Vec::new(),
        targets: Vec::new(),
    };
    tool.platforms[0].artifacts[0].url =
        "https://static.rust-lang.org/dist/rust-1.97.1-x86_64-unknown-linux-gnu.tar.xz".to_owned();
    tool.platforms[0].executables[0].name = "rustc".to_owned();
    tool.platforms[0].executables[0].probe = QualifiedToolProbe::RustcVerbose {
        expected: format!(
            "rustc 1.97.1\ncommit-hash: {}\nrelease: 1.97.1\nhost: x86_64-unknown-linux-gnu",
            "a".repeat(40)
        ),
    };
    tool
}

fn nextest_tool(prebuilt: bool) -> QualifiedTool {
    let mut tool = bun();
    tool.id = "nextest".to_owned();
    tool.version = "0.9.140".to_owned();
    tool.backend = if prebuilt {
        QualifiedToolBackend::Cargo {
            crate_name: "cargo-nextest".to_owned(),
        }
    } else {
        QualifiedToolBackend::Aqua {
            package: "nextest-rs/nextest/cargo-nextest".to_owned(),
        }
    };
    tool.options = if prebuilt {
        QualifiedToolOptions::Cargo {
            default_features: true,
            features: Vec::new(),
            installation: QualifiedCargoInstallation::Prebuilt {
                repository: "nextest-rs/nextest".to_owned(),
            },
        }
    } else {
        QualifiedToolOptions::Default
    };
    tool.platforms[0].artifacts[0].url = "https://github.com/nextest-rs/nextest/releases/download/cargo-nextest-0.9.140/cargo-nextest-linux-x64.tar.gz".to_owned();
    tool.platforms[0].executables[0].name = "cargo-nextest".to_owned();
    tool.platforms[0].executables[0].probe = QualifiedToolProbe::CargoNextestVersion {
        expected: "cargo-nextest 0.9.140".to_owned(),
    };
    tool
}

#[test]
fn compiler_requirements_follow_probe_facts_across_backends() {
    assert!(!bun().requires_compiler());
    assert!(rust_toolchain().requires_compiler());
    for prebuilt in [false, true] {
        let mut nextest = nextest_tool(prebuilt);
        assert!(nextest.requires_compiler());
        let error = validate_qualified_tools(&[nextest.clone()], "config")
            .expect_err("nextest probe compiler");
        assert!(
            error
                .to_string()
                .contains("requires_one_direct_rust_dependency")
        );
        nextest.depends_on = vec!["rust".to_owned()];
        assert!(validate_qualified_tools(&[nextest.clone(), rust_toolchain()], "config").is_ok());
        let mut other = rust_toolchain();
        other.id = "rust-other".to_owned();
        nextest.depends_on.push("rust-other".to_owned());
        assert!(validate_qualified_tools(&[nextest, rust_toolchain(), other], "config").is_err());
    }
}
