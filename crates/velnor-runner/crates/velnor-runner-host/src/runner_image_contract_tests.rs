//! Cross-check the JIT path and container plans against the checked-in images.

use crate::{dind_create, runner_plan};
use velnor_runner_core::{RUNNER_ROOT, RUNNER_WORK_FOLDER, runner_work_path};
use velnor_runner_github::jit_request;

const RUNNER_DOCKERFILE: &str =
    include_str!("../../../../../images/runner/ubuntu-26.04/Dockerfile");
const RUNNER_ENTRYPOINT: &str =
    include_str!("../../../../../images/runner/ubuntu-26.04/entrypoint.sh");
const RUNNER_README: &str = include_str!("../../../../../images/runner/ubuntu-26.04/README.md");
const DIND_DOCKERFILE: &str = include_str!("../../../../../images/dind/Dockerfile");
const DIND_README: &str = include_str!("../../../../../images/dind/README.md");
const DIND_ENTRYPOINT: &str = include_str!("../../../../../images/dind/entrypoint.sh");

#[test]
fn jit_work_folder_resolves_to_both_worker_mounts() -> Result<(), String> {
    // actions/runner v2.338.0 resolves Work as Path.GetFullPath(Root + WorkFolder).
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

    let runner = runner_plan("runner-contract").map_err(|error| error.to_string())?;
    let dind = dind_create(
        "runner-contract",
        crate::worker::test_resource_budget().map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    assert_eq!(runner.mounts[1].target, resolved);
    assert_eq!(dind.mounts[1].target, resolved);
    assert!(runner.mounts.iter().all(|mount| mount.target != "/tmp"));
    assert!(runner.labels.contains(&"velnor.role=runner".to_owned()));
    assert!(dind.labels.contains(&"velnor.role=dind".to_owned()));
    assert!(
        runner
            .labels
            .contains(&"velnor.worker=runner-contract".to_owned())
    );
    assert!(
        dind.labels
            .contains(&"velnor.worker=runner-contract".to_owned())
    );
    Ok(())
}

#[test]
fn checked_in_images_prepare_the_shared_path_for_runner_uid_1000() {
    assert!(RUNNER_DOCKERFILE.contains("ARG RUNNER_VERSION=2.338.0"));
    assert!(RUNNER_DOCKERFILE.contains(
        "ARG RUNNER_SHA256=af4b794c1bc41d73d40535e3fe092a39f9679cd8d965954c2aca25a05ca41d32"
    ));
    assert!(RUNNER_DOCKERFILE.contains("ARG DOCKER_VERSION=29.9.1"));
    assert!(RUNNER_DOCKERFILE.contains(
        "ARG DOCKER_SHA256=c158ecc55adb3afb01bbd3539ed3b997d20c26dbdc418080506cc1bf5243ff57"
    ));
    assert!(RUNNER_DOCKERFILE.contains("ARG COMPOSE_VERSION=5.6.0"));
    assert!(RUNNER_DOCKERFILE.contains(
        "ARG COMPOSE_SHA256=40343e21ca777173e69cff5dbafeb37c6f81f3b0d57d9e597f036e95eb63e76a"
    ));
    assert!(RUNNER_DOCKERFILE.contains("ARG BUILDX_VERSION=0.38.0"));
    assert!(RUNNER_DOCKERFILE.contains(
        "ARG BUILDX_SHA256=4fe4cc38adf48169132749b6ca22a990928db0118e3407584ee553723115d287"
    ));
    assert!(RUNNER_DOCKERFILE.contains("ARG NODE_VERSION=26.12.0"));
    assert!(RUNNER_DOCKERFILE.contains(
        "ARG NODE_SHA256=d9f88af72ad34f7ddf8e67289829f1698f54b6254f7d4f54b7453117ff1a3d2c"
    ));
    assert!(DIND_DOCKERFILE.contains("ARG DOCKER_VERSION=29.9.1"));
    assert!(DIND_DOCKERFILE.contains(
        "ARG DOCKER_SHA256=c158ecc55adb3afb01bbd3539ed3b997d20c26dbdc418080506cc1bf5243ff57"
    ));
    assert!(RUNNER_README.contains("velnor-runner:ubuntu-26.04-2.338.0"));
    assert!(RUNNER_README.contains("actions/runner `2.338.0`"));
    assert!(RUNNER_README.contains("Node.js `26.12.0`"));
    assert!(DIND_README.contains("velnor-dind:29.9.1"));
    assert!(RUNNER_DOCKERFILE.contains("groupadd --gid 1000 runner"));
    assert!(RUNNER_DOCKERFILE.contains("useradd --uid 1000 --gid 1000"));
    assert!(RUNNER_DOCKERFILE.contains("mkdir -p /home/runner/_work"));
    assert!(
        RUNNER_DOCKERFILE.contains("busybox tar -xzf /tmp/actions-runner.tar.gz -C /home/runner")
    );
    assert!(RUNNER_DOCKERFILE.contains("WORKDIR /home/runner"));
    assert!(RUNNER_DOCKERFILE.contains("USER runner"));

    assert!(RUNNER_ENTRYPOINT.contains("root=\"/home/runner\""));
    assert!(RUNNER_ENTRYPOINT.contains("work=\"${root}/_work\""));
    assert!(RUNNER_ENTRYPOINT.contains("listener=\"${root}/bin/Runner.Listener\""));
    assert!(RUNNER_ENTRYPOINT.contains("mktemp /tmp/velnor-jit.XXXXXX"));
    assert!(!RUNNER_ENTRYPOINT.contains("mktemp \"${work}/"));
    assert!(RUNNER_ENTRYPOINT.contains("mkdir -p \"$work\""));
    assert!(!RUNNER_ENTRYPOINT.contains("CARGO_BUILD_JOBS"));
    assert!(!RUNNER_ENTRYPOINT.contains("runner-job-env"));
    assert!(!RUNNER_DOCKERFILE.contains("CARGO_BUILD_JOBS"));
    assert!(!RUNNER_DOCKERFILE.contains("nproc"));
    assert!(!RUNNER_DOCKERFILE.contains("runner-job-env"));
    assert!(!RUNNER_README.contains("half of `nproc`"));
    assert!(RUNNER_README.contains("The image does not override Cargo's job count."));

    assert!(DIND_DOCKERFILE.contains("mkdir -p /var/lib/docker /run /home/runner/_work"));
    assert!(DIND_DOCKERFILE.contains("chown 1000:1000 /home/runner/_work"));
    assert!(DIND_DOCKERFILE.contains("chmod 0755 /home/runner/_work"));
    assert!(RUNNER_README.contains("/home/runner/_work"));
    assert!(RUNNER_README.contains("workFolder: \"_work\""));
    assert!(DIND_README.contains("uid/gid `1000:1000`"));
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
