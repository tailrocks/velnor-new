# Testing the generator

| Layer | Proves | Where |
|---|---|---|
| Rule tests | One function: this input gives this output | Next to the code, in each crate |
| Golden tests | A whole sample repository gives exactly the expected files | `crates/velnor-gen/tests/it/golden.rs` |
| Helper tests | `velnor-ci` commands against temporary folders and recorded API responses | `crates/velnor-ci/tests/it/` |
| Real runs | The generated workflow works on GitHub | `velnor-lab` nightly and this repository's CI |

## Rules

- Assert what a user of the generator can observe: the written files, the selection, an error's type and path. Never the wording of a message, the length of a list, or a private structure.
- A test name states the behavior, for example `docs_only_change_selects_nothing`.
- A rule test is a table of inputs and outputs, not one test per case.
- No test starts a program found through `PATH` or touches the network. `.config/nextest.toml` wraps every test binary, when nextest lists it and when it runs it, in `env PATH=/nonexistent`, and `no_programs` fails if `git`, `cargo` or `sh` can still be found through `PATH`. A program named by an absolute path is not blocked. The `io` module is tested by the real runs.
- One behavior is tested at one layer.

### Timeout tiers

Timeouts are set in `.config/nextest.toml` (at the repository root), not in this file. A test is killed after `period` times `terminate-after`. The table lists each override group once, in file order.

| # | Tests | Period | Terminate after | `profile.default` | `profile.ci` |
|---|---|---|---|---|---|
| 1 | velnor-gen, `binary(=it)`: `release_cargo_include::a_cargo_config_include_is_refused_in_every_form_in_a_real_repository`, `release_cargo_include::a_cargo_config_key_off_the_harmless_list_is_refused_in_a_real_repository`, `release_cargo_include::a_cargo_config_of_harmless_keys_is_accepted_in_a_real_repository`, `release_symlink::a_committed_cargo_symlink_hiding_a_credential_provider_is_refused_in_a_real_repository`, `release_ruleset_lock_version::the_ruleset_command_refuses_a_lock_version_of_a_released_member`, `release_nested_toolchain::a_root_toolchain_taken_from_a_path_is_refused_in_a_real_repository`, `golden_env::the_update_switch_leaves_the_expected_files_alone_when_it_is_zero`, `golden_env::the_update_switch_rewrites_the_expected_files_when_it_is_one`, `golden_env::the_update_switch_leaves_the_ruleset_alone_when_it_is_zero`, `golden_env::the_update_switch_rewrites_the_ruleset_when_it_is_one`, `cache_mode_default::no_generated_job_leaves_the_cache_mode_to_the_default` | 10 s | 1 | yes | yes |
| 2 | velnor-gen: `helper_tool_paths::setup_records_non_default_tools_and_rendered_calls_use_them` | 10 s | 1 | yes | yes |
| 3 | velnor-gen, `binary(=it)`: `freshness::generate_rejects_a_build_script_that_is_a_filesystem_symlink`, `io_identity::a_checkout_without_an_origin_keeps_its_schedule_when_renamed`, `io_identity::generate_refuses_a_hand_written_workflow_with_a_forged_marker`, `io_identity::generate_still_removes_a_genuinely_generated_stale_workflow`, `io_identity::local_origin_generates_when_the_schedule_does_not_need_the_identity`, `io_identity::local_origin_is_rejected_when_a_hashed_schedule_needs_the_identity`, `io_identity::repository_case_does_not_change_schedule_identity`, `io_identity::schedules_use_the_remote_repository_and_transport_independent_identity` | 10 s | 1 | yes | yes |
| 4 | velnor-gen, `binary(=it)`: `golden::every_sample_repository_renders_exactly_its_expected_files`, `fork_fence_invariant::every_job_of_every_sample_that_a_pull_request_reaches_is_fenced`, `trim_job::no_job_of_any_sample_that_can_run_on_a_pull_request_or_merge_group_holds_actions_write`, `unused_anchors::no_rendered_sample_workflow_has_an_anchor_nobody_aliases`, `repo_sample::the_recorded_repository_sample_has_a_root_per_workspace_with_the_members_cargo_reports`, `nested_target_dir::no_root_of_any_sample_builds_inside_a_package_directory_from_the_new_helper` | 10 s | 1 | yes | yes |
| 5 | velnor-ci: the `io_*` test modules `io_restore_cargo::`, `io_restore_cold_cargo::`, `io_raw_paths::`, `io_cold_targets::`, `io_shared_targets::`, `io_foreign_targets::`, `io_restored_targets::`, `io_nested_targets::`, `io_snapshot_storage::`, `io_inputs_scope::`, `io_inputs_batch::`, `io_scope_cache::`, `io_scope_cache_races::`, `io_scope_cache_paths::` | 30 s | 1 | yes | no |
| 6 | velnor-ci: `audit::migration::targets::a_cross_target_created_by_cargo_at_the_source_root_does_not_hide_source_changes`, `audit::migration::nextest_discovery_compiles_first_and_executes_the_tests_once` | 30 s | 1 | yes | yes |
| 7 | velnor-ci: `crates_mirror`, with `success-output = "immediate"` and `threads-required = "num-test-threads"` | 30 s | 4 | yes | yes |
| 8 | velnor-ci: `plan_reads_each_roots_sources_to_decide_the_doctest_and_documentation_steps` | 30 s | 4 | yes | yes |
| 9 | velnor-ci: `delta_baseline::` | 30 s | 1 | yes | yes |
| 10 | velnor-ci: `audit::bounds::` | 30 s | 4 | yes | yes |
| 11 | velnor-ci: `audit::freshness::distinct_declared_trees_cannot_exceed_the_audit_scan_budget`, `audit::freshness::repeated_directory_declarations_share_the_audit_scan_budget`, `audit::inventory::directly_named_files_do_not_consume_the_directory_walk_budget`, `io_plan_docs::plan_treats_sources_over_the_total_size_bound_as_possibly_holding_docs`, `release_manifests::an_excluded_example_and_a_nested_workspace_are_not_packages_of_the_root`, `resolve_qualification::every_recorded_case_matches_the_legacy_op` | 30 s | 1 | yes | yes |
| 12 | catch-all for the velnor-ci, velnor-gen, velnor-model, velnor-release, velnor-render and velnor-rust packages | 2 s | 1 | yes | yes |

Row 12 is the last line of `profile.default`. It applies to every test that no earlier row matches. `profile.ci` has no 2 s line of its own, but nextest makes a custom profile inherit the `profile.default` overrides, so row 12 also applies under `profile.ci`. A test that builds a temporary repository and runs real git, such as `ruleset::the_binary_prints_the_golden_ruleset_for_a_repository_that_holds_the_sample`, matches no row and gets the 2 s limit under both profiles.

## Samples

A sample is a folder `samples/<name>/` with `view/` (the files the generator reads), `manifests.txt` (the tracked `Cargo.toml` and `Cargo.lock` paths), `symlinks.txt` (the tracked symbolic links, which may be empty but must be a regular file), `cargo-metadata/<root>.json` (recorded output) and `expected/` (the files the generator must write). A sample without `symlinks.txt` is rejected, and a sample that records a symbolic link where the generator reads a file is rejected too, because a live checkout with that link is refused. A symbolic link at a Rust source path is the exception: the generator does not read it, and the crate that holds it is selected by any change, because the file it points at is unknown.

Record a sample from a real checkout. Never type one by hand.

```sh
cargo run -p velnor-gen -- --root . record-sample <checkout> <name>
```

Add one sample for each behavior a change introduces, not one for each case. After a change of the output, rewrite `expected/` and review its diff in the pull request, because that diff is the review of what users get.

```sh
UPDATE_GOLDENS=1 cargo nextest run
```

The check `generator-samples` lints every `expected/` workflow with `jactionlint`.

The golden tests read the samples from `VELNOR_GOLDEN_SAMPLES_ROOT` when it is set, so a test can run the golden binary against a temporary copy of a sample.
