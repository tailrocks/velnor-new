// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[path = "preseed_digest.rs"]
mod digest;
#[path = "preseed_order.rs"]
mod order;
#[path = "preseed_steps.rs"]
mod steps;

pub use digest::{source_bound_helper_digest, source_bound_helper_digest_input};
pub use order::check_preseed_step_order;
pub use steps::{
    PreseedStageSource, preseed_build_step, preseed_download_step, preseed_manifest_step,
    preseed_manifest_verify_script, preseed_manifest_verify_step, preseed_stage_step,
    preseed_upload_step, preseed_verify_step,
};

pub const PRESEED_ARTIFACT_NAME: &str = "velnor-preseed-helper";
pub const PRESEED_STAGE_DIR: &str = "$RUNNER_TEMP/velnor/preseed";
pub const PRESEED_MANIFEST_FILE: &str = "preseed-manifest.json";
pub const PRESEED_BUILD_OUTPUT: &str = "target/release/velnor-actions";
pub const PRESEED_DOWNLOADED_BINARY: &str = "$RUNNER_TEMP/velnor/preseed/velnor-actions";
pub const PRESEED_BUILD_NAME: &str = "Build helper (pre-seed trust-on-review)";
pub const PRESEED_VERIFY_NAME: &str = "Verify MBX compile (pre-seed trust-on-review)";
pub const PRESEED_MANIFEST_NAME: &str = "Write helper manifest (pre-seed trust-on-review)";
pub const PRESEED_UPLOAD_NAME: &str = "Upload helper (pre-seed trust-on-review)";
pub const PRESEED_DOWNLOAD_NAME: &str = "Download helper (pre-seed trust-on-review)";
pub const PRESEED_VERIFY_MANIFEST_NAME: &str = "Verify helper manifest (pre-seed trust-on-review)";
pub const PRESEED_STAGE_NAME: &str = "Stage helper (pre-seed trust-on-review)";
pub const PRESEED_MANIFEST_BINARY_ENV: &str = "VELNOR_PRESEED_BINARY";
pub const PRESEED_MANIFEST_OUT_ENV: &str = "VELNOR_PRESEED_OUT";
pub const PRESEED_MANIFEST_TARGET_ENV: &str = "VELNOR_PRESEED_TARGET";
pub const PRESEED_MANIFEST_TOOLCHAIN_ENV: &str = "VELNOR_PRESEED_TOOLCHAIN";
pub const PRESEED_MANIFEST_INPUT_DIGEST_ENV: &str = "VELNOR_PRESEED_INPUT_DIGEST";
