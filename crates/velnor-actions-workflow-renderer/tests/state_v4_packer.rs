// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fmt::Write as _;

use sha2::{Digest as _, Sha256};

use crate::gc1_mbx_stub::EXPORT_MARK_ENV;
use crate::gc1_scene::{Scene, fail};

const BSDTAR: &str = "#!/bin/sh\ncase \"$1\" in\n  --version) printf '%s\\n' 'bsdtar 3.7.2 - libarchive 3.7.2'; exit 0 ;;\nesac\nexit 1\n";

fn sha256_of(scene: &Scene) -> Result<String, Box<dyn Error>> {
    let mut hex = String::new();
    for byte in Sha256::digest(std::fs::read(scene.payload())?) {
        write!(hex, "{byte:02x}")?;
    }
    Ok(hex)
}

#[test]
fn different_exports_pack_to_different_bytes() -> Result<(), Box<dyn Error>> {
    let mut keys = Vec::new();
    for mark in ["first", "second"] {
        let scene = Scene::new("v4-key-binds", crate::gc1_t53::scripts()?)?;
        let key = scene.discover()?;
        scene.mark_producer()?;
        scene.export(&[(EXPORT_MARK_ENV, mark)])?;
        if scene.output_value("exported")? != "true" {
            return Err(fail(&format!("{mark}: the export was not admitted")));
        }
        let digest = sha256_of(&scene)?;
        keys.push(format!("{}s256-{digest}", key.snapshot_prefix));
        scene.finish()?;
    }
    if keys[0] == keys[1] {
        return Err(fail("two different exports packed to the same bytes"));
    }
    let scene = Scene::new("v4-key-refused", crate::gc1_t53::scripts()?)?;
    scene.discover()?;
    scene.mark_producer()?;
    scene.export(&[("MBX_BUILD_SCRIPT_EXECUTION", "1")])?;
    if scene.output_value("key").is_ok() || scene.output_value("exported")? != "false" {
        return Err(fail("a refused export published a key"));
    }
    scene.finish()
}

#[test]
fn publish_goes_cold_without_gnu_tar() -> Result<(), Box<dyn Error>> {
    let scene = Scene::new("v4-bsdtar", crate::gc1_t53::scripts()?)?;
    scene.discover()?;
    scene.mark_producer()?;
    scene.install_shim("tar", BSDTAR)?;
    let output = scene.export(&[])?;
    if !output.contains("not published (tar_not_gnu)") {
        return Err(fail(&format!("a non-GNU tar was not refused: {output}")));
    }
    if scene.output_value("exported")? != "false" || scene.payload().exists() {
        return Err(fail(
            "a non-GNU tar must leave no payload and export nothing",
        ));
    }
    scene.finish()
}
