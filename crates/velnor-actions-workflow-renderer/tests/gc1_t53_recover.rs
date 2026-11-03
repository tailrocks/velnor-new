// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::gc1_scene::Scene;
use crate::gc1_ustar::Archive;

use super::*;

#[test]
#[cfg(target_os = "linux")]
fn gc1_t53_recover_restores_locked_directories() -> Result<(), Box<dyn Error>> {
    let scene = Scene::new("locked-dirs", crate::gc1_t53::scripts()?)?;
    let key = scene.discover()?;
    std::fs::create_dir_all(scene.payload().parent().ok_or_else(|| fail("no parent"))?)?;
    let mut archive = Archive::create(&scene.payload())?;
    archive.dir("state/")?;
    archive.dir("state/action-results/")?;
    archive.file("state/action-results/result", b"velnor")?;
    archive.locked_dir("state/cas/")?;
    archive.file("state/cas/object", b"locked")?;
    archive.file("state/mbx-cache-export-v1.json", b"{\"version\":1}\n")?;
    archive.finish()?;
    scene.recover(&key.exact(), &[], None)?;
    let object = std::fs::read_to_string(scene.store().join("cas/object"))?;
    if object != "locked" {
        return Err(fail("store lost the object behind a locked directory"));
    }
    scene.assert_clean_exit()?;
    scene.finish()
}
