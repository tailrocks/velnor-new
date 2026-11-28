// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::expr_support::{TempDir, expected_digest, text};
use anyhow::Result;
use rustix::fs::{AtFlags, CWD, Mode, OFlags, mkdirat, openat, unlinkat};
use rustix::process::{Resource, Rlimit, getrlimit, setrlimit};
use std::fs::File;
use std::io::Write as _;
use std::os::fd::OwnedFd;
use std::path::Path;

const DEPTH: usize = 400;
const SIBLING_FILES: [(usize, &str, &str); 3] = [(5, "s", "s"), (100, "n", "n"), (350, "m", "m")];
const LOWERED_LIMIT: u64 = 256;
const RAISED_LIMIT: u64 = 16_384;

struct DescriptorLimit(Rlimit);

impl DescriptorLimit {
    fn set(soft: u64) -> Result<Self> {
        let original = getrlimit(Resource::Nofile);
        let soft = original.maximum.map_or(soft, |maximum| soft.min(maximum));
        setrlimit(
            Resource::Nofile,
            Rlimit {
                current: Some(soft),
                maximum: original.maximum,
            },
        )?;
        Ok(Self(original))
    }
}

impl Drop for DescriptorLimit {
    fn drop(&mut self) {
        let _ = setrlimit(Resource::Nofile, self.0);
    }
}

fn open_directory(parent: &OwnedFd, name: &str) -> Result<OwnedFd> {
    Ok(openat(
        parent,
        name,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )?)
}

fn make_file(parent: &OwnedFd, name: &str, contents: &str) -> Result<()> {
    let fd = openat(
        parent,
        name,
        OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::CLOEXEC,
        Mode::RUSR | Mode::WUSR,
    )?;
    File::from(fd).write_all(contents.as_bytes())?;
    Ok(())
}

fn open_workspace(workspace: &Path) -> Result<OwnedFd> {
    Ok(openat(
        CWD,
        workspace,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
        Mode::empty(),
    )?)
}

fn build_branching_tree(workspace: &Path) -> Result<()> {
    let mut directory = open_workspace(workspace)?;
    make_file(&directory, "top.txt", "top")?;
    for level in 0..DEPTH {
        mkdirat(&directory, "b", Mode::RWXU)?;
        if let Some((_, name, contents)) = SIBLING_FILES.iter().find(|(at, _, _)| *at == level) {
            let sibling = open_directory(&directory, "b")?;
            make_file(&sibling, &format!("{name}.txt"), contents)?;
        }
        mkdirat(&directory, "a", Mode::RWXU)?;
        directory = open_directory(&directory, "a")?;
    }
    make_file(&directory, "leaf.txt", "leaf")
}

fn remove_branching_tree(workspace: &Path) -> Result<()> {
    let mut chain = vec![open_workspace(workspace)?];
    for _ in 0..DEPTH {
        let next = open_directory(&chain[chain.len() - 1], "a")?;
        chain.push(next);
    }
    unlinkat(&chain[DEPTH], "leaf.txt", AtFlags::empty())?;
    for level in (0..DEPTH).rev() {
        if let Some((_, name, _)) = SIBLING_FILES.iter().find(|(at, _, _)| *at == level) {
            unlinkat(&chain[level], format!("b/{name}.txt"), AtFlags::empty())?;
        }
        unlinkat(&chain[level], "b", AtFlags::REMOVEDIR)?;
        unlinkat(&chain[level], "a", AtFlags::REMOVEDIR)?;
    }
    unlinkat(&chain[0], "top.txt", AtFlags::empty())?;
    Ok(())
}

#[test]
fn expr_hash_files_walks_a_branching_tree_far_deeper_than_its_descriptor_budget() -> Result<()> {
    let dir = TempDir::new()?;
    dir.write("ws/placeholder.md", "p")?;
    let workspace = dir.workspace()?;
    let raised = DescriptorLimit::set(RAISED_LIMIT)?;
    build_branching_tree(&workspace)?;
    let env = dir.env();

    let digest = {
        let _lowered = DescriptorLimit::set(LOWERED_LIMIT)?;
        text("hashFiles('**/*.txt')", &env)
    };
    remove_branching_tree(&workspace)?;
    drop(raised);

    assert_eq!(
        digest?,
        expected_digest(&[b"leaf", b"m", b"n", b"s", b"top"])
    );
    Ok(())
}
