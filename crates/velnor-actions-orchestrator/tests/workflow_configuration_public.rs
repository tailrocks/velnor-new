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
#[path = "workflow_configuration_public/cache_authority.rs"]
mod cache_authority;
#[path = "workflow_configuration_public/empty_suite.rs"]
mod empty_suite;
#[path = "workflow_configuration_public/feature_toggles.rs"]
mod feature_toggles;
#[path = "workflow_configuration_public/install_profile.rs"]
mod install_profile;
#[path = "workflow_configuration_public/ledger_cli_build_env.rs"]
mod ledger_cli_build_env;
#[path = "workflow_configuration_public/ledger_cli_job.rs"]
mod ledger_cli_job;
#[path = "workflow_configuration_public/ledger_cli_validator.rs"]
mod ledger_cli_validator;
#[path = "workflow_configuration_public/ledger_cli_verify_script.rs"]
mod ledger_cli_verify_script;
#[path = "workflow_configuration_public/maintenance_features.rs"]
mod maintenance_features;
#[path = "workflow_configuration_public/mbx_job_env.rs"]
mod mbx_job_env;
#[path = "workflow_configuration_public/mbx_lineage_format.rs"]
mod mbx_lineage_format;
#[path = "workflow_configuration_public/native_artifacts.rs"]
mod native_artifacts;
#[path = "workflow_configuration_public/native_artifacts_fixture.rs"]
mod native_artifacts_fixture;
#[path = "workflow_configuration_public/native_artifacts_suite_gate.rs"]
mod native_artifacts_suite_gate;
#[path = "workflow_configuration_public/native_artifacts_suites.rs"]
mod native_artifacts_suites;
#[path = "workflow_configuration_public/one_job_per_crate.rs"]
mod one_job_per_crate;
#[path = "workflow_configuration_public/parallel_owners.rs"]
mod parallel_owners;
#[path = "workflow_configuration_public/release_bun_prerequisite.rs"]
mod release_bun_prerequisite;
#[path = "workflow_configuration_public/report_upload.rs"]
mod report_upload;
#[path = "workflow_configuration_public/rust_tree_pin_jobs.rs"]
mod rust_tree_pin_jobs;
#[path = "workflow_configuration_public/rust_tree_pin_rendered.rs"]
mod rust_tree_pin_rendered;
#[path = "workflow_configuration_public/seed_ids.rs"]
mod seed_ids;
#[path = "workflow_configuration_public/seed_off.rs"]
mod seed_off;
#[path = "workflow_configuration_public/source_qualification.rs"]
mod source_qualification;
#[path = "workflow_configuration_public/source_qualification_aggregation.rs"]
mod source_qualification_aggregation;
#[path = "workflow_configuration_public/source_qualification_build.rs"]
mod source_qualification_build;
#[path = "workflow_configuration_public/source_qualification_checkout.rs"]
mod source_qualification_checkout;
#[path = "workflow_configuration_public/source_qualification_jobs.rs"]
mod source_qualification_jobs;
#[path = "workflow_configuration_public/source_qualification_proof.rs"]
mod source_qualification_proof;
#[path = "workflow_configuration_public/support.rs"]
mod support;
#[path = "workflow_configuration_public/timeouts.rs"]
mod timeouts;
#[path = "workflow_configuration_public/tofu_parallel_limit.rs"]
mod tofu_parallel_limit;
#[path = "workflow_configuration_public/tofu_prepare_script.rs"]
mod tofu_prepare_script;
#[path = "workflow_configuration_public/tofu_providers_producer_condition.rs"]
mod tofu_providers_producer_condition;
#[path = "workflow_configuration_public/tofu_providers_producer_timeout.rs"]
mod tofu_providers_producer_timeout;
#[path = "workflow_configuration_public/tofu_providers_producers.rs"]
mod tofu_providers_producers;
#[path = "workflow_configuration_public/tools_cache_forged_outputs.rs"]
mod tools_cache_forged_outputs;
#[path = "workflow_configuration_public/tools_cache_install_run.rs"]
mod tools_cache_install_run;
#[path = "workflow_configuration_public/tools_cache_name_attacks.rs"]
mod tools_cache_name_attacks;
#[path = "workflow_configuration_public/tools_cache_pin_fixture.rs"]
mod tools_cache_pin_fixture;
#[path = "workflow_configuration_public/tools_cache_pin_precedence.rs"]
mod tools_cache_pin_precedence;
#[path = "workflow_configuration_public/tools_cache_symlink_escape.rs"]
mod tools_cache_symlink_escape;
#[path = "workflow_configuration_public/tools_cache_verify_hook.rs"]
mod tools_cache_verify_hook;
#[path = "workflow_configuration_public/tools_producers.rs"]
mod tools_producers;
#[path = "workflow_configuration_public/tools_producers_donor_rust.rs"]
mod tools_producers_donor_rust;
#[path = "workflow_configuration_public/tools_producers_donors.rs"]
mod tools_producers_donors;
#[path = "workflow_configuration_public/tools_producers_mutations.rs"]
mod tools_producers_mutations;
#[path = "workflow_configuration_public/tools_producers_scope.rs"]
mod tools_producers_scope;
#[path = "workflow_configuration_public/verification_tools.rs"]
mod verification_tools;
#[path = "workflow_configuration_public/waste_lint.rs"]
mod waste_lint;
#[path = "workflow_configuration_public/waste_lint_scope.rs"]
mod waste_lint_scope;
#[path = "workflow_configuration_public/waste_lint_verifier.rs"]
mod waste_lint_verifier;
#[path = "workflow_configuration_public/waste_oracle.rs"]
mod waste_oracle;
#[path = "workflow_configuration_public/waste_rules.rs"]
mod waste_rules;
#[path = "workflow_configuration_public/waste_verifier_oracle.rs"]
mod waste_verifier_oracle;
#[path = "workflow_configuration_public/workspace_format.rs"]
mod workspace_format;
