//! Cross-check the JIT path and container plans against the checked-in images.

use crate::{dind_create, runner_plan};
use velnor_runner_core::{RUNNER_ROOT, RUNNER_WORK_FOLDER, runner_work_path};
use velnor_runner_github::jit_request;

#[path = "runner_image_contract_home.rs"]
mod runner_image_contract_home;
#[path = "runner_image_contract_lock.rs"]
mod runner_image_contract_lock;
#[path = "runner_storage_init_image_tests.rs"]
mod runner_storage_init_image_tests;

const RUNNER_LOCK: &str = include_str!("../../../../../.velnor/runner.lock");
const RUNNER_DOCKERFILE: &str =
    include_str!("../../../../../images/runner/ubuntu-26.04/Dockerfile");
const RUNNER_ENTRYPOINT: &str =
    include_str!("../../../../../images/runner/ubuntu-26.04/entrypoint.sh");
const RUNNER_WAIT_DOCKER_API: &str =
    include_str!("../../../../../images/runner/ubuntu-26.04/wait-docker-api.sh");
const RUNNER_README: &str = include_str!("../../../../../images/runner/ubuntu-26.04/README.md");
const RUNNER_HOME_INIT: &str =
    include_str!("../../../../../images/runner/ubuntu-26.04/runner-home.py");
const RUNNER_HOME_TEST: &str =
    include_str!("../../../../../images/runner/ubuntu-26.04/runner-home-test.py");
const GNU_TAR_BUILD_SCRIPT: &str =
    include_str!("../../../../../images/runner/ubuntu-26.04/build-gnu-tar.sh");
const GNU_TAR_SOURCE_LOCK: &str =
    include_str!("../../../../../images/runner/ubuntu-26.04/gnu-tar-source.lock.json");
const DIND_DOCKERFILE: &str = include_str!("../../../../../images/dind/Dockerfile");
const DIND_README: &str = include_str!("../../../../../images/dind/README.md");
const DIND_ENTRYPOINT: &str = include_str!("../../../../../images/dind/entrypoint.sh");
const DIND_START_DOCKERD: &str = include_str!("../../../../../images/dind/start-dockerd.sh");
const RESOURCE_PROBE_BUILD: &str =
    include_str!("../../../../../images/resource-probe/build-runner-images.sh");

#[test]
fn jit_work_folder_resolves_to_both_worker_mounts() -> Result<(), String> {
    let resolved = resolved_jit_work_path()?;
    let runner_plan = runner_plan("runner-contract").map_err(|error| error.to_string())?;
    let budget = crate::worker::test_resource_budget().map_err(|error| error.to_string())?;
    let runner =
        crate::worker::runner_create(&runner_plan, budget).map_err(|error| error.to_string())?;
    let dind = dind_create(
        "runner-contract",
        crate::worker::test_resource_budget().map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    assert_runner_mounts(&runner, &resolved);
    assert_dind_mounts(&dind, &resolved);
    Ok(())
}

// The official Runner resolves the work path with Root and WorkFolder.
fn resolved_jit_work_path() -> Result<String, String> {
    let request = jit_request("runner-contract").map_err(|error| error.to_string())?;
    let config: serde_json::Value =
        serde_json::from_slice(&request).map_err(|error| error.to_string())?;
    let folder = config
        .get("workFolder")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| "JIT request omitted workFolder".to_owned())?;
    let resolved = format!("{RUNNER_ROOT}/{folder}");
    assert_eq!(folder, RUNNER_WORK_FOLDER);
    assert_eq!(resolved, "/home/runner/_work");
    assert_eq!(resolved, runner_work_path());
    Ok(resolved)
}

fn assert_runner_mounts(runner: &crate::worker::CreateProjection, resolved: &str) {
    let runner_home = runner
        .mounts
        .iter()
        .position(|mount| mount.target == "/home/runner")
        .expect("The Runner has a fresh shared Runner home volume.");
    let runner_work = runner
        .mounts
        .iter()
        .position(|mount| mount.target == resolved)
        .expect("The Runner has a separate Work volume.");
    assert!(
        runner_home < runner_work,
        "The Runner home volume must mount before the Work volume."
    );
    assert_eq!(
        runner.mounts[runner_home].source,
        "volume:runner-contract-home"
    );
    assert_eq!(
        runner.mounts[runner_work].source,
        "volume:runner-contract-work"
    );
    assert_eq!(
        runner
            .mounts
            .iter()
            .take(3)
            .map(|mount| mount.target.as_str())
            .collect::<Vec<_>>(),
        ["/run", "/home/runner", "/home/runner/_work"]
    );
    assert!(runner.mounts.iter().all(|mount| mount.target != "/tmp"));
    assert!(runner.labels.contains(&"velnor.role=runner".to_owned()));
    assert!(
        runner
            .labels
            .contains(&"velnor.worker=runner-contract".to_owned())
    );
}

fn assert_dind_mounts(dind: &crate::worker::CreateProjection, resolved: &str) {
    let dind_home = dind
        .mounts
        .iter()
        .position(|mount| mount.target == "/home/runner")
        .expect("DinD has the same fresh shared Runner home volume.");
    let dind_work = dind
        .mounts
        .iter()
        .position(|mount| mount.target == resolved)
        .expect("DinD has the same separate Work volume.");
    assert!(
        dind_home < dind_work,
        "The Runner home volume must mount before the Work volume."
    );
    assert_eq!(dind.mounts[dind_home].source, "volume:runner-contract-home");
    assert_eq!(dind.mounts[dind_work].source, "volume:runner-contract-work");
    assert!(
        dind.mounts
            .iter()
            .any(|mount| mount.target == "/var/lib/docker")
    );
    assert_eq!(
        dind.mounts
            .iter()
            .take(4)
            .map(|mount| mount.target.as_str())
            .collect::<Vec<_>>(),
        [
            "/run",
            "/home/runner",
            "/home/runner/_work",
            "/var/lib/docker"
        ]
    );
    assert!(dind.labels.contains(&"velnor.role=dind".to_owned()));
    assert!(
        dind.labels
            .contains(&"velnor.worker=runner-contract".to_owned())
    );
}

#[test]
fn pinned_runner_and_image_components_match_the_source_lock() -> Result<(), String> {
    let lock: toml::Value = toml::from_str(RUNNER_LOCK).map_err(|error| error.to_string())?;
    runner_image_contract_lock::assert_runner_release_lock(&lock);
    runner_image_contract_lock::assert_runtime_component_lock(&lock);
    runner_image_contract_lock::assert_pinned_base_image(&lock);
    runner_image_contract_lock::assert_dockerfile_component_args(&lock);
    runner_image_contract_lock::assert_runner_artifact_matches_lock(&lock);
    runner_image_contract_lock::assert_dind_artifact_matches_lock(&lock)?;
    Ok(())
}

#[test]
fn checked_in_images_prepare_the_shared_path_for_runner_uid_1000() {
    runner_image_contract_home::assert_home_and_mount_contract();
}

#[test]
fn dind_publishes_only_a_ready_daemon_without_pulling_workload_images() {
    assert!(!DIND_ENTRYPOINT.contains("docker pull"));
    assert!(!DIND_ENTRYPOINT.contains("rabbitmq"));
    assert!(DIND_ENTRYPOINT.contains("if [ \"$i\" -gt 100 ]; then"));
    assert!(DIND_ENTRYPOINT.contains("dockerd socket did not appear"));
    assert!(DIND_ENTRYPOINT.contains("dockerd exited before socket appeared"));
    assert!(DIND_ENTRYPOINT.contains("kill -KILL \"$pid\""));
    assert!(DIND_ENTRYPOINT.contains("exit 1"));
    assert!(DIND_ENTRYPOINT.contains("ln -sfn docker.sock.real \"$public\""));
    assert!(DIND_README.contains("about 10 seconds"));
}

#[test]
fn paired_images_keep_runner_hostname_tcp_and_private_unix_for_actions() {
    assert!(RUNNER_ENTRYPOINT.contains("docker_host='tcp://localhost:2375'"));
    assert!(RUNNER_ENTRYPOINT.contains("unset DOCKER_CONTEXT"));
    assert!(
        RUNNER_ENTRYPOINT.contains("\"${entrypoint_dir}/velnor-wait-docker-api\" \"$docker_host\"")
    );
    assert!(!RUNNER_ENTRYPOINT.contains("clear-dockerenv"));
    assert!(RUNNER_WAIT_DOCKER_API.contains("PRIVATE_ENDPOINT='tcp://localhost:2375'"));
    assert!(RUNNER_WAIT_DOCKER_API.contains("-u DOCKER_HOST -u DOCKER_CONTEXT"));
    assert!(RUNNER_README.contains("`/.dockerenv` remains present"));
    assert!(RUNNER_README.contains("verifies HTTPS certificates normally"));
    assert!(RUNNER_README.contains("not yet qualified with both pinned SDK versions"));
    assert!(RUNNER_README.contains("currently pinned Runner image"));
    assert!(DIND_ENTRYPOINT.contains("--host=tcp://127.0.0.1:2375 --tls=false"));
    assert!(DIND_ENTRYPOINT.contains("ln -sfn docker.sock.real \"$public\""));
    assert!(DIND_START_DOCKERD.contains("--host=tcp://127.0.0.1:2375 --tls=false"));
    assert!(DIND_README.contains("No host port is"));
    assert!(DIND_README.contains("container actions resolve `/var/run/docker.sock`"));
}
