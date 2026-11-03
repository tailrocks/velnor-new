// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_actions_contract::workflow::cache_family::CacheFamily;

use crate::gc1_scene::{Scene, files_under};
use crate::gc1_ustar::Archive;

use super::*;

const MIB: u64 = 1024 * 1024;
const FSIZE_BLOCK: u64 = 512;

fn ceiling() -> u64 {
    CacheFamily::MbxBundle.spec().ceiling_bytes
}

fn check_no_inflate_residue(scene: &Scene) -> Result<(), Box<dyn Error>> {
    for file in files_under(&scene.runner)? {
        if std::fs::symlink_metadata(&file)?.len() > ceiling() {
            return Err(fail(&format!(
                "inflate residue left behind: {}",
                file.display()
            )));
        }
    }
    Ok(())
}

#[test]
#[cfg(unix)]
fn gc1_t53_recover_refuses_oversize_inflate() -> Result<(), Box<dyn Error>> {
    let scene = Scene::new("oversize-bomb", crate::gc1_t53::scripts()?)?;
    let key = scene.discover()?;
    std::fs::create_dir_all(scene.payload().parent().ok_or_else(|| fail("no parent"))?)?;
    let mut archive = Archive::create(&scene.payload())?;
    archive.state_layout("old")?;
    archive.zeros("state/bomb", ceiling() + 64 * MIB)?;
    archive.finish()?;
    let limit = (ceiling() + MIB) / FSIZE_BLOCK;
    let output = scene.recover(&key.key, &key.key, &[], Some(limit))?;
    scene.assert_cold(&output, "refused-oversize")?;
    check_no_inflate_residue(&scene)?;
    scene.finish()
}

#[test]
#[cfg(unix)]
fn gc1_t53_recover_refuses_oversize_payload_before_inflate() -> Result<(), Box<dyn Error>> {
    let scene = Scene::new("oversize-payload", crate::gc1_t53::scripts()?)?;
    let key = scene.discover()?;
    std::fs::create_dir_all(scene.payload().parent().ok_or_else(|| fail("no parent"))?)?;
    std::fs::File::create(scene.payload())?.set_len(ceiling() + 16 * MIB)?;
    let output = scene.recover(&key.key, &key.key, &[], None)?;
    scene.assert_cold(&output, "refused-oversize")?;
    scene.finish()
}
