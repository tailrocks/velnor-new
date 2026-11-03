// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;

use crate::gc1_scene::{Scene, files_under};
use crate::gc1_ustar::{Archive, gzip_again};

use super::*;

const PLANTED: [&str; 3] = ["escape", "pwn", "sentinel"];

#[derive(Clone, Copy, Debug)]
enum Row {
    SiblingFamily,
    ParentTraversal,
    SymlinkWriteThrough,
    HardLink,
    NestedCompression,
}

fn build(row: Row, scene: &Scene, outside: &Path) -> Result<(), Box<dyn Error>> {
    let payload = scene.payload();
    std::fs::create_dir_all(payload.parent().ok_or_else(|| fail("no parent"))?)?;
    if let Row::NestedCompression = row {
        let inner = scene.scratch.join("inner.tgz");
        let mut archive = Archive::create(&inner)?;
        archive.state_layout("nested")?;
        archive.finish()?;
        return gzip_again(&inner, &payload);
    }
    let mut archive = Archive::create(&payload)?;
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
        Row::NestedCompression => {}
    }
    archive.finish()
}

fn check_row(row: Row) -> Result<(), Box<dyn Error>> {
    let scene = Scene::new("hostile", crate::gc1_t53::scripts()?)?;
    let outside = scene.scratch.join("outside");
    std::fs::create_dir_all(&outside)?;
    let key = scene.discover()?;
    build(row, &scene, &outside)?;
    let output = scene.recover(&key.key, &key.key, &[], None)?;
    scene
        .assert_cold(&output, "refused-shadow")
        .map_err(|error| fail(&format!("{row:?}: {error}")))?;
    if std::fs::read_dir(&outside)?.next().is_some() {
        return Err(fail(&format!("{row:?}: wrote outside the stage")));
    }
    for file in files_under(&scene.scratch)? {
        let name = file.file_name().and_then(|name| name.to_str());
        if name.is_some_and(|name| PLANTED.contains(&name)) {
            return Err(fail(&format!(
                "{row:?}: planted file landed at {}",
                file.display()
            )));
        }
    }
    scene.finish()
}

#[test]
#[cfg(unix)]
fn gc1_t53_recover_refuses_hostile_archives() -> Result<(), Box<dyn Error>> {
    for row in [
        Row::SiblingFamily,
        Row::ParentTraversal,
        Row::SymlinkWriteThrough,
        Row::HardLink,
        Row::NestedCompression,
    ] {
        check_row(row)?;
    }
    Ok(())
}
