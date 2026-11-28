// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use tempfile::TempDir;

use super::job_dirs::{remover, sweeper};
use super::user::OWNED_ELSEWHERE;
use crate::rust_runner::shape::USER_HOME;

type Check = Result<(), Box<dyn Error>>;

struct Run {
    code: Option<i32>,
    text: String,
}

struct Sandbox {
    dir: TempDir,
}

impl Sandbox {
    fn new() -> Result<Self, Box<dyn Error>> {
        let dir = tempfile::tempdir()?;
        fs::create_dir(dir.path().join("bin"))?;
        let sandbox = Self { dir };
        sandbox.stub(
            "find",
            r#"case "$*" in *-printf*) for d; do case "$d" in -*) break;; esac; ls -A "$d"; done;; esac"#,
        )?;
        Ok(sandbox)
    }

    fn root(&self) -> &Path {
        self.dir.path()
    }

    fn stub(&self, name: &str, body: &str) -> Check {
        let path = self.root().join("bin").join(name);
        fs::write(&path, format!("#!/bin/sh\n{body}\n"))?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755))?;
        Ok(())
    }

    fn mounts(&self, targets: &[&Path]) -> Check {
        let listed: Vec<String> = targets
            .iter()
            .map(|target| target.display().to_string())
            .collect();
        let table = ["/", "/proc"]
            .into_iter()
            .map(str::to_owned)
            .chain(listed)
            .collect::<Vec<_>>()
            .join("\n");
        fs::write(self.root().join("table"), format!("{table}\n"))?;
        let table = self.root().join("table");
        self.stub("findmnt", &format!("cat '{}'", table.display()))
    }

    fn job(&self, name: &str) -> Result<PathBuf, Box<dyn Error>> {
        let dir = self.root().join("work").join(name);
        fs::create_dir_all(dir.join("tools"))?;
        fs::write(dir.join("tools/precious"), "kept")?;
        Ok(dir)
    }

    fn sh(&self, script: &str, args: &[&Path]) -> Result<Run, Box<dyn Error>> {
        let path = format!("{}:/usr/bin:/bin", self.root().join("bin").display());
        let done = Command::new("/bin/sh")
            .args(["-c", script, "sh"])
            .args(args)
            .env("PATH", path)
            .env("STUBS", self.root())
            .output()?;
        Ok(Run {
            code: done.status.code(),
            text: String::from_utf8_lossy(&done.stdout).into_owned(),
        })
    }
}

#[test]
fn the_remover_removes_nothing_and_returns_73_without_a_readable_mount_table() -> Check {
    let blind: [(&str, &str, &str); 3] = [
        ("findmnt", "echo /; exit 1", "a failing findmnt"),
        ("findmnt", "exit 0", "an empty mount table"),
        ("awk", "exit 2", "an awk that cannot read it"),
    ];
    let mut removed = Vec::new();
    for (tool, body, name) in blind {
        let sandbox = Sandbox::new()?;
        sandbox.mounts(&[])?;
        sandbox.stub(tool, body)?;
        let dir = sandbox.job("1")?;
        let run = sandbox.sh(&remover(), &[&dir])?;
        if run.code != Some(73) || !dir.join("tools/precious").exists() {
            removed.push(format!("{name}: {:?}", run.code));
        }
    }
    assert!(removed.is_empty(), "{removed:?}");
    Ok(())
}

#[test]
fn the_remover_removes_nothing_when_a_mount_is_at_or_below_the_job_dir() -> Check {
    let mut removed = Vec::new();
    for below in ["1/tools", "1"] {
        let sandbox = Sandbox::new()?;
        let dir = sandbox.job("1")?;
        sandbox.mounts(&[&sandbox.root().join("work").join(below)])?;
        let run = sandbox.sh(&remover(), &[&dir])?;
        if run.code != Some(72) || !dir.join("tools/precious").exists() {
            removed.push(format!("{below}: {:?}", run.code));
        }
    }
    assert!(removed.is_empty(), "{removed:?}");
    Ok(())
}

#[test]
fn the_remover_removes_the_dirs_when_nothing_is_mounted_in_them() -> Check {
    let sandbox = Sandbox::new()?;
    sandbox.mounts(&[])?;
    let (first, second) = (sandbox.job("1")?, sandbox.job("2")?);
    let run = sandbox.sh(&remover(), &[&first, &second])?;
    assert_eq!(run.code, Some(0));
    assert!(!first.exists() && !second.exists());
    Ok(())
}

#[test]
fn the_sweeper_removes_nothing_and_returns_73_without_a_readable_mount_table() -> Check {
    let blind: [(&str, &str, &str); 3] = [
        ("findmnt", "echo /; exit 1", "a failing findmnt"),
        ("findmnt", "exit 0", "an empty mount table"),
        ("awk", "exit 2", "an awk that cannot read it"),
    ];
    let mut removed = Vec::new();
    for (tool, body, name) in blind {
        let sandbox = Sandbox::new()?;
        sandbox.mounts(&[])?;
        sandbox.stub(tool, body)?;
        let dir = sandbox.job("1")?;
        let work = sandbox.root().join("work");
        let run = sandbox.sh(&sweeper(), &[&work])?;
        if run.code != Some(73) || !dir.join("tools/precious").exists() {
            removed.push(format!("{name}: {:?}", run.code));
        }
    }
    assert!(removed.is_empty(), "{removed:?}");
    Ok(())
}

#[test]
fn the_sweeper_keeps_a_reservation_with_a_mount_and_removes_the_others() -> Check {
    let sandbox = Sandbox::new()?;
    let (mounted, plain) = (sandbox.job("1")?, sandbox.job("2")?);
    sandbox.mounts(&[&mounted.join("tools")])?;
    let work = sandbox.root().join("work");
    let run = sandbox.sh(&sweeper(), &[&work])?;
    assert_eq!(run.code, Some(0));
    assert!(mounted.join("tools/precious").exists());
    assert!(!plain.exists());
    assert_eq!(run.text.trim(), "1");
    Ok(())
}

#[test]
fn the_sweeper_removes_every_reservation_when_nothing_is_mounted() -> Check {
    let sandbox = Sandbox::new()?;
    sandbox.mounts(&[])?;
    let (first, second) = (sandbox.job("1")?, sandbox.job("2")?);
    let work = sandbox.root().join("work");
    let run = sandbox.sh(&sweeper(), &[&work])?;
    assert_eq!(run.code, Some(0));
    assert!(!first.exists() && !second.exists());
    assert_eq!(run.text.trim(), "");
    Ok(())
}

fn scan(plan: &[&str]) -> Result<(Run, usize), Box<dyn Error>> {
    let sandbox = Sandbox::new()?;
    let calls = sandbox.root().join("calls");
    fs::write(&calls, "")?;
    let outcomes = sandbox.root().join("plan");
    fs::write(&outcomes, plan.join("\n"))?;
    sandbox.stub(
        "find",
        &format!(
            r#"echo x >> '{calls}'; n=$(wc -l < '{calls}' | tr -d ' '); case $(sed -n "${{n}}p" '{plan}') in vanish) echo "find: '/proc/9/x': No such file or directory"; exit 1;; hit) echo /etc/shadow; exit 0;; denied) echo "find: '/x': Permission denied"; exit 1;; broken) exit 1;; esac"#,
            calls = calls.display(),
            plan = outcomes.display()
        ),
    )?;
    let run = sandbox.sh(
        OWNED_ELSEWHERE,
        &[Path::new(USER_HOME), Path::new("500"), Path::new("501")],
    )?;
    let made = fs::read_to_string(&calls)?.lines().count();
    Ok((run, made))
}

#[test]
fn the_owned_files_scan_retries_once_when_an_entry_vanished_and_refuses_after_two() -> Check {
    let (retried, calls) = scan(&["vanish", "clean"])?;
    assert_eq!((retried.code, retried.text.trim(), calls), (Some(0), "", 2));
    let (once, calls) = scan(&["clean"])?;
    assert_eq!((once.code, once.text.trim(), calls), (Some(0), "", 1));
    let (twice, calls) = scan(&["vanish", "vanish", "clean"])?;
    assert_eq!((twice.code, calls), (Some(1), 2));
    let (broken, _) = scan(&["broken", "broken"])?;
    assert_eq!(broken.code, Some(1));
    Ok(())
}

#[test]
fn the_owned_files_scan_reports_a_hit_or_any_other_error_and_never_retries_it() -> Check {
    for outcome in ["hit", "denied"] {
        let (run, calls) = scan(&[outcome, "clean"])?;
        assert!(!run.text.trim().is_empty(), "{outcome}");
        assert_eq!(calls, 1, "{outcome}");
    }
    Ok(())
}
