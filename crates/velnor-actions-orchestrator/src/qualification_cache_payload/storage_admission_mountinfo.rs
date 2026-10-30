//! Check one exact read-only mount entry from Linux mountinfo.

pub(super) fn validate_read_only_mountinfo(
    mountinfo: &str,
    target: &str,
    expected_mount_id: u64,
    expected_device_major: u32,
    expected_device_minor: u32,
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
        let (major, minor) = parse_device(fields[2])?;
        let mount_options = fields[5].split(',').collect::<Vec<_>>();
        if mount_id != expected_mount_id
            || major != expected_device_major
            || minor != expected_device_minor
            || !mount_options.contains(&"ro")
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

fn parse_device(value: &str) -> Result<(u32, u32), &'static str> {
    let mut parts = value.split(':');
    let major = parts
        .next()
        .ok_or("cache_storage_mountinfo_invalid")?
        .parse::<u32>()
        .map_err(|_| "cache_storage_mountinfo_invalid")?;
    let minor = parts
        .next()
        .ok_or("cache_storage_mountinfo_invalid")?
        .parse::<u32>()
        .map_err(|_| "cache_storage_mountinfo_invalid")?;
    if parts.next().is_some() {
        return Err("cache_storage_mountinfo_invalid");
    }
    Ok((major, minor))
}

#[cfg(test)]
#[path = "storage_admission_mountinfo_tests.rs"]
mod tests;
