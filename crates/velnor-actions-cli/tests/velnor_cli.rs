// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

mod cache_no_explanatory_comments;
#[path = "velnor_cli/cargo_artifact.rs"]
mod cargo_artifact;
#[path = "velnor_cli/freshness_path_dependencies.rs"]
mod freshness_path_dependencies;
mod gc1_t28;
mod gc1_t29_common;
mod gc1_t29_preserve;
mod gc1_t29_reject;
mod gc1_t53;
#[path = "velnor_cli/native_required_order.rs"]
mod native_required_order;
#[path = "velnor_cli/operator_toolchain_guard.rs"]
mod operator_toolchain_guard;
