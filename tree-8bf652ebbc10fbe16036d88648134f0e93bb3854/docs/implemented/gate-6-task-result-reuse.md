# Gate 6: task-result reuse

- State: recorded-pending-merge (branch `docs/velnor-actions-spec`; becomes implemented only on merge; required checks green at `bdfffb9` — run `36569723507`, `https://github.com/tailrocks/velnor-new/actions/runs/36569723507`)
- Proof vocabulary: policy-on-source (qualified-only argv shape, no-remote-cache render asserted) + qualification-of-generator (reuse/invalidation legs run green in CI)
- Specification: [implementation-plan.md](../proposed/implementation-plan.md) Gate 6 + [cache-contract.md](../proposed/cache-contract.md)
- Landed by: unmerged branch `docs/velnor-actions-spec`, HEAD `bdfffb9`
- Merge date: TBD
- Delivered: Mise task-result caching enabled only for explicitly qualified tasks (complete declared inputs, environment, outputs, trust rules, independent invalidation fixtures); exact-input warm results reused, each changed declared input invalidates; undeclared/non-deterministic tasks run normally; unavailable or failed-qualification cache executes the task and reports `cache_unavailable`; MBX compilation hits never satisfy task-result reuse. Owning crates: `velnor-actions-mise`, `velnor-actions-orchestrator`.
- Acceptance evidence: local workspace suite green (see Gate 0 record for counts); reuse/invalidation behavior green in dogfood CI 47/47 at `bdfffb9` (run `36569723507`). Key tests: `qualified_argv_matches_fixed_shape`, `unqualified_tasks_never_reach_run_argv`, `fixture_tokens_require_gate6_shape`, `gated_render_configures_no_remote_cache` (`crates/velnor-actions-mise/tests/impl_mise_gate6.rs`).
- Deviations: none.
- Follow-up: none for Gate 6 scope.
