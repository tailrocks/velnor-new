//! Closed Nextest selector application for one Rust test obligation.

use std::ffi::OsString;

use velnor_actions_contract::config::{NextestTestSelection, NextestTestSelectionMode};
use velnor_actions_contract::{ContractError, ProposedTask};

use crate::propose::propose_task;
use crate::tasks::{TaskGroup, TaskKind};

/// Propose a Nextest task with one validated exact-test allowlist.
///
/// The selector changes the run argv only. It never changes the input group,
/// so the producing Build task and archive identity remain independent.
/// # Errors
pub fn propose_nextest_selection(
    group: &TaskGroup,
    selection: &NextestTestSelection,
) -> Result<ProposedTask, ContractError> {
    selection.validate(".velnor/config.toml", "stacks.rust.nextest_test_selectors")?;
    if group.kind != TaskKind::Nextest {
        return Err(ContractError::identity(
            "nextest_selector",
            "requires_nextest_task",
        ));
    }
    if group.package_name != selection.package {
        return Err(ContractError::identity(
            "nextest_selector",
            "package_mismatch",
        ));
    }
    if group.no_test_targets {
        return Err(ContractError::identity(
            "nextest_selector",
            "package_has_no_test_targets",
        ));
    }
    let mut task = propose_task(group)?;
    apply_selector(&mut task.payload, selection)?;
    task.validate()?;
    Ok(task)
}

/// Replace inherited ignored mode and add the closed filter before no-tests.
fn apply_selector(
    payload: &mut Vec<OsString>,
    selection: &NextestTestSelection,
) -> Result<(), ContractError> {
    remove_run_ignored(payload)?;
    let position = no_tests_position(payload)?;
    let mode = match selection.mode {
        NextestTestSelectionMode::IgnoredOnly => "only",
    };
    let args = [
        OsString::from("--run-ignored"),
        OsString::from(mode),
        OsString::from("--filterset"),
        OsString::from(filter_expression(selection)),
    ];
    payload.splice(position..position, args);
    Ok(())
}

/// Remove any profile-derived `--run-ignored <mode>` pair.
fn remove_run_ignored(payload: &mut Vec<OsString>) -> Result<(), ContractError> {
    let mut index = 0;
    while index < payload.len() {
        if payload[index].to_str() == Some("--run-ignored") {
            if index + 1 >= payload.len() {
                return Err(ContractError::identity(
                    "nextest_selector",
                    "malformed_run_ignored_payload",
                ));
            }
            payload.drain(index..index + 2);
        } else {
            index += 1;
        }
    }
    Ok(())
}

/// Locate the fixed fail-closed empty-suite guard.
fn no_tests_position(payload: &[OsString]) -> Result<usize, ContractError> {
    payload
        .windows(2)
        .position(|pair| pair[0].to_str() == Some("--no-tests") && pair[1].to_str() == Some("fail"))
        .ok_or_else(|| ContractError::identity("nextest_selector", "missing_no_tests_fail_guard"))
}

/// Build a deterministic filter only from validated typed values.
fn filter_expression(selection: &NextestTestSelection) -> String {
    let tests = selection
        .test_ids
        .iter()
        .map(|name| format!("test(={name})"))
        .collect::<Vec<_>>()
        .join(" | ");
    format!(
        "package(={}) & binary(={}) & ({tests})",
        selection.package, selection.binary
    )
}
