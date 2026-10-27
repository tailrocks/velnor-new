// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::Path;
use std::process::{Command, ExitCode};

use velnor_actions_orchestrator::EntryTupleV1;
use velnor_actions_orchestrator::cache_recovery::{
    ReadbackInputs, ReadbackVerdict, decide_readback, require_empty_restore_target,
};
use velnor_actions_orchestrator::{InventoryV1, inventory_digest, parse_inventory_bytes};

use super::archive::{list_archive_members, payload_bytes, reread_payload, screen_archive_members};
use super::request::parse_expected;
use super::{
    Expected, STATE_BASE, STATE_ROOT_REL, STATE_STAGE_REL, STATE_TRANSPORT_REL,
    ValidatedCacheRequest, find_entry, observed_entry, single_line,
};
use crate::dispatch::fail_internal;

struct RecoverPlan {
    inventory: InventoryV1,
    expected: Expected,
    entry: EntryTupleV1,
    bytes: Vec<u8>,
}

fn admit_recover(
    request: &ValidatedCacheRequest,
    runner_temp: &Path,
) -> Result<Option<RecoverPlan>, String> {
    let inventory = match parse_inventory_bytes(&request.inventory_bytes) {
        Ok(inventory) => inventory,
        Err(error) => return Err(single_line(&error.to_string())),
    };
    if let Err(error) = inventory_digest(&inventory) {
        return Err(single_line(&error.to_string()));
    }
    let expected = parse_expected(&request.expected_bytes)?;
    let Some(entry) = find_entry(&inventory, &expected).cloned() else {
        println!("velnor: state snapshot no_entry (cold-missing)");
        return Ok(None);
    };
    let payload = runner_temp.join(STATE_TRANSPORT_REL);
    let bytes = match payload_bytes(&payload) {
        Ok(bytes) => bytes,
        Err(problem) if problem == "cache_payload_missing" => {
            println!("velnor: state snapshot no_entry (cold-missing)");
            return Ok(None);
        }
        Err(problem) if problem == "cache_payload_empty" => {
            println!("::warning::cache_corrupt: state payload empty; building cold");
            drop(fs::remove_file(&payload));
            return Ok(None);
        }
        Err(problem) => return Err(problem),
    };
    let len = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    if len != entry.size_in_bytes {
        println!(
            "::warning::cache_corrupt: state payload size disagrees with inventory; building cold"
        );
        drop(fs::remove_file(&payload));
        return Ok(None);
    }
    Ok(Some(RecoverPlan {
        inventory,
        expected,
        entry,
        bytes,
    }))
}

fn extract_staged(payload: &Path, root: &Path, stage: &Path) -> Result<(), String> {
    drop(fs::remove_dir_all(stage));
    if fs::create_dir_all(stage).is_err() {
        return Err("cache recover stage_unwritable".to_owned());
    }
    let extract = Command::new("tar")
        .arg("-xzf")
        .arg(payload)
        .arg("-C")
        .arg(stage)
        .output();
    if !extract.is_ok_and(|output| output.status.success()) {
        drop(fs::remove_dir_all(stage));
        return Err("cache recover payload_unreadable".to_owned());
    }
    let staged_base = stage.join(STATE_BASE);
    let staged_ok = fs::symlink_metadata(&staged_base)
        .is_ok_and(|meta| meta.is_dir() && !meta.file_type().is_symlink());
    if !staged_ok {
        drop(fs::remove_dir_all(stage));
        return Err("cache recover payload_missed_state_root".to_owned());
    }
    if symlink_sweep_refused(&staged_base) {
        drop(fs::remove_dir_all(stage));
        return Err("cache recover refused_shadow: symlinked member".to_owned());
    }
    if fs::rename(&staged_base, root).is_err() {
        drop(fs::remove_dir_all(stage));
        return Err("cache recover handoff_failed".to_owned());
    }
    drop(fs::remove_dir_all(stage));
    Ok(())
}

fn verify_recover(plan: &RecoverPlan, payload: &Path, root: &Path) -> ExitCode {
    let len = u64::try_from(plan.bytes.len()).unwrap_or(u64::MAX);
    let (reread, digest_one) = match reread_payload(payload, &plan.bytes, "recover") {
        Ok(pair) => pair,
        Err(problem) => {
            drop(fs::remove_dir_all(root));
            return fail_internal(&problem);
        }
    };
    let observed = observed_entry(&plan.entry);
    let inputs = ReadbackInputs {
        frozen_archive_digest: &digest_one,
        frozen_archive_bytes: len,
        restored_bytes: Some(&reread),
        inventory_complete: plan.inventory.complete,
        observed_entry: Some(&observed),
        expected_key: &plan.expected.key,
        expected_ref: &plan.expected.ref_name,
        expected_version: &plan.expected.version,
    };
    match decide_readback(&inputs) {
        Ok(ReadbackVerdict::Verified {
            archive_digest,
            stored_bytes,
        }) => {
            println!(
                "velnor: state snapshot recovered (sha256:{archive_digest} bytes:{stored_bytes})"
            );
            ExitCode::SUCCESS
        }
        Ok(ReadbackVerdict::Corrupt { evidence }) => {
            drop(fs::remove_dir_all(root));
            fail_internal(&format!("cache recover readback_corrupt: {evidence}"))
        }
        Ok(ReadbackVerdict::Unavailable { reason }) => {
            drop(fs::remove_dir_all(root));
            fail_internal(&format!("cache recover readback_unavailable: {reason}"))
        }
        Err(error) => {
            drop(fs::remove_dir_all(root));
            fail_internal(&single_line(&error.to_string()))
        }
    }
}

pub(super) fn run_recover(request: &ValidatedCacheRequest, runner_temp: &Path) -> ExitCode {
    let plan = match admit_recover(request, runner_temp) {
        Ok(plan) => plan,
        Err(problem) => return fail_internal(&problem),
    };
    let Some(plan) = plan else {
        return ExitCode::SUCCESS;
    };
    let payload = runner_temp.join(STATE_TRANSPORT_REL);
    let root = runner_temp.join(STATE_ROOT_REL);
    if fs::symlink_metadata(&root).is_ok() && require_empty_restore_target(true).is_err() {
        return fail_internal("cache recover target_not_empty");
    }
    if root
        .parent()
        .is_some_and(|parent| fs::create_dir_all(parent).is_err())
    {
        return fail_internal("cache recover parent_unwritable");
    }
    let members = match list_archive_members(&payload) {
        Ok(members) => members,
        Err(problem) => return fail_internal(&problem),
    };
    if let Err(problem) = screen_archive_members(&members) {
        return fail_internal(&problem);
    }
    let stage = runner_temp.join(STATE_STAGE_REL);
    if let Err(problem) = extract_staged(&payload, &root, &stage) {
        return fail_internal(&problem);
    }
    verify_recover(&plan, &payload, &root)
}

/// Fail closed on any symlink under an extracted tree: symlinks are never
/// followed or resolved, so a hostile link can neither redirect later
/// members nor escape the staging directory.
fn symlink_sweep_refused(dir: &Path) -> bool {
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        let Ok(entries) = fs::read_dir(&current) else {
            return true;
        };
        for entry in entries {
            let Ok(entry) = entry else {
                return true;
            };
            let Ok(file_type) = entry.file_type() else {
                return true;
            };
            if file_type.is_symlink() {
                return true;
            }
            if file_type.is_dir() {
                stack.push(entry.path());
            } else if !file_type.is_file() {
                // FIFOs, sockets, and device nodes are never part of a
                // state snapshot: refuse them before the rename handoff.
                return true;
            }
        }
    }
    false
}
