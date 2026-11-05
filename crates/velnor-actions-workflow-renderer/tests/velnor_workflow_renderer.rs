// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(
    not(target_os = "linux"),
    expect(
        dead_code,
        reason = "the state scenes need GNU tar, du -sb and Linux zstd and run only on Linux"
    )
)]

mod gc1_cache_service;
mod gc1_mbx_stub;
mod gc1_scene;
mod gc1_scene_assert;
#[cfg(target_os = "linux")]
mod gc1_scene_bounded;
mod gc1_scene_key;
mod gc1_scene_resolve;
mod gc1_scene_seal;
mod gc1_scene_tree;
mod gc1_seal_support;
mod gc1_seal_units;
mod gc1_t13;
mod gc1_t14;
mod gc1_t16;
mod gc1_t30;
mod gc1_t31;
mod gc1_t41;
mod gc1_t42;
mod gc1_t43;
mod gc1_t44;
mod gc1_t45;
mod gc1_t46;
mod gc1_t47;
mod gc1_t48;
mod gc1_t50;
mod gc1_t51;
mod gc1_t53;
mod gc1_ustar;
mod gc1_workflow_cases_common;
mod gc1_workflow_cases_preserve;
mod gc1_workflow_cases_reject;
#[cfg(target_os = "linux")]
mod state_round_trip;
mod state_v4_admit;
mod state_v4_attest_save;
mod state_v4_bind;
#[cfg(target_os = "linux")]
mod state_v4_bounds;
#[cfg(target_os = "linux")]
mod state_v4_disk;
mod state_v4_key;
mod state_v4_pack;
#[cfg(target_os = "linux")]
mod state_v4_packer;
mod state_v4_producer;
mod state_v4_restore;
mod state_v4_seal;
mod state_v4_select;
#[cfg(target_os = "linux")]
mod state_v4_select_bounds;
mod state_v4_select_units;
mod state_v4_snapshot;
mod state_v4_tip;
