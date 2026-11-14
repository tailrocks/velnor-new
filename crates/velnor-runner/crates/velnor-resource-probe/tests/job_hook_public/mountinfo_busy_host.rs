// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fmt::Write as _;
use std::fs;

use velnor_resource_probe::ProbeError;
use velnor_resource_probe::mountinfo_io::{MOUNTINFO_LIMIT, read_mountinfo};

use super::ScopedTempDir;

const CGROUP_LINE: &str =
    "35 25 0:30 / /sys/fs/cgroup ro,nosuid,nodev,noexec,relatime - cgroup2 cgroup rw\n";

fn busy_host_mountinfo(minimum_bytes: usize) -> Result<String, std::fmt::Error> {
    let mut text = String::new();
    let mut index = 0;
    while text.len() + CGROUP_LINE.len() < minimum_bytes {
        let id = 1000 + index;
        writeln!(
            text,
            "{id} 28 0:{} / /var/lib/docker/overlay2/{index:064x}/merged rw,relatime shared:{id} - overlay overlay rw,lowerdir=/var/lib/docker/overlay2/l/{index:026x}:/var/lib/docker/overlay2/l/{index:026x},upperdir=/var/lib/docker/overlay2/{index:064x}/diff,workdir=/var/lib/docker/overlay2/{index:064x}/work",
            100 + index
        )?;
        index += 1;
    }
    text.push_str(CGROUP_LINE);
    Ok(text)
}

#[test]
fn probe_reads_mountinfo_of_a_busy_docker_host() -> Result<(), Box<dyn Error>> {
    let text = busy_host_mountinfo(200 * 1024)?;
    assert!(text.len() < 220 * 1024);
    let dir = ScopedTempDir::new("mountinfo-busy")?;
    let path = dir.path().join("mountinfo");
    fs::write(&path, &text)?;
    let bytes = read_mountinfo(path.to_str().ok_or("path")?, "mountinfo")?;
    assert_eq!(bytes, text.as_bytes());
    assert!(text.ends_with(CGROUP_LINE));
    Ok(())
}

#[test]
fn probe_refuses_mountinfo_over_the_bound() -> Result<(), Box<dyn Error>> {
    let dir = ScopedTempDir::new("mountinfo-huge")?;
    let path = dir.path().join("mountinfo");
    fs::write(&path, busy_host_mountinfo(MOUNTINFO_LIMIT + 1)?)?;
    match read_mountinfo(path.to_str().ok_or("path")?, "mountinfo") {
        Err(error @ ProbeError::InputTooLarge("mountinfo")) => {
            assert_eq!(error.to_string(), "mountinfo_too_large");
            Ok(())
        }
        other => Err(format!("expected mountinfo_too_large, got {other:?}").into()),
    }
}
