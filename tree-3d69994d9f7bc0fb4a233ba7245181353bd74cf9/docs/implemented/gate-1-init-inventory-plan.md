# Gate 1: repository root, config, inventory, and plan

- State: recorded-pending-merge (branch `docs/velnor-actions-spec`; becomes implemented only on merge with required checks passing)
- Specification: [implementation-plan.md](../proposed/implementation-plan.md) Gate 1 + [cli-contract.md](../proposed/cli-contract.md) + [architecture.md](../proposed/architecture.md) (discovery/config)
- Landed by: unmerged branch `docs/velnor-actions-spec`, HEAD `f725a87`
- Merge date: TBD
- Delivered: `velnor-actions init` locates the Git root from CWD, writes one non-overwriting sample `.velnor/config.toml`, and writes nowhere else; automatic detector registry (V1 registers Rust only; `[discovery].exclude` applied before detection, `[stacks].ignore` after); `plan` prints a deterministic human-readable summary from the same analysis path as `generate`, writes no files, emits no YAML/JSON; malformed manifests and bad config return documented nonzero codes naming the file/key. Owning crates: `velnor-actions-cli`, `velnor-actions-rust`, `velnor-actions-orchestrator`.
- Acceptance evidence: local workspace suite green (see Gate 0 record for counts); dogfood CI round 5 (`f725a87`) pending — run link to be filled by parent. Key tests: `nested_init_writes_only_root_config`, `init_writes_no_other_file`, `malformed_manifest_fails_naming_file` (`crates/velnor-actions-cli/tests/impl_cli_init.rs`); `discovers_standalone_nested_and_multiple_workspaces_exactly_once`, `exclusions_apply_before_detection`, `ignores_apply_after_detection_with_reason` (`crates/velnor-actions-rust/tests/impl_rust_detect.rs`); `plan_job_ids_match_generated_workflow`, `plan_writes_nothing_and_exposes_no_plan_json` (`crates/velnor-actions-cli/tests/impl_cli_parity.rs`).
- Deviations: none.
- Follow-up: none; TypeScript/Bun detectors are future registrations under the same registry (no V2 claim here).
