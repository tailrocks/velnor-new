use std::path::PathBuf;
use velnor_actions_contract_config::config::*;
use velnor_actions_mise_catalog::discovery::discover_checks;
use velnor_actions_mise_core::{self as mise, checks::CheckCapabilityProof};
type TestResult = Result<(), Box<dyn std::error::Error>>;

fn tool(id: &str, rust: bool) -> QualifiedTool {
    let version = if rust { "1.97.1" } else { "0.3.4" };
    let names = if rust {
        vec!["cargo", "rustc", "rustdoc"]
    } else {
        vec!["codebook"]
    };
    QualifiedTool {
        id: id.into(),
        backend: if rust {
            QualifiedToolBackend::Core {
                tool: "rust".into(),
            }
        } else {
            QualifiedToolBackend::Cargo {
                crate_name: "codebook".into(),
            }
        },
        version: version.into(),
        options: if rust {
            QualifiedToolOptions::Rust {
                components: vec![],
                targets: vec![],
            }
        } else {
            QualifiedToolOptions::Cargo {
                default_features: false,
                features: vec!["cli".into()],
                installation: QualifiedCargoInstallation::Source {
                    source_lock_sha256: "a".repeat(64),
                },
            }
        },
        depends_on: if rust {
            vec![]
        } else {
            vec!["rust-custom".into()]
        },
        platforms: vec![QualifiedToolPlatform {
            platform: CheckPlatform::LinuxX64,
            artifacts: vec![QualifiedToolArtifact {
                url: if rust {
                    "https://static.rust-lang.org/dist/rust-1.97.1-x86_64-unknown-linux-gnu.tar.xz"
                        .into()
                } else {
                    "https://static.crates.io/crates/codebook/codebook-0.3.4.crate".into()
                },
                sha256: "b".repeat(64),
            }],
            dependency_artifacts: vec![],
            install_tree_sha256: "c".repeat(64),
            executables: names
                .into_iter()
                .map(|name| QualifiedToolExecutable {
                    name: name.into(),
                    path: format!("bin/{name}"),
                    sha256: "d".repeat(64),
                    probe: QualifiedToolProbe::Version {
                        expected: format!("{name} {version}"),
                    },
                })
                .collect(),
        }],
    }
}

fn fixture(tools: Vec<QualifiedTool>) -> Result<mise::QualifiedCheck, Box<dyn std::error::Error>> {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_nanos();
    let source =
        std::env::temp_dir().join(format!("velnor-acquisition-{}-{stamp}", std::process::id()));
    std::fs::create_dir(&source)?;
    std::fs::write(source.join("mise.toml"), "[tasks.check]\nrun='true'\n")?;
    let row = MiseCheck {
        id: "check".into(),
        task: "check".into(),
        directory: ".".into(),
        runner: CheckRunner {
            label: "ubuntu-latest".into(),
            platform: CheckPlatform::LinuxX64,
            executor: CheckExecutor::Hosted,
            container: None,
        },
        inputs: vec![],
        tools: vec![],
        system_tools: vec![],
        evidence: None,
        timeout_minutes: 30,
    };
    let result = discover_checks(&source, &[row], &[]);
    std::fs::remove_dir_all(&source)?;
    let mut checks = result?;
    let mut check = checks.remove(0);
    check.qualified_tools = tools;
    Ok(mise::QualifiedCheck::new(
        PathBuf::from("/tmp/owned-acquisition"),
        source,
        check,
        CheckCapabilityProof { container: None },
        &[],
    )?)
}

#[test]
fn source_build_uses_bound_compiler_offline_vendor_and_typed_features() -> TestResult {
    let owned = fixture(vec![tool("rust-custom", true), tool("codebook", false)])?;
    let command = owned.qualified_source_command("codebook")?;
    let argv: Vec<_> = command
        .argv()
        .iter()
        .map(|s| s.to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        argv,
        vec![
            "/tmp/owned-acquisition/tools/rust-custom/prefix/bin/cargo",
            "install",
            "--locked",
            "--offline",
            "--no-track",
            "--path",
            "/tmp/owned-acquisition/tools/codebook/sources",
            "--root",
            "/tmp/owned-acquisition/tools/codebook/prefix",
            "--config",
            "/tmp/owned-acquisition/tools/codebook/cargo-source.toml",
            "--no-default-features",
            "--features",
            "cli",
        ]
    );
    let env = command.full_env();
    assert!(
        env.iter().any(|(k, v)| k == "RUSTC"
            && v == "/tmp/owned-acquisition/tools/rust-custom/prefix/bin/rustc")
    );
    assert!(
        env.iter()
            .any(|(k, v)| k == "CARGO_NET_OFFLINE" && v == "true")
    );
    assert!(
        env.iter()
            .any(|(k, v)| k == "CARGO_HOME" && v == "/tmp/owned-acquisition/tools/codebook/cargo")
    );
    assert!(
        env.iter()
            .any(|(k, v)| k == "MISE_CARGO_HOME"
                && v == "/tmp/owned-acquisition/tools/codebook/cargo")
    );
    assert!(
        env.iter().any(|(k, v)| k == "RUSTUP_TOOLCHAIN"
            && v == "/tmp/owned-acquisition/tools/rust-custom/prefix")
    );
    assert!(env.iter().any(|(k, v)| k == "RUSTDOC"
        && v == "/tmp/owned-acquisition/tools/rust-custom/prefix/bin/rustdoc"));
    assert_eq!(
        command.cwd(),
        Some(&PathBuf::from("/tmp/owned-acquisition/tools/codebook"))
    );
    assert!(owned.qualified_source_command("rust-custom").is_err());
    assert!(owned.qualified_source_command("unknown").is_err());
    Ok(())
}

#[test]
fn local_link_and_fetch_cannot_add_install_fallback_or_raw_source() -> TestResult {
    let owned = fixture(vec![tool("rust-custom", true), tool("codebook", false)])?;
    let link = owned.qualified_link_command("codebook")?;
    let argv: Vec<_> = link
        .argv()
        .iter()
        .map(|s| s.to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        argv,
        vec![
            "/tmp/owned-acquisition/bin/mise",
            "--no-config",
            "--no-env",
            "--no-hooks",
            "link",
            "cargo:codebook@0.3.4",
            "/tmp/owned-acquisition/tools/codebook/prefix"
        ]
    );
    let fetch = owned.qualified_fetch_command("codebook", false, 0)?;
    let argv = fetch.argv();
    assert_eq!(argv[0], "/usr/bin/curl");
    assert_eq!(argv[1], "--disable");
    assert!(
        argv.windows(2)
            .any(|p| p[0] == "--max-filesize" && p[1] == "1073741824")
    );
    assert_eq!(
        argv.last().and_then(|v| v.to_str()),
        Some("https://static.crates.io/crates/codebook/codebook-0.3.4.crate")
    );
    let download = format!(
        "/tmp/owned-acquisition/tools/codebook/downloads/{}",
        "b".repeat(64)
    );
    assert!(argv.iter().any(|v| v.to_str() == Some(download.as_str())));
    assert!(owned.qualified_fetch_command("codebook", true, 0).is_err());
    assert!(owned.qualified_fetch_command("codebook", false, 1).is_err());
    Ok(())
}

#[test]
fn source_acquisition_requires_pinned_rust_programs_and_safe_paths() -> TestResult {
    let source = tool("codebook", false);
    let owned = fixture(vec![source.clone()])?;
    assert!(owned.qualified_source_command("codebook").is_err());
    let mut rust = tool("rust-custom", true);
    rust.platforms[0]
        .executables
        .retain(|exe| exe.name != "cargo");
    let owned = fixture(vec![rust, source.clone()])?;
    assert!(owned.qualified_source_command("codebook").is_err());
    let mut source = source;
    source.platforms[0].executables[0].path = "../../outside/codebook".into();
    let owned = fixture(vec![tool("rust-custom", true), source])?;
    assert!(owned.qualified_link_command("codebook").is_err());
    assert!(owned.qualified_fetch_command("codebook", false, 0).is_err());
    Ok(())
}
