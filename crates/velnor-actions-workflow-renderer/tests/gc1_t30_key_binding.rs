// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

use velnor_actions_contract::cachekey::MAX_CACHE_KEY_BYTES;

#[derive(Clone, Copy)]
struct Inputs {
    share_out_dir: Option<&'static str>,
    build_script_execution: Option<&'static str>,
    mbx_cc: Option<&'static str>,
    image_os: Option<&'static str>,
    run_id: &'static str,
    run_attempt: &'static str,
    runner_name: &'static str,
}

const BASELINE: Inputs = Inputs {
    share_out_dir: Some("1"),
    build_script_execution: Some("0"),
    mbx_cc: Some("0"),
    image_os: Some("ubuntu26"),
    run_id: "1001",
    run_attempt: "1",
    runner_name: "runner-a",
};

fn cell(
    share: &'static str,
    build: &'static str,
    mbx_cc: &'static str,
    image_os: &'static str,
) -> Inputs {
    Inputs {
        share_out_dir: Some(share),
        build_script_execution: Some(build),
        mbx_cc: Some(mbx_cc),
        image_os: Some(image_os),
        ..BASELINE
    }
}

fn matrix() -> Vec<Inputs> {
    let mut cells = Vec::new();
    for share in ["0", "1"] {
        for build in ["0", "1"] {
            for mbx_cc in ["0", "1"] {
                for image_os in ["ubuntu24", "ubuntu26"] {
                    cells.push(cell(share, build, mbx_cc, image_os));
                }
            }
        }
    }
    cells
}

fn set_or_remove(command: &mut std::process::Command, name: &str, value: Option<&str>) {
    match value {
        Some(value) => command.env(name, value),
        None => command.env_remove(name),
    };
}

fn discover_raw(
    setup: &BehaveSetup,
    inputs: Inputs,
) -> Result<(std::process::ExitStatus, String), Box<dyn Error>> {
    std::fs::remove_file(&setup.github_output).ok();
    let mut command = std::process::Command::new("sh");
    command
        .arg("-c")
        .arg(&setup.discover)
        .env("PATH", &setup.path_env)
        .env("RUNNER_TEMP", &setup.runner)
        .env("GITHUB_WORKSPACE", &setup.workspace)
        .env("GITHUB_OUTPUT", &setup.github_output)
        .env("GITHUB_EVENT_NAME", "push")
        .env("GITHUB_REF", "refs/heads/main")
        .env("GITHUB_REF_PROTECTED", "true")
        .env("VELNOR_DEFAULT_REF", "refs/heads/main")
        .env("VELNOR_MBX_BUNDLE_VERSION", "1.2.3")
        .env("VELNOR_MBX_BUNDLE_TOOLCHAIN", "1.89.0")
        .env("VELNOR_MBX_BUNDLE_ROOT", ".")
        .env("GITHUB_RUN_ID", inputs.run_id)
        .env("GITHUB_RUN_ATTEMPT", inputs.run_attempt)
        .env("RUNNER_NAME", inputs.runner_name);
    set_or_remove(&mut command, "MBX_SHARE_OUT_DIR", inputs.share_out_dir);
    set_or_remove(
        &mut command,
        "MBX_BUILD_SCRIPT_EXECUTION",
        inputs.build_script_execution,
    );
    set_or_remove(&mut command, "MBX_CC", inputs.mbx_cc);
    set_or_remove(&mut command, "ImageOS", inputs.image_os);
    let status = command
        .status()
        .map_err(|error| fail(&format!("discover failed to run: {error}")))?;
    let outputs = std::fs::read_to_string(&setup.github_output).unwrap_or_default();
    Ok((status, outputs))
}

fn discover_with(setup: &BehaveSetup, inputs: Inputs) -> Result<(String, String), Box<dyn Error>> {
    let (status, _) = discover_raw(setup, inputs)?;
    if !status.success() {
        return Err(fail("discover script failed on trusted push"));
    }
    Ok((
        github_output_value(&setup.github_output, "key")?,
        github_output_value(&setup.github_output, "restore_keys")?,
    ))
}

#[test]
#[cfg(unix)]
fn gc1_t30_key_is_stable_across_runs_and_versioned() -> Result<(), Box<dyn Error>> {
    let setup = behave_setup()?;
    let first = discover_with(&setup, BASELINE)?;
    let again = discover_with(&setup, BASELINE)?;
    if first != again {
        return Err(fail("same inputs produced different keys"));
    }
    let other_run = discover_with(
        &setup,
        Inputs {
            run_id: "2002",
            run_attempt: "3",
            runner_name: "runner-b",
            ..BASELINE
        },
    )?;
    if first.0 != other_run.0 {
        return Err(fail("key changed with run id, attempt or runner name"));
    }
    if !first.0.starts_with("velnor-v2-state-trusted-s256-") {
        return Err(fail("key lost the schema 2 state prefix"));
    }
    if first.0.len() > MAX_CACHE_KEY_BYTES {
        return Err(fail("key outgrew the cache key cap"));
    }
    std::fs::remove_dir_all(&setup.scratch)
        .map_err(|error| fail(&format!("scratch stuck: {error}")))?;
    Ok(())
}

#[test]
#[cfg(unix)]
fn gc1_t30_key_never_aliases_across_flags_or_image() -> Result<(), Box<dyn Error>> {
    let setup = behave_setup()?;
    let baseline = discover_with(&setup, BASELINE)?;
    let mut seen = std::collections::BTreeMap::new();
    for inputs in matrix() {
        let (key, restore) = discover_with(&setup, inputs)?;
        let label = format!(
            "share={:?} build={:?} cc={:?} image={:?}",
            inputs.share_out_dir, inputs.build_script_execution, inputs.mbx_cc, inputs.image_os
        );
        if let Some(previous) = seen.insert(key.clone(), label.clone()) {
            return Err(fail(&format!("keys collide: {previous} and {label}")));
        }
        if key == baseline.0 {
            continue;
        }
        if baseline.0.starts_with(&restore) || key.starts_with(&baseline.1) {
            return Err(fail(&format!(
                "restore prefix would reach foreign state: {label}"
            )));
        }
    }
    for unset in [
        Inputs {
            share_out_dir: None,
            build_script_execution: None,
            mbx_cc: None,
            ..BASELINE
        },
        Inputs {
            mbx_cc: None,
            ..BASELINE
        },
    ] {
        let (unset_key, _) = discover_with(&setup, unset)?;
        if seen.contains_key(&unset_key) {
            return Err(fail("unset inputs alias a set configuration"));
        }
    }
    std::fs::remove_dir_all(&setup.scratch)
        .map_err(|error| fail(&format!("scratch stuck: {error}")))?;
    Ok(())
}

#[test]
#[cfg(unix)]
fn gc1_t30_missing_image_os_goes_cold() -> Result<(), Box<dyn Error>> {
    let setup = behave_setup()?;
    let (status, baseline) = discover_raw(&setup, BASELINE)?;
    if !status.success()
        || !baseline.lines().any(|line| line == "restore=true")
        || !baseline.lines().any(|line| line == "save=true")
        || baseline.lines().any(|line| line.starts_with("reason="))
    {
        return Err(fail(&format!(
            "baseline lost its warm outputs: {baseline:?}"
        )));
    }
    for image_os in [None, Some("")] {
        let (status, outputs) = discover_raw(
            &setup,
            Inputs {
                image_os,
                ..BASELINE
            },
        )?;
        if !status.success() {
            return Err(fail(&format!(
                "image_os={image_os:?}: discover failed the job"
            )));
        }
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
    let steps = transport_steps()?;
    if steps[4].condition.as_deref() != Some(MBX_BUNDLE_RESTORE_CONDITION) {
        return Err(fail("restore step not gated on the restore output"));
    }
    std::fs::remove_dir_all(&setup.scratch)
        .map_err(|error| fail(&format!("scratch stuck: {error}")))?;
    Ok(())
}
