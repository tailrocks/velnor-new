// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::expr_support::{TempDir, expected_digest, text};
use anyhow::Result;
use rustix::fs::{CWD, Mode, OFlags, mkdirat, openat};
use rustix::process::{Resource, Rlimit, getrlimit, setrlimit};
use std::fs::File;
use std::io::Write as _;
use std::os::fd::OwnedFd;
use std::path::Path;

const DEPTH: usize = 520;
const HELD_LEVEL: usize = 5;
const CHECKPOINT_LEVEL: usize = 204;
const REOPENED_LEVEL: usize = 490;
const LOWERED_LIMIT: u64 = 256;
const RESTORED_LIMIT: u64 = 8_192;

struct DescriptorLimit(Rlimit);

impl DescriptorLimit {
    fn lower_to(soft: u64) -> Result<Self> {
        let original = getrlimit(Resource::Nofile);
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
        let restored = self
            .0
            .maximum
            .map_or(RESTORED_LIMIT, |maximum| maximum.min(RESTORED_LIMIT));
        let _ = setrlimit(
            Resource::Nofile,
            Rlimit {
                current: Some(restored),
                maximum: self.0.maximum,
            },
        );
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

fn make_directory(parent: &OwnedFd, name: &str) -> Result<OwnedFd> {
    mkdirat(parent, name, Mode::RWXU)?;
    open_directory(parent, name)
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

fn build_branching_tree(workspace: &Path) -> Result<()> {
    let mut directory = openat(
        CWD,
        workspace,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
        Mode::empty(),
    )?;
    make_file(&directory, "top.txt", "top")?;
    for level in 0..DEPTH {
        let sibling = make_directory(&directory, "b")?;
        match level {
            HELD_LEVEL => make_file(&sibling, "s.txt", "s")?,
            CHECKPOINT_LEVEL => make_file(&sibling, "n.txt", "n")?,
            REOPENED_LEVEL => make_file(&sibling, "m.txt", "m")?,
            _ => {}
        }
        directory = make_directory(&directory, "a")?;
    }
    make_file(&directory, "leaf.txt", "leaf")
}

#[test]
fn expr_hash_files_walks_a_branching_tree_far_deeper_than_its_descriptor_budget() -> Result<()> {
    let dir = TempDir::new()?;
    dir.write("ws/placeholder.md", "p")?;
    build_branching_tree(&dir.workspace()?)?;
    let env = dir.env();
    let _limit = DescriptorLimit::lower_to(LOWERED_LIMIT)?;

    assert_eq!(
        text("hashFiles('**/*.txt')", &env)?,
        expected_digest(&[b"leaf", b"m", b"n", b"s", b"top"])
    );
    Ok(())
}
