use crate::{ProbeError, units::digits};

#[cfg(any(target_os = "linux", test))]
const TARGET: &str = "/velnor/docker-root";
const MOUNTINFO_LIMIT: usize = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MountEntry {
    pub(crate) mount_id: u64,
    pub(crate) device_major: u32,
    pub(crate) device_minor: u32,
    pub(crate) root: String,
    pub(crate) filesystem_type: String,
    pub(crate) read_only: bool,
    pub(crate) executable: bool,
}

/// Return the unique mount row for one fixed target.
#[cfg(any(target_os = "linux", test))]
pub(crate) fn attachment_mount(input: &[u8], target: &str) -> Result<MountEntry, ProbeError> {
    if input.len() > MOUNTINFO_LIMIT {
        return Err(ProbeError::InputTooLarge("mountinfo"));
    }
    let text = std::str::from_utf8(input).map_err(|_| ProbeError::Invalid("mountinfo_utf8"))?;
    let mut found = None;
    for line in text.lines() {
        let fields = line.split_ascii_whitespace().collect::<Vec<_>>();
        let Some(separator) = fields.iter().position(|field| *field == "-") else {
            return Err(ProbeError::Invalid("mountinfo_separator"));
        };
        if separator < 6 || fields.len() < separator + 3 {
            return Err(ProbeError::Invalid("mountinfo_fields"));
        }
        if decode_mount_field(fields[4])? != target {
            continue;
        }
        if found.is_some() {
            return Err(ProbeError::Invalid("attachment_mount_duplicate"));
        }
        let mount_id = parse_u64(fields[0], "mountinfo_id")?;
        let (device_major, device_minor) = parse_device(fields[2])?;
        let options = fields[5].split(',').collect::<Vec<_>>();
        let root = decode_mount_field(fields[3])?;
        let filesystem_type = fields[separator + 1];
        if filesystem_type.is_empty()
            || !filesystem_type
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
        {
            return Err(ProbeError::Invalid("mountinfo_filesystem_type"));
        }
        found = Some(MountEntry {
            mount_id,
            device_major,
            device_minor,
            root,
            filesystem_type: filesystem_type.to_owned(),
            read_only: options.contains(&"ro"),
            executable: !options.contains(&"noexec"),
        });
    }
    found.ok_or(ProbeError::Invalid("attachment_mount_missing"))
}

/// Return the measured mount source.
#[cfg(any(target_os = "linux", test))]
pub(crate) fn attachment_mount_source(input: &[u8], target: &str) -> Result<String, ProbeError> {
    attachment_mount(input, target)?;
    let text = std::str::from_utf8(input).map_err(|_| ProbeError::Invalid("mountinfo_utf8"))?;
    let mut found = None;
    for line in text.lines() {
        let fields = line.split_ascii_whitespace().collect::<Vec<_>>();
        let separator = fields
            .iter()
            .position(|field| *field == "-")
            .ok_or(ProbeError::Invalid("mountinfo_separator"))?;
        if decode_mount_field(fields[4])? != target {
            continue;
        }
        let source = decode_mount_field(fields[separator + 2])?;
        if source.is_empty() || found.replace(source).is_some() {
            return Err(ProbeError::Invalid("attachment_mount_source"));
        }
    }
    found.ok_or(ProbeError::Invalid("attachment_mount_missing"))
}

#[cfg(not(any(target_os = "linux", test)))]
pub(crate) fn attachment_mount_source(_input: &[u8], _target: &str) -> Result<String, ProbeError> {
    Err(ProbeError::Invalid("linux_guest_required"))
}

/// Require one read-only cgroup-v2 mount rooted at the host hierarchy.
pub(crate) fn cgroup2_root(input: &[u8]) -> Result<MountEntry, ProbeError> {
    unique_mount(
        input,
        "/sys/fs/cgroup",
        "cgroup2",
        "/",
        "cgroup2_mount_missing",
        "cgroup2_mount_duplicate",
        "cgroup2_mount_not_read_only",
    )
}

/// Find the unique read-only mount entry for the fixed Docker-root target.
#[cfg(any(target_os = "linux", test))]
pub(crate) fn docker_root_mount(input: &[u8]) -> Result<MountEntry, ProbeError> {
    unique_mount(
        input,
        TARGET,
        "",
        "",
        "docker_root_mount_missing",
        "docker_root_mount_duplicate",
        "docker_root_mount_not_read_only",
    )
}

fn unique_mount(
    input: &[u8],
    target: &str,
    expected_filesystem: &str,
    expected_root: &str,
    missing_error: &'static str,
    duplicate_error: &'static str,
    read_only_error: &'static str,
) -> Result<MountEntry, ProbeError> {
    if input.len() > MOUNTINFO_LIMIT {
        return Err(ProbeError::InputTooLarge("mountinfo"));
    }
    let text = std::str::from_utf8(input).map_err(|_| ProbeError::Invalid("mountinfo_utf8"))?;
    let mut found = None;
    for line in text.lines() {
        let fields = line.split_ascii_whitespace().collect::<Vec<_>>();
        let Some(separator) = fields.iter().position(|field| *field == "-") else {
            return Err(ProbeError::Invalid("mountinfo_separator"));
        };
        if separator < 6 || fields.len() < separator + 3 {
            return Err(ProbeError::Invalid("mountinfo_fields"));
        }
        if decode_mount_field(fields[4])? != target {
            continue;
        }
        if found.is_some() {
            return Err(ProbeError::Invalid(duplicate_error));
        }
        let mount_id = parse_u64(fields[0], "mountinfo_id")?;
        let (device_major, device_minor) = parse_device(fields[2])?;
        let read_only = fields[5].split(',').any(|option| option == "ro");
        let executable = !fields[5].split(',').any(|option| option == "noexec");
        let root = decode_mount_field(fields[3])?;
        let filesystem_type = fields[separator + 1];
        if filesystem_type.is_empty()
            || !filesystem_type
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
        {
            return Err(ProbeError::Invalid("mountinfo_filesystem_type"));
        }
        found = Some(MountEntry {
            mount_id,
            device_major,
            device_minor,
            root,
            filesystem_type: filesystem_type.to_owned(),
            read_only,
            executable,
        });
    }
    let entry = found.ok_or(ProbeError::Invalid(missing_error))?;
    if !entry.read_only {
        return Err(ProbeError::Invalid(read_only_error));
    }
    if !expected_filesystem.is_empty() && entry.filesystem_type != expected_filesystem {
        return Err(ProbeError::Invalid("mount_filesystem_mismatch"));
    }
    if !expected_root.is_empty() && entry.root != expected_root {
        return Err(ProbeError::Invalid("mount_root_mismatch"));
    }
    Ok(entry)
}

fn parse_device(value: &str) -> Result<(u32, u32), ProbeError> {
    let Some((major, minor)) = value.split_once(':') else {
        return Err(ProbeError::Invalid("mountinfo_device"));
    };
    Ok((
        parse_u32(major, "mountinfo_device")?,
        parse_u32(minor, "mountinfo_device")?,
    ))
}

fn parse_u32(value: &str, name: &'static str) -> Result<u32, ProbeError> {
    if !digits(value) {
        return Err(ProbeError::Invalid(name));
    }
    value.parse().map_err(|_| ProbeError::Overflow(name))
}

fn parse_u64(value: &str, name: &'static str) -> Result<u64, ProbeError> {
    if !digits(value) {
        return Err(ProbeError::Invalid(name));
    }
    value.parse().map_err(|_| ProbeError::Overflow(name))
}

fn decode_mount_field(value: &str) -> Result<String, ProbeError> {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'\\' {
            decoded.push(bytes[index]);
            index += 1;
            continue;
        }
        if index + 3 >= bytes.len()
            || !bytes[index + 1..index + 4]
                .iter()
                .all(|byte| (b'0'..=b'7').contains(byte))
        {
            return Err(ProbeError::Invalid("mountinfo_escape"));
        }
        let octal = &bytes[index + 1..index + 4];
        let value = u16::from(octal[0] - b'0') * 64
            + u16::from(octal[1] - b'0') * 8
            + u16::from(octal[2] - b'0');
        if !matches!(value, 9 | 10 | 32 | 92) {
            return Err(ProbeError::Invalid("mountinfo_escape"));
        }
        decoded.push(u8::try_from(value).map_err(|_| ProbeError::Invalid("mountinfo_escape"))?);
        index += 4;
    }
    String::from_utf8(decoded).map_err(|_| ProbeError::Invalid("mountinfo_utf8"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROW: &str =
        "31 20 41:7 /store /velnor/docker-root ro,nosuid - btrfs /dev/vdb1 rw,subvol=/store\n";

    #[test]
    fn accepts_only_one_read_only_target_row() -> Result<(), ProbeError> {
        let entry = docker_root_mount(ROW.as_bytes())?;
        assert_eq!(entry.mount_id, 31);
        assert_eq!((entry.device_major, entry.device_minor), (41, 7));
        assert_eq!(entry.root, "/store");
        assert_eq!(entry.filesystem_type, "btrfs");
        assert!(entry.read_only);
        assert!(entry.executable);
        Ok(())
    }

    #[test]
    fn attachment_mount_reports_actual_read_and_execution_flags() -> Result<(), ProbeError> {
        let row = "31 20 41:7 /store /home/runner/_work rw,nosuid,nodev - ext4 /dev/vdb1 rw\n";
        let mount = attachment_mount(row.as_bytes(), "/home/runner/_work")?;
        assert!(!mount.read_only);
        assert!(mount.executable);
        let row = row.replace("rw,nosuid", "ro,nosuid,noexec");
        let mount = attachment_mount(row.as_bytes(), "/home/runner/_work")?;
        assert!(mount.read_only);
        assert!(!mount.executable);
        assert!(attachment_mount(format!("{row}{row}").as_bytes(), "/home/runner/_work").is_err());
        Ok(())
    }

    #[test]
    fn attachment_mount_source_decodes_the_unique_source_field() -> Result<(), ProbeError> {
        let row = "31 20 41:7 /store /home/runner/_work rw,nosuid - ext4 /dev/work\\040disk rw\n";
        assert_eq!(
            attachment_mount_source(row.as_bytes(), "/home/runner/_work")?,
            "/dev/work disk"
        );
        assert!(attachment_mount_source(row.as_bytes(), "/missing").is_err());
        assert!(
            attachment_mount_source(format!("{row}{row}").as_bytes(), "/home/runner/_work")
                .is_err()
        );
        Ok(())
    }

    #[test]
    fn cgroup_root_requires_one_read_only_full_cgroup2_hierarchy() -> Result<(), ProbeError> {
        let row = "35 20 0:32 / /sys/fs/cgroup ro,nosuid,nodev,noexec - cgroup2 cgroup rw\n";
        let mount = cgroup2_root(row.as_bytes())?;
        assert_eq!(mount.root, "/");
        assert_eq!(mount.filesystem_type, "cgroup2");
        assert!(mount.read_only);
        assert!(cgroup2_root(row.replace("ro,nosuid", "rw,nosuid").as_bytes()).is_err());
        assert!(cgroup2_root(row.replace("cgroup2 cgroup", "tmpfs cgroup").as_bytes()).is_err());
        assert!(cgroup2_root(row.replace("0:32 / ", "0:32 /docker ").as_bytes()).is_err());
        assert!(cgroup2_root(format!("{row}{row}").as_bytes()).is_err());
        Ok(())
    }

    #[test]
    fn rejects_wrong_missing_duplicate_and_writable_mounts() {
        assert!(matches!(
            docker_root_mount(b""),
            Err(ProbeError::Invalid("docker_root_mount_missing"))
        ));
        assert!(matches!(
            docker_root_mount(ROW.replace("/velnor/docker-root", "/other").as_bytes()),
            Err(ProbeError::Invalid("docker_root_mount_missing"))
        ));
        assert!(matches!(
            docker_root_mount(format!("{ROW}{ROW}").as_bytes()),
            Err(ProbeError::Invalid("docker_root_mount_duplicate"))
        ));
        assert!(matches!(
            docker_root_mount(ROW.replace("ro,nosuid", "rw,nosuid").as_bytes()),
            Err(ProbeError::Invalid("docker_root_mount_not_read_only"))
        ));
    }

    #[test]
    fn rejects_bad_device_or_escape() {
        assert!(docker_root_mount(ROW.replace("41:7", "x:7").as_bytes()).is_err());
        assert!(
            docker_root_mount(
                ROW.replace("/velnor/docker-root", "/velnor\\777docker-root")
                    .as_bytes()
            )
            .is_err()
        );
    }
}
