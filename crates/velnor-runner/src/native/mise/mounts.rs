// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::PathBuf;

use super::walk::writable_directories;
use crate::exec::ExecError;

const MOUNTINFO: &str = "/proc/self/mountinfo";

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

#[cfg(feature = "test-support")]
fn declared_table() -> Option<PathBuf> {
    std::env::var_os("VELNOR_TEST_MOUNTINFO")
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
}

#[cfg(not(feature = "test-support"))]
fn declared_table() -> Option<PathBuf> {
    None
}

fn table_source() -> Option<PathBuf> {
    declared_table().or_else(|| cfg!(target_os = "linux").then(|| PathBuf::from(MOUNTINFO)))
}

async fn writable_points(roots: &[(&str, PathBuf)]) -> Result<Vec<PathBuf>, ExecError> {
    let Some(source) = table_source() else {
        return writable_directories(roots.iter().map(|(_, root)| root.clone()).collect()).await;
    };
    let text = tokio::fs::read_to_string(&source)
        .await
        .map_err(|error| ExecError::io(format!("read {}", source.display()), error))?;
    Ok(parse_mountinfo(&text)?
        .into_iter()
        .filter(|mount| mount.writable)
        .map(|mount| mount.point)
        .collect())
}

pub(super) async fn require_no_writable_mount_under(
    roots: &[(&str, PathBuf)],
) -> Result<(), ExecError> {
    for point in writable_points(roots).await? {
        if let Some((role, root)) = roots.iter().find(|(_, root)| point.starts_with(root)) {
            return Err(ExecError::Invalid(format!(
                "mise policy: {role} {} holds a writable mount at {}; the tools store must be one read-only mount",
                root.display(),
                point.display()
            )));
        }
    }
    Ok(())
}
