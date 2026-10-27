// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

mod workflow_configuration_fixture;

#[path = "workflow_configuration_public/branches.rs"]
mod branches;
#[path = "workflow_configuration_public/bun_assets_routing.rs"]
mod bun_assets_routing;
#[path = "workflow_configuration_public/bun_assets_routing_support.rs"]
mod bun_assets_routing_support;
#[path = "workflow_configuration_public/bun_dispositions.rs"]
mod bun_dispositions;
#[path = "workflow_configuration_public/bun_expansion.rs"]
mod bun_expansion;
#[path = "workflow_configuration_public/bun_malformed.rs"]
mod bun_malformed;
#[path = "workflow_configuration_public/bun_planning.rs"]
mod bun_planning;
#[path = "workflow_configuration_public/bun_planning_fixture.rs"]
mod bun_planning_fixture;
#[path = "workflow_configuration_public/bun_prerequisite.rs"]
mod bun_prerequisite;
#[path = "workflow_configuration_public/bun_private_credentials.rs"]
mod bun_private_credentials;
#[path = "workflow_configuration_public/bun_private_credentials_boundaries.rs"]
mod bun_private_credentials_boundaries;
#[path = "workflow_configuration_public/bun_private_credentials_support.rs"]
mod bun_private_credentials_support;
#[path = "workflow_configuration_public/bun_private_profiles.rs"]
mod bun_private_profiles;
#[path = "workflow_configuration_public/bun_private_repository.rs"]
mod bun_private_repository;
#[path = "workflow_configuration_public/bun_private_repository_conditions.rs"]
mod bun_private_repository_conditions;
#[path = "workflow_configuration_public/bun_private_repository_metadata.rs"]
mod bun_private_repository_metadata;
#[path = "workflow_configuration_public/bun_private_repository_tamper.rs"]
mod bun_private_repository_tamper;
#[path = "workflow_configuration_public/bun_request_lanes.rs"]
mod bun_request_lanes;
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
#[path = "workflow_configuration_public/source_qualification.rs"]
mod source_qualification;
#[path = "workflow_configuration_public/support.rs"]
mod support;
#[path = "workflow_configuration_public/timeouts.rs"]
mod timeouts;
#[path = "workflow_configuration_public/verification_tools.rs"]
mod verification_tools;
#[path = "workflow_configuration_public/workspace_format.rs"]
mod workspace_format;
