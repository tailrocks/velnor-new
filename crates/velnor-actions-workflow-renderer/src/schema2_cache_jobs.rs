// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::super::features::{finish, gated, lane_base, run_step};
use super::steps::{mapping, uses_with};
use super::{Extras, RunnerSpec, emit, lane};
use crate::yaml::Yaml;
use velnor_actions_contract::JobTimeout;
use velnor_actions_contract::workflow::{
    QUALIFICATION_CACHE_GUARD_ERROR, QUALIFICATION_CACHE_GUARD_ID,
};

const CACHE_USES: &str = "actions/cache@55cc8345863c7cc4c66a329aec7e433d2d1c52a9";
const CACHE_KEY: &str = "g4-cache-${{ github.run_id }}";

pub(super) fn jobs(hosted: &RunnerSpec, scale: &RunnerSpec) -> Vec<(String, Yaml)> {
    let perms = cache_permissions();
    vec![
        emit(
            lane(
                "cache",
                "Cache",
                "scale-set",
                "Velnor Scale Set",
                scale,
                &[],
                vec![
                    run_step("Write cache file", "echo cache-ok > g4-cache.txt"),
                    cache_step("Save cache", false),
                ],
            ),
            Extras {
                permissions: Some(perms.clone()),
                cache_writer: true,
                ..Extras::default()
            },
        ),
        emit(
            lane(
                "cache",
                "Cache",
                "hosted",
                "GitHub hosted",
                hosted,
                &["cache-scale-set"],
                vec![
                    cache_step("Restore cache", true),
                    run_step("Check restore", "grep -qx cache-ok g4-cache.txt"),
                ],
            ),
            Extras {
                permissions: Some(perms),
                ..Extras::default()
            },
        ),
    ]
}

pub(super) fn default_branch_guard(hosted: &RunnerSpec) -> (String, Yaml) {
    gated(
        finish(
            QUALIFICATION_CACHE_GUARD_ID,
            lane_base(
                "Cache qualification / default branch guard",
                hosted,
                JobTimeout::DEFAULT,
            ),
            vec![run_step(
                "Reject cache mode on the default branch",
                &format!(
                    "echo '::error title={QUALIFICATION_CACHE_GUARD_ERROR}::Dispatch cache qualification modes from a non-default branch; every cache job is skipped on the default branch.' && exit 1"
                ),
            )],
        ),
        &crate::cache_authority::qualification_default_branch_condition(),
    )
}

fn cache_permissions() -> Yaml {
    mapping(&[("contents", "read"), ("actions", "write")])
}

fn cache_step(name: &str, fail_on_miss: bool) -> Yaml {
    let mut with = vec![("path", "g4-cache.txt"), ("key", CACHE_KEY)];
    if fail_on_miss {
        with.push(("fail-on-cache-miss", "true"));
    }
    uses_with(name, CACHE_USES, &with)
}
