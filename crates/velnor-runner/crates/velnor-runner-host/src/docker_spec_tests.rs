//! Runner plan, mount audit, and id-only delete.

use crate::docker_spec::{ACTION_ARCHIVE_TARGET, ACTION_ARCHIVE_VOLUME, Mount, runner_mounts};
use crate::{
    ContainerPlan, DeleteDecision, HostError, audit_plan, delete_decision, plan_contains,
    runner_plan,
};

fn base_plan() -> ContainerPlan {
    ContainerPlan {
        name: "worker-runner".to_owned(),
        privileged: false,
        platform: "linux/amd64".to_owned(),
        image: "velnor-runner:ubuntu-26.04-2.337.0".to_owned(),
        env: Vec::new(),
        cmd: vec!["/usr/local/bin/velnor-runner-entrypoint".to_owned()],
        labels: Vec::new(),
        mounts: Vec::new(),
    }
}

fn plan_with_mount(source: &str, target: &str) -> ContainerPlan {
    let mut plan = base_plan();
    plan.mounts.push(Mount {
        source: source.to_owned(),
        target: target.to_owned(),
    });
    plan
}

fn plan_with_env(entry: &str) -> ContainerPlan {
    let mut plan = base_plan();
    plan.env.push(entry.to_owned());
    plan
}

#[test]
fn runner_plan_is_not_privileged() -> Result<(), HostError> {
    assert_eq!(runner_plan(""), Err(HostError::ForbiddenMount));
    assert_eq!(runner_plan("a/b"), Err(HostError::ForbiddenMount));
    let plan = runner_plan("worker_a")?;
    assert!(!plan.privileged);
    assert_eq!(plan.platform, "linux/amd64");
    assert_eq!(plan.image, "velnor-runner:ubuntu-26.04-2.337.0");
    assert_eq!(plan.mounts.len(), 2);
    assert_eq!(plan.mounts[0].source, "volume:worker_a");
    assert_eq!(plan.mounts[0].target, "/run");
    assert_eq!(plan.mounts[1].source, "volume:worker_a-work");
    assert_eq!(plan.mounts[1].target, "/home/runner/_work");
    assert!(
        plan.env.is_empty(),
        "archive env comes from projection, not the plan"
    );
    assert!(audit_plan(&plan).is_ok());
    let mut privileged = plan;
    privileged.privileged = true;
    assert_eq!(audit_plan(&privileged), Err(HostError::PrivilegedRunner));
    Ok(())
}

#[test]
fn action_archive_is_shared_and_rejects_a_home_path() -> Result<(), HostError> {
    let plan = runner_plan("worker_a")?;
    assert!(
        plan.env.is_empty(),
        "archive env comes from projection, not the plan"
    );
    let mounts = runner_mounts(&plan.mounts)?;
    let archive = mounts
        .iter()
        .find(|mount| mount.source == ACTION_ARCHIVE_VOLUME)
        .ok_or(HostError::Docker)?;
    assert_eq!(archive.target, ACTION_ARCHIVE_TARGET);
    assert!(audit_plan(&plan).is_ok());
    let mut home = plan;
    home.env
        .push("ACTIONS_RUNNER_ACTION_ARCHIVE_CACHE=/home/runner/action-archive".to_owned());
    assert_eq!(audit_plan(&home), Err(HostError::ForbiddenMount));
    Ok(())
}

#[test]
fn audit_rejects_host_and_secret_mounts() {
    let sources = [
        "/Users/me/src",
        "/users/me/src",
        "/Users",
        "/home/me/src",
        "/HOME/me/src",
        "/home",
        "/var/run/docker.sock",
        "/VAR/RUN/DOCKER.SOCK",
        "/run",
        "/var/run",
        "/private/var/run",
        "/tmp/ssh-agent.sock",
        "/tmp/SSH-AGENT.sock",
        "/Users/me/.ssh/id_ed25519",
        "/Users/me/Library/Keychains/login.keychain-db",
        "/Users/me/Library/Application Support/Velnor/host.toml",
        "/users/me/library/application support/velnor/host.toml",
        "/.orbstack/run/docker.sock",
        "/opt/orbstack/run/docker.sock",
        "/var/run/orbstack.sock",
        "/Users/me/.orbstack/run/docker.sock",
    ];
    for source in sources {
        let plan = plan_with_mount(source, "/mnt");
        assert_eq!(
            audit_plan(&plan),
            Err(HostError::ForbiddenMount),
            "{source}"
        );
    }
    assert!(audit_plan(&plan_with_mount("/opt/homebrew", "/opt/homebrew")).is_ok());
}

#[test]
fn docker_sock_is_only_a_private_volume() -> Result<(), HostError> {
    let plan = runner_plan("priv")?;
    assert_eq!(plan.mounts[0].source, "volume:priv");
    assert_eq!(plan.mounts[0].target, "/run");
    assert!(audit_plan(&plan).is_ok());
    let rejected = [
        ("/var/run/docker.sock", "/var/run/docker.sock"),
        ("/run/docker.sock", "/var/run/docker.sock"),
        (
            "/Users/me/.orbstack/run/docker.sock",
            "/var/run/docker.sock",
        ),
        ("volume:", "/var/run/docker.sock"),
        ("volume:a/b", "/var/run/docker.sock"),
        ("volume:../sock", "/var/run/docker.sock"),
        ("volume:/var/run/docker.sock", "/run"),
    ];
    for (source, target) in rejected {
        assert_eq!(
            audit_plan(&plan_with_mount(source, target)),
            Err(HostError::ForbiddenMount),
            "{source}"
        );
    }
    Ok(())
}

#[test]
fn audit_rejects_host_and_token_env() {
    let entries = [
        "HOME=/Users/me",
        "HOME=/home/me",
        "HOME=/home",
        "SSH_AUTH_SOCK=/tmp/ssh-agent.sock",
        "SSH_AGENT_PID=9",
        "KEYCHAIN_PATH=/Users/me/Library/Keychains/login.keychain-db",
        "VELNOR_HOST=/Users/me/Library/Application Support/Velnor/host.toml",
        "GITHUB_TOKEN=ghs_example",
        "GH_TOKEN=ghs_example",
        "ACTIONS_RUNTIME_TOKEN=secret",
        "GH_PAT=secret",
        "DOCKER_HOST=unix:///var/run/docker.sock",
        "DOCKER_HOST=unix:///Users/me/.orbstack/run/docker.sock",
        "X=--jitconfig secret",
    ];
    for entry in entries {
        assert_eq!(
            audit_plan(&plan_with_env(entry)),
            Err(HostError::ForbiddenMount),
            "{entry}"
        );
    }
}

#[test]
fn platform_stays_linux_amd64() -> Result<(), HostError> {
    let plan = runner_plan("worker_a")?;
    assert_eq!(plan.platform, "linux/amd64");
    assert!(audit_plan(&plan).is_ok());
    for platform in ["linux/arm64", "linux/aarch64", "aarch64", "arm64"] {
        let mut wrong = plan.clone();
        wrong.platform = platform.to_owned();
        assert_eq!(
            audit_plan(&wrong),
            Err(HostError::ForbiddenMount),
            "{platform}"
        );
    }
    let mut arm_image = plan;
    arm_image.image = "velnor-runner:ubuntu-26.04-arm64".to_owned();
    assert_eq!(audit_plan(&arm_image), Err(HostError::ForbiddenMount));
    Ok(())
}

#[test]
fn canary_is_found_in_env_cmd_labels_and_mounts() -> Result<(), HostError> {
    let mut plan = runner_plan("priv")?;
    let canary = "JITCONFIG";
    assert!(!plan_contains(&plan, canary));
    assert!(!plan_contains(&plan, ""));
    plan.env.push(format!("X={canary}"));
    assert!(plan_contains(&plan, canary));
    plan.env.clear();
    assert!(!plan_contains(&plan, canary));
    plan.cmd.push(format!("--{canary}"));
    assert!(plan_contains(&plan, canary));
    plan.cmd.pop();
    assert!(!plan_contains(&plan, canary));
    plan.labels.push(format!("k={canary}"));
    assert!(plan_contains(&plan, canary));
    plan.labels.pop();
    assert!(!plan_contains(&plan, canary));
    plan.mounts.push(Mount {
        source: format!("volume:{canary}"),
        target: "/mnt".to_owned(),
    });
    assert!(plan_contains(&plan, canary));
    Ok(())
}

#[test]
fn normal_plan_omits_canary() -> Result<(), HostError> {
    let plan = runner_plan("worker_a")?;
    assert!(!plan_contains(&plan, "JITCONFIG-CANARY"));
    assert!(audit_plan(&plan).is_ok());
    Ok(())
}

#[test]
fn names_are_not_delete_authority() {
    let owned = "sha256:owned";
    assert_eq!(delete_decision(owned, Some(owned)), DeleteDecision::Delete);
    assert_eq!(
        delete_decision(owned, Some("velnor-runner")),
        DeleteDecision::KeepForeign
    );
    assert_eq!(
        delete_decision(owned, Some("sha256:other")),
        DeleteDecision::KeepForeign
    );
    assert_eq!(delete_decision(owned, None), DeleteDecision::NotDeleted);
    assert_eq!(delete_decision("", Some("")), DeleteDecision::KeepForeign);
    assert_eq!(delete_decision("", None), DeleteDecision::NotDeleted);
}

#[test]
fn cleanup_set_keeps_foreign_name_collision() {
    let owned_id = "sha256:owned";
    let foreign_id = "sha256:foreign";
    let unrelated_id = "sha256:unrelated";
    let observed = [
        ("velnor-runner", owned_id),
        ("velnor-runner", foreign_id),
        ("other", unrelated_id),
    ];
    let mut delete_ids = Vec::new();
    let mut kept = Vec::new();
    for (name, id) in observed {
        let decision = delete_decision(owned_id, Some(id));
        if decision == DeleteDecision::Delete {
            delete_ids.push((name, id));
        }
        if decision == DeleteDecision::KeepForeign {
            kept.push((name, id));
        }
    }
    assert_eq!(delete_ids, vec![("velnor-runner", owned_id)]);
    assert_eq!(
        kept,
        vec![("velnor-runner", foreign_id), ("other", unrelated_id),]
    );
}
