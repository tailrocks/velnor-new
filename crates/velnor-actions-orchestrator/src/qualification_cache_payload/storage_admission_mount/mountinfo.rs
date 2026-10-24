use super::{JOB_FACTS_DIRECTORY, MAX_MOUNTINFO_BYTES, MOUNTINFO_PATH, MountObservation};

pub(super) fn read_mountinfo() -> Result<String, &'static str> {
    use std::fs::File;
    use std::io::Read;

    let mut mountinfo =
        File::open(MOUNTINFO_PATH).map_err(|_| "cache_storage_mountinfo_read_failed")?;
    let mut mountinfo_bytes = Vec::new();
    (&mut mountinfo)
        .take(u64::try_from(MAX_MOUNTINFO_BYTES + 1).unwrap_or(u64::MAX))
        .read_to_end(&mut mountinfo_bytes)
        .map_err(|_| "cache_storage_mountinfo_read_failed")?;
    if mountinfo_bytes.len() > MAX_MOUNTINFO_BYTES {
        return Err("cache_storage_mountinfo_too_large");
    }
    String::from_utf8(mountinfo_bytes).map_err(|_| "cache_storage_mountinfo_invalid")
}

pub(super) fn validate_work_mountinfo(
    mountinfo: &str,
    target: &str,
    mount_id: u64,
    device_major: u32,
    device_minor: u32,
) -> Result<(), &'static str> {
    validate_path_mountinfo(mountinfo, target, mount_id, device_major, device_minor)
}

type SelectedMount<'a> = (usize, u64, u32, u32, Vec<&'a str>, &'a str);

fn validate_path_mountinfo(
    mountinfo: &str,
    target: &str,
    expected_mount_id: u64,
    expected_device_major: u32,
    expected_device_minor: u32,
) -> Result<(), &'static str> {
    if !target.starts_with('/') || target.ends_with('/') && target != "/" {
        return Err("cache_storage_mount_path_invalid");
    }
    let mut selected: Option<SelectedMount<'_>> = None;
    for line in mountinfo.lines() {
        let mut halves = line.splitn(2, " - ");
        let left = halves.next().ok_or("cache_storage_mountinfo_invalid")?;
        let right = halves.next().ok_or("cache_storage_mountinfo_invalid")?;
        let fields = left.split_ascii_whitespace().collect::<Vec<_>>();
        if fields.len() < 6 {
            return Err("cache_storage_mountinfo_invalid");
        }
        let mountpoint = fields[4];
        let covers_target = mountpoint == "/"
            || target == mountpoint
            || target
                .strip_prefix(mountpoint)
                .is_some_and(|suffix| suffix.starts_with('/'));
        if !covers_target {
            continue;
        }
        if selected
            .as_ref()
            .is_some_and(|(length, ..)| mountpoint.len() == *length)
        {
            return Err("cache_storage_mount_ambiguous");
        }
        if selected
            .as_ref()
            .is_some_and(|(length, ..)| mountpoint.len() < *length)
        {
            continue;
        }
        let mount_id = fields[0]
            .parse::<u64>()
            .map_err(|_| "cache_storage_mountinfo_invalid")?;
        let mut device = fields[2].split(':');
        let major = device
            .next()
            .ok_or("cache_storage_mountinfo_invalid")?
            .parse::<u32>()
            .map_err(|_| "cache_storage_mountinfo_invalid")?;
        let minor = device
            .next()
            .ok_or("cache_storage_mountinfo_invalid")?
            .parse::<u32>()
            .map_err(|_| "cache_storage_mountinfo_invalid")?;
        if device.next().is_some() {
            return Err("cache_storage_mountinfo_invalid");
        }
        selected = Some((
            mountpoint.len(),
            mount_id,
            major,
            minor,
            fields[5].split(',').collect(),
            right,
        ));
    }
    let Some((_, mount_id, major, minor, options, filesystem)) = selected else {
        return Err("cache_storage_mount_missing");
    };
    if mount_id != expected_mount_id
        || major != expected_device_major
        || minor != expected_device_minor
        || options.contains(&"ro")
        || !options.contains(&"rw")
        || filesystem.trim().is_empty()
    {
        return Err("cache_storage_mount_not_read_only_bind");
    }
    Ok(())
}

pub(super) fn validate_mountinfo(
    mountinfo: &str,
    expected: MountObservation,
) -> Result<(), &'static str> {
    validate_local_mountinfo(
        mountinfo,
        JOB_FACTS_DIRECTORY,
        expected.mount_id,
        expected.device_major,
        expected.device_minor,
        true,
    )
}

pub(super) fn validate_read_only_mountinfo(
    mountinfo: &str,
    target: &str,
    mount_id: u64,
    device_major: u32,
    device_minor: u32,
) -> Result<(), &'static str> {
    validate_local_mountinfo(
        mountinfo,
        target,
        mount_id,
        device_major,
        device_minor,
        true,
    )
}

fn validate_local_mountinfo(
    mountinfo: &str,
    target: &str,
    expected_mount_id: u64,
    expected_device_major: u32,
    expected_device_minor: u32,
    read_only: bool,
) -> Result<(), &'static str> {
    let mut found = false;
    for line in mountinfo.lines() {
        let mut halves = line.splitn(2, " - ");
        let left = halves.next().ok_or("cache_storage_mountinfo_invalid")?;
        let right = halves.next().ok_or("cache_storage_mountinfo_invalid")?;
        let fields = left.split_ascii_whitespace().collect::<Vec<_>>();
        if fields.len() < 6 {
            return Err("cache_storage_mountinfo_invalid");
        }
        if fields[4] != target {
            continue;
        }
        if found {
            return Err("cache_storage_mount_ambiguous");
        }
        found = true;
        let mount_id = fields[0]
            .parse::<u64>()
            .map_err(|_| "cache_storage_mountinfo_invalid")?;
        let mut device = fields[2].split(':');
        let major = device
            .next()
            .ok_or("cache_storage_mountinfo_invalid")?
            .parse::<u32>()
            .map_err(|_| "cache_storage_mountinfo_invalid")?;
        let minor = device
            .next()
            .ok_or("cache_storage_mountinfo_invalid")?
            .parse::<u32>()
            .map_err(|_| "cache_storage_mountinfo_invalid")?;
        if device.next().is_some() {
            return Err("cache_storage_mountinfo_invalid");
        }
        let mount_options = fields[5].split(',').collect::<Vec<_>>();
        if mount_id != expected_mount_id
            || major != expected_device_major
            || minor != expected_device_minor
            || mount_options.contains(&if read_only { "rw" } else { "ro" })
            || !mount_options.contains(&if read_only { "ro" } else { "rw" })
            || right.trim().is_empty()
        {
            return Err("cache_storage_mount_not_read_only_bind");
        }
    }
    if found {
        Ok(())
    } else {
        Err("cache_storage_mount_missing")
    }
}
