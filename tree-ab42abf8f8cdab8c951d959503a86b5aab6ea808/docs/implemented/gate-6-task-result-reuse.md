# Gate 6: task-result reuse

- State: recorded-pending-merge (branch `docs/velnor-actions-spec`; becomes implemented only on merge with required checks passing)
- Specification: [implementation-plan.md](../proposed/implementation-plan.md) Gate 6 + [cache-contract.md](../proposed/cache-contract.md)
- Landed by: unmerged branch `docs/velnor-actions-spec`, HEAD `f725a87`
- Merge date: TBD
- Delivered: Mise task-result caching enabled only for explicitly qualified tasks (complete declared inputs, environment, outputs, trust rules, independent invalidation fixtures); exact-input warm results reused, each changed declared input invalidates; undeclared/non-deterministic tasks run normally; unavailable or failed-qualification cache executes the task and reports `cache_unavailable`; MBX compilation hits never satisfy task-result reuse. Owning crates: `velnor-actions-mise`, `velnor-actions-orchestrator`.
- Acceptance evidence: local workspace suite green (see Gate 0 record for counts); reuse/invalidation behavior proven in dogfood CI round 5 (`f725a87`), pending — run link to be filled by parent. Key tests: `qualified_argv_matches_fixed_shape`, `unqualified_tasks_never_reach_run_argv`, `fixture_tokens_require_gate6_shape`, `gated_render_configures_no_remote_cache` (`crates/velnor-actions-mise/tests/impl_mise_gate6.rs`).
- Deviations: none.
- Follow-up: none for Gate 6 scope.
