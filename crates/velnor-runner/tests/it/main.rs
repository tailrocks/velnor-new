// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

mod artifact_support;
#[path = "exec_cancel.rs"]
mod cancel;
#[path = "exec_commands.rs"]
mod commands;
#[path = "exec_conditions.rs"]
mod conditions;
#[path = "exec_defaults.rs"]
mod defaults;
#[path = "exec_drain.rs"]
mod drain;
#[path = "exec_builder.rs"]
mod exec_builder;
#[path = "exec_cli.rs"]
mod exec_cli;
#[path = "exec_declared_inputs.rs"]
mod exec_declared_inputs;
#[path = "exec_support.rs"]
mod exec_support;
#[path = "exec_tool_cache.rs"]
mod exec_tool_cache;
#[cfg(target_os = "linux")]
#[path = "expr/format_allocation.rs"]
mod expr_format_allocation;
#[path = "expr/from_json.rs"]
mod expr_from_json;
#[path = "expr/functions.rs"]
mod expr_functions;
#[path = "expr/generated_workflow.rs"]
mod expr_generated_workflow;
#[path = "expr/hash_files.rs"]
mod expr_hash_files;
#[path = "expr/hash_files_depth.rs"]
mod expr_hash_files_depth;
#[path = "expr/hash_files_fs.rs"]
mod expr_hash_files_fs;
#[path = "expr/hash_files_length.rs"]
mod expr_hash_files_length;
#[path = "expr/hash_files_minimatch.rs"]
mod expr_hash_files_minimatch;
#[path = "expr/hash_files_non_utf8.rs"]
mod expr_hash_files_non_utf8;
#[path = "expr/hash_files_patterns.rs"]
mod expr_hash_files_patterns;
#[path = "expr/hash_files_race.rs"]
mod expr_hash_files_race;
#[path = "expr/hash_files_regex_limits.rs"]
mod expr_hash_files_regex_limits;
#[path = "expr/hash_files_relative.rs"]
mod expr_hash_files_relative;
#[path = "expr/hash_files_special.rs"]
mod expr_hash_files_special;
#[path = "expr/hash_files_unicode.rs"]
mod expr_hash_files_unicode;
#[path = "expr/indexing.rs"]
mod expr_indexing;
#[path = "expr/limits.rs"]
mod expr_limits;
#[path = "expr/memory.rs"]
mod expr_memory;
#[path = "expr/memory_json.rs"]
mod expr_memory_json;
#[path = "expr/minimatch_corpus.rs"]
mod expr_minimatch_corpus;
#[path = "expr/operators.rs"]
mod expr_operators;
#[path = "expr/ordinal_case.rs"]
mod expr_ordinal_case;
#[path = "expr/status.rs"]
mod expr_status;
#[path = "expr/step_state.rs"]
mod expr_step_state;
#[path = "expr/support.rs"]
mod expr_support;
#[path = "expr/template.rs"]
mod expr_template;
#[path = "expr/unsupported.rs"]
mod expr_unsupported;
#[path = "expr/vars.rs"]
mod expr_vars;
#[path = "exec_extra.rs"]
mod extra;
#[path = "exec_failures.rs"]
mod failures;
mod fake_results;
#[path = "exec_followup.rs"]
mod followup;
#[path = "exec_hardening.rs"]
mod hardening;
mod job_message;
#[path = "exec_late.rs"]
mod late;
mod lifecycle;
mod lifecycle_abandon;
mod lifecycle_busy;
mod lifecycle_cancel;
mod lifecycle_cancel_timeout;
mod lifecycle_logs;
mod lifecycle_refresh;
mod lifecycle_renew;
mod lifecycle_report;
mod lifecycle_session;
mod lifecycle_session_create;
mod lifecycle_support;
mod lifecycle_timespan;
mod lifecycle_token;
mod lifecycle_token_retry;
mod native_artifact;
mod native_artifact_admission;
mod native_artifact_blobs;
mod native_artifact_cases;
mod native_artifact_disposition;
mod native_artifact_fs;
mod native_artifact_inputs;
mod native_artifact_limits;
mod native_artifact_patterns;
mod native_artifact_retry;
mod native_artifact_review;
mod native_artifact_search;
mod native_artifact_skips;
mod native_artifact_stall;
mod native_artifact_texts;
mod native_artifact_toolkit;
mod native_artifact_upstream;
mod native_artifact_zone_icu;
mod native_artifact_zone_names;
mod native_artifact_zones;
mod native_cache;
mod native_cache_access;
mod native_cache_alias;
mod native_cache_expressions;
mod native_cache_fidelity;
mod native_cache_ids;
mod native_cache_inputs;
mod native_cache_job;
mod native_cache_links;
mod native_cache_modes;
mod native_cache_readers;
mod native_cache_support;
mod native_cache_tar;
mod native_checkout;
mod native_checkout_attack;
mod native_checkout_cancel;
mod native_checkout_contend;
mod native_checkout_depth;
mod native_checkout_env;
mod native_checkout_expression;
mod native_checkout_facts;
mod native_checkout_inputs;
mod native_checkout_layout;
mod native_checkout_links;
mod native_checkout_marker;
mod native_checkout_mirror;
mod native_checkout_mirror_files;
mod native_checkout_mirror_owned;
mod native_checkout_mirror_refs;
mod native_checkout_packed;
mod native_checkout_preflight;
mod native_checkout_quarantine;
mod native_checkout_refetch;
mod native_checkout_repair;
mod native_checkout_sink;
mod native_checkout_torn;
mod native_checkout_unsound;
mod native_checkout_upkeep;
mod native_checkout_walk;
mod native_checkout_window;
mod native_checkout_workspace;
#[path = "exec_offload.rs"]
mod offload;
#[path = "exec_parity.rs"]
mod parity;
#[path = "exec_process.rs"]
mod process;
mod protocol_acquire_renew;
mod protocol_broker;
mod protocol_broker_refusals;
mod protocol_complete;
mod protocol_complete_ack;
mod protocol_feed;
mod protocol_http_guard;
mod protocol_oauth;
mod protocol_results;
mod protocol_results_blobs;
mod redaction;
#[path = "exec_review.rs"]
mod review;
mod run_cli;
mod run_cli_dirs;
mod run_cli_jit;
mod run_cli_memory;
mod run_cli_shutdown;
#[path = "exec_semantics.rs"]
mod semantics;
#[path = "exec_shells.rs"]
mod shells;
#[path = "exec_stopped.rs"]
mod stopped;
#[path = "exec_strict.rs"]
mod strict;
mod support;
#[path = "exec_threads.rs"]
mod threads;
#[path = "exec_tracking.rs"]
mod tracking;
#[path = "exec_triage.rs"]
mod triage;
#[path = "exec_unsettled.rs"]
mod unsettled;
mod zip_forgery;
