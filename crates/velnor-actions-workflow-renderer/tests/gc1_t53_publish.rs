// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::gc1_mbx_stub::{EXPORT_BULK_MIB_ENV, EXPORT_MARK_ENV};
use crate::gc1_scene::{Scene, files_under};

use super::*;

fn unpack_payload(scene: &Scene) -> Result<std::path::PathBuf, Box<dyn Error>> {
    let unpacked = scene.scratch.join("unpacked");
    std::fs::create_dir_all(&unpacked)?;
    let status = std::process::Command::new("tar")
        .arg("-xzf")
        .arg(scene.payload())
        .arg("-C")
        .arg(&unpacked)
        .status()?;
    if !status.success() {
        return Err(fail("published payload is unreadable"));
    }
    Ok(unpacked)
}

#[test]
#[cfg(unix)]
fn gc1_t53_export_publishes_fresh_export() -> Result<(), Box<dyn Error>> {
    let scene = Scene::new("publish-fresh", crate::gc1_t53::scripts()?)?;
    let key = scene.discover()?;
    scene.pack_export("old")?;
    scene.recover(&key.key, &key.key, &[], None)?;
    scene.mark_producer()?;
    std::fs::create_dir_all(scene.state_root())?;
    std::fs::write(scene.state_root().join("stale-leftover"), "residue")?;
    let output = scene.export(&[(EXPORT_MARK_ENV, "fresh")])?;
    require(&output, "readback verified", "export behavior")?;
    forbid(&output, "state snapshot published (", "export behavior")?;
    if scene.output_value("exported")? != "true" {
        return Err(fail("export must admit a small snapshot"));
    }
    if !scene
        .runner
        .join("velnor/state-publication.staged")
        .is_file()
    {
        return Err(fail("export must stage its attestation"));
    }
    if scene
        .runner
        .join("velnor/state-publication.record")
        .exists()
    {
        return Err(fail("export must not write the publication record"));
    }
    require(
        &scene.record()?,
        "state snapshot published (",
        "record behavior",
    )?;
    if !scene
        .runner
        .join("velnor/state-publication.record")
        .is_file()
    {
        return Err(fail("record must write the publication record"));
    }
    let unpacked = unpack_payload(&scene)?;
    for file in files_under(&unpacked)? {
        if file.file_name().and_then(|name| name.to_str()) == Some("stale-leftover") {
            return Err(fail("published payload carries the stale leftover"));
        }
    }
    let object = std::fs::read_to_string(unpacked.join("state/cas/object"))?;
    if object != "fresh" {
        return Err(fail(&format!(
            "published payload holds {object:?}, not the fresh export"
        )));
    }
    scene.finish()
}

#[test]
#[cfg(unix)]
fn gc1_t53_record_without_save_writes_nothing() -> Result<(), Box<dyn Error>> {
    let scene = Scene::new("publish-nosave", crate::gc1_t53::scripts()?)?;
    scene.discover()?;
    let output = scene.record()?;
    forbid(&output, "state snapshot published (", "record behavior")?;
    if scene
        .runner
        .join("velnor/state-publication.record")
        .exists()
    {
        return Err(fail("record must not publish without a save"));
    }
    scene.finish()
}

#[test]
#[cfg(unix)]
fn gc1_t53_export_refuses_uncompressed_over_ceiling() -> Result<(), Box<dyn Error>> {
    let scene = Scene::new("publish-ceiling", crate::gc1_t53::scripts()?)?;
    scene.discover()?;
    scene.mark_producer()?;
    let bulk = OVER_CEILING_MIB.to_string();
    let output = scene.export(&[(EXPORT_BULK_MIB_ENV, bulk.as_str())])?;
    require(&output, "not published (over_ceiling)", "export ceiling")?;
    forbid(&output, "state snapshot published (", "export ceiling")?;
    if scene.payload().exists() {
        return Err(fail("over-ceiling export must never be packed"));
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
        return Err(fail("over-ceiling export must leave no evidence"));
    }
    scene.finish()
}
