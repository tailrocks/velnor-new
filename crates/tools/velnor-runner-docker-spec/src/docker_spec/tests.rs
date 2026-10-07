//! Runner plan, mount audit, and id-only delete.

use crate::ContainerPlan;
use crate::docker_spec::Mount;

pub(super) fn base_plan() -> ContainerPlan {
    ContainerPlan {
        name: "worker-runner".to_owned(),
        privileged: false,
        platform: "linux/amd64".to_owned(),
        readonly_rootfs: false,
        image: "velnor-runner:ubuntu-26.04-2.337.0".to_owned(),
        env: Vec::new(),
        cmd: vec!["/usr/local/bin/velnor-runner-entrypoint".to_owned()],
        labels: Vec::new(),
        mounts: Vec::new(),
        image_mounts: Vec::new(),
        group_add: Vec::new(),
        security_opts: Vec::new(),
    }
}

pub(super) fn plan_with_mount(source: &str, target: &str) -> ContainerPlan {
    let mut plan = base_plan();
    plan.mounts.push(Mount {
        source: source.to_owned(),
        target: target.to_owned(),
        read_only: false,
    });
    plan
}

pub(super) fn plan_with_env(entry: &str) -> ContainerPlan {
    let mut plan = base_plan();
    plan.env.push(entry.to_owned());
    plan
}

mod audit_tests;
mod delete_tests;
mod plan_tests;
