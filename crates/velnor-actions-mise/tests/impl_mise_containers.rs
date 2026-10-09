#![cfg(unix)]
// Synthetic prepared-CLI fixtures exercise adapter contracts and subprocess isolation.
// These tests do not qualify actual Docker bytes, an installation, or a live daemon.

use std::path::{Path, PathBuf};
use velnor_actions_contract::config::{
    CheckExecutor, CheckPlatform, CheckRunner, ContainerPlatform, DaemonIdentityPolicy,
    HostContainerProfile, HostDockerCli, HostDockerDaemon,
};
use velnor_actions_mise::checks::{
    PreparedContainer, validate_check_capability_proof, verify_check_capabilities,
};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

const CONTEXT: &str = "fixture";
const CLI_VERSION: &str = "27.4.1";
const CLI_BUILD: &str = "build-1";
const DAEMON_VERSION: &str = "27.4.1";
const DAEMON_ID: &str = "daemon-1";
const DAEMON_OS: &str = "Docker Desktop";
const DAEMON_ARCH: &str = "x86_64";
const DOCKER_SHA256: &str = "a";
const CONTEXT_FORMAT: &str = "{{.Endpoints.docker.Host}}";
const DAEMON_FORMAT: &str = r#"{"ID":{{json .ID}},"ServerVersion":{{json .ServerVersion}},"OSType":{{json .OSType}},"Architecture":{{json .Architecture}},"OperatingSystem":{{json .OperatingSystem}}}"#;

struct Fixture {
    root: PathBuf,
    home: PathBuf,
    docker_config: PathBuf,
    docker_program: PathBuf,
    marker: PathBuf,
    endpoint: String,
    runner: CheckRunner,
    prepared: PreparedContainer,
    _socket: std::os::unix::net::UnixListener,
}

impl Fixture {
    fn new() -> TestResult<Self> {
        use std::os::unix::fs::MetadataExt;
        let base =
            crate::test_temp_dir::unique_temp_dir_in(Path::new("/tmp"), "velnor-mise-container")?;
        let root = base.canonicalize()?;
        let home = root.join("home");
        let docker_config = home.join("docker-config");
        let docker_program = home.join("docker");
        std::fs::create_dir(&home)?;
        std::fs::create_dir(&docker_config)?;
        let socket = root.join("docker.sock");
        let listener = std::os::unix::net::UnixListener::bind(&socket)?;
        let socket_uid = std::fs::symlink_metadata(&socket)?.uid();
        let socket_text = path_text(&socket)?;
        let docker_text = path_text(&docker_program)?;
        let endpoint = format!("unix://{socket_text}");
        let profile = HostContainerProfile::Docker {
            context: CONTEXT.to_owned(),
            socket_path: socket_text,
            socket_uid,
            cli: HostDockerCli {
                path: docker_text,
                sha256: DOCKER_SHA256.repeat(64),
                version: CLI_VERSION.to_owned(),
                build: CLI_BUILD.to_owned(),
            },
            daemon: HostDockerDaemon {
                version: DAEMON_VERSION.to_owned(),
                platform: ContainerPlatform::LinuxX64,
                operating_system: DAEMON_OS.to_owned(),
                identity_policy: DaemonIdentityPolicy::ExecutionScoped,
            },
        };
        let runner = CheckRunner {
            label: "mise-container-macos".to_owned(),
            platform: CheckPlatform::MacosArm64,
            executor: CheckExecutor::EphemeralSelfHosted,
            container: Some(profile),
        };
        let marker = root.join("docker-invoked");
        let prepared = PreparedContainer {
            home: home.clone(),
            docker_config: docker_config.clone(),
            docker_program: docker_program.clone(),
            docker_sha256: DOCKER_SHA256.repeat(64),
            orbctl_program: None,
            orbctl_sha256: None,
            endpoint: endpoint.clone(),
        };
        let fixture = Self {
            root,
            home,
            docker_config,
            docker_program,
            marker,
            endpoint,
            runner,
            prepared,
            _socket: listener,
        };
        let daemon = daemon_json(DAEMON_ID, DAEMON_VERSION, "linux", DAEMON_ARCH);
        let endpoint = fixture.endpoint.clone();
        fixture.write_probe(
            &endpoint,
            &format!("Docker version {CLI_VERSION}, build {CLI_BUILD}"),
            &daemon,
        )?;
        Ok(fixture)
    }

    fn write_probe(
        &self,
        context_output: &str,
        cli_output: &str,
        daemon_output: &str,
    ) -> TestResult {
        use std::os::unix::fs::PermissionsExt;
        let home = sh_quote(&path_text(&self.home)?);
        let config = sh_quote(&path_text(&self.docker_config)?);
        let endpoint = sh_quote(&self.endpoint);
        let marker = sh_quote(&path_text(&self.marker)?);
        let context_output = sh_quote(context_output);
        let cli_output = sh_quote(cli_output);
        let daemon_output = sh_quote(daemon_output);
        let context = sh_quote(CONTEXT);
        let context_format = sh_quote(CONTEXT_FORMAT);
        let daemon_format = sh_quote(DAEMON_FORMAT);
        let script = format!(
            "#!/bin/sh\nset -eu\nprintf '%s\\n' \"$*\" >> {marker}\ntest \"$HOME\" = {home}\ntest \"$DOCKER_CONFIG\" = {config}\ntest \"$DOCKER_HOST\" = {endpoint}\ntest \"$PATH\" = '/usr/bin:/bin:/usr/sbin:/sbin'\ntest \"$LC_ALL\" = 'C'\ntest \"${{DOCKER_CONTEXT-unset}}\" = unset\ntest \"${{GITHUB_TOKEN-unset}}\" = unset\ntest \"${{GH_TOKEN-unset}}\" = unset\ntest \"${{ACTIONS_RUNTIME_TOKEN-unset}}\" = unset\ntest \"${{ACTIONS_ID_TOKEN_REQUEST_TOKEN-unset}}\" = unset\ntest \"${{CARGO_REGISTRY_TOKEN-unset}}\" = unset\ncase \"$#\" in\n  1)\n    test \"$1\" = '--version'\n    printf '%s\\n' {cli_output}\n    ;;\n  5)\n    if test \"$1\" = 'context'; then\n      test \"$2\" = 'inspect'\n      test \"$3\" = {context}\n      test \"$4\" = '--format'\n      test \"$5\" = {context_format}\n      printf '%s\\n' {context_output}\n    elif test \"$1\" = '--host'; then\n      test \"$2\" = \"$DOCKER_HOST\"\n      test \"$3\" = 'info'\n      test \"$4\" = '--format'\n      test \"$5\" = {daemon_format}\n      printf '%s\\n' {daemon_output}\n    else\n      exit 91\n    fi\n    ;;\n  *) exit 92 ;;\nesac\n",
        );
        std::fs::write(&self.docker_program, script)?;
        std::fs::set_permissions(&self.docker_program, std::fs::Permissions::from_mode(0o700))?;
        Ok(())
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if let Err(error) = std::fs::remove_dir_all(&self.root) {
            eprintln!("container_fixture_cleanup_failed:{:?}", error.kind());
        }
    }
}

fn path_text(path: &Path) -> TestResult<String> {
    path.to_str()
        .map(str::to_owned)
        .ok_or_else(|| "fixture path is not UTF-8".into())
}

fn sh_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn daemon_json(id: &str, version: &str, os: &str, architecture: &str) -> String {
    format!(
        r#"{{"ID":"{id}","ServerVersion":"{version}","OSType":"{os}","Architecture":"{architecture}","OperatingSystem":"{DAEMON_OS}"}}"#
    )
}

#[test]
fn owned_probe_accepts_linux_x64_daemon_on_macos_arm_and_scrubs_ambient_inputs() -> TestResult {
    if std::env::var_os("VELNOR_CONTAINER_PROBE_CHILD").is_none() {
        let output = std::process::Command::new(std::env::current_exe()?)
            .args([
                "--exact",
                "impl_mise_containers::owned_probe_accepts_linux_x64_daemon_on_macos_arm_and_scrubs_ambient_inputs",
                "--nocapture",
            ])
            .env("VELNOR_CONTAINER_PROBE_CHILD", "1")
            .env("PATH", "/poison/path")
            .env("HOME", "/poison/home")
            .env("GITHUB_TOKEN", "ambient-secret")
            .env("DOCKER_CONFIG", "/poison/config")
            .env("DOCKER_HOST", "unix:///poison.sock")
            .env("DOCKER_CONTEXT", "poison-context")
            .output()?;
        assert!(
            output.status.success(),
            "probe child failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8(output.stdout)?.contains("1 passed"));
        return Ok(());
    }
    let fixture = Fixture::new()?;
    let proof =
        verify_check_capabilities(&fixture.runner, Some(&fixture.prepared), test_deadline()?)?;
    let Some(container) = proof.container.as_ref() else {
        return Err("container proof missing".into());
    };
    assert_eq!(container.daemon.platform, ContainerPlatform::LinuxX64);
    assert_eq!(container.daemon.id, DAEMON_ID);
    assert!(fixture.marker.exists());
    Ok(())
}

#[test]
fn mismatched_binary_sha_stops_before_process() -> TestResult {
    let fixture = Fixture::new()?;
    let mut prepared = fixture.prepared.clone();
    prepared.docker_sha256 = "b".repeat(64);
    assert!(verify_check_capabilities(&fixture.runner, Some(&prepared), test_deadline()?).is_err());
    assert!(!fixture.marker.exists());
    Ok(())
}

#[test]
fn wrong_cli_version_or_build_is_rejected() -> TestResult {
    for (version, build) in [("27.4.2", CLI_BUILD), (CLI_VERSION, "build-2")] {
        let fixture = Fixture::new()?;
        let mut runner = fixture.runner.clone();
        let Some(HostContainerProfile::Docker { cli, .. }) = runner.container.as_mut() else {
            return Err("docker profile missing".into());
        };
        cli.version = version.to_owned();
        cli.build = build.to_owned();
        assert!(
            verify_check_capabilities(&runner, Some(&fixture.prepared), test_deadline()?).is_err()
        );
    }
    Ok(())
}

#[test]
fn wrong_context_endpoint_is_rejected() -> TestResult {
    let fixture = Fixture::new()?;
    let wrong_socket = fixture.root.join("wrong.sock");
    let wrong_endpoint = format!("unix://{}", path_text(&wrong_socket)?);
    let daemon = daemon_json(DAEMON_ID, DAEMON_VERSION, "linux", DAEMON_ARCH);
    fixture.write_probe(
        &wrong_endpoint,
        &format!("Docker version {CLI_VERSION}, build {CLI_BUILD}"),
        &daemon,
    )?;
    assert!(
        verify_check_capabilities(&fixture.runner, Some(&fixture.prepared), test_deadline()?)
            .is_err()
    );
    Ok(())
}

#[test]
fn daemon_identity_platform_and_version_drift_are_rejected() -> TestResult {
    for (id, version, os, architecture) in [
        ("", DAEMON_VERSION, "linux", DAEMON_ARCH),
        (DAEMON_ID, DAEMON_VERSION, "linux", "aarch64"),
        (DAEMON_ID, "27.4.2", "linux", DAEMON_ARCH),
    ] {
        let fixture = Fixture::new()?;
        let daemon = daemon_json(id, version, os, architecture);
        fixture.write_probe(
            &fixture.endpoint,
            &format!("Docker version {CLI_VERSION}, build {CLI_BUILD}"),
            &daemon,
        )?;
        assert!(
            verify_check_capabilities(&fixture.runner, Some(&fixture.prepared), test_deadline()?)
                .is_err(),
            "daemon drift must fail: {id}/{version}/{os}/{architecture}"
        );
    }
    Ok(())
}

#[test]
fn required_preparation_and_tampered_daemon_receipt_are_rejected() -> TestResult {
    let fixture = Fixture::new()?;
    assert!(verify_check_capabilities(&fixture.runner, None, test_deadline()?).is_err());
    let mut proof =
        verify_check_capabilities(&fixture.runner, Some(&fixture.prepared), test_deadline()?)?;
    let Some(container) = proof.container.as_mut() else {
        return Err("container proof missing".into());
    };
    container.daemon.id = "tampered-daemon".to_owned();
    assert!(validate_check_capability_proof(&fixture.runner, &proof).is_err());
    Ok(())
}

fn failed_probe(script: &str) -> TestResult<(Fixture, String)> {
    let fixture = Fixture::new()?;
    std::fs::write(&fixture.docker_program, format!("#!/bin/sh\n{script}\n"))?;
    match verify_check_capabilities(&fixture.runner, Some(&fixture.prepared), test_deadline()?) {
        Err(velnor_actions_mise::MiseError::InvalidStepInput { field, value })
            if field == "container_probe" =>
        {
            Ok((fixture, value))
        }
        _ => Err("synthetic failure did not fail closed".into()),
    }
}

#[test]
fn nonzero_status_retains_owned_operation_and_hashes_without_plain_stdout() -> TestResult {
    let (fixture, diagnostic) =
        failed_probe("printf 'PRIVATE_STDOUT'; printf 'failed\n' >&2; exit 23")?;
    assert!(diagnostic.starts_with("bounded_probe_failed:"));
    assert!(diagnostic.contains(&format!(
            "program=\"{}\"",
            fixture
                .docker_program
                .as_os_str()
                .as_encoded_bytes()
                .escape_ascii()
        )));
    assert!(diagnostic.contains("args=[\"--version\"]"));
    assert!(diagnostic.contains("code=Some(23), signal=None"));
    assert!(diagnostic.contains(&velnor_actions_contract::digest_b3(b"PRIVATE_STDOUT")));
    assert!(diagnostic.contains(&velnor_actions_contract::digest_b3(b"failed\n")));
    assert!(!diagnostic.contains("PRIVATE_STDOUT"));
    assert!(!diagnostic.contains('\n'));
    Ok(())
}

#[test]
fn signal_status_is_preserved_separately_from_normal_exit() -> TestResult {
    let (_fixture, diagnostic) = failed_probe("printf 'signal-failure' >&2; kill -TERM $$")?;
    assert!(diagnostic.contains("code=None, signal=Some(15)"));
    assert!(diagnostic.contains("signal-failure"));
    Ok(())
}

#[test]
fn stderr_preview_is_bounded_escaped_and_full_raw_digest_survives() -> TestResult {
    let (_fixture, diagnostic) = failed_probe(
        r#"printf '\033[31mBAD\n' >&2; i=0; while [ "$i" -lt 5000 ]; do printf x >&2; i=$((i+1)); done; printf END_MARKER >&2; exit 7"#,
    )?;
    let mut raw = b"\x1b[31mBAD\n".to_vec();
    raw.extend(std::iter::repeat_n(b'x', 5000));
    raw.extend_from_slice(b"END_MARKER");
    assert!(diagnostic.contains(&velnor_actions_contract::digest_b3(&raw)));
    assert!(!diagnostic.contains("END_MARKER"));
    assert!(!diagnostic.contains('\x1b'));
    assert!(!diagnostic.contains('\n'));
    assert!(diagnostic.contains("\\u{1b}"));
    assert_eq!(
        diagnostic.matches('x').count(),
        4096 - b"\x1b[31mBAD\n".len()
    );
    Ok(())
}

#[test]
fn owned_program_bytes_are_escaped_without_loss() -> TestResult {
    let mut fixture = Fixture::new()?;
    let program = fixture.home.join("docker-\"\\é");
    std::fs::rename(&fixture.docker_program, &program)?;
    std::fs::write(&program, "#!/bin/sh\nexit 23\n")?;
    fixture.prepared.docker_program = program.clone();
    let diagnostic =
        match verify_check_capabilities(&fixture.runner, Some(&fixture.prepared), test_deadline()?)
        {
            Err(velnor_actions_mise::MiseError::InvalidStepInput { field, value })
                if field == "container_probe" =>
            {
                value
            }
            Err(error) => return Err(format!("unexpected refusal: {error:?}").into()),
            Ok(_) => return Err("synthetic failure unexpectedly passed".into()),
        };
    assert!(diagnostic.contains(&format!(
        "program=\"{}\"",
        program.as_os_str().as_encoded_bytes().escape_ascii()
    )));
    assert!(diagnostic.contains("\\\"\\\\\\xc3\\xa9"));
    assert!(!diagnostic.contains('\n'));
    Ok(())
}

fn test_deadline() -> TestResult<velnor_actions_mise::CheckDeadline> {
    Ok(velnor_actions_mise::CheckDeadline::after(
        std::time::Duration::from_secs(60),
    )?)
}
