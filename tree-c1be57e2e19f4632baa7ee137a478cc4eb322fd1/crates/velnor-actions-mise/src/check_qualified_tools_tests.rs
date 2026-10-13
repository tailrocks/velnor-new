//! Synthetic qualification bytes test pure admission and identity, never runtime provenance.
use super::*;
use velnor_actions_contract::config::{
    QualifiedCargoInstallation, QualifiedToolArtifact, QualifiedToolExecutable,
    QualifiedToolOptions, QualifiedToolPlatform, QualifiedToolProbe,
};
#[path = "check_qualified_tool_names_tests.rs"]
mod names;
fn platform(name: &str, version: &str, url: String) -> QualifiedToolPlatform {
    QualifiedToolPlatform {
        platform: CheckPlatform::LinuxX64,
        artifacts: vec![QualifiedToolArtifact {
            url,
            sha256: "a".repeat(64),
        }],
        dependency_artifacts: Vec::new(),
        install_tree_sha256: "b".repeat(64),
        executables: vec![QualifiedToolExecutable {
            name: name.to_owned(),
            path: format!("bin/{name}"),
            sha256: "c".repeat(64),
            probe: QualifiedToolProbe::Version {
                expected: format!("{name} {version}"),
            },
        }],
    }
}
fn node(id: &str, version: &str) -> QualifiedTool {
    QualifiedTool {
        id: id.to_owned(),
        backend: QualifiedToolBackend::Core {
            tool: "node".to_owned(),
        },
        version: version.to_owned(),
        options: QualifiedToolOptions::Default,
        depends_on: Vec::new(),
        platforms: vec![platform(
            "node",
            version,
            format!("https://nodejs.org/dist/v{version}/node-v{version}-linux-x64.tar.xz"),
        )],
    }
}
fn rust() -> QualifiedTool {
    let mut platform = platform(
        "rustc",
        "1.97.1",
        "https://static.rust-lang.org/dist/rust-1.97.1-x86_64-unknown-linux-gnu.tar.xz".to_owned(),
    );
    platform.executables[0].probe = QualifiedToolProbe::RustcVerbose {
        expected: format!(
            "rustc 1.97.1\ncommit-hash: {}\nhost: x86_64-unknown-linux-gnu\nrelease: 1.97.1",
            "d".repeat(40)
        ),
    };
    QualifiedTool {
        id: "rust".to_owned(),
        backend: QualifiedToolBackend::Core {
            tool: "rust".to_owned(),
        },
        version: "1.97.1".to_owned(),
        options: QualifiedToolOptions::Rust {
            components: vec!["clippy".to_owned(), "rustfmt".to_owned()],
            targets: Vec::new(),
        },
        depends_on: Vec::new(),
        platforms: vec![platform],
    }
}
fn codebook() -> QualifiedTool {
    QualifiedTool {
        id: "codebook".to_owned(),
        backend: QualifiedToolBackend::Cargo {
            crate_name: "codebook-lsp".to_owned(),
        },
        version: "0.3.42".to_owned(),
        options: QualifiedToolOptions::Cargo {
            default_features: false,
            features: Vec::new(),
            installation: QualifiedCargoInstallation::Source {
                source_lock_sha256: "d".repeat(64),
            },
        },
        depends_on: vec!["rust".to_owned()],
        platforms: vec![platform(
            "codebook-lsp",
            "0.3.42",
            "https://static.crates.io/crates/codebook-lsp/codebook-lsp-0.3.42.crate".to_owned(),
        )],
    }
}
#[test]
fn explicit_named_rust_retains_the_qualified_repository_version() {
    let resolved = names::resolve_on(&[rust()], &["rust".to_owned()], CheckPlatform::LinuxX64)
        .expect("qualified override");
    assert_eq!(resolved.specs, ["rust@1.97.1"]);
    assert_eq!(resolved.declarations[0].version, "1.97.1");
}

#[test]
fn undeclared_tools_never_resolve_from_the_compiled_catalog() {
    for id in ["rust", "cargo-nextest", "node", "unknown"] {
        assert!(names::resolve_on(&[], &[id.to_owned()], CheckPlatform::LinuxX64).is_err());
    }
    assert!(fingerprint(&[], &["rust@1.98.1".to_owned()]).is_err());
    assert!(fingerprint(&[rust()], &[]).is_err());
}
#[test]
fn installation_dependencies_are_ordered_without_task_graph_edges() {
    let resolved = names::resolve_on(
        &[codebook(), rust()],
        &["codebook".to_owned()],
        CheckPlatform::LinuxX64,
    )
    .expect("closure");
    let ids: Vec<_> = resolved
        .declarations
        .iter()
        .map(|tool| tool.id.as_str())
        .collect();
    assert_eq!(ids, ["rust", "codebook"]);
    assert!(resolved.specs.contains(&"rust@1.97.1".to_owned()));
    assert!(
        resolved
            .specs
            .contains(&"cargo:codebook-lsp@0.3.42".to_owned())
    );
}
#[test]
fn every_qualification_and_option_dimension_changes_the_identity() {
    let registry = vec![codebook(), rust()];
    let digest = |records: &[QualifiedTool]| {
        names::resolve_on(records, &["codebook".to_owned()], CheckPlatform::LinuxX64)
            .expect("qualified closure")
            .fingerprint
    };
    let before = digest(&registry);
    let mut changed = registry.clone();
    changed[0].platforms[0].artifacts[0].sha256 = "e".repeat(64);
    assert_ne!(before, digest(&changed));
    let mut changed = registry.clone();
    changed[0].platforms[0].install_tree_sha256 = "e".repeat(64);
    assert_ne!(before, digest(&changed));
    let mut changed = registry.clone();
    changed[0].platforms[0].executables[0].sha256 = "e".repeat(64);
    assert_ne!(before, digest(&changed));
    let mut changed = registry;
    let QualifiedToolOptions::Cargo {
        default_features, ..
    } = &mut changed[0].options
    else {
        panic!("cargo fixture");
    };
    *default_features = true;
    assert_ne!(before, digest(&changed));
}
#[test]
fn qualified_scope_is_platform_specific_and_backend_conflicts_fail() {
    let row = node("node", "24.18.0");
    assert!(
        names::resolve_on(
            std::slice::from_ref(&row),
            &["node".to_owned()],
            CheckPlatform::MacosArm64,
        )
        .is_err()
    );
    let alternate = node("other-node", "24.17.0");
    assert!(
        names::resolve_on(
            &[row, alternate],
            &["node".to_owned(), "other-node".to_owned()],
            CheckPlatform::LinuxX64,
        )
        .is_err()
    );
}

#[test]
fn pure_projection_preserves_cargo_features_and_explicit_rust_profile() {
    let projected =
        super::super::projection::config_for(&codebook()).expect("typed source options");
    assert!(projected.contains("default-features = false"));
    assert!(!projected.contains("[settings]"));
    let projected = super::super::projection::config_for(&rust()).expect("typed Rust options");
    assert!(projected.contains("profile = \"minimal\""));
    assert!(projected.contains("components = [\"clippy\", \"rustfmt\"]"));
    assert!(projected.contains("version = \"1.97.1\""));
}

#[test]
fn prebuilt_cargo_qualification_has_no_synthetic_installer_dependency() {
    let mut tool = codebook();
    tool.depends_on.clear();
    tool.options = QualifiedToolOptions::Cargo {
        default_features: true,
        features: Vec::new(),
        installation: QualifiedCargoInstallation::Prebuilt {
            repository: "codebook/codebook".to_owned(),
        },
    };
    tool.platforms[0].artifacts[0].url =
        "https://github.com/codebook/codebook/releases/download/v0.3.42/codebook-linux-x64.tar.gz"
            .to_owned();
    let resolved = names::resolve_on(
        std::slice::from_ref(&tool),
        &["codebook".to_owned()],
        CheckPlatform::LinuxX64,
    )
    .expect("direct qualified archive");
    assert_eq!(resolved.declarations, [tool.clone()]);
    assert_eq!(resolved.specs, ["cargo:codebook-lsp@0.3.42"]);
    let projection = super::super::projection::config_for(&tool).expect("options only");
    assert!(!projection.contains("[settings]"));
}

#[test]
fn canonical_fingerprint_ignores_dependency_transport_order() {
    let resolved = names::resolve_on(
        &[codebook(), rust()],
        &["codebook".to_owned()],
        CheckPlatform::LinuxX64,
    )
    .expect("closure");
    let mut reversed = resolved.declarations.clone();
    reversed.reverse();
    assert_eq!(
        resolved.fingerprint,
        fingerprint(&reversed, &resolved.specs).expect("canonical closure")
    );
}

fn discovered_qualification() -> super::super::DiscoveredCheck {
    use velnor_actions_contract::config::{CheckExecutor, CheckRunner, MiseCheck};
    let resolved = names::resolve_on(&[rust()], &["rust".to_owned()], CheckPlatform::LinuxX64)
        .expect("qualified Rust");
    let check = MiseCheck {
        id: "verify".to_owned(),
        task: "verify".to_owned(),
        directory: ".".to_owned(),
        runner: CheckRunner {
            label: "ubuntu-24.04".to_owned(),
            platform: CheckPlatform::LinuxX64,
            executor: CheckExecutor::Hosted,
            container: None,
        },
        inputs: Vec::new(),
        tools: vec!["rust".to_owned()],
        system_tools: Vec::new(),
        evidence: None,
        timeout_minutes: 10,
    };
    let proposal = super::super::propose_check(
        &check,
        "native-task-source",
        &[],
        &resolved.specs,
        &resolved.fingerprint,
    )
    .expect("opaque proposal");
    super::super::DiscoveredCheck {
        check,
        proposal,
        config_inputs: Vec::new(),
        task_config: String::new(),
        tool_specs: resolved.specs,
        qualified_tools: resolved.declarations,
        qualification_digest: resolved.fingerprint,
        config_source: String::new(),
    }
}

#[test]
fn altered_qualification_cannot_retain_the_discovered_proposal_binding() {
    let original = discovered_qualification();
    original
        .verify_qualification_identity()
        .expect("exact binding");
    let mut changed = original.clone();
    changed.qualified_tools[0].platforms[0].install_tree_sha256 = "e".repeat(64);
    assert!(changed.verify_qualification_identity().is_err());
    let mut changed = original.clone();
    changed.tool_specs = vec!["rust@1.98.1".to_owned()];
    assert!(changed.verify_qualification_identity().is_err());
    let mut changed = original;
    changed
        .proposal
        .identity
        .flags
        .retain(|flag| !flag.starts_with("qualified_tools:"));
    assert!(changed.verify_qualification_identity().is_err());
}

#[test]
fn every_host_container_field_changes_the_runner_identity() {
    use velnor_actions_contract::config::{
        ContainerPlatform, DaemonIdentityPolicy, HostContainerProfile, HostDockerCli,
        HostDockerDaemon,
    };
    let mut item = discovered_qualification();
    item.check.runner.container = Some(HostContainerProfile::Docker {
        context: "default".to_owned(),
        socket_path: "/var/run/docker.sock".to_owned(),
        socket_uid: 0,
        cli: HostDockerCli {
            path: "/usr/local/bin/docker".to_owned(),
            sha256: "a".repeat(64),
            version: "28.5.1".to_owned(),
            build: "deadbee".to_owned(),
        },
        daemon: HostDockerDaemon {
            version: "28.5.1".to_owned(),
            platform: ContainerPlatform::LinuxX64,
            operating_system: "Ubuntu 24.04".to_owned(),
            identity_policy: DaemonIdentityPolicy::ExecutionScoped,
        },
    });
    let flags = |item: &super::super::DiscoveredCheck| {
        super::super::check_identity(
            &item.check,
            "native-task-source",
            &[],
            &item.tool_specs,
            &item.qualification_digest,
        )
        .expect("complete runner identity")
        .flags
    };
    let original = flags(&item);
    for field in 0..10 {
        let mut changed = item.clone();
        let Some(HostContainerProfile::Docker {
            context,
            socket_path,
            socket_uid,
            cli,
            daemon,
        }) = &mut changed.check.runner.container
        else {
            panic!("typed Docker fixture");
        };
        match field {
            0 => *context = "isolated".to_owned(),
            1 => *socket_path = "/run/user/1000/docker.sock".to_owned(),
            2 => cli.path = "/opt/docker/bin/docker".to_owned(),
            3 => cli.sha256 = "b".repeat(64),
            4 => cli.version = "28.5.2".to_owned(),
            5 => cli.build = "feedbee".to_owned(),
            6 => daemon.version = "28.5.2".to_owned(),
            7 => daemon.platform = ContainerPlatform::LinuxArm64,
            8 => daemon.operating_system = "Debian 13".to_owned(),
            9 => *socket_uid = 1000,
            _ => unreachable!(),
        }
        assert_ne!(original, flags(&changed), "unbound container field {field}");
    }
}
