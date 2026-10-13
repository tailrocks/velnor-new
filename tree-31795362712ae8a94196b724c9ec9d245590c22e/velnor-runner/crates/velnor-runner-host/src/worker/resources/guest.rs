//! Trusted, read-only samples from the selected Docker Linux guest.
//!
//! CPU count comes from that engine's `/info`; the remaining values come from
//! [`probe_container`]. Its only binds are read-only guest `/proc` and the exact
//! `DockerRootDir` reported by the same engine. The launch path does not yet
//! wire these samples into capacity policy. Existing guest-slot and occupancy
//! rules remain unchanged until their caller is integrated.

use std::collections::HashMap;
use std::path::{Component, Path};

use bollard::models::{
    ContainerCreateBody, HostConfig, Mount, MountBindOptions, MountType, SystemInfo,
};
use bollard::query_parameters::CreateContainerOptions;

use super::super::{DIND_IMAGE, PLATFORM};

/// Bounded runtime sampling for the fixed guest probe.
pub(crate) mod sampler;
pub(crate) use sampler::GuestProbeIdentity;

/// Maximum accepted sample file size from the fixed probe.
const MAX_PROBE_OUTPUT_BYTES: usize = 512;
const PROBE_OUTPUT_PATH: &str = "/tmp/velnor-guest-resource-sample";
const PROC_TARGET: &str = "/velnor/guest-proc";
const DOCKER_ROOT_TARGET: &str = "/velnor/docker-root";
const PROBE_ENTRYPOINT: [&str; 3] = ["/usr/bin/timeout", "4s", "/bin/sh"];
const PROBE_SCRIPT: &str = r#"
mem_available_kib=
while IFS=' ' read -r key value unit rest; do
    if [ "$key" = "MemAvailable:" ] && [ "$unit" = "kB" ]; then
        mem_available_kib=$value
        break
    fi
done < /velnor/guest-proc/meminfo 2>/dev/null

memory_psi_some_avg10=
while IFS=' ' read -r scope avg10 rest; do
    if [ "$scope" = "some" ]; then
        case "$avg10" in
            avg10=*) memory_psi_some_avg10=${avg10#avg10=} ;;
        esac
        break
    fi
done < /velnor/guest-proc/pressure/memory 2>/dev/null

docker_root_free_kib=$(/usr/bin/timeout 2s /usr/bin/df -k --output=avail /velnor/docker-root 2>/dev/null | /usr/bin/sed -n '2p')
docker_root_free_kib=$(printf '%s' "$docker_root_free_kib" | /usr/bin/tr -d '[:space:]')
case "$mem_available_kib" in ''|*[!0-9]*) mem_available_kib= ;; esac
case "$docker_root_free_kib" in ''|*[!0-9]*) docker_root_free_kib= ;; esac
case "$memory_psi_some_avg10" in
    ''|*[!0-9.]*|.*|*.|*.*.*) memory_psi_some_avg10= ;;
esac
[ "${#mem_available_kib}" -le 20 ] || mem_available_kib=
[ "${#docker_root_free_kib}" -le 20 ] || docker_root_free_kib=
[ "${#memory_psi_some_avg10}" -le 6 ] || memory_psi_some_avg10=
umask 077
probe_output_tmp=/tmp/.velnor-guest-resource-sample.tmp
{
    printf 'mem_available_kib=%s\n' "$mem_available_kib"
    printf 'memory_psi_some_avg10=%s\n' "$memory_psi_some_avg10"
    printf 'docker_root_free_kib=%s\n' "$docker_root_free_kib"
} > "$probe_output_tmp" || exit 1
/usr/bin/mv "$probe_output_tmp" /tmp/velnor-guest-resource-sample || exit 1
/usr/bin/sleep 2s
"#;

/// The guest resources needed by the resource policy.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct GuestResourceSample {
    /// Guest CPU capacity in millicores, derived from the daemon's NCPU field.
    pub(crate) cpu_millicores: Option<u32>,
    /// Guest `MemAvailable`, converted from KiB to bytes.
    pub(crate) memory_available_bytes: Option<u64>,
    /// Memory PSI `some avg10`, in hundredths of a percentage point.
    ///
    /// For example, `125` means `1.25%`; `10_000` means `100%`.
    pub(crate) memory_psi_some_avg10_bps: Option<u16>,
    /// Available bytes on the filesystem containing the actual Docker root.
    pub(crate) docker_root_free_bytes: Option<u64>,
}

/// Fixed Docker create request for the controller-owned guest probe.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct GuestProbeCreate {
    /// Query options pinning the probe to the worker platform.
    pub(crate) options: CreateContainerOptions,
    /// Read-only, networkless probe container configuration.
    pub(crate) config: ContainerCreateBody,
}

/// Project the probe from the selected daemon's info response.
///
/// A malformed or missing Docker root path makes the probe unavailable. The
/// daemon-provided path is used as the bind source; it is never shell text.
#[must_use]
pub(crate) fn probe_container(info: &SystemInfo) -> Option<GuestProbeCreate> {
    let root = info.docker_root_dir.as_deref()?;
    let mounts = probe_mounts(root)?;
    Some(GuestProbeCreate {
        options: CreateContainerOptions {
            name: None,
            platform: PLATFORM.to_owned(),
        },
        config: ContainerCreateBody {
            image: Some(DIND_IMAGE.to_owned()),
            user: Some("0:0".to_owned()),
            attach_stdin: Some(false),
            attach_stdout: Some(false),
            attach_stderr: Some(false),
            tty: Some(false),
            entrypoint: Some(
                PROBE_ENTRYPOINT
                    .iter()
                    .map(|item| (*item).to_owned())
                    .collect(),
            ),
            cmd: Some(vec!["-c".to_owned(), PROBE_SCRIPT.to_owned()]),
            host_config: Some(HostConfig {
                mounts: Some(mounts),
                network_mode: Some("none".to_owned()),
                privileged: Some(false),
                readonly_rootfs: Some(true),
                tmpfs: Some(HashMap::from([(
                    "/tmp".to_owned(),
                    "rw,noexec,nosuid,nodev,size=4096".to_owned(),
                )])),
                cap_drop: Some(vec!["ALL".to_owned()]),
                security_opt: Some(vec!["no-new-privileges:true".to_owned()]),
                ..Default::default()
            }),
            ..Default::default()
        },
    })
}

/// Parse one bounded probe result with the selected engine's CPU count.
///
/// Pass `None` when the probe timed out, returned no output, or could not be
/// read. Each malformed, duplicate, absent, or zero measurement remains
/// unknown independently.
#[must_use]
pub(crate) fn parse_sample(info: &SystemInfo, output: Option<&str>) -> GuestResourceSample {
    let Some(output) = output.filter(|text| valid_probe_output(text)) else {
        return GuestResourceSample {
            cpu_millicores: parse_cpu_millicores(info.ncpu),
            ..GuestResourceSample::default()
        };
    };

    let mut values = [None; 3];
    let mut duplicate = [false; 3];
    for line in output.lines() {
        let Some((key, value)) = line.trim_end_matches('\r').split_once('=') else {
            continue;
        };
        let Some(index) = probe_field_index(key) else {
            continue;
        };
        if values[index].is_some() {
            duplicate[index] = true;
        } else {
            values[index] = Some(value.trim());
        }
    }

    GuestResourceSample {
        cpu_millicores: parse_cpu_millicores(info.ncpu),
        memory_available_bytes: unique_value(values[0], duplicate[0]).and_then(parse_kibibytes),
        memory_psi_some_avg10_bps: unique_value(values[1], duplicate[1])
            .and_then(parse_percent_basis_points),
        docker_root_free_bytes: unique_value(values[2], duplicate[2]).and_then(parse_kibibytes),
    }
}

fn valid_probe_output(output: &str) -> bool {
    output.len() <= MAX_PROBE_OUTPUT_BYTES
        && !output
            .bytes()
            .any(|byte| byte.is_ascii_control() && !matches!(byte, b'\n' | b'\r' | b'\t'))
}

fn probe_field_index(key: &str) -> Option<usize> {
    match key {
        "mem_available_kib" => Some(0),
        "memory_psi_some_avg10" => Some(1),
        "docker_root_free_kib" => Some(2),
        _ => None,
    }
}

fn unique_value(value: Option<&str>, duplicate: bool) -> Option<&str> {
    if duplicate { None } else { value }
}

fn parse_cpu_millicores(value: Option<i64>) -> Option<u32> {
    let count = u32::try_from(value?).ok()?;
    if count == 0 {
        return None;
    }
    count.checked_mul(1000)
}

fn parse_kibibytes(value: &str) -> Option<u64> {
    let kibibytes = parse_positive_integer(value)?;
    kibibytes.checked_mul(1024)
}

fn parse_positive_integer(value: &str) -> Option<u64> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let parsed = value.parse::<u64>().ok()?;
    (parsed > 0).then_some(parsed)
}

fn parse_percent_basis_points(value: &str) -> Option<u16> {
    let (whole, fraction) = match value.split_once('.') {
        Some((whole, fraction)) if fraction.len() <= 2 && !fraction.is_empty() => (whole, fraction),
        Some(_) => return None,
        None => (value, ""),
    };
    if !whole.bytes().all(|byte| byte.is_ascii_digit())
        || !fraction.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    let whole = whole.parse::<u16>().ok()?;
    let fraction = match fraction.len() {
        0 => 0,
        1 => fraction.parse::<u16>().ok()?.checked_mul(10)?,
        2 => fraction.parse::<u16>().ok()?,
        _ => return None,
    };
    let basis_points = whole.checked_mul(100)?.checked_add(fraction)?;
    (basis_points <= 10_000).then_some(basis_points)
}

fn probe_mounts(root: &str) -> Option<Vec<Mount>> {
    if !safe_guest_path(root) {
        return None;
    }
    Some(vec![
        read_only_bind("/proc", PROC_TARGET),
        read_only_bind(root, DOCKER_ROOT_TARGET),
    ])
}

fn safe_guest_path(value: &str) -> bool {
    if !value.starts_with('/') || value == "/" || value.bytes().any(|byte| byte.is_ascii_control())
    {
        return false;
    }
    let mut normal_component = false;
    for component in Path::new(value).components() {
        match component {
            Component::RootDir => {}
            Component::Normal(_) => normal_component = true,
            Component::CurDir | Component::ParentDir | Component::Prefix(_) => return false,
        }
    }
    normal_component
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
#[path = "guest/guest_tests.rs"]
mod tests;
