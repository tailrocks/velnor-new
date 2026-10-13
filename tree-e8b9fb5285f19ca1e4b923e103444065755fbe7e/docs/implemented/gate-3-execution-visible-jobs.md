# Gate 3: generated execution and visible jobs

- State: recorded-pending-merge (branch `docs/velnor-actions-spec`; becomes implemented only on merge; required checks green on HEAD — run `36569723507`, `https://github.com/tailrocks/velnor-new/actions/runs/36569723507`)
- Proof vocabulary: qualification-of-generator (matrix/merge semantics proven by the green dogfood run's real reports) + policy-on-source (schedule/merge unit fixtures)
- Specification: [implementation-plan.md](../proposed/implementation-plan.md) Gate 3 + [task-execution-contract.md](../proposed/task-execution-contract.md) + [cache-contract.md](../proposed/cache-contract.md)
- Landed by: unmerged branch `docs/velnor-actions-spec`, HEAD `bdfffb9`
- Merge date: TBD
- Delivered: typed task graph with stable IDs; generated workflow with planning/format job, affected-crate matrix (Clippy → test build → tests → doctests as distinct steps), actionlint job, and explicit required final gate; crate-scoped fail-fast (a crate's tests start only after its Clippy succeeds; no global lint barrier); schema-1 `plan.json` + compact `{"include": [...]}` matrix output with stable `id`/`matrix_key` and run-derived `report_id`/`artifact_id`; final aggregation fails on missing, duplicate, malformed, or unexpected reports. Owning crates: `velnor-actions-orchestrator`, `velnor-actions-workflow-renderer`, `velnor-actions-mise` (command construction), `velnor-actions-contract` (IDs/reports).
- Acceptance evidence: local workspace suite green (see Gate 0 record for counts); execution behavior green in dogfood CI 47/47 on HEAD (run `36569723507`). Key tests: `plan_matrix_agreement`, `task_consumes_matrix_context` (`crates/velnor-actions-orchestrator/tests/impl_matrix.rs`); `clippy_configs_schedule_in_separate_groups`, `single_clippy_config_needs_no_barrier` (`impl_final_orch.rs`); `round_trip_passed_with_counts`, `tampered_reports_rejected`, `empty_plan_merges_no_work` (`impl_merge.rs`); `generated_workflow_has_always_on_lint_job` (`impl_prepare_generate.rs`).
- Deviations: none.
- Follow-up: none for Gate 3 scope.
