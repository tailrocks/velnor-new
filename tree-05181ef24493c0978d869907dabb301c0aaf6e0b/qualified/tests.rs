//! Synthetic qualification bytes test pure admission and identity, never runtime provenance.
use super::*;
use velnor_actions_contract::config::{
    CheckExecutor, CheckPlatform, CheckRunner, MiseCheck, QualifiedCargoInstallation,
    QualifiedTool, QualifiedToolArtifact, QualifiedToolBackend, QualifiedToolExecutable,
    QualifiedToolOptions, QualifiedToolPlatform, QualifiedToolProbe,
};
use velnor_actions_mise_core::MiseError;
pub(super) fn platform(name: &str, version: &str, url: String) -> QualifiedToolPlatform {
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
pub(super) fn node(id: &str, version: &str) -> QualifiedTool {
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
pub(super) fn rust() -> QualifiedTool {
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
pub(super) fn codebook() -> QualifiedTool {
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
pub(super) fn check(platform: CheckPlatform) -> MiseCheck {
    MiseCheck {
        id: "check".to_owned(),
        task: "check".to_owned(),
        directory: ".".to_owned(),
        runner: CheckRunner {
            label: "test-runner".to_owned(),
            platform,
            executor: CheckExecutor::Hosted,
            container: None,
        },
        inputs: Vec::new(),
        tools: Vec::new(),
        system_tools: Vec::new(),
        evidence: None,
        timeout_minutes: 10,
    }
}
pub(super) fn resolve_on(
    registry: &[QualifiedTool],
    roots: &[String],
    platform: CheckPlatform,
) -> Result<ResolvedTools, MiseError> {
    resolve(registry, roots, &check(platform))
}
mod identity_tests;
mod names_tests;
mod resolve_tests;
