//! Responsibility-specific Cargo target directories.

/// Target-directory prefix isolating one lane.
pub const TARGET_DIR_PREFIX: &str = "$RUNNER_TEMP/velnor/target/";

/// Isolated target directory for one lane.
#[must_use]
pub fn target_dir_for_lane(lane_id: &str) -> String {
    format!("{TARGET_DIR_PREFIX}{lane_id}")
}

/// `CARGO_TARGET_DIR` env pair isolating one lane (CACHE-1.20).
#[must_use]
pub fn lane_cargo_target_env(lane_id: &str) -> (String, String) {
    ("CARGO_TARGET_DIR".to_owned(), target_dir_for_lane(lane_id))
}
