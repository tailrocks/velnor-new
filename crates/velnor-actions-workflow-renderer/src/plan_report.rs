// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::steps::{INTERNAL_OP_ENV, STAGED_BINARY_PREFIX};

pub const REPORT_OP: &str = "write-task-report-v1";
pub const TASK_ID_ENV: &str = "VELNOR_TASK_ID";
pub const EXIT_CODE_ENV: &str = "VELNOR_EXIT_CODE";
pub const START_MS_ENV: &str = "VELNOR_START_MS";

#[must_use]
pub fn helper_path_for_version(version: &str) -> String {
    format!("{STAGED_BINARY_PREFIX}{version}")
}

#[must_use]
pub fn report_wrapper_argv(joined: &str, helper: &str) -> Vec<String> {
    vec![
        "sh".to_owned(),
        "-c".to_owned(),
        format!(
            "s=$(date +%s%3N); {joined}; code=$?; {EXIT_CODE_ENV}=\"$code\" {START_MS_ENV}=\"$s\" {INTERNAL_OP_ENV}={REPORT_OP} \"{helper}\"; helper_code=$?; if [ \"$code\" -ne 0 ]; then exit \"$code\"; fi; exit \"$helper_code\""
        ),
    ]
}

#[must_use]
pub fn outcome_wrapper_argv(joined: &str, outcome_path: &str, start_path: &str) -> Vec<String> {
    vec![
        "sh".to_owned(),
        "-c".to_owned(),
        format!(
            "date +%s%3N > \"{start_path}\"; {joined}; code=$?; echo \"$code\" > \"{outcome_path}\"; exit \"$code\""
        ),
    ]
}

#[must_use]
pub fn deferred_report_argv(outcome_path: &str, helper: &str, start_path: &str) -> Vec<String> {
    vec![
        "sh".to_owned(),
        "-c".to_owned(),
        format!(
            "read -r code rest < \"{outcome_path}\"; read -r start_ms rest < \"{start_path}\"; {EXIT_CODE_ENV}=\"$code\" {START_MS_ENV}=\"$start_ms\" {INTERNAL_OP_ENV}={REPORT_OP} \"{helper}\""
        ),
    ]
}

#[must_use]
pub fn outcome_path_for_key(matrix_key: &str) -> String {
    format!("$RUNNER_TEMP/velnor/outcome-{matrix_key}")
}

#[must_use]
pub fn start_path_for_key(matrix_key: &str) -> String {
    format!("$RUNNER_TEMP/velnor/start-{matrix_key}")
}
