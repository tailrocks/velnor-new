// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

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
#[path = "exec_support.rs"]
mod exec_support;
#[path = "expr/functions.rs"]
mod expr_functions;
#[path = "expr/hash_files.rs"]
mod expr_hash_files;
#[path = "expr/hash_files_fs.rs"]
mod expr_hash_files_fs;
#[path = "expr/hash_files_patterns.rs"]
mod expr_hash_files_patterns;
#[path = "expr/indexing.rs"]
mod expr_indexing;
#[path = "expr/limits.rs"]
mod expr_limits;
#[path = "expr/operators.rs"]
mod expr_operators;
#[path = "expr/status.rs"]
mod expr_status;
#[path = "expr/support.rs"]
mod expr_support;
#[path = "expr/template.rs"]
mod expr_template;
#[path = "expr/unsupported.rs"]
mod expr_unsupported;
#[path = "exec_extra.rs"]
mod extra;
#[path = "exec_failures.rs"]
mod failures;
#[path = "exec_followup.rs"]
mod followup;
#[path = "exec_hardening.rs"]
mod hardening;
mod job_message;
#[path = "exec_late.rs"]
mod late;
#[path = "exec_offload.rs"]
mod offload;
#[path = "exec_parity.rs"]
mod parity;
#[path = "exec_process.rs"]
mod process;
mod lifecycle;
mod lifecycle_cancel;
mod lifecycle_logs;
mod lifecycle_renew;
mod lifecycle_report;
mod lifecycle_session;
mod lifecycle_support;
mod lifecycle_token;
mod protocol_broker;
mod protocol_feed;
mod protocol_results;
mod protocol_results_blobs;
mod redaction;
#[path = "exec_review.rs"]
mod review;
mod run_cli;
mod run_cli_shutdown;
#[path = "exec_semantics.rs"]
mod semantics;
#[path = "exec_shells.rs"]
mod shells;
#[path = "exec_stopped.rs"]
mod stopped;
mod support;
#[path = "exec_threads.rs"]
mod threads;
#[path = "exec_triage.rs"]
mod triage;
#[path = "exec_unsettled.rs"]
mod unsettled;
