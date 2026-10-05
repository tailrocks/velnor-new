//! Execute the generated stock-MBX helper with the verified upstream CLI.
#![cfg(all(target_os = "linux", target_arch = "x86_64"))]

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::fs::{self, OpenOptions};
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

use velnor_actions_contract::{Step, StepKind};

use crate::cache_steps::{
    CompileDriver, MBX_ACTION_NAME, TOOLS_RESTORE_USES, TOOLS_SAVE_USES, mbx_steps_for_driver,
};
use crate::stock_mbx_bundle::{MBX_BUNDLE_CACHE_PATH, MbxBundleCacheSteps, lifecycle_steps};

#[path = "stock_mbx_bundle_cli_preflight_env.rs"]
mod preflight_env;
use preflight_env::PREFLIGHT_ENV;

const BINARY_ENV: &str = "VELNOR_MBX_CACHE_QUALIFICATION_BINARY";
const BINARY_SHA256: &str = "97984b8c92953cefc027014d24c8abf8773f2d12ded0385156d2050da8d5fb8c";
const MBX_SOURCE: &str = "a0a44c61ca6aaa8da41d59deeebdfc46fc9d3313";
const MBX_ARCHIVE: &str = "https://github.com/jdx/mr-boxington/releases/download/v1.21.1/mbx-x86_64-unknown-linux-gnu.tar.gz";
const MBX_ARCHIVE_SHA256: &str = "1ecb4d55582a40a1227e8ca3450da054ea464e5ff976bb5762943fb1ce6f31da";
const MBX_MANIFEST_SHA256: &str =
    "38e681992b87d182b8a3468bf7ae00dfc1aa9ae800210d8dede63a1b6a5bca3f";
const MISE_BINARY_ENV: &str = "VELNOR_MISE_CACHE_QUALIFICATION_BINARY";
const MISE_BINARY_SHA256: &str = "8f5f6660336f572830e33cd9b378d3131e529a0d4c4f0c553776be90a1ba302a";
const MISE_SOURCE: &str = "44ea2537166efbe21b19d808355d9914e830941a";
const MISE_ARCHIVE_SHA256: &str =
    "79a2bf0ffc9b8a9a6391344e875b3c3679c15053fda3e8728ddf1790d63db788";
const CACHE_KEY: &str = "velnor-mbx-cli-lifecycle-v1";
const CACHE_PREFIX: &str = "velnor-mbx-cli-lifecycle-";
const SAVE_CONDITION: &str = "success() && github.event_name == 'push' && toJSON(github.event.repository.fork) == 'false' && github.event.repository.default_branch != '' && github.ref == format('refs/heads/{0}', github.event.repository.default_branch) && github.ref_protected == true";
const REPOSITORY_ID: &str = "1";
const JOB_ID: &str = "j";
const LINUX_SUN_PATH_CAPACITY: usize = 108;
const LOCK: &str = "version = 4\n\n[[package]]\nname = \"mbx-cache-probe-app\"\nversion = \"0.1.0\"\ndependencies = [\n \"mbx-cache-probe-lib\",\n]\n\n[[package]]\nname = \"mbx-cache-probe-lib\"\nversion = \"0.1.0\"\n";
static NEXT_ROOT: AtomicU64 = AtomicU64::new(0);

#[path = "stock_mbx_bundle_cli_bundle.rs"]
mod bundle;
#[path = "stock_mbx_bundle_cli_cleanup_tests.rs"]
mod cleanup_tests;
#[path = "stock_mbx_bundle_cli_failure_hooks.rs"]
mod failure_hooks;
#[path = "stock_mbx_bundle_cli_failure_tests.rs"]
mod failure_tests;
#[path = "stock_mbx_bundle_cli_fixture.rs"]
mod fixture;
#[path = "stock_mbx_bundle_cli_lifecycle_tests.rs"]
mod lifecycle_tests;
#[path = "stock_mbx_bundle_cli_predicate_tests.rs"]
mod predicate_tests;
#[path = "stock_mbx_bundle_cli_process_tests.rs"]
mod process_tests;
#[path = "stock_mbx_bundle_cli_runner.rs"]
mod runner;
#[path = "stock_mbx_bundle_cli_step_capture.rs"]
mod step_capture;
#[path = "stock_mbx_bundle_cli_tool_identity.rs"]
mod tool_identity;

use bundle::{
    bundle_path, check_cache_path_contract, check_distinct_targets, check_fresh_reader_state,
    check_imported_cache, check_outputs, check_private_bundle_root, cleanup_after_export,
    copy_bundle, largest_file, target_dir, tree_hash,
};
use cleanup_tests::exercise_pre_export_cleanup;
use failure_hooks::{exercise_metadata_cancellation, inject_enospc};
use fixture::{Fixture, Selection, Stage, StepResult, path_identity};
use process_tests::exercise_fast_exit_import;
use runner::{
    GateAction, GatedShell, build, check_empty_store, check_local_setup_env, env_pairs,
    generated_preflight_steps, import_miss, number, preflight_step, primary_selection,
    restore_inputs, run_shell, run_shell_gated, selection, stats, steps, text,
};
use step_capture::{capture_shell_step, check_shell_syntax, generated_env, generated_run};

pub(super) fn capture_action_step(step: &Step, label: &str) -> Result<(), String> {
    let Some(directory) = capture_directory()? else {
        return Ok(());
    };
    let StepKind::Action { uses, with, env } = &step.kind else {
        return Err(format!("{} is not a generated action", step.name));
    };
    let mut action = BTreeMap::from([
        ("name".to_owned(), step.name.clone()),
        ("uses".to_owned(), uses.clone()),
    ]);
    for (key, value) in with {
        action.insert(format!("with.{key}"), value.clone());
    }
    if let Some(condition) = &step.condition {
        action.insert("condition".to_owned(), condition.clone());
    }
    if let Some(continue_on_error) = step.continue_on_error {
        action.insert(
            "continue_on_error".to_owned(),
            continue_on_error.to_string(),
        );
    }
    write_map(&directory.join(format!("{label}.action")), &action)?;
    write_map(&directory.join(format!("{label}.env")), env)
}

pub(super) fn capture_label(label: &str) -> String {
    let test = std::thread::current()
        .name()
        .unwrap_or("test")
        .replace("::", "-");
    format!("{test}-{label}")
}

pub(super) fn capture_runtime(
    label: &str,
    resolved_env: &BTreeMap<String, String>,
    outputs: &BTreeMap<String, String>,
    github_env: &BTreeMap<String, String>,
    status: i32,
    text: &str,
) -> Result<(), String> {
    let Some(directory) = capture_directory()? else {
        return Ok(());
    };
    write_map(
        &directory.join(format!("{label}.resolved-env")),
        resolved_env,
    )?;
    write_map(&directory.join(format!("{label}.outputs")), outputs)?;
    write_map(&directory.join(format!("{label}.github-env")), github_env)?;
    fs::write(
        directory.join(format!("{label}.status")),
        format!("{status}\n"),
    )
    .map_err(|err| err.to_string())?;
    fs::write(directory.join(format!("{label}.result")), text).map_err(|err| err.to_string())
}

fn capture_directory() -> Result<Option<PathBuf>, String> {
    std::env::var_os("VELNOR_MBX_STEP_CAPTURE_DIR")
        .map(PathBuf::from)
        .map(|directory| {
            fs::create_dir_all(&directory).map_err(|err| err.to_string())?;
            Ok(directory)
        })
        .transpose()
}

fn write_map(path: &Path, values: &BTreeMap<String, String>) -> Result<(), String> {
    let mut text = String::new();
    for (key, value) in values {
        text.push_str(key);
        text.push('=');
        text.push_str(value);
        text.push('\n');
    }
    fs::write(path, text).map_err(|err| err.to_string())
}

fn json_string(value: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut json = String::from("\"");
    for character in value.chars() {
        match character {
            '"' => json.push_str("\\\""),
            '\\' => json.push_str("\\\\"),
            '\n' => json.push_str("\\n"),
            '\r' => json.push_str("\\r"),
            '\t' => json.push_str("\\t"),
            '\u{0008}' => json.push_str("\\b"),
            '\u{000c}' => json.push_str("\\f"),
            control if control <= '\u{001f}' => {
                let scalar = control as usize;
                json.push_str("\\u00");
                json.push(char::from(HEX[(scalar >> 4) & 0x0f]));
                json.push(char::from(HEX[scalar & 0x0f]));
            }
            ordinary => json.push(ordinary),
        }
    }
    json.push('"');
    json
}

fn check_lifecycle_env(step: &Step, is_import: bool) -> Result<(), String> {
    let env = generated_env(step)?;
    let mut expected: BTreeMap<_, _> = PREFLIGHT_ENV
        .iter()
        .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
        .collect();
    expected.extend(crate::toolchain_env::credential_scrub());
    let output_source = if is_import {
        "mbx_cache_preflight"
    } else {
        "mbx_cache_import"
    };
    expected.insert(
        "MBX_CACHE_DIR".to_owned(),
        format!("${{{{ steps.{output_source}.outputs.cache_dir }}}}"),
    );
    expected.insert(
        "MBX_CACHE_EXPORT_GROUP".to_owned(),
        format!("${{{{ steps.{output_source}.outputs.export_group }}}}"),
    );
    if is_import {
        expected.insert(
            "MBX_CACHE_RESTORE_OUTCOME".to_owned(),
            "${{ steps.mbx_cache_restore.outcome }}".to_owned(),
        );
        expected.insert(
            "MBX_CACHE_RESTORE_MATCHED_KEY".to_owned(),
            "${{ steps.mbx_cache_restore.outputs.cache-matched-key }}".to_owned(),
        );
    }
    if env != &expected {
        return Err(format!(
            "{} has unexpected generated environment: {env:?}",
            step.name
        ));
    }
    Ok(())
}

fn check_lifecycle_shell(step: &Step, expected_program: &str) -> Result<(), String> {
    let run = generated_run(step)?;
    let typed = matches!(&step.kind, StepKind::MbxProcessGroupShell(_));
    if run.len() != 3
        || run[0] != expected_program
        || run[1] != "-c"
        || typed != (expected_program == "bash")
    {
        return Err(format!(
            "{} shell argv is not [{expected_program}, -c, script]: {run:?}",
            step.name
        ));
    }
    if expected_program == "sh" && run[2].contains(" & ") {
        return Err(format!(
            "ordinary shell step {} must not launch background commands",
            step.name
        ));
    }
    Ok(())
}

fn check_cleanup_env(step: &Step) -> Result<(), String> {
    let env = generated_env(step)?;
    let mut expected: BTreeMap<_, _> = PREFLIGHT_ENV
        .iter()
        .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
        .collect();
    expected.extend(crate::toolchain_env::credential_scrub());
    expected.insert(
        "MBX_CACHE_EXPORT_GROUP".to_owned(),
        "${{ steps.mbx_cache_preflight.outputs.export_group }}".to_owned(),
    );
    expected.insert(
        "VELNOR_MBX_EXPORTED_BUNDLE_IDENTITY".to_owned(),
        "${{ steps.mbx_cache_export.outputs.bundle_identity }}".to_owned(),
    );
    expected.insert(
        "VELNOR_MBX_IMPORTED_BUNDLE_IDENTITY".to_owned(),
        "${{ steps.mbx_cache_import.outputs.bundle_identity }}".to_owned(),
    );
    expected.insert(
        "VELNOR_MBX_EXPORT_OUTCOME".to_owned(),
        "${{ steps.mbx_cache_export.outcome }}".to_owned(),
    );
    expected.insert(
        "VELNOR_MBX_EXPORT_CLEANUP_SAFE".to_owned(),
        "${{ steps.mbx_cache_export.outputs.cleanup_safe }}".to_owned(),
    );
    expected.insert(
        "VELNOR_MBX_IMPORT_OUTCOME".to_owned(),
        "${{ steps.mbx_cache_import.outcome }}".to_owned(),
    );
    expected.insert(
        "VELNOR_MBX_IMPORT_QUIESCED".to_owned(),
        "${{ steps.mbx_cache_import.outputs.mbx_quiesced }}".to_owned(),
    );
    expected.insert(
        "VELNOR_MBX_RESTORE_OUTCOME".to_owned(),
        "${{ steps.mbx_cache_restore.outcome }}".to_owned(),
    );
    if env != &expected {
        return Err(format!(
            "{} has unexpected generated environment: {env:?}",
            step.name
        ));
    }
    Ok(())
}

fn check_export_gates(lifecycle: &MbxBundleCacheSteps) -> Result<(), String> {
    let export = lifecycle.save[0]
        .condition
        .as_deref()
        .ok_or("generated export has no condition")?;
    if !export.contains("outputs.export_allowed == 'true'")
        || !export.contains("outputs.store_ready == 'true'")
    {
        return Err(format!(
            "generated export condition lacks cold-import gates: {export}"
        ));
    }
    let env = generated_env(&lifecycle.save[0])?;
    if env.get("MBX_CACHE_EXPORT_GROUP").map(String::as_str)
        != Some("${{ steps.mbx_cache_import.outputs.export_group }}")
        || env.get("MBX_CACHE_DIR").map(String::as_str)
            != Some("${{ steps.mbx_cache_import.outputs.cache_dir }}")
    {
        return Err("export does not consume the selected import store and group".to_owned());
    }
    if lifecycle.save[1]
        .condition
        .as_deref()
        .is_none_or(|value| !value.contains("outputs.ready == 'true'"))
    {
        return Err("cache save is not gated by a useful export".to_owned());
    }
    Ok(())
}
