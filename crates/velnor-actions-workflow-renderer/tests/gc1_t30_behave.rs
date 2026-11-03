// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::gc1_scene::Scene;

fn record_field<'a>(record: &'a str, name: &str) -> Option<&'a str> {
    record
        .lines()
        .find_map(|line| line.strip_prefix(&format!("{name}=")))
}

#[cfg(unix)]
fn check_restored(
    scene: &Scene,
    output: &str,
    mark: &str,
    decision: &str,
) -> Result<(), Box<dyn Error>> {
    let record = std::fs::read_to_string(scene.recovery_record())
        .map_err(|error| fail(&format!("recovery record unreadable ({error}): {output}")))?;
    if record_field(&record, "decision") != Some(decision) {
        return Err(fail(&format!("recover misclassified {decision}: {record}")));
    }
    let manifest = record_field(&record, "manifest").unwrap_or_default();
    if manifest.len() != 64 || !manifest.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(fail(&format!("recover lost its manifest digest: {record}")));
    }
    if record_field(&record, "size").is_none() {
        return Err(fail("recover lost its payload size"));
    }
    let object = std::fs::read_to_string(scene.store().join("cas/object"))
        .map_err(|error| fail(&format!("store lost the restored object: {error}")))?;
    if object != mark {
        return Err(fail(&format!("store holds {object:?}, expected {mark:?}")));
    }
    for (path, what) in [
        (scene.stage(), "stage"),
        (scene.state_root(), "state root"),
        (scene.workspace.join("target"), "workspace target"),
    ] {
        if path.exists() {
            return Err(fail(&format!("recover left the {what}")));
        }
    }
    Ok(())
}

#[test]
#[cfg(unix)]
fn gc1_t30_scripts_behave() -> Result<(), Box<dyn Error>> {
    let scene = Scene::new("behave", crate::gc1_t53::scripts()?)?;
    let key = scene.discover()?;
    let fallback = format!("{}oldsnap-0", key.restore_prefix);
    let exact = key.exact();
    for (matched, decision, mark) in [
        (exact.as_str(), "exact-hit", "mark-exact"),
        (fallback.as_str(), "predecessor-fallback", "mark-fallback"),
    ] {
        scene.pack_export(mark)?;
        let output = scene.recover(matched, &[], None)?;
        check_restored(&scene, &output, mark, decision)?;
        std::fs::remove_file(scene.recovery_record())?;
    }
    scene.finish()
}

fn image_os_outputs(scene: &Scene, image_os: Option<&str>) -> Result<String, Box<dyn Error>> {
    match image_os {
        Some(value) => scene.discover_outputs(&[("ImageOS", value)], &[]),
        None => scene.discover_outputs(&[], &["ImageOS"]),
    }
}

#[test]
#[cfg(unix)]
fn gc1_t30_missing_image_os_goes_cold() -> Result<(), Box<dyn Error>> {
    let scene = Scene::new("image-os-cold", crate::gc1_t53::scripts()?)?;
    let baseline = image_os_outputs(&scene, Some("ubuntu26"))?;
    for expected in ["restore=true", "save=true"] {
        if !baseline.lines().any(|line| line == expected) {
            return Err(fail(&format!("baseline lost {expected}: {baseline:?}")));
        }
    }
    if baseline.lines().any(|line| line.starts_with("reason=")) {
        return Err(fail(&format!("baseline went cold: {baseline:?}")));
    }
    for image_os in [None, Some("")] {
        let outputs = image_os_outputs(&scene, image_os)?;
        for expected in ["reason=image_os_missing", "save=false", "restore=false"] {
            if !outputs.lines().any(|line| line == expected) {
                return Err(fail(&format!(
                    "image_os={image_os:?}: missing {expected}; outputs: {outputs:?}"
                )));
            }
        }
        if outputs.lines().any(|line| line.starts_with("key=")) {
            return Err(fail(&format!(
                "image_os={image_os:?}: cold discover still published a key"
            )));
        }
    }
    scene.finish()
}
