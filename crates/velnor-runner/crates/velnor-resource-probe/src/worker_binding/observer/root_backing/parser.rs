use crate::ProbeError;

pub(super) const ROOT_PATH_LIMIT: usize = 4096;
#[cfg(any(target_os = "linux", test))]
const MOUNTINFO_LIMIT: usize = 65_536;
#[cfg(any(target_os = "linux", test))]
const MAX_LOWER_DIRS: usize = 16;
#[cfg(any(target_os = "linux", test))]
const MAX_OVERLAY_OPTIONS: usize = 128;
#[cfg(any(target_os = "linux", test))]
const MAX_OVERLAY_PATH_BYTES: usize = 12_288;

#[derive(Debug, PartialEq, Eq)]
#[cfg(any(target_os = "linux", test))]
pub(super) struct OverlayPaths {
    pub(super) upperdir: String,
    pub(super) workdir: String,
    pub(super) lowerdirs: Vec<String>,
}

#[derive(Debug, PartialEq, Eq)]
#[cfg(any(target_os = "linux", test))]
pub(super) struct RootMount {
    pub(super) mount_id: u64,
    pub(super) device_major: u32,
    pub(super) device_minor: u32,
    pub(super) mount_root: String,
    pub(super) read_only: bool,
    pub(super) filesystem_type: String,
    pub(super) mount_source: String,
    pub(super) super_options: String,
}

#[derive(Debug, PartialEq, Eq)]
#[cfg(any(target_os = "linux", test))]
pub(super) struct BackingMount {
    pub(super) mount_id: u64,
    pub(super) device_major: u32,
    pub(super) device_minor: u32,
    pub(super) root: String,
    pub(super) source: String,
    pub(super) filesystem_type: String,
    pub(super) read_only: bool,
}

pub(crate) fn validate_docker_root_path(path: &str) -> Result<(), ProbeError> {
    if path.is_empty()
        || path.len() > ROOT_PATH_LIMIT
        || !path.starts_with('/')
        || path == "/"
        || path.starts_with("//")
        || path.ends_with('/')
        || path.bytes().any(|byte| byte.is_ascii_control())
        || path
            .split('/')
            .skip(1)
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(ProbeError::Invalid("docker_root_path"));
    }
    Ok(())
}

#[cfg(any(target_os = "linux", test))]
pub(super) fn parse_root_mount(input: &[u8]) -> Result<RootMount, ProbeError> {
    if input.len() > MOUNTINFO_LIMIT {
        return Err(ProbeError::InputTooLarge("process_mountinfo"));
    }
    let text = std::str::from_utf8(input).map_err(|_| ProbeError::Invalid("mountinfo_utf8"))?;
    let mut result = None;
    for line in text.lines() {
        let fields = line.split_ascii_whitespace().collect::<Vec<_>>();
        let separator = fields
            .iter()
            .position(|field| *field == "-")
            .ok_or(ProbeError::Invalid("mountinfo_separator"))?;
        if separator < 6 || fields.len() != separator + 4 {
            return Err(ProbeError::Invalid("mountinfo_fields"));
        }
        let mountpoint = decode_mount_field(fields[4])?;
        if mountpoint != "/" {
            continue;
        }
        if result.is_some() {
            return Err(ProbeError::Invalid("root_mount_duplicate"));
        }
        let mount_id = parse_decimal(fields[0], "mountinfo_id")?;
        let (device_major, device_minor) = parse_device(fields[2])?;
        let mount_root = decode_mount_field(fields[3])?;
        if mount_root != "/" {
            return Err(ProbeError::Invalid("root_mount_root"));
        }
        let filesystem_type = fields[separator + 1];
        let mount_source = decode_mount_field(fields[separator + 2])?;
        let super_options = fields[separator + 3];
        if filesystem_type.is_empty() || mount_source.is_empty() || super_options.is_empty() {
            return Err(ProbeError::Invalid("root_mount_fields"));
        }
        let read_only = fields[5].split(',').any(|option| option == "ro");
        result = Some(RootMount {
            mount_id,
            device_major,
            device_minor,
            mount_root,
            read_only,
            filesystem_type: filesystem_type.to_owned(),
            mount_source,
            super_options: super_options.to_owned(),
        });
    }
    result.ok_or(ProbeError::Invalid("root_mount_missing"))
}

#[cfg(any(target_os = "linux", test))]
pub(super) fn backing_mount(input: &[u8], target: &str) -> Result<BackingMount, ProbeError> {
    if input.len() > MOUNTINFO_LIMIT || !target.starts_with('/') {
        return Err(ProbeError::Invalid("backing_mount_input"));
    }
    let text = std::str::from_utf8(input).map_err(|_| ProbeError::Invalid("mountinfo_utf8"))?;
    let mut selected: Option<(usize, BackingMount)> = None;
    for line in text.lines() {
        let fields = line.split_ascii_whitespace().collect::<Vec<_>>();
        let separator = fields
            .iter()
            .position(|field| *field == "-")
            .ok_or(ProbeError::Invalid("mountinfo_separator"))?;
        if separator < 6 || fields.len() < separator + 4 {
            return Err(ProbeError::Invalid("mountinfo_fields"));
        }
        let mountpoint = decode_mount_field(fields[4])?;
        let is_parent = mountpoint == "/"
            || target == mountpoint
            || target
                .strip_prefix(&mountpoint)
                .is_some_and(|suffix| suffix.starts_with('/'));
        if !is_parent {
            continue;
        }
        let mount_id = parse_decimal(fields[0], "mountinfo_id")?;
        let (device_major, device_minor) = parse_device(fields[2])?;
        let candidate = BackingMount {
            mount_id,
            device_major,
            device_minor,
            root: decode_mount_field(fields[3])?,
            source: decode_mount_field(fields[separator + 2])?,
            filesystem_type: fields[separator + 1].to_owned(),
            read_only: fields[5].split(',').any(|option| option == "ro"),
        };
        match &selected {
            Some((length, _)) if *length > mountpoint.len() => {}
            Some((length, _)) if *length == mountpoint.len() => {
                return Err(ProbeError::Invalid("backing_mount_duplicate"));
            }
            _ => selected = Some((mountpoint.len(), candidate)),
        }
    }
    selected
        .map(|(_, mount)| mount)
        .ok_or(ProbeError::Invalid("backing_mount_missing"))
}

#[cfg(any(target_os = "linux", test))]
pub(super) fn decode_mount_field(value: &str) -> Result<String, ProbeError> {
    let bytes = value.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'\\' {
            output.push(bytes[index]);
            index += 1;
            continue;
        }
        if index + 3 >= bytes.len() {
            return Err(ProbeError::Invalid("mountinfo_escape"));
        }
        let value = std::str::from_utf8(&bytes[index + 1..index + 4])
            .map_err(|_| ProbeError::Invalid("mountinfo_escape"))?;
        let decoded = match value {
            "040" => b' ',
            "011" => b'\t',
            "012" => b'\n',
            "134" => b'\\',
            _ => return Err(ProbeError::Invalid("mountinfo_escape")),
        };
        output.push(decoded);
        index += 4;
    }
    String::from_utf8(output).map_err(|_| ProbeError::Invalid("mountinfo_escape"))
}

#[cfg(any(target_os = "linux", test))]
pub(super) fn parse_overlay_paths(input: &str) -> Result<OverlayPaths, ProbeError> {
    if input.len() > ROOT_PATH_LIMIT * 8 {
        return Err(ProbeError::InputTooLarge("overlay_options"));
    }
    let options = input.split(',').collect::<Vec<_>>();
    if options.is_empty() || options.len() > MAX_OVERLAY_OPTIONS {
        return Err(ProbeError::Invalid("overlay_option_count"));
    }
    let mut upperdir = None;
    let mut workdir = None;
    let mut lowerdirs = None;
    for option in options {
        if let Some(value) = option.strip_prefix("upperdir=") {
            if upperdir.replace(value.to_owned()).is_some() {
                return Err(ProbeError::Invalid("overlay_upper_duplicate"));
            }
        } else if let Some(value) = option.strip_prefix("workdir=") {
            if workdir.replace(value.to_owned()).is_some() {
                return Err(ProbeError::Invalid("overlay_work_duplicate"));
            }
        } else if let Some(value) = option.strip_prefix("lowerdir=") {
            if lowerdirs.replace(parse_lowerdirs(value)?).is_some() {
                return Err(ProbeError::Invalid("overlay_lower_duplicate"));
            }
        } else if !supported_overlay_option(option) {
            return Err(ProbeError::Invalid("overlay_option_unsupported"));
        }
    }
    let upperdir = upperdir.ok_or(ProbeError::Invalid("overlay_upper_missing"))?;
    let workdir = workdir.ok_or(ProbeError::Invalid("overlay_work_missing"))?;
    let lowerdirs = lowerdirs.ok_or(ProbeError::Invalid("overlay_lower_missing"))?;
    validate_absolute_path(&upperdir, "overlay_upper_path")?;
    validate_absolute_path(&workdir, "overlay_work_path")?;
    if upperdir == workdir
        || lowerdirs
            .iter()
            .any(|path| path == &upperdir || path == &workdir)
    {
        return Err(ProbeError::Invalid("overlay_path_duplicate"));
    }
    let path_bytes = upperdir
        .len()
        .checked_add(workdir.len())
        .and_then(|size| {
            lowerdirs
                .iter()
                .try_fold(size, |total, path| total.checked_add(path.len()))
        })
        .ok_or(ProbeError::Overflow("overlay_path_bytes"))?;
    if path_bytes > MAX_OVERLAY_PATH_BYTES {
        return Err(ProbeError::InputTooLarge("overlay_paths"));
    }
    Ok(OverlayPaths {
        upperdir,
        workdir,
        lowerdirs,
    })
}

#[cfg(any(target_os = "linux", test))]
pub(super) fn supported_overlay_option(option: &str) -> bool {
    matches!(
        option,
        "rw" | "ro"
            | "userxattr"
            | "volatile"
            | "index=on"
            | "index=off"
            | "redirect_dir=on"
            | "redirect_dir=off"
            | "metacopy=on"
            | "metacopy=off"
            | "xino=on"
            | "xino=off"
            | "uuid=on"
            | "uuid=off"
    )
}

#[cfg(any(target_os = "linux", test))]
pub(super) fn parse_lowerdirs(value: &str) -> Result<Vec<String>, ProbeError> {
    let paths = value.split(':').map(str::to_owned).collect::<Vec<_>>();
    if paths.is_empty() || paths.len() > MAX_LOWER_DIRS {
        return Err(ProbeError::Invalid("overlay_lower_count"));
    }
    let mut unique = std::collections::BTreeSet::new();
    for path in &paths {
        validate_absolute_path(path, "overlay_lower_path")?;
        if !unique.insert(path) {
            return Err(ProbeError::Invalid("overlay_lower_duplicate_path"));
        }
    }
    Ok(paths)
}

#[cfg(any(target_os = "linux", test))]
pub(super) fn validate_absolute_path(path: &str, error: &'static str) -> Result<(), ProbeError> {
    if path.len() > ROOT_PATH_LIMIT
        || !path.starts_with('/')
        || path.starts_with("//")
        || path.ends_with('/')
        || path
            .bytes()
            .any(|byte| byte.is_ascii_control() || byte == b'\\')
        || path
            .split('/')
            .skip(1)
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(ProbeError::Invalid(error));
    }
    Ok(())
}

#[cfg(any(target_os = "linux", test))]
pub(super) fn parse_decimal(value: &str, error: &'static str) -> Result<u64, ProbeError> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(ProbeError::Invalid(error));
    }
    value.parse().map_err(|_| ProbeError::Overflow(error))
}

#[cfg(any(target_os = "linux", test))]
pub(super) fn parse_device(value: &str) -> Result<(u32, u32), ProbeError> {
    let (major, minor) = value
        .split_once(':')
        .ok_or(ProbeError::Invalid("mountinfo_device"))?;
    let major = u32::try_from(parse_decimal(major, "mountinfo_device")?)
        .map_err(|_| ProbeError::Overflow("mountinfo_device"))?;
    let minor = u32::try_from(parse_decimal(minor, "mountinfo_device")?)
        .map_err(|_| ProbeError::Overflow("mountinfo_device"))?;
    Ok((major, minor))
}
