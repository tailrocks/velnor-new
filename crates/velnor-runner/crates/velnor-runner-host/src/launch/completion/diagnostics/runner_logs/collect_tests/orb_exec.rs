use std::sync::atomic::AtomicBool;
use std::time::Instant;

use bollard::Docker;
use bollard::exec::CreateExecOptions;
use bollard::models::ExecInspectResponse;

use super::{collect_exec_output, create_exec_options, start_exec_options, validate_exec_exit};

const DAEMON_ID: &str = "bc9058a0-c807-412b-a088-6c1d96ddd462";
const IMAGE_ID: &str = "sha256:e7c053d394855a853ccc387a9e6760fcec7fc3530fb31ca7e42081f91b7d68bc";

struct Fixture {
    docker: Docker,
    container_id: String,
    label: String,
    volume: String,
}

#[test]
#[ignore = "requires the explicitly identified disposable DinD-image fixture"]
fn orb_dind_exec_runs_collector_for_valid_missing_and_unsafe_diag_roots() -> Result<(), String> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| error.to_string())?
        .block_on(run_cases())
}

async fn run_cases() -> Result<(), String> {
    let fixture = fixture_from_env()?;
    fixture.assert_identity().await?;
    for case in [Case::Valid, Case::Missing, Case::Symlink, Case::RegularFile] {
        run_case(&fixture, case).await?;
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum Case {
    Valid,
    Missing,
    Symlink,
    RegularFile,
}

impl Case {
    fn name(self) -> &'static str {
        match self {
            Self::Valid => "valid",
            Self::Missing => "missing",
            Self::Symlink => "symlink",
            Self::RegularFile => "regular-file",
        }
    }

    fn setup_script(self) -> &'static str {
        match self {
            Self::Valid => {
                "rm -rf /home/runner/_diag /home/runner/outside; mkdir -p /home/runner/_diag; printf '%s\\n' 'private-canary-valid' > /home/runner/_diag/Runner_20261021-042500-utc.log; printf '%s\\n' 'private-canary-valid' > /home/runner/_diag/Worker_20261021-042501-utc.log"
            }
            Self::Missing => "rm -rf /home/runner/_diag /home/runner/outside",
            Self::Symlink => {
                "rm -rf /home/runner/_diag /home/runner/outside; mkdir -p /home/runner/outside; printf '%s\\n' 'private-canary-outside' > /home/runner/outside/Runner_20261021-042500-utc.log; ln -s /home/runner/outside /home/runner/_diag"
            }
            Self::RegularFile => {
                "rm -rf /home/runner/_diag /home/runner/outside; printf '%s\\n' 'not a directory' > /home/runner/_diag"
            }
        }
    }
}

async fn run_case(fixture: &Fixture, case: Case) -> Result<(), String> {
    fixture.run_setup(case.setup_script()).await?;
    let outcome = fixture.collect_case(case).await?;
    match case {
        Case::Valid => {
            assert_valid_archive(&outcome.map_err(|_| "valid archive was rejected".to_owned())?)
        }
        Case::Missing => {
            assert_missing_archive(&outcome.map_err(|_| "missing _diag was rejected".to_owned())?)
        }
        Case::Symlink | Case::RegularFile => assert_unsafe_root_rejected(&outcome),
    }
}

fn fixture_from_env() -> Result<Fixture, String> {
    let socket = std::env::var("VELNOR_ORB_DOCKER_SOCKET")
        .map_err(|_| "missing explicit Docker socket".to_owned())?;
    let docker = crate::docker_client::connect_unix(&socket)
        .map_err(|error| format!("selected Docker endpoint rejected: {error:?}"))?;
    Ok(Fixture {
        docker,
        container_id: std::env::var("VELNOR_ORB_FIXTURE_CONTAINER_ID")
            .map_err(|_| "missing exact container ID".to_owned())?,
        label: std::env::var("VELNOR_ORB_FIXTURE_LABEL")
            .map_err(|_| "missing fixture label".to_owned())?,
        volume: std::env::var("VELNOR_ORB_FIXTURE_VOLUME")
            .map_err(|_| "missing exact volume name".to_owned())?,
    })
}

impl Fixture {
    async fn run_setup(&self, script: &str) -> Result<(), String> {
        let (exec_id, output, result) = self.run_exec(shell_options(script)).await?;
        if !output.is_empty() || result.exit_code != Some(0) {
            return Err("fixture case setup did not complete cleanly".to_owned());
        }
        if result.id.as_deref() != Some(exec_id.as_str()) {
            return Err("fixture setup exec identity changed".to_owned());
        }
        Ok(())
    }

    async fn run_exec(
        &self,
        options: CreateExecOptions<String>,
    ) -> Result<(String, Vec<u8>, ExecInspectResponse), String> {
        self.assert_identity().await?;
        let created = self
            .docker
            .create_exec(&self.container_id, options)
            .await
            .map_err(|_| "fixture setup exec creation failed".to_owned())?;
        self.assert_identity().await?;
        let output = self
            .docker
            .start_exec(&created.id, Some(start_exec_options()))
            .await
            .map_err(|_| "fixture setup exec start failed".to_owned())?;
        let bytes = collect_exec_output(output, Instant::now(), &AtomicBool::new(false))
            .await
            .map_err(|error| format!("fixture setup output failed: {error:?}"))?;
        let result = self.inspect_exec(&created.id).await?;
        self.check_exec_target(&created.id, &result)?;
        self.assert_identity().await?;
        Ok((created.id, bytes, result))
    }

    async fn inspect_exec(&self, exec_id: &str) -> Result<ExecInspectResponse, String> {
        self.docker
            .inspect_exec(exec_id)
            .await
            .map_err(|_| "exec inspect failed".to_owned())
    }

    fn check_exec_target(&self, exec_id: &str, result: &ExecInspectResponse) -> Result<(), String> {
        if result.container_id.as_deref() != Some(&self.container_id)
            || result.id.as_deref() != Some(exec_id)
            || result.running != Some(false)
        {
            return Err("exec terminal identity or quiescence check failed".to_owned());
        }
        Ok(())
    }

    async fn assert_identity(&self) -> Result<(), String> {
        let info = self
            .docker
            .info()
            .await
            .map_err(|_| "daemon info failed".to_owned())?;
        if info.id.as_deref() != Some(DAEMON_ID) {
            return Err("daemon identity changed".to_owned());
        }
        self.assert_image().await?;
        self.assert_container_and_volume().await
    }

    async fn assert_image(&self) -> Result<(), String> {
        let image = self
            .docker
            .inspect_image(IMAGE_ID)
            .await
            .map_err(|_| "authorized DinD image is unavailable".to_owned())?;
        if image.id.as_deref() != Some(IMAGE_ID) {
            return Err("DinD image identity changed".to_owned());
        }
        Ok(())
    }

    async fn assert_container_and_volume(&self) -> Result<(), String> {
        let container = self
            .docker
            .inspect_container(&self.container_id, None)
            .await
            .map_err(|_| "exact fixture container is unavailable".to_owned())?;
        let volume = self
            .docker
            .inspect_volume(&self.volume)
            .await
            .map_err(|_| "exact fixture volume is unavailable".to_owned())?;
        self.check_container(&container)?;
        self.check_volume(&container, &volume)
    }

    fn check_container(
        &self,
        container: &bollard::models::ContainerInspectResponse,
    ) -> Result<(), String> {
        let labels = container
            .config
            .as_ref()
            .and_then(|config| config.labels.as_ref());
        let host = container.host_config.as_ref();
        let user = container
            .config
            .as_ref()
            .and_then(|config| config.user.as_deref());
        if container.id.as_deref() != Some(&self.container_id)
            || container.image.as_deref() != Some(IMAGE_ID)
            || container.path.as_deref() != Some("/bin/sh")
            || container.state.as_ref().and_then(|state| state.running) != Some(true)
            || labels
                .and_then(|labels| labels.get("com.velnor.diagnostic"))
                .map(String::as_str)
                != Some(&self.label)
            || user != Some("1000:1000")
            || host.is_none_or(|host| {
                host.privileged != Some(false)
                    || host.network_mode.as_deref() != Some("none")
                    || host.readonly_rootfs != Some(true)
                    || host.memory != Some(536_870_912)
                    || host.nano_cpus != Some(1_000_000_000)
                    || host.pids_limit != Some(128)
                    || host.cap_drop.as_deref().is_none_or(|caps| caps != ["ALL"])
                    || host.security_opt.as_deref().is_none_or(|opts| {
                        !opts.iter().any(|value| value == "no-new-privileges:true")
                    })
            })
        {
            return Err("fixture container identity or resource limits changed".to_owned());
        }
        Ok(())
    }

    fn check_volume(
        &self,
        container: &bollard::models::ContainerInspectResponse,
        volume: &bollard::models::Volume,
    ) -> Result<(), String> {
        let mounts = container.mounts.as_deref().unwrap_or_default();
        let correct_mount = mounts.len() == 1
            && mounts[0].name.as_deref() == Some(&self.volume)
            && mounts[0].destination.as_deref() == Some("/home/runner")
            && mounts[0].rw == Some(true);
        let options = &volume.options;
        if volume.name != self.volume
            || volume.driver != "local"
            || volume
                .labels
                .get("com.velnor.diagnostic")
                .map(String::as_str)
                != Some(&self.label)
            || options.get("type").map(String::as_str) != Some("tmpfs")
            || options.get("device").map(String::as_str) != Some("tmpfs")
            || options.get("o").is_none_or(|value| {
                !value.contains("size=4194304") || !value.contains("nr_inodes=128")
            })
            || !correct_mount
        {
            return Err("fixture volume identity or bound mount changed".to_owned());
        }
        Ok(())
    }
}

fn shell_options(script: &str) -> CreateExecOptions<String> {
    CreateExecOptions {
        attach_stdin: Some(false),
        attach_stdout: Some(true),
        attach_stderr: Some(false),
        tty: Some(false),
        env: Some(vec!["PATH=/usr/bin:/bin".to_owned()]),
        cmd: Some(vec![
            "/bin/sh".to_owned(),
            "-c".to_owned(),
            script.to_owned(),
        ]),
        privileged: Some(false),
        user: Some("1000:1000".to_owned()),
        working_dir: Some("/home/runner".to_owned()),
        ..CreateExecOptions::default()
    }
}

fn assert_valid_archive(archive: &[u8]) -> Result<(), String> {
    let projection = super::super::super::projection::sanitize_archive(archive)
        .map_err(|_| "valid archive failed sanitization".to_owned())?;
    assert_presence(&projection, true, true)?;
    if projection
        .windows(b"private-canary".len())
        .any(|part| part == b"private-canary")
    {
        return Err("raw canary reached the safe projection".to_owned());
    }
    Ok(())
}

fn assert_missing_archive(archive: &[u8]) -> Result<(), String> {
    let projection = super::super::super::projection::sanitize_archive(archive)
        .map_err(|_| "missing _diag archive failed sanitization".to_owned())?;
    assert_presence(&projection, false, false)
}

fn assert_unsafe_root_rejected(
    result: &Result<Vec<u8>, crate::error::HostError>,
) -> Result<(), String> {
    if *result == Err(crate::error::HostError::Docker) {
        Ok(())
    } else {
        Err("unsafe _diag root was accepted".to_owned())
    }
}

fn assert_presence(projection: &[u8], runner: bool, worker: bool) -> Result<(), String> {
    let value: serde_json::Value = serde_json::from_slice(projection)
        .map_err(|_| "sanitized projection is not JSON".to_owned())?;
    if value["complete"] != true
        || value["runner_log_present"] != runner
        || value["worker_log_present"] != worker
    {
        return Err("sanitized projection has unexpected completeness or log flags".to_owned());
    }
    Ok(())
}

impl Fixture {
    async fn collect_case(
        &self,
        case: Case,
    ) -> Result<Result<Vec<u8>, crate::error::HostError>, String> {
        self.assert_identity().await?;
        let created = self
            .docker
            .create_exec(&self.container_id, create_exec_options())
            .await
            .map_err(|_| format!("collector exec creation failed for {}", case.name()))?;
        self.assert_identity().await?;
        let started = Instant::now();
        let output = self
            .docker
            .start_exec(&created.id, Some(start_exec_options()))
            .await
            .map_err(|_| format!("collector exec start failed for {}", case.name()))?;
        let archive = collect_exec_output(output, started, &AtomicBool::new(false))
            .await
            .map_err(|error| format!("collector output failed for {}: {error:?}", case.name()))?;
        let result = self.inspect_exec(&created.id).await?;
        self.check_exec_target(&created.id, &result)?;
        self.assert_identity().await?;
        let expected = if matches!(case, Case::Symlink | Case::RegularFile) {
            24
        } else if matches!(case, Case::Missing) {
            3
        } else {
            0
        };
        if result.exit_code != Some(expected) {
            return Err(format!("collector exit code differed for {}", case.name()));
        }
        Ok(validate_exec_exit(&created.id, &result, archive))
    }
}
