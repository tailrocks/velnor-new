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
    for (matched, decision, mark) in [
        (key.key.as_str(), "exact-hit", "mark-exact"),
        (fallback.as_str(), "predecessor-fallback", "mark-fallback"),
    ] {
        scene.pack_export(mark)?;
        let output = scene.recover(&key.key, matched, &[], None)?;
        check_restored(&scene, &output, mark, decision)?;
        std::fs::remove_file(scene.recovery_record())?;
    }
    scene.finish()
}

#[test]
#[cfg(unix)]
fn gc1_t30_key_binds_source_date_epoch() -> Result<(), Box<dyn Error>> {
    let scene = Scene::new("sde", crate::gc1_t53::scripts()?)?;
    let discover = |value| scene.discover_with(&[("SOURCE_DATE_EPOCH", value)]);
    let zero = discover("0")?;
    let one = discover("1")?;
    let unset = discover("")?;
    let again = discover("0")?;
    if zero.key != again.key || zero.restore_prefix != again.restore_prefix {
        return Err(fail("same SOURCE_DATE_EPOCH produced different keys"));
    }
    let states = [("0", &zero), ("1", &one), ("unset", &unset)];
    for (index, (label, state)) in states.iter().enumerate() {
        for (other_label, other) in &states[index + 1..] {
            if state.key == other.key {
                return Err(fail(&format!(
                    "same key across SOURCE_DATE_EPOCH values {label} and {other_label}"
                )));
            }
            if other.key.starts_with(&state.restore_prefix)
                || state.key.starts_with(&other.restore_prefix)
            {
                return Err(fail(&format!(
                    "restore prefix of SOURCE_DATE_EPOCH {label} reaches state of {other_label}"
                )));
            }
        }
    }
    scene.finish()
}
