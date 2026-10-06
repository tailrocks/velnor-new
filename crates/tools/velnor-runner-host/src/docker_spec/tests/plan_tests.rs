use crate::docker_spec::Mount;
use crate::{HostError, audit_plan, plan_contains, runner_plan};

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
