//! Exact native source selectors are handed to the owner without installer fallback.
use super::*;
use crate::{DiscoveredCheck, QualifiedCheck};
use std::collections::BTreeMap;
use velnor_actions_contract::config::{
    CheckExecutor, CheckPlatform, CheckRunner, MiseCheck, QualifiedToolArtifact,
    QualifiedToolExecutable, QualifiedToolPlatform, QualifiedToolProbe,
};
use velnor_actions_contract::graph::{CachePolicy, ResourceClass, ResourceDemand};
use velnor_actions_contract::propose::{IdentityInputs, ProposedTask};

fn core_tool(id: &str) -> QualifiedTool {
    let (version, backend, url, name, path, probe) = match id {
        "zig" => (
            "0.17.0",
            "zig",
            "https://ziglang.org/download/0.17.0/zig-x86_64-linux-0.17.0.tar.xz",
            "zig",
            "zig",
            QualifiedToolProbe::VersionSubcommand {
                expected: "0.17.0".to_owned(),
            },
        ),
        "python" => (
            "3.15.0",
            "python",
            "https://github.com/astral-sh/python-build-standalone/releases/download/20261013/cpython-3.15.0%2B20261013-x86_64-unknown-linux-gnu-install_only_stripped.tar.gz",
            "python",
            "bin/python3.15",
            QualifiedToolProbe::Version {
                expected: "Python 3.15.0".to_owned(),
            },
        ),
        _ => unreachable!("fixture identity is closed"),
    };
    QualifiedTool {
        id: id.to_owned(),
        backend: QualifiedToolBackend::Core {
            tool: backend.to_owned(),
        },
        version: version.to_owned(),
        options: QualifiedToolOptions::Default,
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
                name: name.to_owned(),
                path: path.to_owned(),
                sha256: "c".repeat(64),
                probe,
            }],
        }],
    }
}

fn qualified(tools: Vec<QualifiedTool>) -> QualifiedCheck {
    let check = MiseCheck {
        id: "native-acquire".to_owned(),
        task: "check:tools".to_owned(),
        directory: ".".to_owned(),
        runner: CheckRunner {
            label: "ubuntu-26.04".to_owned(),
            platform: CheckPlatform::LinuxX64,
            executor: CheckExecutor::Hosted,
            container: None,
        },
        inputs: Vec::new(),
        tools: Vec::new(),
        system_tools: Vec::new(),
        evidence: None,
        timeout_minutes: 10,
    };
    QualifiedCheck {
        home: PathBuf::from("/tmp/native-acquire-home"),
        config: PathBuf::from("/tmp/native-acquire-home/tasks.toml"),
        cwd: PathBuf::from("/tmp/native-acquire-work"),
        check: DiscoveredCheck {
            check,
            proposal: ProposedTask {
                task_id: "native-acquire".to_owned(),
                stack_id: "rust".to_owned(),
                component_id: "native-acquire".to_owned(),
                task_kind: "check".to_owned(),
                configuration: String::new(),
                depends_on: Vec::new(),
                gated_by: Vec::new(),
                reads: Vec::new(),
                writes: Vec::new(),
                outputs: Vec::new(),
                resource: ResourceDemand {
                    class: ResourceClass::Lightweight,
                    cpu_milli: None,
                    memory_mb: None,
                    needs_network: false,
                    service: None,
                },
                cache_policy: CachePolicy {
                    allow_compilation_reuse: false,
                    allow_task_reuse: false,
                },
                identity: IdentityInputs {
                    unit_id: String::new(),
                    unit_key: "native-acquire".to_owned(),
                    unit_path: "checks/native-acquire".to_owned(),
                    project_root: ".".to_owned(),
                    target: "x86_64-unknown-linux-gnu".to_owned(),
                    features: Vec::new(),
                    flags: Vec::new(),
                    compile_driver: "none".to_owned(),
                    test_runner: "none".to_owned(),
                    environment: BTreeMap::new(),
                    declared_inputs: Vec::new(),
                    undeclared_reads: false,
                },
                payload: Vec::new(),
                display_name: String::new(),
                uses_clock: false,
                uses_random: false,
                no_targets: false,
                runner_profile: String::new(),
            },
            config_inputs: Vec::new(),
            task_config: String::new(),
            tool_specs: Vec::new(),
            qualified_tools: tools,
            qualification_digest: String::new(),
            config_source: String::new(),
        },
        capability_env: Vec::new(),
    }
}

#[test]
fn official_zig_and_python_use_the_fixed_fetch_and_link_commands() {
    for (id, url, selector) in [
        (
            "zig",
            "https://ziglang.org/download/0.17.0/zig-x86_64-linux-0.17.0.tar.xz",
            "core:zig@0.17.0",
        ),
        (
            "python",
            "https://github.com/astral-sh/python-build-standalone/releases/download/20261013/cpython-3.15.0%2B20261013-x86_64-unknown-linux-gnu-install_only_stripped.tar.gz",
            "core:python@3.15.0",
        ),
    ] {
        let check = qualified(vec![core_tool(id)]);
        let fetch = check
            .qualified_fetch_command(id, false, 0)
            .expect("fixed fetch");
        assert_eq!(fetch.program, "/usr/bin/curl");
        assert!(fetch.args.iter().any(|arg| arg == url));
        assert!(fetch.args.windows(2).any(
            |pair| pair[0] == "--output" && pair[1].to_string_lossy().contains(&"a".repeat(64))
        ));
        let link = check.qualified_link_command(id).expect("local link");
        assert!(link.args.iter().any(|arg| arg == selector));
        assert!(!link.args.iter().any(|arg| arg == "install"));
    }
}
