//! Hash-free candidate recipes share runtime's closed metadata admission.
use super::*;

fn recipe(
    tool: &str,
    version: &str,
    url: &str,
    name: &str,
    path: &str,
    expected: &str,
) -> QualifiedToolRecipe {
    QualifiedToolRecipe {
        id: tool.to_owned(),
        backend: QualifiedToolBackend::Core {
            tool: tool.to_owned(),
        },
        version: version.to_owned(),
        options: QualifiedToolOptions::Default,
        depends_on: Vec::new(),
        platforms: vec![QualifiedToolRecipePlatform {
            platform: CheckPlatform::LinuxX64,
            artifacts: vec![QualifiedToolArtifactRecipe {
                url: url.to_owned(),
            }],
            dependency_artifacts: Vec::new(),
            executables: vec![QualifiedToolExecutableRecipe {
                name: name.to_owned(),
                path: path.to_owned(),
                probe: if tool == "zig" {
                    QualifiedToolProbe::VersionSubcommand {
                        expected: expected.to_owned(),
                    }
                } else {
                    QualifiedToolProbe::Version {
                        expected: expected.to_owned(),
                    }
                },
            }],
        }],
    }
}

fn zig() -> QualifiedToolRecipe {
    recipe(
        "zig",
        "0.17.0",
        "https://ziglang.org/download/0.17.0/zig-x86_64-linux-0.17.0.tar.xz",
        "zig",
        "zig",
        "0.17.0",
    )
}

fn python() -> QualifiedToolRecipe {
    recipe(
        "python",
        "3.15.0",
        "https://github.com/astral-sh/python-build-standalone/releases/download/20261013/cpython-3.15.0%2B20261013-x86_64-unknown-linux-gnu-install_only_stripped.tar.gz",
        "python",
        "bin/python3.15",
        "Python 3.15.0",
    )
}

#[test]
fn official_core_zig_and_python_recipes_are_accepted_without_hash_placeholders() {
    assert!(validate_qualified_tool_recipe(&zig(), "candidate", "tool").is_ok());
    assert!(validate_qualified_tool_recipe(&python(), "candidate", "tool").is_ok());
    let serialized = serde_json::to_value(python()).expect("recipe JSON");
    assert!(
        serialized["platforms"][0]["artifacts"][0]
            .get("sha256")
            .is_none()
    );
    assert!(
        serialized["platforms"][0]
            .get("install_tree_sha256")
            .is_none()
    );
}

#[test]
fn zig_recipe_is_exactly_host_version_platform_and_archive_bound() {
    let valid = zig();
    for invalid in [
        valid
            .clone()
            .with_url("http://ziglang.org/download/0.17.0/zig-x86_64-linux-0.17.0.tar.xz"),
        valid
            .clone()
            .with_url("https://attacker.invalid/download/0.17.0/zig-x86_64-linux-0.17.0.tar.xz"),
        valid
            .clone()
            .with_url("https://ziglang.org/download/0.17.1/zig-x86_64-linux-0.17.0.tar.xz"),
        valid
            .clone()
            .with_url("https://ziglang.org/download/0.17.0/zig-aarch64-macos-0.17.0.tar.xz"),
        valid
            .clone()
            .with_url("https://ziglang.org/download/0.17.0/zig-x86_64-linux-0.17.0.zip"),
        valid
            .clone()
            .with_url("https://ziglang.org/download/0.17.0/zig-x86_64-linux-0.17.0.tar.xz?x=1"),
    ] {
        assert!(
            invalid.validate("candidate", "tool").is_err(),
            "{invalid:?}"
        );
    }
}

#[test]
fn python_recipe_admits_only_canonical_encoded_official_release_asset() {
    let valid = python();
    for invalid in [
        valid.clone().with_url("https://github.com/attacker/python-build-standalone/releases/download/20261013/cpython-3.15.0%2B20261013-x86_64-unknown-linux-gnu-install_only_stripped.tar.gz"),
        valid.clone().with_url("https://github.com/astral-sh/python-build-standalone/releases/download/20261012/cpython-3.15.0%2B20261013-x86_64-unknown-linux-gnu-install_only_stripped.tar.gz"),
        valid.clone().with_url("https://github.com/astral-sh/python-build-standalone/releases/download/20261013/cpython-3.15.0+20261013-x86_64-unknown-linux-gnu-install_only_stripped.tar.gz"),
        valid.clone().with_url("https://github.com/astral-sh/python-build-standalone/releases/download/20261013/cpython-3.15.0%2b20261013-x86_64-unknown-linux-gnu-install_only_stripped.tar.gz"),
        valid.clone().with_url("https://github.com/astral-sh/python-build-standalone/releases/download/20261013/cpython-3.15.0%2F20261013-x86_64-unknown-linux-gnu-install_only_stripped.tar.gz"),
        valid.clone().with_url("https://github.com/astral-sh/python-build-standalone/releases/download/20260230/cpython-3.15.0%2B20260230-x86_64-unknown-linux-gnu-install_only_stripped.tar.gz"),
        valid.clone().with_url("https://github.com/astral-sh/python-build-standalone/releases/download/20261013/cpython-3.15.0%2B20261013-aarch64-apple-darwin-install_only_stripped.tar.gz"),
        valid.clone().with_url("https://github.com/astral-sh/python-build-standalone/releases/download/20261013/cpython-3.15.0%2B20261013-x86_64-unknown-linux-gnu-install_only.tar.gz?download=1"),
    ] {
        assert!(invalid.validate("candidate", "tool").is_err(), "{invalid:?}");
    }
}

#[test]
fn core_layouts_bind_only_the_upstream_executable_and_probe_shape() {
    let mut zig_recipe = zig();
    zig_recipe.platforms[0].executables[0].path = "bin/zig".to_owned();
    assert!(zig_recipe.validate("candidate", "tool").is_err());
    let mut zig_recipe = zig();
    zig_recipe.platforms[0].executables[0].probe = QualifiedToolProbe::Version {
        expected: "0.17.0".to_owned(),
    };
    assert!(zig_recipe.validate("candidate", "tool").is_err());

    let mut python_recipe = python();
    python_recipe.platforms[0].executables[0].path = "bin/python".to_owned();
    assert!(python_recipe.validate("candidate", "tool").is_err());
    let mut python_recipe = python();
    python_recipe.platforms[0].executables[0].name = "python3.15".to_owned();
    assert!(python_recipe.validate("candidate", "tool").is_err());
    let mut python_recipe = python();
    python_recipe.platforms[0].executables[0].probe = QualifiedToolProbe::VersionSubcommand {
        expected: "Python 3.15.0".to_owned(),
    };
    assert!(python_recipe.validate("candidate", "tool").is_err());
}

#[test]
fn official_asset_mappings_cover_only_declared_runner_platforms() {
    let mut zig_arm = zig();
    zig_arm.platforms[0].platform = CheckPlatform::MacosArm64;
    zig_arm.platforms[0].artifacts[0].url =
        "https://ziglang.org/download/0.17.0/zig-aarch64-macos-0.17.0.tar.xz".to_owned();
    assert!(zig_arm.validate("candidate", "tool").is_ok());
    let mut zig_intel = zig_arm.clone();
    zig_intel.platforms[0].platform = CheckPlatform::MacosX64;
    zig_intel.platforms[0].artifacts[0].url =
        "https://ziglang.org/download/0.17.0/zig-x86_64-macos-0.17.0.tar.xz".to_owned();
    assert!(zig_intel.validate("candidate", "tool").is_ok());

    let mut python_arm = python();
    python_arm.platforms[0].platform = CheckPlatform::MacosArm64;
    python_arm.platforms[0].artifacts[0].url =
        "https://github.com/astral-sh/python-build-standalone/releases/download/20261013/cpython-3.15.0%2B20261013-aarch64-apple-darwin-install_only_stripped.tar.gz".to_owned();
    assert!(python_arm.validate("candidate", "tool").is_ok());
    let mut python_intel = python_arm.clone();
    python_intel.platforms[0].platform = CheckPlatform::MacosX64;
    python_intel.platforms[0].artifacts[0].url =
        "https://github.com/astral-sh/python-build-standalone/releases/download/20261013/cpython-3.15.0%2B20261013-x86_64-apple-darwin-install_only_stripped.tar.gz".to_owned();
    assert!(python_intel.validate("candidate", "tool").is_ok());
}

#[test]
fn complete_tool_conversion_preserves_recipe_and_requires_measured_hashes() {
    let tool = QualifiedTool {
        id: "zig".to_owned(),
        backend: QualifiedToolBackend::Core {
            tool: "zig".to_owned(),
        },
        version: "0.17.0".to_owned(),
        options: QualifiedToolOptions::Default,
        depends_on: Vec::new(),
        platforms: vec![QualifiedToolPlatform {
            platform: CheckPlatform::LinuxX64,
            artifacts: vec![QualifiedToolArtifact {
                url: "https://ziglang.org/download/0.17.0/zig-x86_64-linux-0.17.0.tar.xz"
                    .to_owned(),
                sha256: "a".repeat(64),
            }],
            dependency_artifacts: Vec::new(),
            install_tree_sha256: "b".repeat(64),
            executables: vec![QualifiedToolExecutable {
                name: "zig".to_owned(),
                path: "zig".to_owned(),
                sha256: "c".repeat(64),
                probe: QualifiedToolProbe::VersionSubcommand {
                    expected: "0.17.0".to_owned(),
                },
            }],
        }],
    };
    assert_eq!(QualifiedToolRecipe::from(&tool), zig());
    assert!(tool.validate("config", "qualified_tools[0]").is_ok());
    let mut missing_measurement = tool;
    missing_measurement.platforms[0].artifacts[0].sha256.clear();
    assert!(
        missing_measurement
            .validate("config", "qualified_tools[0]")
            .is_err()
    );
}

trait WithUrl {
    fn with_url(self, url: &str) -> Self;
}

impl WithUrl for QualifiedToolRecipe {
    fn with_url(mut self, url: &str) -> Self {
        self.platforms[0].artifacts[0].url = url.to_owned();
        self
    }
}
