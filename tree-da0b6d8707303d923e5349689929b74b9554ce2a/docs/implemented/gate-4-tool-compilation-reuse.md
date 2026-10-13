# Gate 4: tool and compilation reuse

- State: recorded-pending-merge (branch `docs/velnor-actions-spec`; becomes implemented only on merge with required checks passing)
- Specification: [implementation-plan.md](../proposed/implementation-plan.md) Gate 4 + [cache-contract.md](../proposed/cache-contract.md) + [tooling-input-contract.md](../proposed/tooling-input-contract.md)
- Landed by: unmerged branch `docs/velnor-actions-spec`, HEAD `f725a87`
- Merge date: TBD
- Delivered: layered caches in plan order — exact Velnor-pinned Mise tool install, Cargo-source transport, MBX compilation objects — each with one owner and reasoned hit/miss/bypass/invalidation/unavailable reports; cache misses execute the task (never skip checks); MBX hits never satisfy test obligations; untrusted PR caches cannot enter the release path; project tool files stay byte-identical (read-only inspection + human recommendations only). Owning crates: `velnor-actions-mise`, `velnor-actions-orchestrator`, `velnor-actions-workflow-renderer`.
- Acceptance evidence: local workspace suite green (see Gate 0 record for counts); cold/warm/unchanged/source-change qualification runs in dogfood CI round 5 (`f725a87`), pending — run link to be filled by parent. Key tests: `restore_verification_orders_evidence_then_outputs`, `miss_reasons_cover_contract_set_exactly`, `fallback_maps_every_error_and_executes` (`crates/velnor-actions-mise/tests/impl_mise_restore.rs`); `descriptor_without_sources_is_not_eligible`, `read_and_verify_artifact_roundtrip` (`impl_mise_cache.rs`); `mbx_emitted_only_for_mbx_driver`, `mbx_gating_rejects_unselected_mbx` (`crates/velnor-actions-workflow-renderer/tests/impl_renderer_mbxgate.rs`); `ver34_tool_files_untouched` (`crates/velnor-actions-cli/tests/impl_repo_freshness.rs`).
- Deviations: none.
- Follow-up: none; task-result caching stayed disabled through this gate per plan (enabled only at Gate 6).
