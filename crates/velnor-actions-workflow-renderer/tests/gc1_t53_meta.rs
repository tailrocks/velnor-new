// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::gc1_mbx_stub::FAIL_ENV;
use crate::gc1_scene::Scene;
use crate::gc1_ustar::Archive;

use super::*;

const NOT_REGULAR: &str = "not a regular file";
const ANCESTOR: &str = "payload path is a symlink";
const ROOT_LINK: &str = "state root is a symlink";

fn set_mode(path: &Path, mode: u32) -> Result<(), Box<dyn Error>> {
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))?;
    Ok(())
}

fn payload_dir(scene: &Scene) -> Result<PathBuf, Box<dyn Error>> {
    Ok(scene
        .payload()
        .parent()
        .ok_or_else(|| fail("no parent"))?
        .to_path_buf())
}

#[test]
#[cfg(unix)]
fn gc1_t53_recover_goes_cold_on_an_unreadable_payload() -> Result<(), Box<dyn Error>> {
    let scene = Scene::new("meta-unreadable", crate::gc1_t53::scripts()?)?;
    scene.probe_modes()?;
    let key = scene.discover()?;
    scene.pack_export("old")?;
    set_mode(&scene.payload(), 0)?;
    let before = scene.tree()?;
    let output = scene.recover(&key.key, &key.key, &[], None)?;
    scene.assert_cold(&output, "size unreadable")?;
    scene.assert_confined(&before)?;
    scene.finish()
}

#[test]
#[cfg(unix)]
fn gc1_t53_recover_goes_cold_in_a_read_only_transport_directory() -> Result<(), Box<dyn Error>> {
    let scene = Scene::new("meta-read-only", crate::gc1_t53::scripts()?)?;
    scene.probe_modes()?;
    let key = scene.discover()?;
    let parent = payload_dir(&scene)?;
    std::fs::create_dir_all(&parent)?;
    std::fs::write(scene.payload(), b"never a gzip stream\n")?;
    set_mode(&parent, 0o555)?;
    let before = scene.tree()?;
    let output = scene.recover(&key.key, &key.key, &[], None)?;
    scene.assert_cold(&output, "payload unreadable")?;
    scene.assert_confined_beyond(&before, &[parent])?;
    scene.finish()
}

#[test]
#[cfg(unix)]
fn gc1_t53_recover_stays_cold_when_removal_is_refused() -> Result<(), Box<dyn Error>> {
    let scene = Scene::new("meta-refused-removal", crate::gc1_t53::scripts()?)?;
    scene.probe_modes()?;
    scene.shim_chmod_failure()?;
    let key = scene.discover()?;
    std::fs::create_dir_all(payload_dir(&scene)?)?;
    let mut archive = Archive::create(&scene.payload())?;
    archive.dir("state/")?;
    archive.dir("state/action-results/")?;
    archive.file("state/action-results/result", b"velnor")?;
    archive.locked_dir("state/cas/")?;
    archive.file("state/cas/object", b"locked")?;
    archive.dir_mode("state/cas/inner/", 0o644)?;
    archive.file("state/cas/inner/object", b"locked")?;
    archive.file("state/mbx-cache-export-v1.json", b"{\"version\":1}\n")?;
    archive.finish()?;
    let output = scene.recover(&key.key, &key.key, &[(FAIL_ENV, "import")], None)?;
    if !output.contains("::warning::cache_corrupt") {
        return Err(fail(&format!("recover lost its cold warning: {output}")));
    }
    scene.finish()
}

fn plant_outside(scene: &Scene) -> Result<PathBuf, Box<dyn Error>> {
    let outside = scene.scratch.join("outside");
    std::fs::create_dir_all(&outside)?;
    std::fs::write(outside.join("sentinel"), b"outside must stay as it is")?;
    set_mode(&outside.join("sentinel"), 0o400)?;
    Ok(outside)
}

#[test]
#[cfg(unix)]
fn gc1_t53_recover_never_follows_a_symlink_state_root() -> Result<(), Box<dyn Error>> {
    let scene = Scene::new("meta-root-link", crate::gc1_t53::scripts()?)?;
    let key = scene.discover()?;
    scene.pack_export("old")?;
    let outside = plant_outside(&scene)?;
    set_mode(&outside, 0o555)?;
    std::os::unix::fs::symlink(&outside, scene.state_root())?;
    let before = scene.tree()?;
    let output = scene.recover(&key.key, &key.key, &[], None)?;
    scene.assert_cold(&output, ROOT_LINK)?;
    scene.assert_confined(&before)?;
    scene.finish()
}

fn check_ancestor(name: &str, link_transport: bool) -> Result<(), Box<dyn Error>> {
    let scene = Scene::new(name, crate::gc1_t53::scripts()?)?;
    let key = scene.discover()?;
    scene.pack_export("old")?;
    let outside = scene.scratch.join("outside");
    let link = if link_transport {
        scene.runner.join("velnor-gc1/transport")
    } else {
        payload_dir(&scene)?
    };
    std::fs::rename(&link, &outside)?;
    std::fs::write(outside.join("sentinel"), b"outside must stay as it is")?;
    set_mode(&outside.join("sentinel"), 0o400)?;
    set_mode(&outside, 0o555)?;
    std::os::unix::fs::symlink(&outside, &link)?;
    let before = scene.tree()?;
    let output = scene.recover(&key.key, &key.key, &[], None)?;
    scene.assert_cold(&output, ANCESTOR)?;
    scene.assert_confined_beyond(&before, &[link])?;
    scene.finish()
}

#[test]
#[cfg(unix)]
fn gc1_t53_recover_never_follows_a_symlink_transport_directory() -> Result<(), Box<dyn Error>> {
    check_ancestor("meta-transport-link", true)
}

#[test]
#[cfg(unix)]
fn gc1_t53_recover_never_follows_a_symlink_state_directory() -> Result<(), Box<dyn Error>> {
    check_ancestor("meta-state-link", false)
}

#[derive(Clone, Copy, Debug)]
enum Object {
    Directory,
    LockedSubdirectory,
    Fifo,
    LinkToValid,
    DanglingLink,
}

const OBJECTS: [Object; 5] = [
    Object::Directory,
    Object::LockedSubdirectory,
    Object::Fifo,
    Object::LinkToValid,
    Object::DanglingLink,
];

fn plant(scene: &Scene, object: Object) -> Result<Option<PathBuf>, Box<dyn Error>> {
    let payload = scene.payload();
    let valid = scene.scratch.join("valid.tar.gz");
    match object {
        Object::Directory | Object::LockedSubdirectory => {
            let nested = payload.join("nested");
            std::fs::create_dir_all(&nested)?;
            std::fs::write(nested.join("file"), b"inside a payload directory")?;
            if let Object::LockedSubdirectory = object {
                set_mode(&nested, 0)?;
            }
            Ok(None)
        }
        Object::Fifo => {
            let made = Command::new("mkfifo").arg(&payload).status()?;
            if made.success() {
                Ok(None)
            } else {
                Err(fail("fixture fifo failed"))
            }
        }
        Object::LinkToValid => {
            scene.pack_export("old")?;
            std::fs::rename(&payload, &valid)?;
            std::os::unix::fs::symlink(&valid, &payload)?;
            Ok(Some(valid))
        }
        Object::DanglingLink => {
            std::os::unix::fs::symlink(scene.scratch.join("missing"), &payload)?;
            Ok(None)
        }
    }
}

fn check_object(object: Object) -> Result<(), Box<dyn Error>> {
    let scene = Scene::new("meta-object", crate::gc1_t53::scripts()?)?;
    scene.probe_modes()?;
    let key = scene.discover()?;
    std::fs::create_dir_all(payload_dir(&scene)?)?;
    let target = plant(&scene, object)?;
    let before = scene.tree()?;
    let output = scene
        .recover(&key.key, &key.key, &[], None)
        .map_err(|error| fail(&format!("{object:?}: {error}")))?;
    scene
        .assert_cold(&output, NOT_REGULAR)
        .map_err(|error| fail(&format!("{object:?}: {error}")))?;
    scene
        .assert_confined(&before)
        .map_err(|error| fail(&format!("{object:?}: {error}")))?;
    if let Some(target) = target
        && std::fs::metadata(&target)?.len() == 0
    {
        return Err(fail(&format!("{object:?}: the link target was emptied")));
    }
    scene.finish()
}

#[test]
#[cfg(unix)]
fn gc1_t53_recover_removes_payload_objects_that_are_not_regular_files() -> Result<(), Box<dyn Error>>
{
    for object in OBJECTS {
        check_object(object)?;
    }
    Ok(())
}

#[test]
#[cfg(unix)]
fn gc1_t53_recover_restores_a_read_only_payload_in_a_read_only_directory()
-> Result<(), Box<dyn Error>> {
    let scene = Scene::new("meta-read-only-twin", crate::gc1_t53::scripts()?)?;
    let key = scene.discover()?;
    scene.pack_export("old")?;
    let parent = payload_dir(&scene)?;
    set_mode(&scene.payload(), 0o400)?;
    set_mode(&parent, 0o555)?;
    let before = scene.tree()?;
    let output = scene.recover(&key.key, &key.key, &[], None)?;
    if output.contains("::warning::") {
        return Err(fail(&format!("recover refused a valid payload: {output}")));
    }
    let record = std::fs::read_to_string(scene.recovery_record())?;
    if !record.contains("decision=exact-hit") {
        return Err(fail(&format!("recover lost its exact hit: {record}")));
    }
    if std::fs::read_to_string(scene.store().join("cas/object"))? != "old" {
        return Err(fail("store lost the restored object"));
    }
    scene.assert_clean_exit()?;
    scene.assert_confined_recovered(&before)?;
    scene.finish()
}
