// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fmt::Write as _;

use sha2::{Digest as _, Sha256};

#[cfg(target_os = "linux")]
use crate::gc1_mbx_stub::EXPORT_MARK_ENV;
use crate::gc1_scene::Scene;
#[cfg(target_os = "linux")]
use crate::gc1_scene::files_under;
use crate::gc1_ustar::list_members;

use super::*;

const ZSTD_MAGIC: [u8; 4] = [0x28, 0xb5, 0x2f, 0xfd];

fn payload_sha256(scene: &Scene) -> Result<String, Box<dyn Error>> {
    let digest = Sha256::digest(std::fs::read(scene.payload())?);
    let mut hex = String::new();
    for byte in digest {
        write!(hex, "{byte:02x}")?;
    }
    Ok(hex)
}

fn check_published_payload(scene: &Scene) -> Result<(), Box<dyn Error>> {
    let bytes = std::fs::read(scene.payload())?;
    if bytes.get(..4) != Some(&ZSTD_MAGIC[..]) {
        return Err(fail("published payload does not start with the zstd magic"));
    }
    for member in list_members(&scene.payload())?.lines() {
        if member != "state/" && !member.starts_with("state/") {
            return Err(fail(&format!("published payload lists {member}")));
        }
    }
    Ok(())
}

#[test]
#[cfg(target_os = "linux")]
fn gc1_t53_export_publishes_fresh_export() -> Result<(), Box<dyn Error>> {
    let scene = Scene::new("publish-fresh", crate::gc1_t53::scripts()?)?;
    let key = scene.discover()?;
    scene.pack_export("old")?;
    scene.recover(&format!("{}oldsnap-0", key.restore_prefix), &[], None)?;
    scene.assert_clean_exit()?;
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
    check_published_payload(&scene)?;
    let saved = scene.output_value("key")?;
    let digest = payload_sha256(&scene)?;
    if saved.rsplit('-').next() != Some(digest.as_str()) {
        return Err(fail(&format!("save key {saved} does not end in {digest}")));
    }
    scene.recover(&saved, &[], None)?;
    let record = std::fs::read_to_string(scene.recovery_record())?;
    if !record.contains("decision=exact-hit") {
        return Err(fail(&format!("recover lost its exact hit: {record}")));
    }
    for file in files_under(&scene.store())? {
        if file.file_name().and_then(|name| name.to_str()) == Some("stale-leftover") {
            return Err(fail("recovered store carries the stale leftover"));
        }
    }
    let object = std::fs::read_to_string(scene.store().join("cas/object"))?;
    if object != "fresh" {
        return Err(fail(&format!(
            "recovered store holds {object:?}, not the fresh export"
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
