// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::{Path, PathBuf};
use std::process::Stdio;

use crate::exec::ExecError;

const MOUNTINFO: &str = "/proc/self/mountinfo";
const MOUNT_COMMAND: &str = "/sbin/mount";

struct Mount {
    point: PathBuf,
    writable: bool,
}

fn unreadable(source: &str) -> ExecError {
    ExecError::Invalid(format!(
        "mise policy: the mount table from {source} cannot be parsed"
    ))
}

fn has_option(options: &str, wanted: &str) -> bool {
    options.split(',').any(|option| option == wanted)
}

fn octal(rest: &[u8]) -> Option<u8> {
    let digits = rest.strip_prefix(b"\\")?.get(..3)?;
    u8::from_str_radix(std::str::from_utf8(digits).ok()?, 8).ok()
}

fn unescape(field: &str) -> String {
    let mut decoded = Vec::with_capacity(field.len());
    let mut rest = field.as_bytes();
    while let Some((&byte, tail)) = rest.split_first() {
        if let Some(value) = octal(rest) {
            decoded.push(value);
            rest = rest.get(4..).unwrap_or_default();
        } else {
            decoded.push(byte);
            rest = tail;
        }
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

fn parse_mountinfo(text: &str) -> Result<Vec<Mount>, ExecError> {
    let parse = |line: &str| {
        let fields: Vec<&str> = line.split(' ').collect();
        let separator = fields.iter().position(|field| *field == "-")?;
        let mount_options = fields.get(5)?;
        let super_options = fields.get(separator + 3)?;
        Some(Mount {
            point: PathBuf::from(unescape(fields.get(4)?)),
            writable: has_option(mount_options, "rw") && has_option(super_options, "rw"),
        })
    };
    text.lines()
        .filter(|line| !line.is_empty())
        .map(parse)
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| unreadable(MOUNTINFO))
}

fn parse_mount_output(text: &str) -> Result<Vec<Mount>, ExecError> {
    let parse = |line: &str| {
        let (_, rest) = line.split_once(" on ")?;
        let (point, options) = rest.rsplit_once(" (")?;
        let options = options.strip_suffix(')')?;
        Some(Mount {
            point: PathBuf::from(point),
            writable: !options.split(", ").any(|option| option == "read-only"),
        })
    };
    text.lines()
        .filter(|line| !line.is_empty())
        .map(parse)
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| unreadable(MOUNT_COMMAND))
}

#[cfg(feature = "test-support")]
fn declared_table() -> Option<PathBuf> {
    std::env::var_os("VELNOR_TEST_MOUNTINFO").map(PathBuf::from)
}

#[cfg(not(feature = "test-support"))]
fn declared_table() -> Option<PathBuf> {
    None
}

async fn read_table() -> Result<Vec<Mount>, ExecError> {
    let io = |what: &str| {
        let what = what.to_owned();
        move |source| ExecError::io(what, source)
    };
    if let Some(declared) = declared_table() {
        let text = tokio::fs::read_to_string(&declared)
            .await
            .map_err(io(&format!("read {}", declared.display())))?;
        return parse_mountinfo(&text);
    }
    if cfg!(target_os = "linux") {
        let text = tokio::fs::read_to_string(MOUNTINFO)
            .await
            .map_err(io(&format!("read {MOUNTINFO}")))?;
        return parse_mountinfo(&text);
    }
    let output = tokio::process::Command::new(MOUNT_COMMAND)
        .env_clear()
        .stdin(Stdio::null())
        .output()
        .await
        .map_err(io(&format!("run {MOUNT_COMMAND}")))?;
    if !output.status.success() {
        return Err(unreadable(MOUNT_COMMAND));
    }
    parse_mount_output(&String::from_utf8_lossy(&output.stdout))
}

pub(super) async fn require_no_writable_mount_under(
    directories: &[(&str, &Path)],
) -> Result<(), ExecError> {
    let mut roots = Vec::with_capacity(directories.len());
    for (role, directory) in directories {
        let real = tokio::fs::canonicalize(directory)
            .await
            .map_err(|source| ExecError::io(format!("resolve {}", directory.display()), source))?;
        roots.push((*role, real));
    }
    for mount in read_table().await?.iter().filter(|mount| mount.writable) {
        if let Some((role, root)) = roots.iter().find(|(_, root)| mount.point.starts_with(root)) {
            return Err(ExecError::Invalid(format!(
                "mise policy: {role} {} holds a writable mount at {}; the tools store must be one read-only mount",
                root.display(),
                mount.point.display()
            )));
        }
    }
    Ok(())
}
