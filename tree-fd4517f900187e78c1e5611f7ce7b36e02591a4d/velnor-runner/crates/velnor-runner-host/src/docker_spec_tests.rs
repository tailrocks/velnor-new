//! Runner plan, mount audit, and id-only delete.

use crate::docker_spec::Mount;
use crate::{
    ContainerPlan, DeleteDecision, HostError, audit_plan, delete_decision, plan_contains,
    runner_plan,
};

fn plan_with_mount(source: &str, target: &str) -> ContainerPlan {
    ContainerPlan {
        privileged: false,
        env: Vec::new(),
        cmd: Vec::new(),
        labels: Vec::new(),
        mounts: vec![Mount {
            source: source.to_owned(),
            target: target.to_owned(),
        }],
    }
}

#[test]
fn runner_plan_is_not_privileged() -> Result<(), HostError> {
    assert_eq!(runner_plan(""), Err(HostError::ForbiddenMount));
    assert_eq!(runner_plan("a/b"), Err(HostError::ForbiddenMount));
    let plan = runner_plan("worker_a")?;
    assert!(!plan.privileged);
    assert_eq!(plan.mounts.len(), 1);
    assert_eq!(plan.mounts[0].source, "volume:worker_a");
    assert_eq!(plan.mounts[0].target, "/var/run/docker.sock");
    assert!(audit_plan(&plan).is_ok());
    let mut privileged = plan;
    privileged.privileged = true;
    assert_eq!(audit_plan(&privileged), Err(HostError::PrivilegedRunner));
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
        "/tmp/ssh-agent.sock",
        "/tmp/SSH-AGENT.sock",
        "/Users/me/Library/Keychains/login.keychain-db",
        "/Users/me/Library/Application Support/Velnor/host.toml",
        "/users/me/library/application support/velnor/host.toml",
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
    assert!(audit_plan(&plan).is_ok());
    let rejected = [
        ("/var/run/docker.sock", "/var/run/docker.sock"),
        ("/run/docker.sock", "/var/run/docker.sock"),
        ("volume:", "/var/run/docker.sock"),
        ("volume:a/b", "/var/run/docker.sock"),
        ("volume:../sock", "/var/run/docker.sock"),
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
fn canary_is_found_in_env_cmd_and_labels() -> Result<(), HostError> {
    let mut plan = runner_plan("priv")?;
    let canary = "JITCONFIG";
    assert!(!plan_contains(&plan, canary));
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
