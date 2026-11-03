// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use velnor_actions_mise::{CheckDeadline, GitRequest, TapTreeEntry, read_tap_tree};

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Result<Self, Box<dyn Error>> {
        let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
            .join(format!("checkout-history-{name}-{}", std::process::id()));
        if path.exists() {
            fs::remove_dir_all(&path)?;
        }
        fs::create_dir_all(&path)?;
        Ok(Self(path))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).ok();
    }
}

fn git(cwd: &Path, args: &[&str]) -> Result<String, Box<dyn Error>> {
    let output = Command::new("git")
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .args([
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
        ])
        .args([
            "-c",
            "commit.gpgsign=false",
            "-c",
            "protocol.file.allow=always",
        ])
        .args(args)
        .current_dir(cwd)
        .output()?;
    if !output.status.success() {
        return Err(format!(
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    Ok(String::from_utf8(output.stdout)?.trim().to_owned())
}

fn origin(scratch: &Scratch) -> Result<(PathBuf, String), Box<dyn Error>> {
    let origin = scratch.0.join("origin");
    fs::create_dir_all(origin.join("dir"))?;
    git(&origin, &["init", "-q", "-b", "main"])?;
    fs::write(origin.join("a.txt"), "first\n")?;
    git(&origin, &["add", "-A"])?;
    git(&origin, &["commit", "-q", "-m", "first"])?;
    fs::write(origin.join("a.txt"), "second\n")?;
    fs::write(origin.join("dir/b.txt"), "nested\n")?;
    git(&origin, &["add", "-A"])?;
    git(&origin, &["commit", "-q", "-m", "second"])?;
    let head = git(&origin, &["rev-parse", "HEAD"])?;
    Ok((origin, head))
}

struct HeadReads {
    head: String,
    tree: String,
    worktree_clean: bool,
    ls_tree: Vec<TapTreeEntry>,
}

fn read_head(root: &Path) -> Result<HeadReads, Box<dyn Error>> {
    let text = |args: &[&str]| -> Result<String, Box<dyn Error>> {
        let request = GitRequest::rev_parse(args.iter().map(OsString::from).collect());
        let output = request.run_in(root)?;
        output.require_success("git")?;
        Ok(output.stdout_text("git")?.trim().to_owned())
    };
    let head = text(&["HEAD"])?;
    let tree = text(&["HEAD^{tree}"])?;
    let clean = GitRequest::diff(
        ["--quiet", "HEAD", "--"]
            .iter()
            .map(OsString::from)
            .collect(),
    )
    .run_in(root)?;
    let deadline = CheckDeadline::after(Duration::from_secs(60))?;
    let ls_tree = read_tap_tree(root, &head, deadline)?;
    Ok(HeadReads {
        head,
        tree,
        worktree_clean: clean.success,
        ls_tree,
    })
}

#[test]
fn head_reads_of_the_verdict_job_match_on_a_depth_one_checkout() -> Result<(), Box<dyn Error>> {
    let scratch = Scratch::new("head-reads")?;
    let (origin, head) = origin(&scratch)?;
    let url = format!("file://{}", origin.display());

    let full = scratch.0.join("full");
    git(&scratch.0, &["clone", "-q", &url, &full.to_string_lossy()])?;
    let shallow = scratch.0.join("shallow");
    fs::create_dir_all(&shallow)?;
    git(&shallow, &["init", "-q", "-b", "main"])?;
    git(
        &shallow,
        &[
            "fetch",
            "-q",
            "--depth=1",
            "--no-tags",
            &url,
            &format!("+{head}:refs/remotes/origin/main"),
        ],
    )?;
    git(&shallow, &["checkout", "-q", "--detach", &head])?;

    assert_eq!(
        git(&shallow, &["rev-parse", "--is-shallow-repository"])?,
        "true"
    );
    assert_eq!(
        git(&full, &["rev-parse", "--is-shallow-repository"])?,
        "false"
    );
    assert!(git(&shallow, &["rev-parse", "--verify", "--quiet", "HEAD^"]).is_err());
    assert!(git(&full, &["rev-parse", "--verify", "--quiet", "HEAD^"]).is_ok());

    let full_reads = read_head(&full)?;
    let shallow_reads = read_head(&shallow)?;
    assert_eq!(full_reads.head, head);
    assert_eq!(shallow_reads.head, full_reads.head);
    assert_eq!(shallow_reads.tree, full_reads.tree);
    assert!(full_reads.worktree_clean);
    assert!(shallow_reads.worktree_clean);
    assert_eq!(shallow_reads.ls_tree, full_reads.ls_tree);
    assert_eq!(full_reads.ls_tree.len(), 2);
    Ok(())
}
