# Release coverage map

**Status:** Proposed. Test evidence for `docs/proposed/release-contract.md`
and `docs/proposed/release-config-schema.md`. Test files live next to the
crate they cover; every file stays under the 400-line gate.

## Selection, graph, emission (`velnor-actions-rust`)

| Behavior | Tests |
|---|---|
| Disabled short-circuits without parsing | `impl_rust_release::disabled_release_short_circuits_without_parsing` |
| One-crate root package, default registry | `impl_rust_release::single_root_package_selects_with_default_registry` |
| Virtual multi-member workspace | `impl_rust_release_modes::virtual_workspace_multi_member_selects_explicit` |
| Independent versions unforced without groups | `impl_rust_release_modes::independent_versions_stay_unforced_without_groups` |
| Groups take max, unchanged stay | `impl_rust_release::version_groups_take_max_while_unchanged_stay` |
| Explicit subset ignores CI affected-state | `impl_rust_release::explicit_subset_ignores_ci_affected_state` |
| `publish = false` never publishes | `impl_rust_release::publishable_workspace_skips_private_helpers` |
| Bad/restricted registries fail closed | `impl_rust_release::unsupported_registry_fails_closed_until_supported`, `impl_rust_release_modes::publishable_workspace_fails_on_restricted_registries` |
| Unknown/duplicate/invalid/external names | `impl_rust_release::unknown_duplicate_invalid_and_external_selection_fail`, `impl_rust_release_graph::ambiguous_workspace_names_and_cycles_fail_closed` |
| Path escape / outside root | `impl_rust_release::escaping_and_outside_manifests_fail_closed` |
| Missing/unparseable requirements | `impl_rust_release::unparseable_and_unsatisfied_requirements_fail` |
| Unpublished local dep names the fix | `impl_rust_release::unpublished_local_dep_names_fix_or_uses_registry` |
| Mixed new/published registry state | `impl_rust_release_modes::mixed_registry_state_orders_new_before_published_deps` |
| Dependency-ordered publication | `impl_rust_release_graph::dependency_chain_publishes_leaves_first` |
| Optional/build/target edges constrain order | `impl_rust_release_graph::optional_build_and_target_edges_constrain_order` |
| Dev edges invent no cycles; real cycles fail | `impl_rust_release::dev_edges_cannot_invent_publish_cycles`, `impl_rust_release_graph::ambiguous_workspace_names_and_cycles_fail_closed` |
| Registry/git-only deps validated | `impl_rust_release::registry_deps_need_published_satisfying_versions`, `::git_only_dependencies_are_rejected` |
| Deterministic emission + bootstrap variant | `impl_rust_release::emission_is_deterministic_with_bootstrap_variant` |
| Tag collision vs repair vs released | `impl_rust_release::tag_collision_registry_absence_and_repair_stay_distinct` |

## Config and IR (`velnor-actions-contract`)

`impl_contract_release.rs`: disabled-by-default, unknown-field rejection,
duplicate/unsorted/unsafe selection, contradictory auth modes, bootstrap
mismatch fail-closed, manifest/environment/tag validation, non-lockstep
allowlist-bound groups, stack key paths, determinism, IR permission rules,
dispatch charset/order, schedule and environment safety.

## Coordinator argv (`velnor-actions-mise`)

`impl_mise_release_plz.rs`: exact `0.3.170` pin, full-SHA256 cksum,
release-pr/release argv shapes, phase separation, explicit `--config`,
token/OIDC constructor split, trusted-publishing gate, pinned coordinator
command, policy/freshness mirrors. `impl_mise_release_modes.rs` adds:
dry-run snapshot without token, auth no-blend/no-fallback, single `-p`
semantics, spaced-path argv safety, coordinator payload equality past `--`.

## Workflow rendering (`velnor-actions-workflow-renderer`)

| Behavior | Tests |
|---|---|
| Scalar validators (env, repo, SHA, plan, package, version) | `impl_renderer_release_spec` (5 tests) |
| Dispatch binds approved plan + source SHA | `impl_renderer_release_spec::dispatch_inputs_bind_the_approved_plan_and_source` |
| Exact branches; no PR/fork event shapes | `impl_renderer_release_spec::triggers_pin_exact_branches_without_pr_or_fork_events` |
| Stable lock queues; never cancels | `impl_renderer_release_spec::concurrency_queues_on_a_stable_lock_and_never_cancels` |
| Lock anchors on repository identity | `impl_renderer_release_spec::lock_anchor_requires_the_repository_identity` |
| Publish gate binds repo/plan/source | `impl_renderer_release_spec::publish_gate_binds_repo_plan_and_source_exactly` |
| Effective config validation + golden TOML | `impl_renderer_release_config` (6 tests incl. golden snapshot) |
| Roles, permission matrix, shapes, graph | `impl_renderer_release_jobs` (9 tests) |
| Step gates (secrets, tokens, checkout, config) | `impl_renderer_release_gates` + `impl_renderer_release_publish` (10 tests) |
| Workflow render, files, determinism, cleanup | `impl_renderer_release_tree` (5 tests) |
| Byte-exact workflow snapshot | `impl_renderer_release_snapshot::golden_workflow_snapshot` |

## Cross-cutting semantics

- **SHA vs workflow SHA:** preflight and publishers check out the approved
  `source_sha` exactly (`checkout_without_exact_source` otherwise); the
  publish gate references only the approved plan/source, never
  `github.sha`/`github.ref` (asserted in the gate test).
- **Release-PR head vs merge:** dispatch defaults bind the approved plan
  and source; any rebound input fails `dispatch_plan_mismatch` at
  generation and the gate condition at runtime.
- **Fork/event gates:** the trigger type cannot express PR,
  `pull_request_target`, or `workflow_run` events; branches are exact
  (no globs); the gate fails forks on repository identity.
- **Timeouts, index delay, mid-fail, repeat, queue, cancel:** the
  generator emits the structure CI executes — publishers never cancel
  (`publisher_cancel` rejected), overlapping sets serialize on the
  stable repository-anchored lock (run/version-unique keys rejected),
  every publish needs preflight (eligibility rechecked after the lock),
  reconcile `always()` runs and independently verifies, and identical
  inputs render byte-identical files (determinism tests). The generator
  performs no retries itself.
- **Preview/migration/cleanup:** `generate --output-dir` previews the
  same tree; `release_stale_paths` names the three release-owned paths
  the tree swap removes when release is disabled
  (`stale_paths_cover_the_release_family_for_cleanup`); the bootstrap
  config is a separate single-flag file retired after OIDC handover.

## Known gaps

- Renderer `validate_environment` accepts `../x`-style names (dots and
  slashes allowed, no segment check) while the contract boundary rejects
  them. Input config is still safe; renderer-layer defense is weaker.
- Orchestrator release wiring and CLI dispatch are covered by sibling
  work packages, not here.
