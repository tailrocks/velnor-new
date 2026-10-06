//! Tool qualification rejects implicit backends, unsafe data and incomplete identity.
use super::*;

mod cargo;
mod compiler;
mod declaration;

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
