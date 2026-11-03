// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::gc1_scene::Scene;

use super::*;

const OTHER_KEY: &str = "velnor-v2-unrelated-key";

#[test]
#[cfg(unix)]
fn gc1_t53_recover_without_a_payload_stays_silent() -> Result<(), Box<dyn Error>> {
    let scene = Scene::new("exit-absent", crate::gc1_t53::scripts()?)?;
    let key = scene.discover()?;
    let before = scene.tree()?;
    let output = scene.recover(&key.key, &key.key, &[], None)?;
    scene.assert_silent_cold(&output, "no_entry")?;
    scene.assert_confined(&before)?;
    scene.finish()
}

#[test]
#[cfg(unix)]
fn gc1_t53_recover_without_a_matched_key_stays_silent() -> Result<(), Box<dyn Error>> {
    let scene = Scene::new("exit-cold-missing", crate::gc1_t53::scripts()?)?;
    let key = scene.discover()?;
    scene.pack_export("old")?;
    let output = scene.recover(&key.key, "", &[], None)?;
    scene.assert_silent_cold(&output, "cold-missing")?;
    scene.finish()
}

#[test]
#[cfg(unix)]
fn gc1_t53_recover_removes_an_empty_payload() -> Result<(), Box<dyn Error>> {
    let scene = Scene::new("exit-empty", crate::gc1_t53::scripts()?)?;
    let key = scene.discover()?;
    std::fs::create_dir_all(scene.payload().parent().ok_or_else(|| fail("no parent"))?)?;
    std::fs::write(scene.payload(), b"")?;
    let before = scene.tree()?;
    let output = scene.recover(&key.key, &key.key, &[], None)?;
    scene.assert_cold(&output, "payload empty")?;
    scene.assert_confined(&before)?;
    scene.finish()
}

#[test]
#[cfg(unix)]
fn gc1_t53_recover_refuses_a_payload_without_a_discovery_record() -> Result<(), Box<dyn Error>> {
    let scene = Scene::new("exit-no-record", crate::gc1_t53::scripts()?)?;
    let key = scene.discover()?;
    scene.pack_export("old")?;
    std::fs::remove_file(scene.runner.join("velnor/state-discovery.record"))?;
    let before = scene.tree()?;
    let output = scene.recover(&key.key, &key.key, &[], None)?;
    scene.assert_cold(&output, "refused-provenance")?;
    scene.assert_confined_beyond(
        &before,
        &[scene.runner.join("velnor/state-discovery.record")],
    )?;
    scene.finish()
}

#[test]
#[cfg(unix)]
fn gc1_t53_recover_refuses_a_key_outside_the_restore_prefix() -> Result<(), Box<dyn Error>> {
    let scene = Scene::new("exit-foreign-key", crate::gc1_t53::scripts()?)?;
    let key = scene.discover()?;
    scene.pack_export("old")?;
    let before = scene.tree()?;
    let output = scene.recover(&key.key, OTHER_KEY, &[], None)?;
    scene.assert_cold(&output, "refused-provenance")?;
    scene.assert_confined(&before)?;
    scene.finish()
}
