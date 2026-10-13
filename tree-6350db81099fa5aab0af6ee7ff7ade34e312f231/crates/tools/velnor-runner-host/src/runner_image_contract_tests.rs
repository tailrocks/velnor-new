//! Cross-check the JIT path and container plans against the checked-in images.

use crate::{dind_create, runner_plan};
use velnor_runner_core::{RUNNER_ROOT, RUNNER_WORK_FOLDER, runner_work_path};
use velnor_runner_github::jit_request;

const RUNNER_DOCKERFILE: &str =
    include_str!("../../../../images/runner/ubuntu-26.04/Dockerfile");
const RUNNER_ENTRYPOINT: &str =
    include_str!("../../../../images/runner/ubuntu-26.04/entrypoint.sh");
const RUNNER_README: &str = include_str!("../../../../images/runner/ubuntu-26.04/README.md");
const DIND_DOCKERFILE: &str = include_str!("../../../../images/dind/Dockerfile");
const DIND_README: &str = include_str!("../../../../images/dind/README.md");

#[test]
fn jit_work_folder_resolves_to_both_worker_mounts() -> Result<(), String> {
    // actions/runner v2.337.0 resolves Work as Path.GetFullPath(Root + WorkFolder).
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
    let dind = dind_create("runner-contract").map_err(|error| error.to_string())?;
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

    assert!(DIND_DOCKERFILE.contains("mkdir -p /var/lib/docker /run /home/runner/_work"));
    assert!(DIND_DOCKERFILE.contains("chown 1000:1000 /home/runner/_work"));
    assert!(DIND_DOCKERFILE.contains("chmod 0755 /home/runner/_work"));
    assert!(RUNNER_README.contains("/home/runner/_work"));
    assert!(RUNNER_README.contains("workFolder: \"_work\""));
    assert!(DIND_README.contains("uid/gid `1000:1000`"));
}
