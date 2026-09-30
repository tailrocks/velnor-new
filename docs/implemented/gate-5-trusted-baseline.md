# Gate 5: trusted baseline coverage

- State: recorded-pending-merge (branch `docs/velnor-actions-spec`; becomes implemented only on merge; required checks green at `bdfffb9` — run `36569723507`, `https://github.com/tailrocks/velnor-new/actions/runs/36569723507`)
- Proof vocabulary: policy-on-source (baseline manifest validation, exact-base lookup argv, publish-forbidden rules) + qualification-of-generator (cover/narrow legs run green in CI)
- Specification: [implementation-plan.md](../proposed/implementation-plan.md) Gate 5 + [cache-contract.md](../proposed/cache-contract.md) + [parallelism-and-selection-contract.md](../proposed/parallelism-and-selection-contract.md)
- Landed by: unmerged branch `docs/velnor-actions-spec`, HEAD `bdfffb9`
- Merge date: TBD
- Delivered: evidence-only baseline artifact published after a protected default-branch push passes the final gate; exact-base-commit lookup for PR/merge-group/subsequent-push; planner classifies all obligations before building the execute-only matrix; exact successful base proof covers only matching task/input identities; missing/untrusted/malformed baselines broaden execution; PRs cannot publish trusted evidence; qualified forward dependency propagation, carry-forward, expiry, malformed artifacts, and final coverage aggregation. Owning crates: `velnor-actions-orchestrator`, `velnor-actions-mise` (pinned `gh` fetch argv).
- Acceptance evidence: local workspace suite green (see Gate 0 record for counts); baseline publish/lookup behavior green in dogfood CI 47/47 at `bdfffb9` (run `36569723507`). Key tests: `valid_manifest_covers_exact_obligations`, `wrong_base_manifest_schedules_everything`, `malformed_manifest_is_a_miss_not_a_failure`, `tampered_task_entry_executes_with_miss_warning`, `pr_plan_records_publish_forbidden`, `merge_rejects_covered_claims_without_manifest`, `merge_group_classifies_like_pull_request` (`crates/velnor-actions-orchestrator/tests/impl_gates_cover.rs`); `baseline_download_args_name_exact_artifact`, `baseline_argv_runs_pinned_gh`, `baseline_lookup_rejects_malformed_inputs` (`crates/velnor-actions-mise/tests/impl_mise_baseline.rs`).
- Deviations: none.
- Follow-up: none for Gate 5 scope.
