//! Cross-check the JIT path and container plans against the checked-in images.

use crate::docker_spec::RUNNER_IMAGE;
use crate::{dind_create, runner_plan};
use velnor_runner_core::{RUNNER_ROOT, RUNNER_WORK_FOLDER, runner_work_path};
use velnor_runner_github::jit_request;

const RUNNER_LOCK: &str = include_str!("../../../../../.velnor/runner.lock");
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

fn lock_str<'a>(lock: &'a toml::Value, key: &str) -> &'a str {
    lock.get(key)
        .and_then(toml::Value::as_str)
        .unwrap_or_else(|| panic!("runner.lock is missing string field {key}"))
}

fn docker_arg<'a>(dockerfile: &'a str, name: &str) -> Option<&'a str> {
    let prefix = format!("ARG {name}=");
    dockerfile
        .lines()
        .find_map(|line| line.strip_prefix(prefix.as_str()))
}

fn assert_runner_release_lock(lock: &toml::Value) {
    assert_eq!(lock_str(lock, "platform"), "linux/amd64");
    assert_eq!(lock_str(lock, "runner_platform"), "linux/amd64");
    assert_eq!(lock_str(lock, "runner_release_channel"), "stable");
    assert_eq!(lock_str(lock, "runner_release_observed_date"), "2026-10-18");
    assert_eq!(lock_str(lock, "runner_version"), "2.338.0");
    assert_eq!(
        lock_str(lock, "runner_source_commit"),
        "197ca027b90de199464dee3bb9ff198a1db60967"
    );
    assert_eq!(
        lock_str(lock, "runner_sha256"),
        "af4b794c1bc41d73d40535e3fe092a39f9679cd8d965954c2aca25a05ca41d32"
    );
    assert_eq!(lock_str(lock, "runner_scope_download_version"), "2.337.0");
    assert_ne!(
        lock_str(lock, "runner_scope_download_version"),
        lock_str(lock, "runner_version")
    );
}

fn assert_runtime_component_lock(lock: &toml::Value) {
    assert_eq!(lock_str(lock, "docker_version"), "29.9.1");
    assert_eq!(
        lock_str(lock, "docker_sha256"),
        "c158ecc55adb3afb01bbd3539ed3b997d20c26dbdc418080506cc1bf5243ff57"
    );
    assert_eq!(lock_str(lock, "buildx_version"), "0.38.0");
    assert_eq!(
        lock_str(lock, "buildx_sha256"),
        "4fe4cc38adf48169132749b6ca22a990928db0118e3407584ee553723115d287"
    );
    assert_eq!(lock_str(lock, "compose_version"), "5.6.0");
    assert_eq!(
        lock_str(lock, "compose_sha256"),
        "40343e21ca777173e69cff5dbafeb37c6f81f3b0d57d9e597f036e95eb63e76a"
    );
    assert_eq!(lock_str(lock, "node_version"), "26.12.0");
    assert_eq!(lock_str(lock, "node_release_channel"), "Current");
    assert_eq!(
        lock_str(lock, "node_sha256"),
        "d9f88af72ad34f7ddf8e67289829f1698f54b6254f7d4f54b7453117ff1a3d2c"
    );
    assert_eq!(lock_str(lock, "turso"), "=0.8.3");
}

fn assert_pinned_base_image(lock: &toml::Value) {
    let base = format!(
        "FROM {}@{}",
        lock_str(lock, "base_image"),
        lock_str(lock, "base_image_digest")
    );
    assert!(RUNNER_DOCKERFILE.lines().any(|line| line == base.as_str()));
    assert!(DIND_DOCKERFILE.lines().any(|line| line == base.as_str()));
    assert_eq!(
        lock_str(lock, "base_image_index_digest"),
        "sha256:c098726dd9b9733f3e885f8de0ef375206a311b0a317eadc40d347b1115fb944"
    );
}

fn assert_dockerfile_component_args(lock: &toml::Value) {
    for (dockerfile, arg, field) in [
        (&RUNNER_DOCKERFILE, "RUNNER_VERSION", "runner_version"),
        (&RUNNER_DOCKERFILE, "RUNNER_SHA256", "runner_sha256"),
        (&RUNNER_DOCKERFILE, "DOCKER_VERSION", "docker_version"),
        (&RUNNER_DOCKERFILE, "DOCKER_SHA256", "docker_sha256"),
        (&RUNNER_DOCKERFILE, "COMPOSE_VERSION", "compose_version"),
        (&RUNNER_DOCKERFILE, "COMPOSE_SHA256", "compose_sha256"),
        (&RUNNER_DOCKERFILE, "BUILDX_VERSION", "buildx_version"),
        (&RUNNER_DOCKERFILE, "BUILDX_SHA256", "buildx_sha256"),
        (&RUNNER_DOCKERFILE, "NODE_VERSION", "node_version"),
        (&RUNNER_DOCKERFILE, "NODE_SHA256", "node_sha256"),
        (&DIND_DOCKERFILE, "DOCKER_VERSION", "docker_version"),
        (&DIND_DOCKERFILE, "DOCKER_SHA256", "docker_sha256"),
    ] {
        assert_eq!(
            docker_arg(dockerfile, arg),
            Some(lock_str(lock, field)),
            "{arg}"
        );
    }
}

fn assert_runner_artifact_matches_lock(lock: &toml::Value) {
    let runner_tag = format!(
        "velnor-runner:ubuntu-26.04-{}",
        lock_str(lock, "runner_version")
    );
    assert_eq!(RUNNER_IMAGE, runner_tag);
    assert!(RUNNER_README.contains(runner_tag.as_str()));
    assert!(RUNNER_README.contains(lock_str(lock, "runner_source_commit")));
    assert!(RUNNER_README.contains("not evidence that `2.338.0` is\nrejected"));
    assert!(RUNNER_DOCKERFILE.contains("actions-runner-linux-x64-${RUNNER_VERSION}.tar.gz"));
    let checksum_check = RUNNER_DOCKERFILE
        .find("sha256sum -c -")
        .expect("Runner archive checksum is checked");
    let runner_extract = RUNNER_DOCKERFILE
        .find("busybox tar -xzf /tmp/actions-runner.tar.gz")
        .expect("Runner archive is extracted");
    assert!(checksum_check < runner_extract);
    assert!(RUNNER_DOCKERFILE.contains("Does not patch Runner.Listener"));
    assert!(!RUNNER_DOCKERFILE.contains("externals/node24"));
}

fn assert_dind_artifact_matches_lock(lock: &toml::Value) -> Result<(), String> {
    let dind_tag = format!("velnor-dind:{}", lock_str(lock, "docker_version"));
    assert!(DIND_README.contains(dind_tag.as_str()));
    assert!(DIND_README.contains(lock_str(lock, "docker_source_commit")));

    let resource_budget =
        crate::worker::test_resource_budget().map_err(|error| error.to_string())?;
    let runtime_plan =
        dind_create("runner-contract", resource_budget).map_err(|error| error.to_string())?;
    assert_eq!(runtime_plan.image, dind_tag);
    Ok(())
}

#[test]
fn pinned_runner_and_image_components_match_the_source_lock() -> Result<(), String> {
    let lock: toml::Value = toml::from_str(RUNNER_LOCK).map_err(|error| error.to_string())?;
    assert_runner_release_lock(&lock);
    assert_runtime_component_lock(&lock);
    assert_pinned_base_image(&lock);
    assert_dockerfile_component_args(&lock);
    assert_runner_artifact_matches_lock(&lock);
    assert_dind_artifact_matches_lock(&lock)?;
    Ok(())
}

#[test]
fn checked_in_images_prepare_the_shared_path_for_runner_uid_1000() {
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
