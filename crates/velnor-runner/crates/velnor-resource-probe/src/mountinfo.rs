use crate::{ProbeError, units::digits};

const TARGET: &str = "/velnor/docker-root";
const MOUNTINFO_LIMIT: usize = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MountEntry {
    pub(crate) mount_id: u64,
    pub(crate) device_major: u32,
    pub(crate) device_minor: u32,
    pub(crate) filesystem_type: String,
    pub(crate) read_only: bool,
}

/// Find the unique read-only mount entry for the fixed Docker-root target.
pub(crate) fn docker_root_mount(input: &[u8]) -> Result<MountEntry, ProbeError> {
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
        if decode_mount_field(fields[4])? != TARGET {
            continue;
        }
        if found.is_some() {
            return Err(ProbeError::Invalid("docker_root_mount_duplicate"));
        }
        let mount_id = parse_u64(fields[0], "mountinfo_id")?;
        let (device_major, device_minor) = parse_device(fields[2])?;
        let read_only = fields[5].split(',').any(|option| option == "ro");
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
            filesystem_type: filesystem_type.to_owned(),
            read_only,
        });
    }
    let entry = found.ok_or(ProbeError::Invalid("docker_root_mount_missing"))?;
    if !entry.read_only {
        return Err(ProbeError::Invalid("docker_root_mount_not_read_only"));
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
        assert_eq!(entry.filesystem_type, "btrfs");
        assert!(entry.read_only);
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
