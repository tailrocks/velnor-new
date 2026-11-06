// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

const TARGET: &str = ".github";
const PRIVATE_MODE: u32 = 0o700;
const STAGED_MODE: u32 = 0o777;
const PRIVATE_STAGE_PREFIX: &str = ".github.velnor-stage-";

#[path = "generate_stage_cleanup.rs"]
mod cleanup;
#[path = "generate_stage_root/guards.rs"]
mod guards;
#[path = "generate_stage_root/root.rs"]
mod root;
#[path = "generate_stage_root/stage.rs"]
mod stage;

pub(super) use root::{FsIdentity, RootDirectory};
pub(super) use stage::StageRoot;
