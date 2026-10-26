// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

pub const TARGET_DIR_PREFIX: &str = "$RUNNER_TEMP/velnor/target/";

#[must_use]
pub fn target_dir_for_lane(lane_id: &str) -> String {
    format!("{TARGET_DIR_PREFIX}{lane_id}")
}

#[must_use]
pub fn lane_cargo_target_env(lane_id: &str) -> (String, String) {
    ("CARGO_TARGET_DIR".to_owned(), target_dir_for_lane(lane_id))
}
