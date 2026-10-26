// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

mod workflow_configuration_fixture;

#[path = "workflow_configuration_public/branches.rs"]
mod branches;
#[path = "workflow_configuration_public/bun_assets_routing.rs"]
mod bun_assets_routing;
#[path = "workflow_configuration_public/bun_expansion.rs"]
mod bun_expansion;
#[path = "workflow_configuration_public/bun_malformed.rs"]
mod bun_malformed;
#[path = "workflow_configuration_public/bun_planning.rs"]
mod bun_planning;
#[path = "workflow_configuration_public/bun_planning_fixture.rs"]
mod bun_planning_fixture;
#[path = "workflow_configuration_public/bun_routing.rs"]
mod bun_routing;
#[path = "workflow_configuration_public/bun_runtime.rs"]
mod bun_runtime;
#[path = "workflow_configuration_public/empty_suite.rs"]
mod empty_suite;
#[path = "workflow_configuration_public/feature_toggles.rs"]
mod feature_toggles;
#[path = "workflow_configuration_public/maintenance_features.rs"]
mod maintenance_features;
#[path = "workflow_configuration_public/parallel_owners.rs"]
mod parallel_owners;
#[path = "workflow_configuration_public/report_upload.rs"]
mod report_upload;
#[path = "workflow_configuration_public/seed_ids.rs"]
mod seed_ids;
#[path = "workflow_configuration_public/support.rs"]
mod support;
#[path = "workflow_configuration_public/timeouts.rs"]
mod timeouts;
#[path = "workflow_configuration_public/verification_tools.rs"]
mod verification_tools;
#[path = "workflow_configuration_public/workspace_format.rs"]
mod workspace_format;
