use bollard::models::{
    ContainerCreateBody, HostConfig, HostConfigCgroupnsModeEnum, Mount, MountBindOptions, MountType,
};

use super::{
    CGROUP_TARGET, DAEMON_ARCH_LABEL, DockerRoot, PROBE_CPU_PERIOD, PROBE_CPU_QUOTA,
    PROBE_MEMORY_BYTES, PROBE_PIDS_LIMIT, PROBE_TARGET, PROBE_USER, SOURCE_LABEL,
    VerifiedProbeImage, digest_text,
};
use crate::journal::ResourceProbePurpose;

pub(super) fn config(
    root: &DockerRoot,
    image: &VerifiedProbeImage,
    operation_id: &str,
    instance_id: &str,
    engine_id: &str,
    purpose: ResourceProbePurpose,
    request_sha256: &str,
) -> ContainerCreateBody {
    let purpose_config = PurposeConfig::for_purpose(purpose, request_sha256);
    let engine_digest = digest_text(engine_id.as_bytes());
    let mut labels = std::collections::HashMap::new();
    labels.insert("velnor.role".to_owned(), "resource-probe".to_owned());
    labels.insert("velnor.operation".to_owned(), operation_id.to_owned());
    labels.insert("velnor.instance".to_owned(), instance_id.to_owned());
    labels.insert("velnor.engine".to_owned(), engine_digest);
    if let Some(label) = purpose_config.purpose_label {
        labels.insert("velnor.probe.purpose".to_owned(), label.to_owned());
    }
    if let Some(request_sha256) = purpose_config.request_sha256 {
        labels.insert("velnor.probe.request".to_owned(), request_sha256.to_owned());
    }
    labels.insert(
        "velnor.probe.binding".to_owned(),
        image.binding_fingerprint.clone(),
    );
    labels.insert(SOURCE_LABEL.to_owned(), image.source_revision.clone());
    labels.insert(
        DAEMON_ARCH_LABEL.to_owned(),
        image.daemon_architecture.clone(),
    );
    ContainerCreateBody {
        image: Some(image.runtime_id.clone()),
        user: Some(PROBE_USER.to_owned()),
        env: None,
        entrypoint: None,
        working_dir: None,
        labels: Some(labels),
        attach_stdin: Some(purpose_config.attach_stdin),
        open_stdin: Some(purpose_config.open_stdin),
        stdin_once: Some(purpose_config.stdin_once),
        attach_stdout: Some(true),
        attach_stderr: Some(false),
        tty: Some(false),
        network_disabled: Some(true),
        host_config: Some(host_config(root)),
        cmd: purpose_config.cmd,
        ..Default::default()
    }
}

struct PurposeConfig<'a> {
    purpose_label: Option<&'static str>,
    request_sha256: Option<&'a str>,
    attach_stdin: bool,
    open_stdin: bool,
    stdin_once: bool,
    cmd: Option<Vec<String>>,
}

impl<'a> PurposeConfig<'a> {
    fn for_purpose(purpose: ResourceProbePurpose, request_sha256: &'a str) -> Self {
        let (purpose_label, accepts_input, mode) = match purpose {
            ResourceProbePurpose::Capacity => (None, false, None),
            ResourceProbePurpose::OwnedCgroups => {
                (Some("owned-cgroups"), true, Some("owned_container_cgroup"))
            }
            ResourceProbePurpose::CleanupObserver => {
                (Some("cleanup-observer"), true, Some("cleanup_observer"))
            }
        };
        Self {
            purpose_label,
            request_sha256: purpose_label.map(|_| request_sha256),
            attach_stdin: accepts_input,
            open_stdin: accepts_input,
            stdin_once: accepts_input,
            cmd: mode.map(|mode| vec!["--mode".to_owned(), mode.to_owned()]),
        }
    }
}

fn host_config(root: &DockerRoot) -> HostConfig {
    HostConfig {
        auto_remove: Some(false),
        cap_drop: Some(vec!["ALL".to_owned()]),
        cgroupns_mode: Some(HostConfigCgroupnsModeEnum::HOST),
        cpu_period: Some(PROBE_CPU_PERIOD),
        cpu_quota: Some(PROBE_CPU_QUOTA),
        mounts: Some(vec![
            read_only_bind(root.path(), PROBE_TARGET),
            read_only_bind(CGROUP_TARGET, CGROUP_TARGET),
        ]),
        network_mode: Some("none".to_owned()),
        pid_mode: Some("host".to_owned()),
        privileged: Some(false),
        memory: Some(PROBE_MEMORY_BYTES),
        memory_swap: Some(PROBE_MEMORY_BYTES),
        pids_limit: Some(PROBE_PIDS_LIMIT),
        readonly_rootfs: Some(true),
        security_opt: Some(vec!["no-new-privileges:true".to_owned()]),
        ..Default::default()
    }
}

fn read_only_bind(source: &str, target: &str) -> Mount {
    Mount {
        target: Some(target.to_owned()),
        source: Some(source.to_owned()),
        typ: Some(MountType::BIND),
        read_only: Some(true),
        bind_options: Some(MountBindOptions {
            create_mountpoint: Some(false),
            ..Default::default()
        }),
        ..Default::default()
    }
}

#[cfg(test)]
#[path = "container_config/tests.rs"]
mod tests;
