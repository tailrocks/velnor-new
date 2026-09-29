# Requirements-to-evidence map (Gates 0–8)

Scope: every MUST/MUST NOT + acceptance criterion in `docs/proposed/` (13 contracts),
grouped one row per clause cluster. Terse per RQ §5 (this doc ≤400 lines).
HEAD: branch `docs/velnor-actions-spec` @ `f725a87` (unmerged; CI round 5 pending).
Tree note: evidence ran on clean HEAD; concurrent uncommitted sibling edits in
`crates/velnor-actions-contract/` (mid-edit; its test target currently fails to
compile) arrived after verification and were left untouched.

Legend — crates: CT contract RS rust MI mise AL actionlint RD renderer OR
orchestrator CLI cli. Paths abbreviated: `OR/src/x.rs` =
`crates/velnor-actions-orchestrator/src/x.rs`; tests `OR/tests/f.rs:name`.
Evidence: T = full `cargo test --workspace --locked` green 2026-09-29 (1016
pass/0 fail: 949 integ + 67 unit); F = also individually filter-verified ≥1
`... ok`; C = clippy contract pkg clean; Fmt = fmt clean; Deny = deny ok;
R = file read @HEAD; CI5 = dogfood CI round 5 pending, no run link.

## Gate 0 — repository contract

| Requirement | Gt | Crate | Impl file(s) | Regression test(s) | Evidence |
|---|---|---|---|---|---|
| RQ-1/ARCH-1: exactly 7 `velnor-actions-*` pkgs, virtual ws, explicit members | 0 | CLI | `Cargo.toml` (R) | `CLI/tests/impl_repo_policy.rs:workspace_lists_exactly_seven_members` | T+F |
| RQ-1/ARCH-1/CLI-1: generic names forbidden; `velnor` reserved; CLI owns `velnor-actions` bin | 0 | CLI | `Cargo.toml`, `CLI/src/main.rs` (R) | `CLI/tests/impl_repo_policy.rs:generic_names_forbidden`, `CLI/tests/impl_repo_policy.rs:velnor_name_never_published` | T+F |
| RQ-1/ARCH-1/PLAN-cfg: ownership table + dep direction (adapters→CT only, OR composes, RS↔MI banned, no tool calls in CLI) | 0 | OR | `Cargo.toml`s (R) | `OR/tests/impl_orch_intake.rs:intake_adapter_dependency_direction`, `CLI/tests/impl_repo_deps.rs:dependency_edges_match_ownership_table` | T |
| RQ-1/ARCH-1.10: product roots under `crates/`; fixtures not packages; alint scopes product paths | 0 | CLI | `.alint.yml` (R) | `CLI/tests/impl_repo_shape.rs:arch110_product_roots_live_under_crates`, `CLI/tests/impl_repo_shape.rs:arch112_alint_scopes_product_paths` | T+F |
| RQ-2: edition 2024, resolver 3, MSRV 1.98, inherit ws metadata+lints, lock committed, `--locked` | 0 | — | `Cargo.toml`, `Cargo.lock`, `AGENTS.md` (R) | suite/deny/clippy all run `--locked`; `RD/tests/impl_renderer_msrv.rs:msrv_step_pins_tool_to_rust_version_and_locked` | T+R+Deny |
| RQ-4 MUST-NOT: no custom layout parser; alint generic-only; SHOULD deviations recorded | 0 | CT | `.alint.yml`, `docs/implemented/deviations.md` (R) | `RS/tests/impl_rust_f2b.rs:alint_config_holds_generic_rules_only`, `CLI/tests/impl_repo_deps.rs:test_entries_match_layout_and_stay_far_below_cases`, `:fixtures_stay_independent_and_cover_failures` | T+F; SHOULD→deviations |
| RQ-4: ≥1 test/crate; nextest `--no-tests fail`; doctests separate gate; no self-comparing asserts | 0 | MI/RD | `.config/nextest.toml` (R) | `MI/tests/impl_mise_nextest.rs:run_argv_is_byte_exact_with_no_tests_fail`, `RS/tests/impl_rust_tasks.rs:doctest_stays_separate_in_both_profiles` | T |
| RQ-5: 400/150/80 limits; docs 400; no baseline/ratchet/relabelling | 0 | — | `.alint.yml`, `clippy.toml` (R) | `CLI/tests/impl_repo_shape.rs:rq53_limits_discipline_no_baseline`, `CT/tests/impl_alint_negative.rs:rust_max_lines_fixture_exceeds_400`, `:lib_main_max_lines_fixture_exceeds_150` | T+F |
| RQ-6: ws lint baseline (unsafe forbid, unwrap/expect/panic/todo/dbg deny); clippy per-pkg; fmt baseline | 0 | — | `Cargo.toml`, `clippy.toml`, `rustfmt.toml` (R) | `RS/tests/impl_adapter_wire_rust.rs:clippy_denies_warnings_after_all_targets`, `CLI/tests/impl_repo_policy.rs:clippy_toml_has_five_settings` | T+C+Fmt |
| RQ-7: narrow deps, `[workspace.dependencies]` opt-in; deny+machete in CI; no wildcard/yanked; dup review | 0 | — | `Cargo.toml`, `deny.toml` (R) | `OR/tests/impl_wire_w1.rs:w1_policy_carries_zizmor_after_machete`, `RD/tests/impl_renderer_sweep.rs:policy_renders_deny_machete_zizmor_in_order` | T+Deny |
| RQ-8/PLAN-G0: separate required `velnor-alint` job @ exactly `asamarts/alint@v0.16.1`; consumer-v1 MUST NOT emit/require | 0 | RD | `RD/src/support.rs`, `RD/src/render.rs` | `RD/tests/impl_renderer_tree.rs:velnor_policy_renders_alint_and_policy_only`, `OR/tests/zizmor_staging.rs:velnor_policy_blessed_tag_validates_green`, `:blessed_repo_wrong_tag_still_fails` | T+F; live job CI5 |
| PLAN-G0: negative fixture rejects EACH alint rule; no 2nd structure linter; deny/actionlint/zizmor separate | 0 | CT | `fixtures/alint-negative/*`, `CT/tests/impl_alint_negative.rs` | `CT/tests/impl_alint_negative.rs:required_files_fixture_reports_exactly_one_missing`, `:crates_only_fixture_path_is_rejected` (+2 size rows above) | T; live-binary proof CI5 |
| VER-0/RQ-2: version-policy §0 header + catalog mirror; stable channel; no nightly | 0 | MI | `.velnor/version-policy.toml`, `MI/src/catalog.rs` (R) | `CLI/tests/impl_repo_freshness.rs:boot34_mise_version_matches_catalog`, `CLI/tests/impl_repo_policy.rs:no_nightly_toolchain`, `CT/tests/impl_remed_policy.rs:ver_nightly_record_requires_dated_toolchain` | T+F |
| AGENT accept: repo-policy/toolchain/workspace/tests/limits/lints/deps rows | 0 | — | `AGENTS.md` (R, 25 lines) | covered by rows above (alint, pins, ws-shape, suite, limits, clippy, deny) | T+C+Fmt+Deny |

## Gate 1 — init, inventory, plan

| Requirement | Gt | Crate | Impl file(s) | Regression test(s) | Evidence |
|---|---|---|---|---|---|
| CLI: exactly init/plan/generate; Clap derive; no root/stack/format/check flags; bad usage exit 2 | 1 | CLI | `CLI/src/args.rs` | `CLI/tests/impl_cli_args.rs:unknown_flags_exit_two`, `:extra_positionals_exit_two`, `:bare_invocation_and_unknown_commands_exit_two`, `OR/tests/impl_orch_f2a.rs:cli_carries_no_stack_flags` | T |
| CLI/ARCH: help = stack-generic generator; internal ops env-gated, public tree unaffected; unknown op exit 2 | 1 | CLI | `CLI/src/args.rs`, `CLI/src/dispatch.rs` | `CLI/tests/impl_cli_args.rs:help_describes_stack_generic_generator`, `CLI/tests/impl_cli_gate.rs:internal_op_requires_env_gate_and_keeps_public_tree`, `:public_commands_ignore_internal_env`, `:unknown_op_matches_bare` | T |
| CLI: Git-root discovery from CWD; symlink→canonical; root+CWD in diagnostics | 1 | OR | `OR/src/root.rs` | `OR/tests/impl_orch_intake.rs:intake_symlinked_cwd_resolves_to_canonical_root`, `:intake_not_work_tree_records_calling_cwd` | T |
| CLI/GEN-init: init writes only root `.velnor/config.toml` (schema=1 sample), refuses overwrite/file-block, touches nothing else | 1 | OR | `OR/src/init.rs` | `CLI/tests/impl_cli_init.rs:nested_init_writes_only_root_config`, `:init_writes_no_other_file` | T+F |
| CLI/PLAN: plan = same prep as generate; deterministic text; no YAML/JSON, no writes, no exec; lists stacks/crates/jobs/findings; no cache claims | 1 | OR/CLI | `OR/src/plan.rs`, `CLI/src/dispatch.rs` | `CLI/tests/impl_cli_parity.rs:plan_job_ids_match_generated_workflow`, `:plan_writes_nothing_and_exposes_no_plan_json`, `OR/tests/impl_prepare_generate.rs:prepare_plan_text_is_deterministic_and_write_free` | T+F |
| CLI/ARCH: CLI never invokes Git/Cargo/Mise/MBX/Nextest/gh; OR never builds shell/launches procs | 1 | CLI/OR/MI | `CLI/src/dispatch.rs`, `MI/src/command.rs` | `OR/tests/impl_orch_intake.rs:intake_adapter_dependency_direction`, `RD/tests/impl_renderer_steps.rs:argv_validation_rejects_policy_violations`, `AL/tests/impl_adapter_wire_actionlint.rs:actionlint_symbols_route_to_owning_stacks` | T |
| ARCH-3: config TOML schema=1, unknown→`unknown_config_field`, bad schema→`unsupported_schema`, file+key+problem; no invented defaults | 1 | CT | `CT/src/config/mod.rs` | `CT/tests/impl_remed_contract.rs:arch_unknown_keys_fail_with_unknown_config_field`, `:arch_unknown_schema_fails_with_unsupported_schema`, `:arch_required_schema_has_no_default` | T |
| ARCH-3: policy consumer-v1 default / velnor-repo-v1 + repo identity; consumer ignores velnor files | 1 | CT/OR | `CT/src/config/workflow.rs` | `OR/tests/impl_orch_intake.rs:intake_consumer_default_ignores_velnor_files` | T |
| ARCH-3: runner latest_default vs exact catalog override; default_branch origin/HEAD else fail naming fix; budgets/shards | 1 | CT | `CT/src/config/workflow.rs`, `resources.rs` | `OR/tests/impl_config_internal.rs:default_branch_prefers_config_then_origin_head`, `:branch_failure_hints_default_branch_setting`, `OR/tests/impl_orch_intake.rs:intake_default_budgets_are_conservative_and_reported` | T |
| ARCH-3/PLAN-cfg: ignore sorted/dup-free/applied-after+reason; exclude globs-before; rust-only; reject shell/raw-uses/tasks | 1 | CT/RS | `CT/src/config/stacks.rs`, `discovery.rs`, `RS/src/detect.rs` | `CT/tests/impl_remed_par.rs:par_ignore_sorted_dup_unknown`, `RS/tests/impl_rust_detect.rs:exclusions_apply_before_detection`, `:ignores_apply_after_detection_with_reason`, `OR/tests/impl_prepare_generate.rs:ignored_rust_plans_no_work` | T+F |
| ARCH-4: metadata `--no-deps` exact argv; conservative local graph incl optional/target edges; invalid→fail, no source fallback | 1 | RS/MI | `RS/src/metadata.rs`, `graph.rs` | `RS/tests/impl_rust_detect.rs:discovers_standalone_nested_and_multiple_workspaces_exactly_once`, `CLI/tests/impl_cli_init.rs:malformed_manifest_fails_naming_file`, `OR/tests/impl_orch_intake.rs:intake_malformed_manifest_fails_selected_or_ignored` | T+F |
| ARCH-4: `--locked --offline` qual; missing deps→`preparation_incomplete`, no fetch; lockfile-absent unpinned; cargo_metadata never executes | 1 | OR/MI | `MI/src/requests.rs`, `OR/src/inventory.rs` | `OR/tests/impl_orch_f2d.rs:offline_dependency_aborts_plan`, `OR/tests/impl_orch_f2b.rs:offline_stderr_aborts_preparation`, `OR/tests/impl_prepare_generate.rs:cold_registry_stderr_classifies_incomplete` | T |
| ARCH-4: inventory retains full field set (ids, paths, targets, deps, features, doctests, build scripts) | 1 | RS | `RS/src/metadata.rs`, `index.rs` | `MI/tests/impl_mise_nextest_plan.rs:sorted_inventory_rejects_unsorted_or_duplicate`, `RS/tests/impl_rust.rs:index_builds_sorted_posix_paths` | T |
| TOOL: tool files read-only; findings+recommendations; malformed→`tooling_input_invalid`; plan≈generate | 1 | RS/MI/OR | `RS/src/toolfiles.rs`, `MI/src/toolfiles.rs`, `OR/src/toolfindings.rs` | `OR/tests/impl_orch_intake.rs:intake_tool_inputs_untouched_and_reported`, `:intake_missing_tool_files_recommend_without_writes`, `:intake_tool_changes_refresh_findings_not_tasks` | T |
| ARCH-7/PLAN-G1: each Cargo root once; no-Rust→valid no-work, no validation claim; ignored recorded; exits name file | 1 | RS/OR | `RS/src/detect.rs` | `OR/tests/impl_orch_f2d.rs:undetected_stacks_plan_no_work`, `OR/tests/impl_orch_intake.rs:intake_detection_without_tool_files`, `OR/tests/impl_config_internal.rs:internal_plan_selects_affected_and_validates` | T |

## Gate 2 — deterministic generator

| Requirement | Gt | Crate | Impl file(s) | Regression test(s) | Evidence |
|---|---|---|---|---|---|
| CLI: staging outside repo; atomic dir replace; any failure→old tree byte-identical | 2 | OR | `OR/src/generate.rs`, `prepare.rs` | `OR/tests/impl_prepare_generate.rs:atomic_replace_failure_preserves_old_tree`, `OR/tests/impl_orch_gen.rs:orch_gen_preview_and_in_place_tree_hygiene` | T+F |
| CLI: `--output-dir` fresh/empty/absent, outside repo, unique tmp; identical tree; repo untouched | 2 | OR | `OR/src/generate.rs` | `OR/tests/impl_prepare_generate.rs:generate_preview_matches_in_place_and_preserves_repo`, `:preview_refuses_unsafe_destinations`, `OR/tests/impl_orch_f2c.rs:preview_dirs_are_unique_tmp_roots` | T+F |
| GEN/CLI: exact first-line version marker, no dates, every file | 2 | CT/RD | `CT/src/marker.rs`, `RD/src/marker.rs` | `RD/tests/impl_renderer_tree.rs:marker_is_exact_first_line_without_dates`, `CT/src/marker.rs:prefix_matches_rendered_first_line` | T+F; cited `_sample` name MISSING (0 match), corrected name F |
| GEN: deterministic bytes; stable key order; safe quoting; sorted paths/ids/steps; reject invalid | 2 | RD | `RD/src/render.rs`, `yaml.rs` | `RD/tests/impl_renderer_tree.rs:workflow_render_is_byte_stable`, `RD/tests/impl_renderer_yaml.rs:yaml_render_is_byte_stable`, `:yaml_quotes_only_when_required` | T+F |
| GEN: fixed-argv steps only; no config shell/raw-YAML/arbitrary-uses; unvalidated IR rejected | 2 | RD | `RD/src/steps.rs`, `commands.rs` | `RD/tests/impl_renderer_steps.rs:shell_step_joins_fixed_argv_with_quoting`, `:uses_validation_rejects_moving_refs_and_forbidden_actions`, `RD/tests/impl_renderer_tree_policy.rs:renderer_rejects_unvalidated_steps_inside_ir` | T |
| GEN: full `.github` replace (consumer exactly 2 sorted paths); preview byte-equal; cross-checkout stable | 2 | RD/OR | `RD/src/document.rs` | `RD/tests/impl_renderer_tree.rs:consumer_tree_has_exactly_two_sorted_paths`, `OR/tests/impl_orch_f2d.rs:plans_are_byte_deterministic`, `OR/tests/impl_gen_gates.rs:cross_checkout_determinism` | T+F |
| WF/PLAN-G2: action allowlist, full-SHA pins, no `taiki-e/install-action`, no broad suppressions | 2 | AL/RD | `AL/src/actions.rs`, `RD/src/steps.rs` | `AL/tests/impl_actionlint_actions.rs:forbidden_installer_actions_rejected`, `RD/tests/impl_renderer_steps.rs:checkout_template_pins_action_without_credentials` | T |
| WF: actionlint.yaml rendered by AL (no `-init-config`), deterministic+header; runner bridge until supported | 2 | AL | `AL/src/config.rs` | `AL/tests/impl_actionlint_lint.rs:bridge_label_shape_is_hosted_ubuntu`, `AL/tests/impl_actionlint_caps.rs:bridge_required_until_label_recognized` | T |
| PLAN-G2: staged lint (actionlint/ShellCheck/zizmor) pre-replace; fixtures incl parallel/wait held rejected | 2 | AL/OR | `AL/src/tools.rs`, `OR/tests/zizmor_staging.rs` | `OR/tests/zizmor_staging.rs:velnor_policy_blessed_tag_validates_green`, `:blessed_repo_wrong_tag_still_fails`, `AL/tests/impl_actionlint_caps.rs:native_parallelism_unqualified_by_default` | T+F; live lint CI5 |
| ARCH-5: plan.json schema-1 full shape; sorted; run_key; matrix bytes == GITHUB_OUTPUT | 2 | OR | `OR/src/internal_plan.rs`, `plan.rs` | `OR/tests/impl_orch_core.rs:orch_core_plan_matrix_outputs_byte_identical`, `CT/tests/impl_remed_contract.rs:gen_task_ids_preserved_through_plan_and_matrix`, `RD/tests/impl_renderer_f2close_matrix.rs:plan_outputs_publish_plan_id_run_key_and_matrix` | T |
| ARCH-5: digest grammar (b3 canonical, path-independent; run-key NOT in digests; numeric IDs evidence-only) | 2 | CT | `CT/src/canonical.rs`, `cachekey.rs` | `CT/tests/impl_contract_ids.rs:canonical_json_sorts_keys_and_handles_floats`, `:identities_are_path_independent_and_artifact_id_is_derived_name`, `:digest_helpers_use_b3_prefix` | T |

## Gate 3 — execution, visible jobs

| Requirement | Gt | Crate | Impl file(s) | Regression test(s) | Evidence |
|---|---|---|---|---|---|
| TE: 13 named steps (checkout/Mise-install/MBX-only-for-MBX/sources); no-op steps write reports | 3 | MI/RD | `MI/src/steps.rs`, `RD/src/task_steps.rs` | `RD/tests/impl_renderer_tasksteps.rs:task_steps_render_in_order_with_mixed_modes`, `CLI/tests/impl_cli_protocol.rs:plan_writes_response_and_github_outputs` | T |
| TE: clippy before test-compile; entries parallel; fail-fast false; clippy-fail skips pkg tests; fmt in plan | 3 | OR | `OR/src/schedule.rs`, `clippy_groups.rs` | `OR/tests/impl_final_orch.rs:clippy_configs_schedule_in_separate_groups`, `:single_clippy_config_needs_no_barrier`, `RS/tests/impl_rust_tasks.rs:derives_groups_with_clippy_gates` | T+F |
| WF: exact triggers (PR+push+merge_group ready); default-branch rule; plan runs with no work | 3 | RD | `RD/src/render.rs`, `document.rs` | `RD/tests/impl_renderer_tree_policy.rs:triggers_must_be_exact`, `CLI/tests/impl_cli_protocol.rs:merge_no_work_plan_passes` | T |
| WF: minimal permissions; exact concurrency; single runs-on (latest/override); overrides validated | 3 | RD | `RD/src/render.rs` | `RD/tests/impl_renderer_sweep.rs:permissions_block_is_exactly_read_read`, `RD/tests/impl_renderer_tree_policy.rs:concurrency_and_label_must_be_exact`, `RS/tests/impl_rust_evidence.rs:conflicting_runners_rejected` | T |
| WF: final `Velnor / Required` if:always needs plan+tasks+lint; 256KiB matrix cap→broaden hint; run_key rules | 3 | RD/OR | `RD/src/final_steps.rs` | `RD/tests/impl_renderer_tree_policy.rs:final_gate_keeps_exact_name_and_condition`, `RD/tests/impl_renderer_sweep.rs:final_gate_needs_plan_task_lint_and_support`, `OR/tests/impl_orch_merge.rs:orch_core_matrix_budget_guides_broaden_or_reduce` | T |
| WF: plan uploads plan.json+matrix.json; task consumes exact matrix, no rediscovery | 3 | OR | `OR/src/internal_plan.rs` | `OR/tests/impl_matrix.rs:plan_matrix_agreement`, `:task_consumes_matrix_context` | T+F |
| CACHE: task report JSON shape; no secrets; not_selected reasons; sorted matrix aggregate; final report | 3 | CT/OR | `CT/src/workflow/artifacts.rs`, `OR/src/merge.rs` | `OR/tests/impl_merge.rs:round_trip_passed_with_counts`, `:tampered_reports_rejected`, `:empty_plan_merges_no_work`, `OR/tests/impl_orch_f2b.rs:not_selected_tasks_fold_to_not_run` | T+F |
| CACHE: final downloads exact artifacts; ordered validation incl generator==lock; miss/dup/malformed fail | 3 | OR | `OR/src/merge.rs`, `retrieve_reports.rs` | `OR/tests/impl_orch_f2d.rs:merge_without_reports_is_not_run`, `:cache_miss_cannot_fail_merge`, `OR/tests/impl_orch_broaden.rs:shard_aggregation_rejects_mismatch_extra_and_unproven` | T |
| ARCH/PAR: explicit base/head; local worktree diff; merge candidate; broaden on uncertainty; reverse-deps; global→all | 3 | OR/RS | `OR/src/select_affected.rs`, `RS/src/graph.rs` | `OR/tests/impl_orch_f2d.rs:missing_base_broadens_with_warning`, `:global_config_changes_broaden_explicitly`, `:test_only_changes_still_propagate`, `OR/tests/impl_config_internal.rs:internal_plan_selects_affected_and_validates` | T |
| PAR: typed graph pre-YAML; node fields; edge kinds; no name-inferred independence; deterministic schedule; visible steps | 3 | OR | `OR/src/select.rs`, `select_edges.rs`, `schedule.rs` | `OR/tests/impl_orch_f2d.rs:omitted_tasks_carry_internal_reasons`, `CT/tests/impl_remed_par.rs:par_task_ids_match_obligations`, `OR/tests/impl_orch_merge.rs:orch_core_plan_is_deterministic` | T |

## Gate 4 — tool & compilation reuse

| Requirement | Gt | Crate | Impl file(s) | Regression test(s) | Evidence |
|---|---|---|---|---|---|
| CACHE: identity exact fields; env declared; secrets never; undeclared rejected; strict dup-key JSON | 4 | CT | `CT/src/cachekey.rs`, `strict_json.rs` | `CT/tests/impl_remed_cache.rs:cache_input_digest_covers_dependencies`, `:cache_strict_json_rejects_duplicate_keys`, `:cache_trust_stays_out_of_input_digest` | T |
| CACHE: key shape ≤512B else fail; restore prefixes; SHA-alone-not-identity; lane_id | 4 | CT | `CT/src/cachekey.rs` | `CT/tests/impl_remed_cache.rs:cache_key_shape_and_bound`, `CT/tests/impl_remed_cache_b.rs:cache_commit_sha_alone_is_not_identity`, `CT/tests/impl_remed_cache.rs:cache_entry_records_five_identity_digests` | T |
| CACHE: owned paths; isolated CARGO_HOME; source archive; MBX action owns objects (MBX only); no dual-mechanism | 4 | MI/RD | `MI/src/cache.rs`, `RD/src/cache_steps.rs` | `MI/tests/impl_mise_cache.rs:descriptor_without_sources_is_not_eligible`, `:read_and_verify_artifact_roundtrip`, `RD/tests/impl_renderer_mbxgate.rs:mbx_emitted_only_for_mbx_driver`, `:mbx_gating_rejects_unselected_mbx` | T+F |
| CACHE: trust matrix (PR restore-only, fork RO, release rejects PR archives; no MBX/task archives in release) | 4 | MI | `MI/src/restore.rs`, `verify.rs` | `MI/tests/impl_mise_restore.rs:restore_verification_orders_evidence_then_outputs`, `:miss_reasons_cover_contract_set_exactly`, `:fallback_maps_every_error_and_executes` | T+F |
| CACHE: miss/malformed→discard+execute; miss never fails; save-fail reported; no unchanged-save/prune | 4 | MI/OR | `MI/src/reuse.rs`, `OR/src/merge.rs` | `CT/tests/impl_remed_cache_b.rs:cache_miss_reason_membership_enforced`, `OR/tests/impl_orch_f2c.rs:reuse_qualification_rejects_nondeterminism` | T |
| RQ-3: `.mise-version` exact; bootstrap from lock SHA; Mise installs/selects all later tools | 4 | MI | `MI/src/catalog.rs`, `build.rs` | `CLI/tests/impl_repo_freshness.rs:boot34_mise_version_matches_catalog`, `RD/tests/impl_adapter_wire_renderer.rs:bare_cargo_scan_rejects_unpinned_rust`, `MI/tests/impl_mise_negative.rs:gh_pinned_exec_is_exact` | T+F |
| RQ-3: `mise run/exec` only; no abs-cargo/cargo-install/2nd cache; auto-install off post-prep | 4 | MI | `MI/src/command.rs` | `MI/tests/impl_mise_surface.rs:catalog_pins_ignore_project_selectors`, `OR/tests/impl_orch_f2e.rs:conflicting_tool_pins_recommend` | T |
| RQ-3: preflight proves route per ws (MBX tool/ver/invocation vs exact cargo, no wrapper); per-lane target dirs | 4 | MI | `MI/src/preflight.rs` | `MI/tests/impl_mise_preflight.rs:cargo_proof_pins_exact_toolchain_without_wrapper`, `:mbx_proof_reports_tool_version_and_invocation`, `:unreportable_format_fails_without_guessing` | T |
| TE: profile per ws (driver/runner/evidence); sticky overrides fail closed; RUSTUP_TOOLCHAIN exact; no repo writes | 4 | RS/MI | `RS/src/evidence.rs`, `MI/src/command.rs` | `RS/tests/impl_rust_f2a.rs:adapter_entry_metadata_carries_driver_runner_evidence`, `RS/tests/impl_rust_tasks.rs:carries_driver_runner_and_sorted_features` | T; transient→finding+exit-1 MISSING (no `transient` in crates/) |
| TOOL: tool files byte-identical across generate/replace (verified pre-replace) | 4 | OR | `OR/src/generate.rs` | `CLI/tests/impl_repo_freshness.rs:ver34_tool_files_untouched`, `OR/tests/impl_orch_intake.rs:intake_tool_inputs_untouched_and_reported` | T+F |
| PAR: 6 resource classes; finite budgets; ≤10 bg steps/job; exclusions on shared dirs; cargo never MBX | 4 | OR/MI | `OR/src/schedule.rs` | `CT/tests/impl_remed_par.rs:par_resource_classes_cover_six_kinds`, `OR/tests/impl_orch_intake.rs:intake_default_budgets_are_conservative_and_reported` | T |

## Gate 5 — trusted baseline

| Requirement | Gt | Crate | Impl file(s) | Regression test(s) | Evidence |
|---|---|---|---|---|---|
| CACHE: baseline after protected push final-pass only; exact-base lookup; classify-first; broaden on fail | 5 | OR | `OR/src/cover_baseline.rs`, `cover.rs` | `OR/tests/impl_gates_cover.rs:valid_manifest_covers_exact_obligations`, `:wrong_base_manifest_schedules_everything`, `:malformed_manifest_is_a_miss_not_a_failure` | T+F |
| CACHE: tamper→execute+miss warning; PR publish forbidden; covered-claims need manifest; merge_group≈PR | 5 | OR | `OR/src/cover.rs` | `OR/tests/impl_gates_cover.rs:tampered_task_entry_executes_with_miss_warning`, `:pr_plan_records_publish_forbidden`, `:merge_rejects_covered_claims_without_manifest`, `:merge_group_narrows_like_pull_request` | T+F |
| PAR: manifest binds repo/commit/workflow/run/final/gen/compat/proofs; carry-forward keeps orig run; never newest-as-base | 5 | CT/OR | `CT/src/workflow/baseline.rs`, `OR/src/cover_identity.rs` | `OR/tests/impl_wire_w2.rs:baseline_rejects_other_branch_manifests`, `OR/tests/impl_orch_f2b.rs:exact_base_run_filter_pins_provenance`, `OR/tests/impl_select.rs:corrupt_base_manifest_tags_comparison_unavailable` | T |
| PAR: pinned gh via Mise; exact artifact id; malformed inputs rejected | 5 | MI | `MI/src/gh.rs` | `MI/tests/impl_mise_baseline.rs:baseline_download_args_name_exact_artifact`, `:baseline_argv_runs_pinned_gh`, `:baseline_lookup_rejects_malformed_inputs` | T+F |

## Gate 6 — task-result reuse

| Requirement | Gt | Crate | Impl file(s) | Regression test(s) | Evidence |
|---|---|---|---|---|---|
| CACHE: qualified-only fixed argv; unqualified never reach run; gate tokens exact shape | 6 | MI | `MI/src/gate6.rs` | `MI/tests/impl_mise_gate6.rs:qualified_argv_matches_fixed_shape`, `:unqualified_tasks_never_reach_run_argv`, `:fixture_tokens_require_gate6_shape` | T+F |
| CACHE: no remote_url/namespaces/tokens/OIDC; tmp task files w/ marker; per-input invalidate; unavailable→execute+`cache_unavailable` | 6 | MI | `MI/src/gate6.rs`, `template.rs` | `MI/tests/impl_mise_gate6.rs:gated_render_configures_no_remote_cache`, `MI/tests/impl_mise_reuse.rs:qualification_rejects_nondeterministic_and_undeclared_state`, `MI/tests/impl_mise_cache_gates.rs:reuse_qualification_rejects_nondeterministic_tasks` | T+F |
| CACHE: MBX hits never satisfy task reuse; undeclared/nondet run normally | 6 | MI/OR | `MI/src/reuse.rs` | `RS/tests/impl_adapter_wire_rust.rs:rust_payloads_declare_no_nondeterminism`, `OR/tests/impl_orch_f2c.rs:reuse_qualification_rejects_nondeterminism` | T |

## Gate 7 — parallel fan-out

| Requirement | Gt | Crate | Impl file(s) | Regression test(s) | Evidence |
|---|---|---|---|---|---|
| PAR/TE: nextest-only archives once per pkg/config; small suites single step; shard on measured timing; cargo-test no archive | 7 | MI/OR | `MI/src/nextest.rs`, `nextest_plan.rs`, `OR/src/shard.rs` | `MI/tests/impl_mise_nextest.rs:archive_argv_is_byte_exact_per_driver`, `:partitions_never_carry_compile_inputs`, `MI/tests/impl_adapter_wire_mise.rs:single_shard_skips_archive_write_and_transfer` | T+F |
| PAR: merge exact coverage (union==selected, intersections empty); missing/dup/tampered fail; empty-partition proofs; budgets revalidated | 7 | OR | `OR/src/shard.rs`, `merge.rs` | `OR/tests/impl_gates_shard.rs:sharded_merge_passes_with_exact_proofs`, `:duplicate_missing_and_tampered_shards_fail`, `:empty_partition_needs_inventory_proof`, `:archive_without_shard_proofs_never_passes`, `:limits_and_reference_revalidate_at_merge` | T+F |
| PAR: max_parallel==strategy; per-manifest ≤ test budget; hash partitioning; weights never omit; retries zero | 7 | OR/RD | `OR/src/schedule.rs`, `RD/src/matrix.rs` | `OR/tests/impl_matrix.rs:max_parallel_honored` + `RD/tests/impl_renderer_matrix.rs:max_parallel_honored`, `CT/tests/impl_remed_par.rs:par_shards_capped_by_test_budget`, `:par_shard_changes_require_timing_evidence` | T+F |
| PAR: native parallel/background/wait banned until qualified actionlint; no shell `&`/wait; per-lane target isolation | 7 | OR/RD | `RD/src/matrix.rs` | `AL/tests/impl_actionlint_caps.rs:native_parallelism_unqualified_by_default`, `AL/tests/impl_adapter_wire_actionlint.rs:native_parallelism_needs_every_concern`, `OR/tests/impl_orch_intake.rs:intake_generated_workflow_uses_matrix_not_native_parallel` | T |
| TE: nextest only on explicit usage; archive identity full fields; empty inventory fails unless metadata proves none | 7 | RS/MI | `RS/src/tasks.rs`, `MI/src/nextest_plan.rs` | `RS/tests/impl_rust_gates.rs:only_nextest_shards`, `OR/tests/impl_wire_w2.rs:cargo_test_shards_reject_config`, `:nextest_shards_expand_plan_and_verify`, `MI/tests/impl_mise_nextest_plan.rs:archive_plan_runs_once_per_package_config` | T |
| WF: entries carry detected profile; no invented nextest; doctest separate; no `--all-features`; x-pkg explicit; empty test visible | 7 | RD | `RD/src/matrix.rs` | `RS/tests/impl_rust_argv.rs:payloads_never_emit_all_features`, `OR/tests/impl_gates_shard.rs:proven_no_target_shard_passes_without_tests`, `RS/tests/impl_rust_tasks.rs:doctest_stays_separate_in_both_profiles` | T |

## Gate 8 — dogfooding, bootstrap, release

| Requirement | Gt | Crate | Impl file(s) | Regression test(s) | Evidence |
|---|---|---|---|---|---|
| BOOT: init+generate usable; consumer self-contained (no policy/lock); no floating `latest` | 8 | OR | `OR/src/generate.rs` | `MI/tests/impl_mise_catalog.rs:exact_version_validation_accepts_only_pins`, `OR/tests/zizmor_staging.rs:different_unpinned_tag_still_fails` | T |
| BOOT: release = per-target immutable assets + versioned manifest; own-version validated; never unverified URL | 8 | CT/MI | `CT/src/candidate_manifest.rs` | `CT/tests/impl_contract_targets.rs:candidate_manifest_validates_all_fields`, `RD/tests/impl_renderer_f2close.rs:candidate_verify_script_checks_live_manifest`, `:candidate_job_verifies_manifest_before_running_binary` | T |
| BOOT: lock one record/target == catalog; consumer never reads lock; `.mise-version`==lock Mise | 8 | MI/OR | `MI/src/catalog.rs` | `OR/tests/impl_gate8_e.rs:candidate_never_plans_and_lock_matches_catalog_per_target` + `RD/tests/impl_renderer_tree_policy.rs:candidate_never_plans_and_lock_matches_catalog_per_target`, `CLI/tests/impl_repo_freshness.rs:boot34_mise_version_matches_catalog` | T+F; `.velnor/generator.lock` file MISSING (BOOT-3.4 NEEDS-HUMAN) |
| BOOT/PLAN: bootstrap plans graph, never waits current-src binary; candidate artifact-only qual; never plans matrix | 8 | OR | `OR/src/qualify.rs`, `pins.rs` | `OR/tests/impl_orch_gen.rs:orch_gen_candidate_qualify_is_artifact_only`, `OR/tests/impl_e2e_wiring.rs:emitted_yaml_preseed_builds_once_and_shares_artifact` | T+F |
| BOOT: fixed build vector via catalog pins; manifest+SHA recorded; downstream never rebuilds; golden oracle, real digests | 8 | MI/RD | `MI/src/build.rs`, `RD/src/preseed.rs` | `MI/tests/impl_mise_negative.rs:candidate_build_vector_pins_implemented_trio_form`, `RD/tests/impl_renderer_preseed.rs:preseed_templates_carry_trust_mark_and_exact_artifact`, `:strict_preseed_closure_rejects_gaps` | T+F |
| BOOT: seed v0 (2 admin approvals + repro rebuild); protected release job; separate lock update; no PR promotion | 8 | — | — | MISSING — NEEDS-HUMAN per `release-gates.md` BOOT-4.2/4.7/2.1 | MISSING |
| PLAN-8: plan→matrix→reports→final wiring; artifact IDs; candidate evidence validated; PR/push/merge_group; final never skipped | 8 | RD/OR | `RD/src/candidate.rs`, `final_steps.rs` | `OR/tests/impl_gate8_acquire.rs:consumer_plan_job_carries_acquire_step`, `:version_policy_drift_fails_velnor_generate`, `OR/tests/impl_wire_w2.rs:empty_matrix_folds_conclusions`, `RD/tests/impl_renderer_gate8.rs:artifact_steps_pin_actions_and_reject_empty` | T+F |
| VER: newest-stable updates; exceptions ≤14d + expiry fail; security same-day; check script gates release | 8 | — | `scripts/check-freshness.sh`, `docs/implemented/update-procedure.md` (R) | `MI/tests/impl_adapter_wire_mise.rs:freshness_requirements_enforced`, `RD/tests/impl_renderer_planclose.rs:freshness_step_shape_exact` | T; human-run/release-gate halves NEEDS-HUMAN (VER-1.5/1.7/2.18/3.x/4.4) |
| VER: runner inventory latest-default + exact supported; reject `*-latest`/absent; default==latest family | 8 | CT | `CT/src/config/workflow.rs`, `targets.rs` | `RS/tests/impl_rust_evidence.rs:conflicting_runners_rejected`, `RD/tests/impl_renderer_tree_policy.rs:concurrency_and_label_must_be_exact` | T |
| VER: exact tool pins incl scheduled checks; exact direct deps + locked transitive; Renovate majors; git-deps approved-only | 8 | MI | `MI/src/catalog.rs`, `deny.toml` (R) | `MI/tests/impl_adapter_wire_mise.rs:freshness_requirements_enforced`, `OR/tests/impl_orch_f2c.rs:extension_identity_tracks_lock_digest` | T+Deny; updater run NEEDS-HUMAN |
| RQ-9: per-pkg clippy/test/doctest/doc/msrv tasks; one matrix entry per crate; no ws-wide default; SHA+least-priv | 8 | MI/RD | `MI/src/steps.rs`, `RD/src/msrv.rs` | `RS/tests/impl_rust_tasks.rs:clippy_names_exactly_one_package`, `RS/tests/impl_adapter_wire_rust.rs:doc_gated_by_doctest_and_clippy`, `RD/tests/impl_renderer_msrv.rs:msrv_job_is_per_crate`, `:pr_render_rejects_msrv_steps`, `OR/tests/impl_wire_w1.rs:w1_pr_workflows_carry_no_msrv` | T |
| RQ-9: risk-triggered mutants/fuzz/Miri/Loom/semver; coverage≠evidence; retries need reason | 8 | — | `docs/implemented/verification-triggers.md` (R) | MISSING in suite (manual-trigger procedure; no proptest/mutants/semver tests by design) | MISSING (procedure only) |
| AGENT: 7 perf cases on named hw; budgets (2s preflight, warm-leaf seconds, 2min warm); queue split; no auto-baseline | 8 | — | — | MISSING — unmeasured per `README.md` (`OR/tests/impl_final_orch.rs:critical_path_reports_task_durations` exists, no budgets/timings) | MISSING |
| AGENT: readiness = all rows green from clean checkout + negative suite rejects each bypass | 8 | — | — | MISSING — branch unmerged, CI5 pending, seed/human gates open | MISSING |
| AGENT: AGENTS.md short, points at contracts, requires regression tests, forbids suppression/deletion/hand-edits | 8 | — | `AGENTS.md` (R) | read @HEAD: short + commands + gate rules + spec pointers; suite green upholds test rule | T+R (partial: no literal delete-tests line) |
| PLAN: gates in order; proofs labelled policy-on-source vs qualification-of-generator; record-in-same-PR rule | 8 | — | `docs/implemented/gate-*.md` (R) | gate records 0–8 present on branch; proof-vocabulary labels MISSING in records | partial: labels MISSING |

## SHOULD deviations (per `deviations.md`)

RQ-4.1 test layout → `tests/impl_*.rs` + single `[[test]]` entry (accepted).
RQ-4.3 entry naming → `tests/velnor_<crate>.rs` (accepted; orchestrator honestly
carries 2nd hermetic target). RQ-4.7 proptest → hand-written boundary fixtures
until first risk trigger (revisit at V1 code-complete). RQ-2.12 `mise.lock`
absent = specified state, no deviation. VER-0.1 header-only file = residual gap,
not a deviation.

## MISSING summary (7)

1. Cited `prefix_matches_rendered_first_line_sample`: 0 matches (stale name;
   real `prefix_matches_rendered_first_line` F-verified). 2. TE transient-profile
   finding + generate exit 1 (no `transient` in `crates/`). 3. `.velnor/generator.
   lock` + seed/release/protection (BOOT NEEDS-HUMAN). 4. Release-gate wiring +
   human update/security runs (VER NEEDS-HUMAN). 5. Risk-triggered tool runs
   (manual procedure only). 6. Perf budgets/measurements. 7. CI round-5 run links
   + proof-vocabulary labels.
