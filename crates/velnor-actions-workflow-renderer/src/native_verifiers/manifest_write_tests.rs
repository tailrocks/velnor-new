// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::PUT;
use std::{error::Error, fs, process::Command};

#[test]
fn concurrent_manifest_writers_preserve_verified_bytes_and_clean_up() -> Result<(), Box<dyn Error>>
{
    let root = tempfile::tempdir()?;
    let source = root.path().join("channel.toml");
    let destination = root.path().join("installed.toml");
    let gate = root.path().join("start");
    let bytes = vec![b'x'; 4 * 1024 * 1024];
    fs::write(&source, &bytes)?;
    let script = format!(
        "set -eu; ev() {{ /usr/bin/env \"$@\"; }}; {PUT}; while [ ! -f \"$gate\" ]; do sleep 0.01; done; put \"$source\" \"$destination\""
    );
    let mut children = Vec::new();
    for index in 0..8 {
        let work = root.path().join(format!("work-{index}"));
        fs::create_dir(&work)?;
        children.push(
            Command::new("/bin/sh")
                .arg("-c")
                .arg(&script)
                .env("d", work)
                .env("source", &source)
                .env("destination", &destination)
                .env("gate", &gate)
                .spawn()?,
        );
    }
    fs::write(&gate, b"start")?;
    for mut child in children {
        assert!(child.wait()?.success());
    }
    assert_eq!(fs::read(&destination)?, bytes);
    assert_eq!(
        fs::read_dir(root.path())?
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .filter(|entry| entry
                .file_name()
                .to_string_lossy()
                .starts_with("installed.toml."))
            .count(),
        0
    );
    fs::remove_file(&destination)?;
    fs::create_dir(&destination)?;
    let status = Command::new("/bin/sh")
        .arg("-c")
        .arg(&script)
        .env("d", root.path().join("work-0"))
        .env("source", &source)
        .env("destination", &destination)
        .env("gate", &gate)
        .status()?;
    assert!(!status.success());
    assert!(destination.is_dir());
    fs::remove_dir(&destination)?;
    fs::remove_file(&source)?;
    let status = Command::new("/bin/sh")
        .arg("-c")
        .arg(&script)
        .env("d", root.path().join("work-0"))
        .env("source", &source)
        .env("destination", &destination)
        .env("gate", &gate)
        .status()?;
    assert!(!status.success());
    assert!(!destination.exists());
    assert_eq!(
        fs::read_dir(root.path())?
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .filter(|entry| entry
                .file_name()
                .to_string_lossy()
                .starts_with("installed.toml."))
            .count(),
        0
    );
    Ok(())
}
