use bollard::models::{ContainerCreateBody, SystemInfo};

use super::{
    DOCKER_ROOT_TARGET, GuestResourceSample, PROBE_OUTPUT_PATH, PROC_TARGET, parse_sample,
    probe_container,
};

const VALID_OUTPUT: &str =
    "mem_available_kib=2048\nmemory_psi_some_avg10=1.25\ndocker_root_free_kib=4096\n";

fn info() -> SystemInfo {
    SystemInfo {
        ncpu: Some(4),
        docker_root_dir: Some("/srv/docker-data".to_owned()),
        ..Default::default()
    }
}

#[test]
fn parses_guest_cpu_memory_psi_and_docker_root_space() {
    assert_eq!(
        parse_sample(&info(), Some(VALID_OUTPUT)),
        GuestResourceSample {
            cpu_millicores: Some(4000),
            memory_available_bytes: Some(2_097_152),
            memory_psi_some_avg10_bps: Some(125),
            docker_root_free_bytes: Some(4_194_304),
        }
    );
}

#[test]
fn invalid_or_duplicate_values_remain_unknown() {
    let output = concat!(
        "mem_available_kib=18446744073709551615\n",
        "memory_psi_some_avg10=100.01\n",
        "memory_psi_some_avg10=1.00\n",
        "docker_root_free_kib=-1\n",
    );
    assert_eq!(
        parse_sample(&info(), Some(output)),
        GuestResourceSample {
            cpu_millicores: Some(4000),
            memory_available_bytes: None,
            memory_psi_some_avg10_bps: None,
            docker_root_free_bytes: None,
        }
    );
    let invalid_cpu = SystemInfo {
        ncpu: Some(0),
        ..info()
    };
    assert_eq!(
        parse_sample(&invalid_cpu, Some(VALID_OUTPUT)).cpu_millicores,
        None
    );
}

#[test]
fn missing_or_oversized_probe_data_is_unavailable() {
    let expected = GuestResourceSample {
        cpu_millicores: Some(4000),
        ..GuestResourceSample::default()
    };
    assert_eq!(parse_sample(&info(), None), expected);
    assert_eq!(parse_sample(&info(), Some(&"x".repeat(513))), expected);
    assert_eq!(
        parse_sample(
            &info(),
            Some("mem_available_kib=0\nmemory_psi_some_avg10=NaN\ndocker_root_free_kib=0\n")
        ),
        expected
    );
    let overflow_cpu = SystemInfo {
        ncpu: Some(4_294_968),
        ..info()
    };
    assert_eq!(
        parse_sample(&overflow_cpu, Some(VALID_OUTPUT)).cpu_millicores,
        None
    );
}

#[test]
fn probe_binds_the_actual_docker_root_read_only() -> Result<(), String> {
    let probe = probe_container(&info()).ok_or("probe")?;
    let config: &ContainerCreateBody = &probe.config;
    let script = config
        .cmd
        .as_ref()
        .and_then(|items| items.last())
        .map(String::as_str)
        .ok_or("probe command")?;
    assert!(script.contains("/usr/bin/df -k --output=avail /velnor/docker-root"));
    assert!(script.contains("probe_output_tmp=/tmp/.velnor-guest-resource-sample.tmp"));
    assert!(script.contains(&format!(
        "/usr/bin/mv \"$probe_output_tmp\" {PROBE_OUTPUT_PATH}"
    )));
    assert!(script.contains("/usr/bin/sleep 2s"));
    assert!(!script.contains("df -P"));
    assert_eq!(config.tty, Some(false));
    assert_eq!(probe.options.platform, "linux/amd64");
    let host = config.host_config.as_ref().ok_or("host config")?;
    let mounts = host.mounts.as_ref().ok_or("mounts")?;
    assert_eq!(mounts.len(), 2);
    assert_eq!(mounts[0].source.as_deref(), Some("/proc"));
    assert_eq!(mounts[0].target.as_deref(), Some(PROC_TARGET));
    assert_eq!(mounts[0].read_only, Some(true));
    assert_eq!(mounts[1].source.as_deref(), Some("/srv/docker-data"));
    assert_eq!(mounts[1].target.as_deref(), Some(DOCKER_ROOT_TARGET));
    assert_eq!(mounts[1].read_only, Some(true));
    assert_eq!(host.readonly_rootfs, Some(true));
    assert_eq!(
        host.tmpfs
            .as_ref()
            .and_then(|mounts| mounts.get("/tmp"))
            .map(String::as_str),
        Some("rw,noexec,nosuid,nodev,size=4096")
    );
    assert_eq!(host.network_mode.as_deref(), Some("none"));
    assert_eq!(host.privileged, Some(false));
    assert_eq!(
        host.security_opt
            .as_ref()
            .and_then(|options| options.first())
            .map(String::as_str),
        Some("no-new-privileges:true")
    );
    assert_eq!(
        host.cap_drop
            .as_ref()
            .and_then(|items| items.first())
            .map(String::as_str),
        Some("ALL")
    );
    assert_eq!(host.cap_drop.as_ref().map(Vec::len), Some(1));
    assert_eq!(host.pid_mode, None);
    assert_eq!(config.env, None);
    for root in ["/", "relative", "/var/../etc"] {
        assert!(
            probe_container(&SystemInfo {
                docker_root_dir: Some(root.to_owned()),
                ..info()
            })
            .is_none()
        );
    }
    Ok(())
}
