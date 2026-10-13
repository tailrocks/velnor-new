# Gate 3: generated execution and visible jobs

- State: recorded-pending-merge (branch `docs/velnor-actions-spec`; becomes implemented only on merge with required checks passing)
- Specification: [implementation-plan.md](../proposed/implementation-plan.md) Gate 3 + [task-execution-contract.md](../proposed/task-execution-contract.md) + [cache-contract.md](../proposed/cache-contract.md)
- Landed by: unmerged branch `docs/velnor-actions-spec`, HEAD `f725a87`
- Merge date: TBD
- Delivered: typed task graph with stable IDs; generated workflow with planning/format job, affected-crate matrix (Clippy → test build → tests → doctests as distinct steps), actionlint job, and explicit required final gate; crate-scoped fail-fast (a crate's tests start only after its Clippy succeeds; no global lint barrier); schema-1 `plan.json` + compact `{"include": [...]}` matrix output with stable `id`/`matrix_key` and run-derived `report_id`/`artifact_id`; final aggregation fails on missing, duplicate, malformed, or unexpected reports. Owning crates: `velnor-actions-orchestrator`, `velnor-actions-workflow-renderer`, `velnor-actions-mise` (command construction), `velnor-actions-contract` (IDs/reports).
- Acceptance evidence: local workspace suite green (see Gate 0 record for counts); execution behavior proven in dogfood CI round 5 (`f725a87`), pending — run link to be filled by parent. Key tests: `plan_matrix_agreement`, `task_consumes_matrix_context` (`crates/velnor-actions-orchestrator/tests/impl_matrix.rs`); `clippy_configs_schedule_in_separate_groups`, `single_clippy_config_needs_no_barrier` (`impl_final_orch.rs`); `round_trip_passed_with_counts`, `tampered_reports_rejected`, `empty_plan_merges_no_work` (`impl_merge.rs`); `generated_workflow_has_always_on_lint_job` (`impl_prepare_generate.rs`).
- Deviations: none.
- Follow-up: none for Gate 3 scope.
