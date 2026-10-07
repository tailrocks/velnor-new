use crate::docker_spec::Mount;
use std::time::{Duration, UNIX_EPOCH};

use crate::{
    HostError, audit_plan, plan_contains, resolve_runner_profile, runner_plan,
    runner_plan_for_profile,
};

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
    assert_eq!(plan.env.len(), 0);
    assert!(audit_plan(&plan).is_ok());
    let mut privileged = plan;
    privileged.privileged = true;
    assert_eq!(audit_plan(&privileged), Err(HostError::PrivilegedRunner));
    Ok(())
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
fn official_profile_is_digest_pinned_and_fails_closed_on_selector_mismatch() -> Result<(), HostError>
{
    let profile = resolve_runner_profile("ubuntu-24.04-amd64", "ubuntu-24.04-scale-set")?;
    assert_eq!(profile.key(), "ubuntu-24.04-amd64");
    assert_eq!(profile.scale_set_name(), "ubuntu-24.04-scale-set");
    assert_eq!(profile.platform(), "linux/amd64");
    assert_eq!(profile.runner_os(), "ubuntu24");
    assert_eq!(profile.runner_version(), "2.338.0");
    assert_eq!(
        profile.runner_manifest_digest(),
        "sha256:660f7b9d1e0007274f7c867e220b3c382137ef5a30d8e501a025ad50dbd5fb9d"
    );
    assert_eq!(
        profile.runner_index_digest(),
        "sha256:4ffadc0002b2581327e06101fc8c06cd189232baf79fe561fac9caeb76f5e807"
    );
    assert_eq!(profile.dind_version(), "29.8.2");
    assert_eq!(
        profile.dind_manifest_digest(),
        "sha256:dcac6f16dc25ddec91e2d467605775b95a035ab884b94cb4c2cc7cbef6fd726d"
    );
    assert_eq!(profile.runner_uid(), 1001);
    assert_eq!(profile.runner_gid(), 1001);
    assert_eq!(profile.runner_docker_gid(), 123);
    assert_eq!(profile.dind_socket_group(), "docker");
    assert_eq!(profile.dind_socket_gid(), 2375);
    assert_eq!(profile.runner_requalify_by(), "2026-11-05T13:55:11Z");
    assert_eq!(
        resolve_runner_profile("ubuntu-24.04-amd64", "ubuntu-26.04-scale-set"),
        Err(HostError::Config)
    );
    assert_eq!(
        resolve_runner_profile("ubuntu-26.04-amd64", "ubuntu-26.04-scale-set"),
        Err(HostError::Config)
    );
    Ok(())
}

#[test]
fn official_profile_expires_at_the_30_day_requalification_deadline() {
    let before = UNIX_EPOCH + Duration::from_secs(1_793_886_910);
    let deadline = UNIX_EPOCH + Duration::from_secs(1_793_886_911);
    assert!(
        super::super::profile::resolve_runner_profile_at(
            "ubuntu-24.04-amd64",
            "ubuntu-24.04-scale-set",
            before,
        )
        .is_ok()
    );
    assert_eq!(
        super::super::profile::resolve_runner_profile_at(
            "ubuntu-24.04-amd64",
            "ubuntu-24.04-scale-set",
            deadline,
        ),
        Err(HostError::Config)
    );
}

#[test]
fn official_runner_plan_keeps_jit_payload_out_of_docker_configuration() -> Result<(), HostError> {
    let profile = resolve_runner_profile("ubuntu-24.04-amd64", "ubuntu-24.04-scale-set")?;
    let plan = runner_plan_for_profile("worker_a", &profile)?;
    assert!(!plan.privileged);
    assert_eq!(plan.image, profile.runner_image());
    assert_eq!(plan.platform, "linux/amd64");
    assert_eq!(
        plan.env,
        vec![
            "DOCKER_HOST=unix:///run/docker/docker.sock".to_owned(),
            "RUNNER_WAIT_FOR_DOCKER_IN_SECONDS=120".to_owned(),
        ]
    );
    assert_eq!(plan.mounts.len(), 3);
    assert_eq!(plan.mounts[0].target, "/run/docker");
    assert_eq!(plan.mounts[1].target, "/home/runner/_work");
    assert_eq!(plan.mounts[2].source, "volume:worker_a-externals");
    assert_eq!(plan.mounts[2].target, "/home/runner/externals");
    assert_eq!(plan.security_opts, ["apparmor=velnor-runner".to_owned()]);
    assert_eq!(plan.group_add, ["2375".to_owned()]);
    let bootstrap = &plan.cmd[2];
    assert!(bootstrap.contains("[ \"$ready\" -eq 1 ]"));
    let docker_probe = bootstrap.find("docker ps").ok_or(HostError::Config)?;
    let jit_read = bootstrap
        .find("read -r ACTIONS_RUNNER_INPUT_JITCONFIG")
        .ok_or(HostError::Config)?;
    assert!(docker_probe < jit_read);
    assert!(bootstrap.contains("ACTIONS_RUNNER_INPUT_JITCONFIG"));
    assert!(!plan_contains(&plan, "JIT-SECRET-CANARY"));
    assert!(audit_plan(&plan).is_ok());

    let mut exposed = plan;
    exposed.security_opts.push("JIT-SECRET-CANARY".to_owned());
    assert!(plan_contains(&exposed, "JIT-SECRET-CANARY"));
    assert_eq!(audit_plan(&exposed), Err(HostError::ForbiddenMount));

    let mut wrong_socket = runner_plan_for_profile("worker_a", &profile)?;
    wrong_socket.mounts[0].target = "/run".to_owned();
    assert_eq!(audit_plan(&wrong_socket), Err(HostError::ForbiddenMount));
    Ok(())
}
