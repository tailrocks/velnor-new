// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::gc1_mbx_stub::FAIL_ENV;
use crate::gc1_scene::Scene;

use super::*;

#[test]
#[cfg(unix)]
fn gc1_t53_cold_trusted_push_publishes_first_entry() -> Result<(), Box<dyn Error>> {
    let scene = Scene::new("cold-first", crate::gc1_t53::scripts()?)?;
    scene.discover()?;
    scene.mark_producer()?;
    if scene.state_root().exists() {
        return Err(fail("cold fixture leaked a state root"));
    }
    let output = scene.export(&[])?;
    require(&output, "readback verified", "cold export")?;
    forbid(&output, "state snapshot published (", "cold export")?;
    if scene.output_value("exported")? != "true" {
        return Err(fail("cold export must admit the produced snapshot"));
    }
    if !scene
        .runner
        .join("velnor/state-publication.staged")
        .is_file()
    {
        return Err(fail("cold export must stage its attestation"));
    }
    if scene
        .runner
        .join("velnor/state-publication.record")
        .exists()
    {
        return Err(fail("cold export must not write the publication record"));
    }
    require(
        &scene.record()?,
        "state snapshot published (",
        "cold record",
    )?;
    if !scene
        .runner
        .join("velnor/state-publication.record")
        .is_file()
    {
        return Err(fail("record must write the publication record"));
    }
    scene.finish()
}

#[test]
#[cfg(unix)]
fn gc1_t53_cold_export_failure_skips_clean() -> Result<(), Box<dyn Error>> {
    let scene = Scene::new("cold-export-fail", crate::gc1_t53::scripts()?)?;
    scene.discover()?;
    scene.mark_producer()?;
    let output = scene.export(&[(FAIL_ENV, "export")])?;
    require(&output, "not published (export_failed)", "cold export")?;
    forbid(&output, "state snapshot published (", "cold export")?;
    if scene.payload().exists() {
        return Err(fail("failed export must leave no payload"));
    }
    if scene
        .runner
        .join("velnor/state-publication.staged")
        .exists()
        || scene
            .runner
            .join("velnor/state-publication.record")
            .exists()
    {
        return Err(fail("failed export must leave no evidence"));
    }
    scene.finish()
}

#[test]
#[cfg(unix)]
fn gc1_t53_recover_import_failure_goes_cold() -> Result<(), Box<dyn Error>> {
    let scene = Scene::new("cold-recover-fail", crate::gc1_t53::scripts()?)?;
    let key = scene.discover()?;
    scene.pack_export("old")?;
    let output = scene.recover(&key.key, &key.key, &[(FAIL_ENV, "import")], None)?;
    if scene.recovery_record().exists() {
        return Err(fail("failed import must leave no recovery record"));
    }
    if scene.state_root().exists() || scene.stage().exists() {
        return Err(fail("failed import must leave no state root or stage"));
    }
    require(&output, "import_failed", "failed import")?;
    require(&output, "cache_corrupt", "failed import")?;
    scene.finish()
}
