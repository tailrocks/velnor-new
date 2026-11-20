// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! Every key `.velnor/rust-ci.toml` accepts, with the first `velnor-ci` that parses it.

use super::{ConfigFile::RustCi, KeyGate, gate};
use crate::HELPER_MINIMUM;

pub(super) const RUST_CI: &[KeyGate] = &[
    // helper-gate: first in tools-v0.1.92 via 1605e9c34d
    gate(RustCi, "budget", (0, 1, 92)),
    // helper-gate: first in tools-v0.1.104 via 1a20fc47fe
    gate(RustCi, "budget.affected", (0, 1, 104)),
    // helper-gate: first in tools-v0.1.104 via 1a20fc47fe
    gate(RustCi, "budget.affected.main_wall_seconds", (0, 1, 104)),
    // helper-gate: first in tools-v0.1.104 via 1a20fc47fe
    gate(RustCi, "budget.affected.pr_wall_seconds", (0, 1, 104)),
    // helper-gate: first in tools-v0.1.104 via 1a20fc47fe
    gate(RustCi, "budget.affected.runner_seconds", (0, 1, 104)),
    // helper-gate: first in tools-v0.1.104 via 1a20fc47fe
    gate(RustCi, "budget.affected.setup_seconds", (0, 1, 104)),
    // helper-gate: first in tools-v0.1.104 via 1a20fc47fe
    gate(RustCi, "budget.full", (0, 1, 104)),
    // helper-gate: first in tools-v0.1.104 via 1a20fc47fe
    gate(RustCi, "budget.full.main_wall_seconds", (0, 1, 104)),
    // helper-gate: first in tools-v0.1.104 via 1a20fc47fe
    gate(RustCi, "budget.full.pr_wall_seconds", (0, 1, 104)),
    // helper-gate: first in tools-v0.1.104 via 1a20fc47fe
    gate(RustCi, "budget.full.runner_seconds", (0, 1, 104)),
    // helper-gate: first in tools-v0.1.104 via 1a20fc47fe
    gate(RustCi, "budget.full.setup_seconds", (0, 1, 104)),
    // helper-gate: first in tools-v0.1.92 via 1605e9c34d
    gate(RustCi, "budget.main_wall_seconds", (0, 1, 92)),
    // helper-gate: first in tools-v0.1.104 via 1a20fc47fe
    gate(RustCi, "budget.none", (0, 1, 104)),
    // helper-gate: first in tools-v0.1.104 via 1a20fc47fe
    gate(RustCi, "budget.none.main_wall_seconds", (0, 1, 104)),
    // helper-gate: first in tools-v0.1.104 via 1a20fc47fe
    gate(RustCi, "budget.none.pr_wall_seconds", (0, 1, 104)),
    // helper-gate: first in tools-v0.1.104 via 1a20fc47fe
    gate(RustCi, "budget.none.runner_seconds", (0, 1, 104)),
    // helper-gate: first in tools-v0.1.104 via 1a20fc47fe
    gate(RustCi, "budget.none.setup_seconds", (0, 1, 104)),
    // helper-gate: first in tools-v0.1.92 via 1605e9c34d
    gate(RustCi, "budget.pr_wall_seconds", (0, 1, 92)),
    // helper-gate: first in tools-v0.1.92 via 1605e9c34d
    gate(RustCi, "budget.runner_seconds", (0, 1, 92)),
    // helper-gate: first in tools-v0.1.92 via 1605e9c34d
    gate(RustCi, "budget.setup_seconds", (0, 1, 92)),
    // helper-gate: first in tools-v0.1.88 via 08d1b3495d
    gate(RustCi, "cache", (0, 1, 88)),
    // First released in tools-v0.1.109 (not tagged yet when this gate was written).
    gate(RustCi, "cache.mbx", (0, 1, 109)),
    // helper-gate: first in tools-v0.1.88 via 08d1b3495d
    gate(RustCi, "cache.mbx_remote", (0, 1, 88)),
    // helper-gate: first in rust-ci-v0.1.82 via c0d0b51181
    gate(RustCi, "cache_budget_bytes", (0, 1, 82)),
    gate(RustCi, "cache_check_blocks", HELPER_MINIMUM),
    // helper-gate: first in rust-ci-v0.1.82 via bfc3528bd7
    gate(RustCi, "cache_debug", (0, 1, 82)),
    // helper-gate: first in tools-v0.1.91 via 240848918a
    gate(RustCi, "check_alone", (0, 1, 91)),
    // helper-gate: first in tools-v0.1.88 via 5e731ce593
    gate(RustCi, "ci", (0, 1, 88)),
    // helper-gate: first in tools-v0.1.88 via 5e731ce593
    gate(RustCi, "ci.skip_push_after_merge_group", (0, 1, 88)),
    gate(RustCi, "ci_debuginfo", HELPER_MINIMUM),
    // helper-gate: first in tools-v0.1.93 via 3147d5485a
    gate(RustCi, "crates_mirror", (0, 1, 93)),
    // helper-gate: first in tools-v0.1.93 via 3147d5485a
    gate(RustCi, "crates_mirror.cainfo", (0, 1, 93)),
    // helper-gate: first in tools-v0.1.93 via 3147d5485a
    gate(RustCi, "crates_mirror.registry", (0, 1, 93)),
    // helper-gate: first in tools-v0.1.93 via 3147d5485a
    gate(RustCi, "crates_mirror.runner", (0, 1, 93)),
    gate(RustCi, "default_branch", HELPER_MINIMUM),
    gate(RustCi, "default_ignores", HELPER_MINIMUM),
    // helper-gate: first in tools-v0.1.88 via 76a0485485
    gate(RustCi, "docs", (0, 1, 88)),
    gate(RustCi, "exclude", HELPER_MINIMUM),
    // First released in tools-v0.1.109 (not tagged yet when this gate was written).
    gate(RustCi, "external_workflows", (0, 1, 109)),
    // helper-gate: first in rust-ci-v0.1.82 via b2413d7709
    gate(RustCi, "fork_runner", (0, 1, 82)),
    gate(RustCi, "full_run", HELPER_MINIMUM),
    gate(RustCi, "helper", HELPER_MINIMUM),
    gate(RustCi, "helper.path", HELPER_MINIMUM),
    // helper-gate: first in tools-v0.1.99 via 0b991d8ce7
    gate(RustCi, "hosted", (0, 1, 99)),
    gate(RustCi, "inputs", HELPER_MINIMUM),
    gate(RustCi, "inputs.glob", HELPER_MINIMUM),
    gate(RustCi, "inputs.packages", HELPER_MINIMUM),
    // helper-gate: first in tools-v0.1.89 via 330573ef6d
    gate(RustCi, "lint", (0, 1, 89)),
    // helper-gate: first in tools-v0.1.89 via 330573ef6d
    gate(RustCi, "lint.config", (0, 1, 89)),
    // helper-gate: first in tools-v0.1.89 via 330573ef6d
    gate(RustCi, "lint.id", (0, 1, 89)),
    // helper-gate: first in tools-v0.1.89 via 330573ef6d
    gate(RustCi, "lint.kind", (0, 1, 89)),
    // helper-gate: first in tools-v0.1.89 via 330573ef6d
    gate(RustCi, "lint.name", (0, 1, 89)),
    // helper-gate: first in tools-v0.1.89 via 330573ef6d
    gate(RustCi, "lint.probe", (0, 1, 89)),
    // helper-gate: first in tools-v0.1.89 via 330573ef6d
    gate(RustCi, "lint.task", (0, 1, 89)),
    // helper-gate: first in tools-v0.1.89 via 330573ef6d
    gate(RustCi, "lint.timeout_minutes", (0, 1, 89)),
    // helper-gate: first in tools-v0.1.89 via 330573ef6d
    gate(RustCi, "lint.tools", (0, 1, 89)),
    // helper-gate: first in tools-v0.1.85 via e79060d249
    gate(RustCi, "mbx_shared_store", (0, 1, 85)),
    gate(RustCi, "nextest_harnesses", HELPER_MINIMUM),
    gate(RustCi, "non_inputs", HELPER_MINIMUM),
    gate(RustCi, "on_main", HELPER_MINIMUM),
    // First released in tools-v0.1.109 (not tagged yet when this gate was written).
    gate(RustCi, "policy", (0, 1, 109)),
    // First released in tools-v0.1.109 (not tagged yet when this gate was written).
    gate(RustCi, "policy.deny", (0, 1, 109)),
    // First released in tools-v0.1.109 (not tagged yet when this gate was written).
    gate(RustCi, "policy.deny_checks", (0, 1, 109)),
    // helper-gate: first in tools-v0.1.88 via e061b56eac
    gate(RustCi, "pr_ci", (0, 1, 88)),
    // helper-gate: first in tools-v0.1.88 via eb3f4deda4
    gate(RustCi, "private_sources", (0, 1, 88)),
    // helper-gate: first in tools-v0.1.88 via eb3f4deda4
    gate(RustCi, "private_sources.app_id", (0, 1, 88)),
    // helper-gate: first in tools-v0.1.88 via eb3f4deda4
    gate(RustCi, "private_sources.private_key", (0, 1, 88)),
    // helper-gate: first in tools-v0.1.88 via eb3f4deda4
    gate(RustCi, "private_sources.repositories", (0, 1, 88)),
    // First released in tools-v0.1.109 (not tagged yet when this gate was written).
    gate(RustCi, "release_assets", (0, 1, 109)),
    // First released in tools-v0.1.109 (not tagged yet when this gate was written).
    gate(RustCi, "release_assets.asset", (0, 1, 109)),
    // First released in tools-v0.1.109 (not tagged yet when this gate was written).
    gate(RustCi, "release_assets.dest", (0, 1, 109)),
    // First released in tools-v0.1.109 (not tagged yet when this gate was written).
    gate(RustCi, "release_assets.repo", (0, 1, 109)),
    // First released in tools-v0.1.109 (not tagged yet when this gate was written).
    gate(RustCi, "release_assets.sha256", (0, 1, 109)),
    // First released in tools-v0.1.109 (not tagged yet when this gate was written).
    gate(RustCi, "release_assets.tag", (0, 1, 109)),
    gate(RustCi, "required_check", HELPER_MINIMUM),
    // First released in tools-v0.1.109 (not tagged yet when this gate was written).
    gate(RustCi, "root_timeouts", (0, 1, 109)),
    gate(RustCi, "runner", HELPER_MINIMUM),
    gate(RustCi, "shape", HELPER_MINIMUM),
    gate(RustCi, "skip_draft_pull_requests", HELPER_MINIMUM),
    // helper-gate: first in tools-v0.1.92 via b3dda9d553
    gate(RustCi, "sparse_checkout", (0, 1, 92)),
    // helper-gate: first in tools-v0.1.90 via 85f4392434
    gate(RustCi, "test_groups", (0, 1, 90)),
    // helper-gate: first in tools-v0.1.90 via 85f4392434
    gate(RustCi, "test_groups.events", (0, 1, 90)),
    // helper-gate: first in tools-v0.1.90 via 85f4392434
    gate(RustCi, "test_groups.filter", (0, 1, 90)),
    // helper-gate: first in tools-v0.1.99 via 0b991d8ce7
    gate(RustCi, "test_groups.hosted", (0, 1, 99)),
    // helper-gate: first in tools-v0.1.90 via 85f4392434
    gate(RustCi, "test_groups.name", (0, 1, 90)),
    // helper-gate: first in tools-v0.1.90 via 85f4392434
    gate(RustCi, "test_groups.needs", (0, 1, 90)),
    // helper-gate: first in tools-v0.1.107 via fde99af9d9
    gate(RustCi, "test_groups.no_tests", (0, 1, 107)),
    // helper-gate: first in tools-v0.1.90 via 85f4392434
    gate(RustCi, "test_groups.paths", (0, 1, 90)),
    // helper-gate: first in tools-v0.1.107 via ffac3edc63
    gate(RustCi, "test_groups.restore_cache", (0, 1, 107)),
    // helper-gate: first in tools-v0.1.90 via 85f4392434
    gate(RustCi, "test_groups.root", (0, 1, 90)),
    // helper-gate: first in tools-v0.1.90 via 85f4392434
    gate(RustCi, "test_groups.runner", (0, 1, 90)),
    // helper-gate: first in tools-v0.1.90 via 85f4392434
    gate(RustCi, "test_groups.test_threads", (0, 1, 90)),
    // helper-gate: first in tools-v0.1.90 via 85f4392434
    gate(RustCi, "test_groups.timeout_minutes", (0, 1, 90)),
    // helper-gate: first in tools-v0.1.93 via 3099634289
    gate(RustCi, "test_lane", (0, 1, 93)),
    // helper-gate: first in tools-v0.1.107 via ecf0352364
    gate(RustCi, "test_lane.all_features", (0, 1, 107)),
    // helper-gate: first in tools-v0.1.93 via 3099634289
    gate(RustCi, "test_lane.doc", (0, 1, 93)),
    // helper-gate: first in tools-v0.1.93 via 3099634289
    gate(RustCi, "test_lane.features", (0, 1, 93)),
    // helper-gate: first in tools-v0.1.93 via 3099634289
    gate(RustCi, "test_lane.filter", (0, 1, 93)),
    // helper-gate: first in tools-v0.1.107 via 9d5c60385d
    gate(RustCi, "test_lane.job", (0, 1, 107)),
    // helper-gate: first in tools-v0.1.93 via 3099634289
    gate(RustCi, "test_lane.lint", (0, 1, 93)),
    // helper-gate: first in tools-v0.1.107 via ecf0352364
    gate(RustCi, "test_lane.no_default_features", (0, 1, 107)),
    // helper-gate: first in tools-v0.1.93 via 3099634289
    gate(RustCi, "test_lane.packages", (0, 1, 93)),
    // helper-gate: first in tools-v0.1.93 via 3099634289
    gate(RustCi, "test_lane.root", (0, 1, 93)),
    // First released in tools-v0.1.109 (not tagged yet when this gate was written).
    gate(RustCi, "test_lane.timeout_minutes", (0, 1, 109)),
    // helper-gate: first in rust-ci-v0.1.82 via f304690c07
    gate(RustCi, "test_setup", (0, 1, 82)),
    // helper-gate: first in rust-ci-v0.1.82 via f304690c07
    gate(RustCi, "test_setup.name", (0, 1, 82)),
    // helper-gate: first in rust-ci-v0.1.82 via f304690c07
    gate(RustCi, "test_setup.root", (0, 1, 82)),
    // helper-gate: first in rust-ci-v0.1.82 via f304690c07
    gate(RustCi, "test_setup.run", (0, 1, 82)),
    gate(RustCi, "timeout_minutes", HELPER_MINIMUM),
    // helper-gate: first in tools-v0.1.96 via 61067c17fc
    gate(RustCi, "workspace_groups", (0, 1, 96)),
    // helper-gate: first in tools-v0.1.96 via 61067c17fc
    gate(RustCi, "workspace_groups.name", (0, 1, 96)),
    // helper-gate: first in tools-v0.1.96 via 61067c17fc
    gate(RustCi, "workspace_groups.roots", (0, 1, 96)),
];
