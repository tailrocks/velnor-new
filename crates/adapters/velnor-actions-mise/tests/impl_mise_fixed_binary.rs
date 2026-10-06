#![cfg(unix)]

use std::path::{Path, PathBuf};
use velnor_actions_contract::config::{
    CheckExecutor, CheckPlatform, CheckRunner, MiseCheck, QualifiedTool, QualifiedToolArtifact,
    QualifiedToolBackend, QualifiedToolExecutable, QualifiedToolOptions, QualifiedToolPlatform,
    QualifiedToolProbe,
};
use velnor_actions_mise::checks::{CheckCapabilityProof, QualifiedCheck};
use velnor_actions_mise::{MISE_VERSION, discover_checks};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> TestResult<Self> {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("mise-fixed-binary-{}-{stamp}", std::process::id()));
        std::fs::create_dir(&path)?;
        Ok(Self(path.canonicalize()?))
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if let Err(error) = std::fs::remove_dir_all(&self.0) {
            eprintln!("fixed_binary_fixture_cleanup_failed:{:?}", error.kind());
        }
    }
}

fn write_program(path: &Path, version: &str, body: &str) -> TestResult {
    use std::os::unix::fs::PermissionsExt;
    let script = format!(
        "#!/bin/sh\nif [ \"$4\" = version ]; then printf '%s\\n' '{version}'; exit 0; fi\n{body}\n"
    );
    std::fs::write(path, script)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    Ok(())
}

fn qualified(repo: &Path, home: &Path) -> TestResult<QualifiedCheck> {
    let row = MiseCheck {
        id: "fixed-binary".into(),
        task: "probe".into(),
        directory: ".".into(),
        runner: CheckRunner {
            label: "macos-14".into(),
            platform: CheckPlatform::MacosArm64,
            executor: CheckExecutor::Hosted,
            container: None,
        },
        inputs: vec![],
        tools: vec!["rust-check".into()],
        system_tools: vec![],
        evidence: None,
        timeout_minutes: 30,
    };
    let mut checks = discover_checks(repo, &[row], &[declared_rust()])?;
    let handle = QualifiedCheck::new(
        home.to_owned(),
        repo.to_owned(),
        checks.remove(0),
        CheckCapabilityProof { container: None },
        &[],
    )?;
    std::fs::write(home.join("tasks.toml"), handle.bound_projection()?)?;
    Ok(handle)
}

// Synthetic declaration tests command construction; no acquisition proof is claimed.
fn declared_rust() -> QualifiedTool {
    QualifiedTool {
        id: "rust-check".into(),
        backend: QualifiedToolBackend::Core {
            tool: "rust".into(),
        },
        version: "1.97.1".into(),
        options: QualifiedToolOptions::Rust {
            components: vec![],
            targets: vec![],
        },
        depends_on: vec![],
        platforms: vec![QualifiedToolPlatform {
            platform: CheckPlatform::MacosArm64,
            artifacts: vec![QualifiedToolArtifact {
                url: "https://static.rust-lang.org/dist/rust-1.97.1-aarch64-apple-darwin.tar.xz"
                    .into(),
                sha256: "a".repeat(64),
            }],
            dependency_artifacts: vec![],
            install_tree_sha256: "b".repeat(64),
            executables: vec![QualifiedToolExecutable {
                name: "rustc".into(),
                path: "bin/rustc".into(),
                sha256: "c".repeat(64),
                probe: QualifiedToolProbe::RustcVerbose {
                    expected: format!(
                        "rustc 1.97.1\ncommit-hash: {}\nhost: aarch64-apple-darwin\nrelease: 1.97.1",
                        "d".repeat(40)
                    ),
                },
            }],
        }],
    }
}

#[test]
fn fixed_binary_rejects_ambient_path_replacement() -> TestResult {
    if let (Some(repo), Some(home)) = (
        std::env::var_os("VELNOR_FIXED_BINARY_REPO"),
        std::env::var_os("VELNOR_FIXED_BINARY_HOME"),
    ) {
        let handle = qualified(Path::new(&repo), Path::new(&home))?;
        let command = handle.command(test_deadline()?, MISE_VERSION)?;
        assert_eq!(command.program(), handle.mise_program().as_os_str());
        assert!(!command.argv().iter().any(|arg| arg == "--tool"));
        let env = command.full_env();
        assert!(env.iter().any(|(key, value)| {
            key == "MISE_OVERRIDE_TOOL_VERSIONS_FILENAMES" && value == "none"
        }));
        assert!(env.iter().any(|(key, value)| {
            key == "MISE_IDIOMATIC_VERSION_FILE_ENABLE_TOOLS" && value.is_empty()
        }));
        let prefix = Path::new(&home).join("tools/rust-check/prefix");
        assert!(
            command
                .full_env()
                .iter()
                .any(|(key, value)| { key == "RUSTUP_TOOLCHAIN" && value == prefix.as_os_str() })
        );
        let result = command.run()?;
        assert!(result.success);
        assert_eq!(result.stdout, b"OWNED_EXECUTABLE\n");
        return Ok(());
    }
    let fixture = Fixture::new()?;
    let repo = fixture.0.join("repo");
    let home = fixture.0.join("home");
    let poison = fixture.0.join("poison");
    std::fs::create_dir(&repo)?;
    std::fs::create_dir_all(home.join("bin"))?;
    std::fs::create_dir(&poison)?;
    std::fs::write(repo.join("mise.toml"), "[tasks.probe]\nrun='true'\n")?;
    write_program(
        &home.join("bin/mise"),
        velnor_actions_mise::MISE_VERSION,
        r#"test "$HTTP_PROXY" = 'http://proxy-bait.invalid:9' && test "$HTTPS_PROXY" = 'http://proxy-bait.invalid:9' && test "$NO_PROXY" = 'http://proxy-bait.invalid:9' && test "$ALL_PROXY" = 'http://proxy-bait.invalid:9' && test "$http_proxy" = 'http://proxy-bait.invalid:9' && test "$https_proxy" = 'http://proxy-bait.invalid:9' && test "$no_proxy" = 'http://proxy-bait.invalid:9' && test "$all_proxy" = 'http://proxy-bait.invalid:9' || exit 88; test -z "${GH_TOKEN+x}${DOCKER_HOST+x}" || exit 89; printf 'OWNED_EXECUTABLE\n'"#,
    )?;
    write_program(
        &poison.join("mise"),
        velnor_actions_mise::MISE_VERSION,
        "touch \"$HOME/AMBIENT_EXECUTABLE_RAN\"; exit 77",
    )?;
    let child = std::process::Command::new(std::env::current_exe()?)
        .args([
            "--exact",
            "impl_mise_fixed_binary::fixed_binary_rejects_ambient_path_replacement",
            "--nocapture",
        ])
        .env("VELNOR_FIXED_BINARY_REPO", &repo)
        .env("VELNOR_FIXED_BINARY_HOME", &home)
        .env("PATH", &poison)
        .env("GH_TOKEN", "credential-bait")
        .env("DOCKER_HOST", "endpoint-bait")
        .envs(
            velnor_actions_mise::command::PROXY_ENV_KEYS
                .map(|key| (key, "http://proxy-bait.invalid:9")),
        )
        .output()?;
    assert!(
        child.status.success(),
        "{}",
        String::from_utf8_lossy(&child.stderr)
    );
    assert!(!home.join("AMBIENT_EXECUTABLE_RAN").exists());
    Ok(())
}

#[test]
fn owned_binary_wrong_version_fails_before_task_execution() -> TestResult {
    let fixture = Fixture::new()?;
    let repo = fixture.0.join("repo");
    let home = fixture.0.join("home");
    std::fs::create_dir(&repo)?;
    std::fs::create_dir_all(home.join("bin"))?;
    std::fs::write(repo.join("mise.toml"), "[tasks.probe]\nrun='true'\n")?;
    write_program(
        &home.join("bin/mise"),
        "0.0.0",
        "touch \"$HOME/TASK_EXECUTED\"",
    )?;
    let handle = qualified(&repo, &home)?;
    assert!(handle.command(test_deadline()?, MISE_VERSION).is_err());
    assert!(!home.join("TASK_EXECUTED").exists());
    Ok(())
}

#[test]
fn qualified_children_keep_proxy_keys_without_ambient_credentials_or_endpoints() {
    use velnor_actions_mise::command::{EnvPolicy, PROXY_ENV_KEYS, proxy_passthrough};
    let parent: Vec<_> = PROXY_ENV_KEYS
        .iter()
        .map(|key| ((*key).into(), "http://proxy-bait.invalid:9".into()))
        .chain([
            ("GH_TOKEN".into(), "credential-bait".into()),
            ("DOCKER_HOST".into(), "endpoint-bait".into()),
        ])
        .collect();
    let additions = vec![("HOME".into(), "/owned/home".into())];
    let expected_proxy = proxy_passthrough(&parent);
    for policy in [EnvPolicy::QualifiedCheck, EnvPolicy::QualifiedAcquisition] {
        assert!(!policy.inherits_parent());
        assert!(policy.allows_proxy_passthrough());
        let mut expected = expected_proxy.clone();
        expected.extend(additions.clone());
        assert_eq!(policy.child_env(&parent, &additions), expected);
    }
    assert!(!EnvPolicy::QualifiedProbe.allows_proxy_passthrough());
    assert_eq!(
        EnvPolicy::QualifiedProbe.child_env(&parent, &additions),
        additions
    );
    assert!(EnvPolicy::RepoTask.allows_proxy_passthrough());
    assert_eq!(EnvPolicy::RepoTask.child_env(&parent, &[]), expected_proxy);
}

fn test_deadline() -> TestResult<velnor_actions_mise::CheckDeadline> {
    Ok(velnor_actions_mise::CheckDeadline::after(
        std::time::Duration::from_secs(60),
    )?)
}
