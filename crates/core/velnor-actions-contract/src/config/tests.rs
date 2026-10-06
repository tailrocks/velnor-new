//! Named checks require explicit qualified declarations, including former catalog IDs.
use super::*;

fn config(tool: Option<&str>) -> VelnorConfig {
    serde_json::from_value(serde_json::json!({
        "schema": 1,
        "workflow": {"name":"CI", "policy":"consumer-v1", "generator_validation":"bootstrap", "max_parallel_jobs":2},
        "resources": {"compiler_process_budget":2,"test_process_budget":2},
        "test_sharding": {"default_shards":1,"by_manifest":{}},
        "stacks": {"ignore":[]}, "discovery": {"exclude":[]},
        "checks": [{"id":"native", "task":"test:native", "inputs":["mise.toml"], "tools":tool.into_iter().collect::<Vec<_>>(),
            "runner":{"label":"ubuntu-26.04","platform":"linux_x64","executor":"hosted"}}]
    })).expect("config fixture")
}

fn declaration(id: &str) -> QualifiedTool {
    let (version, backend, options, url, executable, expected) = if id == "rust" {
        (
            "1.97.1",
            QualifiedToolBackend::Core {
                tool: "rust".to_owned(),
            },
            QualifiedToolOptions::Rust {
                components: Vec::new(),
                targets: Vec::new(),
            },
            "https://static.rust-lang.org/dist/rust-1.97.1-x86_64-unknown-linux-gnu.tar.xz",
            "rustc",
            "rustc 1.97.1",
        )
    } else {
        (
            "0.9.140",
            QualifiedToolBackend::Aqua {
                package: "nextest-rs/nextest/cargo-nextest".to_owned(),
            },
            QualifiedToolOptions::Default,
            "https://github.com/nextest-rs/nextest/releases/download/cargo-nextest-0.9.140/cargo-nextest-linux-x64.tar.gz",
            "cargo-nextest",
            "cargo-nextest 0.9.140",
        )
    };
    QualifiedTool {
        id: id.to_owned(),
        version: version.to_owned(),
        backend,
        options,
        depends_on: Vec::new(),
        platforms: vec![QualifiedToolPlatform {
            platform: CheckPlatform::LinuxX64,
            artifacts: vec![QualifiedToolArtifact {
                url: url.to_owned(),
                sha256: "a".repeat(64),
            }],
            dependency_artifacts: Vec::new(),
            install_tree_sha256: "b".repeat(64),
            executables: vec![QualifiedToolExecutable {
                name: executable.to_owned(),
                path: format!("bin/{executable}"),
                sha256: "c".repeat(64),
                probe: QualifiedToolProbe::Version {
                    expected: expected.to_owned(),
                },
            }],
        }],
    }
}

#[test]
fn former_catalog_ids_are_rejected_without_explicit_qualification() {
    for id in ["rust", "nextest", "unknown"] {
        let error = config(Some(id))
            .validate("config.toml")
            .expect_err("implicit tool reference");
        assert!(
            error
                .to_string()
                .contains(&format!("unknown_qualified_tool:{id}"))
        );
        assert!(error.to_string().contains("checks[0].tools"));
    }
}

#[test]
fn explicit_rust_and_nextest_declarations_admit_named_references() {
    for id in ["rust", "nextest"] {
        let mut config = config(Some(id));
        config.qualified_tools.push(declaration(id));
        assert!(config.validate("config.toml").is_ok(), "{id}");
    }
}

#[test]
fn native_only_checks_need_no_installed_tool_declarations() {
    assert!(config(None).validate("config.toml").is_ok());
}
