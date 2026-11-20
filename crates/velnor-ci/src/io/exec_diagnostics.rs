// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::process::Command;

use crate::exec_failure::denied_execs;
use crate::tests_run::runs_nextest;

pub(super) fn record(log: &mut File, path: &Path, command: &[String], code: i32, from: usize) {
    if code == 0 || !runs_nextest(command) {
        return;
    }
    for line in lines(path, from) {
        eprintln!("{line}");
        writeln!(log, "{line}").ok();
    }
}

fn lines(log: &Path, from: usize) -> Vec<String> {
    let Ok(text) = read_from(log, from) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for path in denied_execs(&text) {
        out.push(format!(
            "velnor-ci exec-failure: {path}: {}",
            stat(Path::new(path))
        ));
        out.extend(mount(Path::new(path)));
    }
    out
}

fn read_from(log: &Path, from: usize) -> std::io::Result<String> {
    let mut file = File::open(log)?;
    file.seek(SeekFrom::Start(u64::try_from(from).unwrap_or(u64::MAX)))?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn stat(path: &Path) -> String {
    match std::fs::metadata(path) {
        Ok(meta) => format!(
            "mode={:o} uid={} gid={} size={} mtime={}",
            meta.mode() & 0o7777,
            meta.uid(),
            meta.gid(),
            meta.size(),
            meta.mtime()
        ),
        Err(error) => format!("stat failed: {error}"),
    }
}

fn mount(path: &Path) -> Vec<String> {
    let header = format!("velnor-ci exec-failure: findmnt -T {}:", path.display());
    match Command::new("findmnt").arg("-T").arg(path).output() {
        Ok(output) => std::iter::once(header)
            .chain(
                String::from_utf8_lossy(&output.stdout)
                    .lines()
                    .chain(String::from_utf8_lossy(&output.stderr).lines())
                    .map(|line| format!("velnor-ci exec-failure: {line}")),
            )
            .collect(),
        Err(error) => vec![
            header,
            format!("velnor-ci exec-failure: findmnt failed: {error}"),
        ],
    }
}
