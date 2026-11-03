// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_actions_contract::workflow::cache_family::CacheFamily;

use crate::gc1_scene::Scene;
use crate::gc1_ustar::Archive;

use super::*;

const MIB: u64 = 1024 * 1024;
const BLOCK_FILE: usize = 4096;
const SMALL_FILES: u64 = 60_000;
const FILES_PER_DIR: u64 = 1_000;
const NEAR_MARGIN: u64 = 16 * MIB;
const MIN_HEADROOM: u64 = 8 * MIB;
const OVER_MARGIN: u64 = 8 * MIB;

fn ceiling() -> u64 {
    CacheFamily::MbxBundle.spec().ceiling_bytes
}

fn pack_near_ceiling(scene: &Scene) -> Result<(u64, u64), Box<dyn Error>> {
    std::fs::create_dir_all(scene.payload().parent().ok_or_else(|| fail("no parent"))?)?;
    let mut archive = Archive::create(&scene.payload())?;
    archive.state_layout("old")?;
    let block = vec![0u8; BLOCK_FILE];
    for index in 0..SMALL_FILES {
        if index % FILES_PER_DIR == 0 {
            archive.dir(&format!("state/cas/d{}/", index / FILES_PER_DIR))?;
        }
        archive.file(
            &format!("state/cas/d{}/f{index}", index / FILES_PER_DIR),
            &block,
        )?;
    }
    let bulk = ceiling() - NEAR_MARGIN - SMALL_FILES * BLOCK_FILE as u64;
    archive.zeros("state/cas/bulk", bulk)?;
    let content = archive.content_bytes();
    let tar_total = archive.finish()?;
    Ok((tar_total, content))
}

#[test]
#[cfg(unix)]
fn gc1_t53_recover_restores_export_near_the_ceiling() -> Result<(), Box<dyn Error>> {
    let scene = Scene::new("near-ceiling", crate::gc1_t53::scripts()?)?;
    let key = scene.discover()?;
    let (tar_total, content) = pack_near_ceiling(&scene)?;
    if tar_total <= ceiling() {
        return Err(fail("fixture tar does not pass the old cap"));
    }
    if content + MIN_HEADROOM > ceiling() {
        return Err(fail("fixture content is not under the ceiling"));
    }
    let output = scene.recover(&key.key, &key.key, &[], None)?;
    let record = std::fs::read_to_string(scene.recovery_record())
        .map_err(|error| fail(&format!("recovery record unreadable ({error}): {output}")))?;
    if !record.contains("decision=exact-hit") {
        return Err(fail(&format!("recover lost its exact hit: {record}")));
    }
    for path in ["cas/object", "cas/bulk", "cas/d59/f59999"] {
        if !scene.store().join(path).is_file() {
            return Err(fail(&format!("store lost {path}")));
        }
    }
    scene.assert_clean_exit()?;
    scene.finish()
}

#[test]
#[cfg(unix)]
fn gc1_t53_recover_refuses_export_just_over_the_ceiling() -> Result<(), Box<dyn Error>> {
    let scene = Scene::new("over-ceiling", crate::gc1_t53::scripts()?)?;
    let key = scene.discover()?;
    std::fs::create_dir_all(scene.payload().parent().ok_or_else(|| fail("no parent"))?)?;
    let mut archive = Archive::create(&scene.payload())?;
    archive.state_layout("old")?;
    archive.zeros("state/cas/over", ceiling() + OVER_MARGIN)?;
    archive.finish()?;
    let output = scene.recover(&key.key, &key.key, &[], None)?;
    scene.assert_cold(&output, "over ceiling")?;
    scene.finish()
}

#[test]
#[cfg(unix)]
fn gc1_t53_recover_goes_cold_when_the_import_directory_is_in_the_workspace()
-> Result<(), Box<dyn Error>> {
    let scene = Scene::runner_in_workspace("import-in-workspace", crate::gc1_t53::scripts()?)?;
    let key = scene.discover()?;
    scene.pack_export("old")?;
    let output = scene.recover(&key.key, &key.key, &[], None)?;
    scene.assert_cold(&output, "import_in_workspace")?;
    if scene.workspace.join("target").exists() {
        return Err(fail("import wrote the workspace target"));
    }
    scene.finish()
}
