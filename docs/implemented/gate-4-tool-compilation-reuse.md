# Gate 4: tool and compilation reuse

- State: recorded-pending-merge (branch `docs/velnor-actions-spec`; becomes implemented only on merge; required checks green at `bdfffb9` — run `36569723507`, `https://github.com/tailrocks/velnor-new/actions/runs/36569723507`)
- Proof vocabulary: policy-on-source (cache identity/ownership/trust asserted over descriptors) + qualification-of-generator (restore/miss/fallback legs run green in CI)
- Specification: [implementation-plan.md](../proposed/implementation-plan.md) Gate 4 + [cache-contract.md](../proposed/cache-contract.md) + [tooling-input-contract.md](../proposed/tooling-input-contract.md)
- Landed by: unmerged branch `docs/velnor-actions-spec`, HEAD `bdfffb9`
- Merge date: TBD
- Delivered: layered caches in plan order — exact Velnor-pinned Mise tool install, Cargo-source transport, MBX compilation objects — each with one owner and reasoned hit/miss/bypass/invalidation/unavailable reports; cache misses execute the task (never skip checks); MBX hits never satisfy test obligations; untrusted PR caches cannot enter the release path; project tool files stay byte-identical (read-only inspection + human recommendations only). Owning crates: `velnor-actions-mise`, `velnor-actions-orchestrator`, `velnor-actions-workflow-renderer`.
- Acceptance evidence: local workspace suite green (see Gate 0 record for counts); cold/warm/unchanged/source-change qualification ran green in dogfood CI 47/47 at `bdfffb9` (run `36569723507`). Key tests: `restore_verification_orders_evidence_then_outputs`, `miss_reasons_cover_contract_set_exactly`, `fallback_maps_every_error_and_executes` (`crates/velnor-actions-mise/tests/impl_mise_restore.rs`); `descriptor_without_sources_is_not_eligible`, `read_and_verify_artifact_roundtrip` (`impl_mise_cache.rs`); `mbx_emitted_only_for_mbx_driver`, `mbx_gating_rejects_unselected_mbx` (`crates/velnor-actions-workflow-renderer/tests/impl_renderer_mbxgate.rs`); `ver34_tool_files_untouched` (`crates/velnor-actions-cli/tests/impl_repo_freshness.rs`).
- Deviations: same-repo scoped PR rendering remains pending R13 nonce integration.
- Follow-up: task-result caching stayed disabled through this gate per plan (enabled only at Gate 6).
- Cache-save policy (R13): emitted saves remain producer-success- plus push-gated at
  runtime (`success() && github.event_name == 'push'`,
  `contract::workflow::ir::CACHE_SAVE_CONDITION`; step `if:` replaces the
  default `success()`, so the gate restates it). The Mise trusted-save
  authorizer validates this protected-push policy intrinsically; action-level
  PR-save capability does not change trusted-save authorization. Same-repo and
  fork `pull_request` runs are read-only under the current generator policy.
  Same-repo scoped rendering remains pending nonce integration and its
  typed-config, renderer, and save-lifecycle owners. Push runs save into the
  repository that owns the run (GitHub cache scope is per-repo), keeping fork
  pushes confined to the fork. The `Save Cargo sources` step is gated through
  `orchestrator::source_cache`; `Save Mise tools` steps are gated on the elected
  writer per tools key through `renderer::cache_elect`. Every `Setup Mise` step
  stays restore-only (`cache_save: "false"`) because the pinned action saves
  only inside its disabled `install` leg. `Step.condition`
  (`contract::workflow::ir::Step`) serializes through both workflow renderers.
  Tests: `impl_orch_p08.rs:c11_cache_saves_push_only_prs_and_forks_read_only`,
  `impl_renderer_p08.rs:step_conditions_serialize_as_if_with_upload_default`,
  `impl_mise_p08.rs:c9_pr_action_capability_keeps_forks_read_only`,
  `impl_mise_p08.rs:c10b_trusted_save_authorizes_only_the_push_only_gate`.
- V2 tool archive migration: generated Velnor workflows now disable `jdx/mise-action`'s built-in cache and emit one explicit `actions/cache/restore` over the typed tools payload. The payload key binds exact tool selectors, Mise action and binary pins, Rust toolchain/components, target, runner label, and archive paths; a runtime identity step qualifies hosted image and absolute cache roots, with unknown or Scale Set identities taking the cold path. The archive owns Mise installs, Rustup components, Cargo-installed binaries, and their install receipts. The independent Cargo source layer owns only `registry/index`, `registry/cache`, and `git/db`; no path has two cache owners. `cache_elect` adds one save per runtime-qualified key on a protected default-branch push. `Setup Mise` keeps both `cache` and `cache_save` disabled. Tests: renderer V2 payload, runtime identity, source/tool ownership, writer election, strict rendering, and orchestrator emitted-YAML checks. This migration adds no cold-writer/warm-reader acceptance result or performance claim.
- Format scoping (R28): one `Format` step per crate job, never an overlapping plan `fmt --all` — `derive_for_config` suppresses the workspace `Fmt` group when per-package `Fmt` groups exist for the same config (package-less workspaces keep their one distinct plan scope). Test: `impl_wire_w1.rs:w1_plan_format_runs_fmt_check` (total `Format` steps == crate-job count, none in plan).
