#![cfg(unix)]
use std::path::PathBuf;
use velnor_actions_contract::config::{
    CheckPlatform, QualifiedTool, QualifiedToolArtifact, QualifiedToolBackend,
    QualifiedToolExecutable, QualifiedToolOptions, QualifiedToolPlatform, QualifiedToolProbe,
};
use velnor_actions_mise_probes::check_tool_probes::{
    QualifiedExecutableObservation, QualifiedProbeHomes, validate_executable_proofs,
    verify_qualified_executable,
};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;
struct Fixture {
    root: PathBuf,
    homes: QualifiedProbeHomes,
}
impl Fixture {
    fn new() -> TestResult<Self> {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "qualified-tool-probe-{}-{stamp}",
            std::process::id()
        ));
        std::fs::create_dir(&root)?;
        let root = root.canonicalize()?;
        let homes = QualifiedProbeHomes {
            home: root.clone(),
            bin_dir: root.join("bin"),
            cargo_home: root.join("cargo"),
            rust_home: root.join("rust-home"),
            compiler_toolchain: Some(root.join("compiler")),
        };
        let fixture = Self { root, homes };
        for path in [
            &fixture.homes.bin_dir,
            &fixture.homes.cargo_home,
            &fixture.homes.rust_home,
        ] {
            std::fs::create_dir(path)?;
        }
        std::fs::create_dir(fixture.root.join("compiler"))?;
        Ok(fixture)
    }
    fn observation(&self, name: &str, script: &str) -> TestResult<QualifiedExecutableObservation> {
        use std::os::unix::fs::PermissionsExt;
        let path = self.homes.bin_dir.join(name);
        std::fs::write(&path, format!("#!/bin/sh\nset -eu\n{script}\n"))?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))?;
        Ok(QualifiedExecutableObservation {
            name: name.into(),
            path,
            sha256: "c".repeat(64),
        })
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if let Err(error) = std::fs::remove_dir_all(&self.root) {
            eprintln!("qualified_probe_cleanup_failed:{:?}", error.kind());
        }
    }
}
fn platform() -> TestResult<CheckPlatform> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => Ok(CheckPlatform::LinuxX64),
        ("macos", "aarch64") => Ok(CheckPlatform::MacosArm64),
        ("macos", "x86_64") => Ok(CheckPlatform::MacosX64),
        _ => Err("unsupported fixture host".into()),
    }
}

fn tool(
    name: &str,
    version: &str,
    probe: QualifiedToolProbe,
    platform: CheckPlatform,
) -> QualifiedTool {
    QualifiedTool {
        id: name.into(),
        version: version.into(),
        backend: QualifiedToolBackend::Aqua {
            package: format!("fixture/{name}"),
        },
        options: QualifiedToolOptions::Default,
        depends_on: vec![],
        platforms: vec![QualifiedToolPlatform {
            platform,
            artifacts: vec![QualifiedToolArtifact {
                url: format!(
                    "https://github.com/fixture/{name}/releases/download/v{version}/{name}.tar.gz"
                ),
                sha256: "a".repeat(64),
            }],
            dependency_artifacts: vec![],
            install_tree_sha256: "b".repeat(64),
            executables: vec![QualifiedToolExecutable {
                name: name.into(),
                path: format!("bin/{name}"),
                sha256: "c".repeat(64),
                probe,
            }],
        }],
    }
}

#[test]
fn exact_version_probe_clears_ambient_compiler_selector_and_credentials() -> TestResult {
    if std::env::var_os("QUALIFIED_PROBE_POISON_CHILD").is_none() {
        let output = std::process::Command::new(std::env::current_exe()?)
            .args([
                "--exact",
                "impl_mise_tool_probes::exact_version_probe_clears_ambient_compiler_selector_and_credentials",
                "--nocapture",
            ])
            .env("QUALIFIED_PROBE_POISON_CHILD", "1")
            .env("GITHUB_TOKEN", "poisoned-ambient-token")
            .env("RUSTUP_TOOLCHAIN", "poisoned-download-selector")
            .env("MISE_CONFIG_FILE", "/poisoned/consumer/mise.toml")
            .output()?;
        assert!(output.status.success(), "{output:?}");
        assert!(String::from_utf8(output.stdout)?.contains("1 passed"));
        return Ok(());
    }
    let fixture = Fixture::new()?;
    let platform = platform()?;
    let tool = tool(
        "demo",
        "1.2.3",
        QualifiedToolProbe::Version {
            expected: "demo 1.2.3".into(),
        },
        platform,
    );
    let script = "test \"$1\" = --version; test \"${RUSTUP_TOOLCHAIN-unset}\" = unset; test \"$CARGO_HOME\" = \"$HOME/cargo\"; test \"${GITHUB_TOKEN-unset}\" = unset; test \"${MISE_CONFIG_FILE-unset}\" = unset; test \"${MISE_NO_CONFIG-unset}:${MISE_NO_ENV-unset}:${MISE_NO_HOOKS-unset}:${MISE_LOCKFILE-unset}\" = unset:unset:unset:unset; printf 'demo 1.2.3\\nadditional version detail\\n'";
    let observed = fixture.observation("demo", script)?;
    let proof = verify_qualified_executable(
        &tool,
        platform,
        &tool.platforms[0].executables[0],
        &observed,
        &fixture.homes,
        test_deadline()?,
    )?;
    validate_executable_proofs(&tool, platform, std::slice::from_ref(&proof))?;
    let mut altered = proof;
    altered.stdout.push_str("altered");
    assert!(validate_executable_proofs(&tool, platform, &[altered]).is_err());
    Ok(())
}

#[test]
fn wrong_observed_digest_refuses_before_any_executable_runs() -> TestResult {
    let fixture = Fixture::new()?;
    let platform = platform()?;
    let tool = tool(
        "demo",
        "1.2.3",
        QualifiedToolProbe::Version {
            expected: "demo 1.2.3".into(),
        },
        platform,
    );
    let mut observed =
        fixture.observation("demo", "touch \"$HOME/EXECUTED\"; printf 'demo 1.2.3\\n'")?;
    observed.sha256 = "d".repeat(64);
    assert!(
        verify_qualified_executable(
            &tool,
            platform,
            &tool.platforms[0].executables[0],
            &observed,
            &fixture.homes,
            test_deadline()?
        )
        .is_err()
    );
    assert!(!fixture.root.join("EXECUTED").exists());
    Ok(())
}

#[test]
fn exact_first_line_rejects_prefix_or_patch_version_drift() -> TestResult {
    for output in ["demo 1.2.30", "prefix demo 1.2.3"] {
        let fixture = Fixture::new()?;
        let platform = platform()?;
        let tool = tool(
            "demo",
            "1.2.3",
            QualifiedToolProbe::Version {
                expected: "demo 1.2.3".into(),
            },
            platform,
        );
        let observed = fixture.observation("demo", &format!("printf '%s\\n' '{output}'"))?;
        assert!(
            verify_qualified_executable(
                &tool,
                platform,
                &tool.platforms[0].executables[0],
                &observed,
                &fixture.homes,
                test_deadline()?
            )
            .is_err()
        );
    }
    Ok(())
}

#[test]
fn version_subcommand_is_fixed_positional_and_not_caller_argv() -> TestResult {
    let fixture = Fixture::new()?;
    let platform = platform()?;
    let tool = tool(
        "demo",
        "1.2.3",
        QualifiedToolProbe::VersionSubcommand {
            expected: "demo 1.2.3".into(),
        },
        platform,
    );
    let observed = fixture.observation(
        "demo",
        "test \"$#\" = 1; test \"$1\" = version; printf 'demo 1.2.3\\n'",
    )?;
    verify_qualified_executable(
        &tool,
        platform,
        &tool.platforms[0].executables[0],
        &observed,
        &fixture.homes,
        test_deadline()?,
    )?;
    Ok(())
}

#[test]
fn stderr_version_and_foreign_owned_paths_are_rejected() -> TestResult {
    let fixture = Fixture::new()?;
    let foreign = Fixture::new()?;
    let platform = platform()?;
    let tool = tool(
        "demo",
        "1.2.3",
        QualifiedToolProbe::Version {
            expected: "demo 1.2.3".into(),
        },
        platform,
    );
    let declared = &tool.platforms[0].executables[0];
    let observed = fixture.observation("demo", "printf 'demo 1.2.3\\n' >&2")?;
    assert!(
        verify_qualified_executable(
            &tool,
            platform,
            declared,
            &observed,
            &fixture.homes,
            test_deadline()?
        )
        .is_err()
    );
    let foreign_observed = foreign.observation("demo", "printf 'demo 1.2.3\\n'")?;
    assert!(
        verify_qualified_executable(
            &tool,
            platform,
            declared,
            &foreign_observed,
            &fixture.homes,
            test_deadline()?
        )
        .is_err()
    );
    Ok(())
}

#[test]
fn bun_probe_ignores_an_uninstalled_future_rust_prefix() -> TestResult {
    let fixture = Fixture::new()?;
    let platform = platform()?;
    let mut tool = tool(
        "bun",
        "1.3.0",
        QualifiedToolProbe::Version {
            expected: "1.3.0".into(),
        },
        platform,
    );
    tool.backend = QualifiedToolBackend::Core { tool: "bun".into() };
    tool.platforms[0].artifacts[0].url =
        "https://github.com/oven-sh/bun/releases/download/bun-v1.3.0/bun.zip".into();
    let observed = fixture.observation(
        "bun",
        "test \"${RUSTUP_TOOLCHAIN-unset}\" = unset; printf '1.3.0\\n'",
    )?;
    let mut homes = fixture.homes.clone();
    let future = fixture.root.join("tools/future-rust/prefix");
    assert!(!future.exists());
    homes.compiler_toolchain = Some(future);
    let proof = verify_qualified_executable(
        &tool,
        platform,
        &tool.platforms[0].executables[0],
        &observed,
        &homes,
        test_deadline()?,
    )?;
    validate_executable_proofs(&tool, platform, &[proof])?;
    Ok(())
}

fn test_deadline() -> TestResult<velnor_actions_mise_core::CheckDeadline> {
    Ok(velnor_actions_mise_core::CheckDeadline::after(
        std::time::Duration::from_secs(60),
    )?)
}

#[test]
fn missing_proofs_fail_the_count_gate_before_identity_checks() -> TestResult {
    let platform = platform()?;
    let tool = tool(
        "probe-count",
        "0.0.0",
        QualifiedToolProbe::Version {
            expected: "probe-count 0.0.0".into(),
        },
        platform,
    );
    assert!(validate_executable_proofs(&tool, platform, &[]).is_err());
    Ok(())
}

#[path = "impl_mise_tool_probe_rust_tests.rs"]
mod rust_tests;
