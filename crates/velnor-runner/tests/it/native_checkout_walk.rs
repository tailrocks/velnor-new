// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fmt::Write as _;
use std::io::Write;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use anyhow::{Context, Result, bail};

use crate::exec_support::{Ran, Scratch};
use crate::native_checkout::{Rig, commit, finish, git};
use crate::native_checkout_quarantine::quarantined;
use crate::native_checkout_repair::{object_path, overwrite};

fn finish_within(child: Child, limit: Duration) -> Result<Ran> {
    let pid = child.id();
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || sender.send(finish(child)));
    if let Ok(ran) = receiver.recv_timeout(limit) {
        return ran;
    }
    let _ = Command::new("kill")
        .args(["-KILL", &pid.to_string()])
        .status();
    bail!("velnor-runner did not finish within {limit:?}")
}

fn deep(rig: &Rig, sha: &str, depth: usize) -> Result<(Ran, Scratch)> {
    let job = rig.job_edit(
        "checkout_depth2",
        sha,
        &rig.server_url(),
        &[("\"lit\": \"2\"", &format!("\"lit\": \"{depth}\""))],
    )?;
    let scratch = Scratch::new()?;
    let ran = finish_within(rig.start(&job, &scratch)?, Duration::from_secs(120))?;
    Ok((ran, scratch))
}

fn checks_out(rig: &Rig, sha: &str, depth: usize) -> Result<Scratch> {
    let (ran, scratch) = deep(rig, sha, depth)?;
    assert_eq!(ran.code, Some(0), "{}\n{}", ran.stdout, ran.stderr);
    let workspace = scratch.workspace();
    assert_eq!(git(&workspace, &["rev-parse", "HEAD"])?, sha);
    git(&workspace, &["rev-list", "--objects", "--quiet", "HEAD"])?;
    Ok(scratch)
}

fn warm(rig: &Rig, sha: &str) -> Result<()> {
    let ran = rig.run("checkout_depth1", sha, &Scratch::new()?)?;
    assert_eq!(ran.code, Some(0), "{}\n{}", ran.stdout, ran.stderr);
    Ok(())
}

pub(crate) fn publish(rig: &Rig) -> Result<()> {
    let upstream = rig.root.join("up/tailrocks/velnor-new.git");
    git(
        &rig.root.join("work"),
        &["push", "-q", "-f", &upstream.to_string_lossy(), "main"],
    )?;
    Ok(())
}

pub(crate) fn import(rig: &Rig, stream: &str) -> Result<()> {
    let work = rig.root.join("work");
    let mut child = Command::new("git")
        .arg("-C")
        .arg(&work)
        .args(["fast-import", "--quiet", "--force"])
        .stdin(Stdio::piped())
        .spawn()
        .context("start git fast-import")?;
    child
        .stdin
        .take()
        .context("fast-import stdin")?
        .write_all(stream.as_bytes())?;
    assert!(child.wait()?.success());
    Ok(())
}

fn linear_history(rig: &Rig, count: usize) -> Result<String> {
    let mut stream = String::new();
    for i in 0..count {
        write!(
            stream,
            "commit refs/heads/main\ncommitter t <t@t> {} +0000\ndata 1\nx\n",
            1_700_000_000 + i
        )?;
        if i == 0 {
            writeln!(stream, "from {}", rig.head)?;
        }
        write!(stream, "M 644 inline g{i}\ndata 1\n{}\n\n", i % 10)?;
    }
    import(rig, &stream)?;
    git(&rig.root.join("work"), &["reset", "-q", "--hard", "main"])?;
    publish(rig)?;
    git(&rig.root.join("work"), &["rev-parse", "main"])
}

fn empty_tree_of(rig: &Rig, commit: &str) -> Result<String> {
    let tree = git(&rig.mirror(), &["rev-parse", &format!("{commit}^{{tree}}")])?;
    overwrite(&object_path(&rig.mirror(), &tree), b"")?;
    Ok(tree)
}

#[test]
fn damage_below_an_unreadable_commit_is_still_found() -> Result<()> {
    let rig = Rig::new()?;
    let work = rig.root.join("work");
    git(&work, &["checkout", "-q", "-B", "p1", &rig.head])?;
    let p1 = commit(&work, "p1")?;
    git(&work, &["checkout", "-q", "main"])?;
    let below = commit(&work, "below")?;
    let p2 = commit(&work, "p2")?;
    let tree = git(&work, &["rev-parse", &format!("{p2}^{{tree}}")])?;
    let merge = git(
        &work,
        &["commit-tree", &tree, "-p", &p1, "-p", &p2, "-m", "m"],
    )?;
    git(&work, &["reset", "-q", "--hard", &merge])?;
    git(&work, &["branch", "-D", "p1"])?;
    publish(&rig)?;
    warm(&rig, &merge)?;
    std::fs::remove_file(object_path(&rig.mirror(), &p1))?;
    let tree = empty_tree_of(&rig, &below)?;
    for _ in 0..2 {
        checks_out(&rig, &merge, 3)?;
    }
    assert_eq!(rig.refetches(), 1);
    let moved = quarantined(&rig);
    assert!(moved.len() == 1 && moved[0].starts_with(&tree), "{moved:?}");
    Ok(())
}

#[test]
fn damage_outside_the_window_does_not_wedge_the_refetch() -> Result<()> {
    let rig = Rig::new()?;
    let base = linear_history(&rig, 300)?;
    warm(&rig, &base)?;
    let old = rig.advance("old")?;
    warm(&rig, &old)?;
    rig.advance("mid")?;
    let parent = rig.advance("parent")?;
    let tip = rig.advance("tip")?;
    warm(&rig, &tip)?;
    let outside = empty_tree_of(&rig, &old)?;
    empty_tree_of(&rig, &parent)?;
    for _ in 0..2 {
        checks_out(&rig, &tip, 2)?;
    }
    assert_eq!(rig.refetches(), 1);
    git(&rig.mirror(), &["cat-file", "-t", &outside])?;
    assert!(!object_path(&rig.mirror(), &outside).exists());
    git(&rig.mirror(), &["rev-list", "--objects", "--quiet", &tip])?;
    Ok(())
}

#[test]
fn a_deep_window_is_read_by_one_process() -> Result<()> {
    let rig = Rig::new()?;
    let base = linear_history(&rig, 400)?;
    warm(&rig, &base)?;
    rig.advance("y1")?;
    let tip = rig.advance("y2")?;
    warm(&rig, &tip)?;
    empty_tree_of(&rig, &format!("{tip}~1"))?;
    checks_out(&rig, &tip, 450)?;
    assert_eq!(rig.refetches(), 1);
    assert_eq!(started(&rig, "\"cat-file\",\"--batch\"]"), 1);
    assert_eq!(started(&rig, "\"--parents\""), 0);
    assert!(longest_argv(&rig) < 4096, "{} bytes", longest_argv(&rig));
    Ok(())
}

pub(crate) fn started(rig: &Rig, argument: &str) -> usize {
    std::fs::read_to_string(rig.trace())
        .unwrap_or_default()
        .lines()
        .filter(|line| line.contains("\"event\":\"start\"") && line.contains(argument))
        .count()
}

fn longest_argv(rig: &Rig) -> usize {
    std::fs::read_to_string(rig.trace())
        .unwrap_or_default()
        .lines()
        .filter(|line| line.contains("\"event\":\"start\""))
        .filter_map(|line| line.split_once("\"argv\":").map(|(_, argv)| argv.len()))
        .max()
        .unwrap_or_default()
}

#[test]
fn a_level_wider_than_the_pipe_buffers_is_walked() -> Result<()> {
    let rig = Rig::new()?;
    let width = 3000;
    let mut stream = String::new();
    for i in 1..=width {
        write!(
            stream,
            "reset refs/heads/tmp\ncommit refs/heads/tmp\nmark :{i}\ncommitter t <t@t> 1700000000 +0000\ndata 1\nx\nM 644 inline f{i}\ndata 1\n{}\n\n",
            i % 10
        )?;
    }
    write!(
        stream,
        "commit refs/heads/main\ncommitter t <t@t> 1700000001 +0000\ndata 1\nm\nfrom {}\n",
        rig.head
    )?;
    for i in 1..=width {
        writeln!(stream, "merge :{i}")?;
    }
    stream.push_str("M 644 inline wide\ndata 1\nw\n\n");
    import(&rig, &stream)?;
    git(&rig.root.join("work"), &["branch", "-D", "tmp"])?;
    git(&rig.root.join("work"), &["reset", "-q", "--hard", "main"])?;
    publish(&rig)?;
    let wide = git(&rig.root.join("work"), &["rev-parse", "main"])?;
    warm(&rig, &wide)?;
    let first = rig.advance("t1")?;
    warm(&rig, &first)?;
    rig.advance("t2")?;
    let tip = rig.advance("t3")?;
    warm(&rig, &tip)?;
    empty_tree_of(&rig, &first)?;
    checks_out(&rig, &tip, 6)?;
    assert_eq!(rig.refetches(), 1);
    assert_eq!(started(&rig, "\"cat-file\",\"--batch\"]"), 1);
    Ok(())
}
