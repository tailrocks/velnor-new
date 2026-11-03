// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::gc1_scene::{Key, Scene};

use velnor_actions_contract::cachekey::MAX_CACHE_KEY_BYTES;

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

#[derive(Clone, Copy, Debug)]
struct Inputs {
    share_out_dir: Option<&'static str>,
    build_script_execution: Option<&'static str>,
    mbx_cc: Option<&'static str>,
    source_date_epoch: Option<&'static str>,
    image_os: Option<&'static str>,
}

const BASELINE: Inputs = Inputs {
    share_out_dir: Some("1"),
    build_script_execution: Some("0"),
    mbx_cc: Some("0"),
    source_date_epoch: Some("0"),
    image_os: Some("ubuntu26"),
};

fn cells() -> Vec<Inputs> {
    let mut cells = Vec::new();
    for share_out_dir in ["0", "1"] {
        for build_script_execution in ["0", "1"] {
            for mbx_cc in ["0", "1"] {
                for source_date_epoch in ["0", "1"] {
                    for image_os in ["ubuntu24", "ubuntu26"] {
                        cells.push(Inputs {
                            share_out_dir: Some(share_out_dir),
                            build_script_execution: Some(build_script_execution),
                            mbx_cc: Some(mbx_cc),
                            source_date_epoch: Some(source_date_epoch),
                            image_os: Some(image_os),
                        });
                    }
                }
            }
        }
    }
    cells.extend([
        Inputs {
            share_out_dir: None,
            build_script_execution: None,
            mbx_cc: None,
            source_date_epoch: None,
            ..BASELINE
        },
        Inputs {
            mbx_cc: None,
            ..BASELINE
        },
        Inputs {
            source_date_epoch: None,
            ..BASELINE
        },
    ]);
    cells
}

fn discover_inputs(
    scene: &Scene,
    inputs: Inputs,
    extra: &[(&str, &str)],
) -> Result<String, Box<dyn Error>> {
    let mut set = extra.to_vec();
    let mut unset = Vec::new();
    for (name, value) in [
        ("MBX_SHARE_OUT_DIR", inputs.share_out_dir),
        ("MBX_BUILD_SCRIPT_EXECUTION", inputs.build_script_execution),
        ("MBX_CC", inputs.mbx_cc),
        ("SOURCE_DATE_EPOCH", inputs.source_date_epoch),
        ("ImageOS", inputs.image_os),
    ] {
        match value {
            Some(value) => set.push((name, value)),
            None => unset.push(name),
        }
    }
    scene.discover_outputs(&set, &unset)
}

fn key_of(scene: &Scene, inputs: Inputs, extra: &[(&str, &str)]) -> Result<Key, Box<dyn Error>> {
    discover_inputs(scene, inputs, extra)?;
    Ok(Key {
        key: scene.output_value("key")?,
        restore_prefix: scene.output_value("restore_keys")?,
    })
}

#[test]
#[cfg(unix)]
fn gc1_t30_key_is_stable_across_runs_and_versioned() -> Result<(), Box<dyn Error>> {
    let scene = Scene::new("key-stable", crate::gc1_t53::scripts()?)?;
    let first = key_of(&scene, BASELINE, &[])?;
    let again = key_of(&scene, BASELINE, &[])?;
    if first.key != again.key || first.restore_prefix != again.restore_prefix {
        return Err(fail("same inputs produced different keys"));
    }
    let other_run = key_of(
        &scene,
        BASELINE,
        &[
            ("GITHUB_RUN_ID", "2002"),
            ("GITHUB_RUN_ATTEMPT", "3"),
            ("RUNNER_NAME", "runner-b"),
        ],
    )?;
    if first.key != other_run.key || first.restore_prefix != other_run.restore_prefix {
        return Err(fail("key changed with run id, attempt or runner name"));
    }
    if !first.key.starts_with("velnor-v3-state-trusted-s256-") {
        return Err(fail("key lost the schema 3 state prefix"));
    }
    if first.key.len() > MAX_CACHE_KEY_BYTES {
        return Err(fail("key outgrew the cache key cap"));
    }
    scene.finish()
}

#[test]
#[cfg(unix)]
fn gc1_t30_key_never_aliases_across_flags_epoch_or_image() -> Result<(), Box<dyn Error>> {
    let scene = Scene::new("key-alias", crate::gc1_t53::scripts()?)?;
    let mut seen: Vec<(Inputs, Key)> = Vec::new();
    for inputs in cells() {
        seen.push((inputs, key_of(&scene, inputs, &[])?));
    }
    for (index, (inputs, state)) in seen.iter().enumerate() {
        for (other_inputs, other) in &seen[index + 1..] {
            if state.key == other.key {
                return Err(fail(&format!(
                    "keys collide: {inputs:?} and {other_inputs:?}"
                )));
            }
            if other.key.starts_with(&state.restore_prefix)
                || state.key.starts_with(&other.restore_prefix)
            {
                return Err(fail(&format!(
                    "restore prefix reaches foreign state: {inputs:?} and {other_inputs:?}"
                )));
            }
        }
    }
    scene.finish()
}

#[test]
#[cfg(unix)]
fn gc1_t30_missing_image_os_goes_cold() -> Result<(), Box<dyn Error>> {
    let scene = Scene::new("image-os-cold", crate::gc1_t53::scripts()?)?;
    let baseline = discover_inputs(&scene, BASELINE, &[])?;
    for expected in ["restore=true", "save=true"] {
        if !baseline.lines().any(|line| line == expected) {
            return Err(fail(&format!("baseline lost {expected}: {baseline:?}")));
        }
    }
    if baseline.lines().any(|line| line.starts_with("reason=")) {
        return Err(fail(&format!("baseline went cold: {baseline:?}")));
    }
    for image_os in [None, Some("")] {
        let outputs = discover_inputs(
            &scene,
            Inputs {
                image_os,
                ..BASELINE
            },
            &[],
        )?;
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
