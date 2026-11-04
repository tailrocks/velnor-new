// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::gc1_scene::Scene;

use super::*;

const RECORDED_IMAGE: (&str, &str) = ("ImageOS", "ubuntu24");

#[test]
#[cfg(target_os = "linux")]
fn gc1_t53_recover_goes_cold_when_the_compat_drifted() -> Result<(), Box<dyn Error>> {
    let scene = Scene::new("drift-recover", crate::gc1_t53::scripts()?)?;
    let key = scene.discover_with(&[RECORDED_IMAGE])?;
    scene.pack_export("old")?;
    let output = scene.recover(&key.exact(), &[], None)?;
    scene.assert_cold(&output, "state snapshot coverage drift; building cold")?;
    scene.pack_export("old")?;
    scene.recover(&key.exact(), &[RECORDED_IMAGE], None)?;
    let record = std::fs::read_to_string(scene.recovery_record())?;
    if !record.contains("decision=exact-hit") {
        return Err(fail(&format!(
            "matching compat lost its exact hit: {record}"
        )));
    }
    let object = std::fs::read_to_string(scene.store().join("cas/object"))?;
    if object != "old" {
        return Err(fail(&format!("matching compat imported {object:?}")));
    }
    scene.assert_clean_exit()?;
    scene.finish()
}

#[test]
#[cfg(target_os = "linux")]
fn gc1_t53_export_skips_when_the_compat_drifted() -> Result<(), Box<dyn Error>> {
    let scene = Scene::new("drift-publish", crate::gc1_t53::scripts()?)?;
    scene.discover_with(&[RECORDED_IMAGE])?;
    scene.mark_producer()?;
    let output = scene.export(&[])?;
    require(&output, "not published (coverage_drift)", "drifted export")?;
    forbid(&output, "readback verified", "drifted export")?;
    if scene.output_value("exported")? != "false" {
        return Err(fail("drifted export must not report exported=true"));
    }
    if scene.state_root().exists() {
        return Err(fail("drifted export produced a state root"));
    }
    let output = scene.export(&[RECORDED_IMAGE])?;
    require(&output, "readback verified", "matching export")?;
    if scene.output_value("exported")? != "true" {
        return Err(fail("matching compat must admit the snapshot"));
    }
    scene.finish()
}
