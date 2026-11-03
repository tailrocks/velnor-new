// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;

use crate::gc1_scene::Scene;
use crate::gc1_ustar::{Archive, gzip_again, polyglot};

use super::*;

const SHADOW: &str = "refused-shadow";
const UNREADABLE: &str = "payload unreadable";
const MEMBER_BOUND: u32 = 100_000;
const ZERO_TAR: u64 = 10 * 1024;

#[derive(Clone, Copy, Debug)]
enum Row {
    SiblingFamily,
    ParentTraversal,
    SymlinkWriteThrough,
    HardLink,
    NestedCompression,
    EmptyTar,
    ZeroTar,
    NewlineName,
    AbsoluteName,
    Fifo,
    NotGzip,
    Polyglot,
    TooManyMembers,
}

const ROWS: [Row; 13] = [
    Row::SiblingFamily,
    Row::ParentTraversal,
    Row::SymlinkWriteThrough,
    Row::HardLink,
    Row::NestedCompression,
    Row::EmptyTar,
    Row::ZeroTar,
    Row::NewlineName,
    Row::AbsoluteName,
    Row::Fifo,
    Row::NotGzip,
    Row::Polyglot,
    Row::TooManyMembers,
];

fn token(row: Row) -> &'static str {
    match row {
        Row::NotGzip => UNREADABLE,
        _ => SHADOW,
    }
}

fn absolute_target() -> String {
    format!("/tmp/velnor-gc1-absolute-{}/pwn", std::process::id())
}

fn build_wrapped(row: Row, scene: &Scene) -> Result<(), Box<dyn Error>> {
    let payload = scene.payload();
    let inner = scene.scratch.join("inner.tgz");
    let mut archive = Archive::create(&inner)?;
    archive.state_layout("nested")?;
    archive.finish()?;
    if let Row::Polyglot = row {
        let patched = scene.scratch.join("patched.tgz");
        polyglot(&inner, &patched)?;
        return gzip_again(&patched, &payload);
    }
    gzip_again(&inner, &payload)
}

fn build_bare(row: Row, scene: &Scene) -> Result<(), Box<dyn Error>> {
    let mut archive = Archive::create(&scene.payload())?;
    match row {
        Row::EmptyTar => {}
        Row::ZeroTar => archive.raw_zeros(ZERO_TAR)?,
        _ => return Err(fail("row is not a bare stream")),
    }
    archive.finish_untrailed()?;
    Ok(())
}

fn build_members(row: Row, scene: &Scene, outside: &Path) -> Result<(), Box<dyn Error>> {
    let mut archive = Archive::create(&scene.payload())?;
    archive.state_layout("old")?;
    match row {
        Row::SiblingFamily => {
            archive.dir("tools/")?;
            archive.file("tools/sentinel", b"must never land")?;
        }
        Row::ParentTraversal => archive.file("state/../escape", b"must never land")?,
        Row::SymlinkWriteThrough => {
            archive.symlink("state/lnk", &outside.display().to_string())?;
            archive.file("state/lnk/pwn", b"must never land")?;
        }
        Row::HardLink => {
            archive.file("state/obj", b"linked")?;
            archive.hardlink("state/hl", "state/obj")?;
        }
        Row::NewlineName => archive.file("state/a\nb", b"must never land")?,
        Row::AbsoluteName => archive.file(&absolute_target(), b"must never land")?,
        Row::Fifo => archive.fifo("state/pipe")?,
        Row::TooManyMembers => {
            for index in 0..=MEMBER_BOUND {
                archive.file(&format!("state/cas/f{index}"), b"")?;
            }
        }
        _ => return Err(fail("row is not a member stream")),
    }
    archive.finish()?;
    Ok(())
}

fn build(row: Row, scene: &Scene, outside: &Path) -> Result<(), Box<dyn Error>> {
    std::fs::create_dir_all(scene.payload().parent().ok_or_else(|| fail("no parent"))?)?;
    match row {
        Row::NestedCompression | Row::Polyglot => build_wrapped(row, scene),
        Row::EmptyTar | Row::ZeroTar => build_bare(row, scene),
        Row::NotGzip => Ok(std::fs::write(
            scene.payload(),
            b"this payload was never a gzip stream\n",
        )?),
        _ => build_members(row, scene, outside),
    }
}

fn check_row(row: Row) -> Result<(), Box<dyn Error>> {
    let scene = Scene::new("hostile", crate::gc1_t53::scripts()?)?;
    let outside = scene.scratch.join("outside");
    std::fs::create_dir_all(&outside)?;
    let key = scene.discover()?;
    build(row, &scene, &outside)?;
    let before = scene.tree()?;
    let output = scene
        .recover(&key.exact(), &[], None)
        .map_err(|error| fail(&format!("{row:?}: {error}")))?;
    scene
        .assert_cold(&output, token(row))
        .map_err(|error| fail(&format!("{row:?}: {error}")))?;
    scene
        .assert_confined(&before)
        .map_err(|error| fail(&format!("{row:?}: {error}")))?;
    if Path::new(&absolute_target()).exists() {
        return Err(fail(&format!("{row:?}: wrote to an absolute path")));
    }
    scene.finish()
}

#[test]
#[cfg(unix)]
fn gc1_t53_recover_refuses_hostile_archives() -> Result<(), Box<dyn Error>> {
    for row in ROWS {
        check_row(row)?;
    }
    Ok(())
}
