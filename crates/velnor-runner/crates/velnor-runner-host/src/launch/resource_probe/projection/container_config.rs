use bollard::models::{
    ContainerCreateBody, HostConfig, HostConfigCgroupnsModeEnum, Mount, MountBindOptions, MountType,
};

use super::{
    CGROUP_TARGET, DAEMON_ARCH_LABEL, DockerRoot, PROBE_CPU_PERIOD, PROBE_CPU_QUOTA,
    PROBE_MEMORY_BYTES, PROBE_PIDS_LIMIT, PROBE_TARGET, PROBE_USER, SOURCE_LABEL,
    VerifiedProbeImage, digest_text,
};

pub(super) fn config(
    root: &DockerRoot,
    image: &VerifiedProbeImage,
    operation_id: &str,
    instance_id: &str,
    engine_id: &str,
) -> ContainerCreateBody {
    let engine_digest = digest_text(engine_id.as_bytes());
    let mut labels = std::collections::HashMap::new();
    labels.insert("velnor.role".to_owned(), "resource-probe".to_owned());
    labels.insert("velnor.operation".to_owned(), operation_id.to_owned());
    labels.insert("velnor.instance".to_owned(), instance_id.to_owned());
    labels.insert("velnor.engine".to_owned(), engine_digest);
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
        cmd: None,
        entrypoint: None,
        working_dir: None,
        labels: Some(labels),
        attach_stdin: Some(false),
        open_stdin: Some(false),
        stdin_once: Some(false),
        attach_stdout: Some(true),
        attach_stderr: Some(false),
        tty: Some(false),
        network_disabled: Some(true),
        host_config: Some(host_config(root)),
        ..Default::default()
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
