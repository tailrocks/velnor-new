# T01 caller map — every cache caller and explicit test root

Source: `rg -n "cache|preseed|prepare_check|source_steps" crates/velnor-actions-*`
(observed 2026-10-26 at `693f3aae`; counts are matches per file, case-sensitive).
Pattern hits include incidental substrings (e.g. `cached`, `cache_dir`);
owner tasks must confirm each file's semantic role before editing.

## 1. Contract identity and policy (`velnor-actions-contract`)

Pure identity, coverage, admission, and validation. No process execution.

| File | Matches | Role |
|---|---|---|
| `src/cachekey.rs` | 18 | MBX/cache key generation and validation |
| `src/workflow/cache_family.rs` | 17 | Family policy (sources/tools/state) |
| `src/discover/cache_admission.rs` | 33 | Byte-admission policy |
| `src/discover/index.rs` | 14 | Generation discovery index |
| `src/workflow/cache_ids.rs` | 10 | Generation/coverage identity |
| `src/workflow/step_protocol.rs` | 24 | Step protocol incl. cache ops |
| `src/workflow/identity.rs` (qualification_cache_lineage) | 12 | Lineage identity |
| `src/workflow/cache_family_mbx.rs` | 2 | MBX family — removal target (T69) |
| `src/workflow/dependency_seed.rs` | 4 | Seed identity — removal target (T68) |
| `src/workflow/cache_family_steps.rs` | 1 | Family step bindings |
| `src/workflow/step_identity.rs` | 9 | Step identity |
| `src/workflow/report.rs` / `report_validate.rs` | 3 / 8 | Report bindings (T05 scope) |
| `src/workflow/runtime.rs` (lineage) | 9 | Lineage runtime |
| `src/workflow/ir.rs` | 8 | Workflow IR |
| `src/workflow/matrix_entry.rs` | 7 | Matrix entries |
| `src/workflow/mod.rs` | 7 | Module registration |
| `src/propose/identity.rs`, `src/propose/task.rs` | 1 / 3 | Proposal identity |
| `src/archive.rs`, `src/canonical.rs`, `src/extensions.rs`, `src/freshness.rs`, `src/graph.rs`, `src/lib.rs`, `src/config/*`, `src/cachekey_platform.rs`, `src/cachekey_observed.rs`, `src/workflow/*` (remainder) | 1–4 each | Archive guards, canonical form, config, platform/observed keys, lineage receipt/validation, step payloads |

## 2. Acquisition and execution (`velnor-actions-mise`)

Child-process and origin acquisition boundary. Owner of restore/verify/reuse.

| File | Matches | Role |
|---|---|---|
| `src/cache.rs` | 10 | Cache acquire/verify core |
| `src/restore.rs` | 9 | Restore path |
| `src/cache_sources.rs` | 3 | Source payload hydration |
| `src/cache_transport.rs` | 3 | Transport handling |
| `src/cache_trust.rs` | 6 | Trust policy — removal target (T70) |
| `src/check_execution.rs` | 2 | `compiler_prefix`; child execution seam (ARCHITECTURE §2) |
| `src/qualified_acquisition.rs` (`qualified_tool_home`, `qualified_source_command`, `qualified_link_command`, `qualified_fetch_command` — matched via `restore_evidence`/`preflight` refs) | — | Qualified acquisition seam (ARCHITECTURE §2) |
| `src/check_command.rs`, `src/check_discovery.rs`, `src/check_metadata.rs`, `src/command_tofu.rs`, `src/error.rs`, `src/gate6.rs`, `src/lib.rs`, `src/preflight.rs`, `src/restore_evidence.rs`, `src/reuse.rs`, `src/runtime_paths.rs`, `src/verify.rs` | 1–6 each | Discovery, preflight, evidence, paths, error surface |

## 3. Planning and orchestration (`velnor-actions-orchestrator`)

Planning, seeds, preparation, merge, and report transport.

| File | Matches | Role |
|---|---|---|
| `src/dependency_seed.rs` | 2 | Seed planner — removal target (T68) |
| `src/workflow_jobs.rs` | 10 | Job wiring touched by seed removal (T68) |
| `src/check_prepare.rs` | 2 | Preparation seam; homes module parent (T09) |
| `src/check_runtime.rs` | 2 | Runtime observation collection |
| `src/cache_keys.rs` | 6 | Discovery/intent owner (with planned children) |
| `src/source_cache.rs` | 5 | Source-cache handling |
| `src/workflow_jobs_cache.rs` | 5 | Cache job wiring |
| `src/cover.rs` | 10 | Coverage derivation |
| `src/plan_output_limits.rs` | 13 | Output/byte limits |
| `src/identities.rs` | 12 | Identity bindings |
| `src/merge.rs` / `src/merge_checks.rs` | 7 / 10 | Final merge and Required binding |
| `src/plan.rs`, `src/plan_obligation.rs` | 2 / 8 | Plan ownership |
| `src/tofu_cache.rs` | 12 | Tofu family cache |
| `src/attach.rs` | 19 | Attach path |
| remainder (`ansible_steps`, `bun_*`, `cache_cleanup_emit`, `cache_prune_emit`, `check_jobs`, `config_workflow`, `crate_jobs*`, `dbt_steps`, `decisions`, `discover_*`, `generate*`, `image_*`, `internal_plan`, `inventory_tofu`, `matrix_*`, `named_checks`, `package_release_*`, `preseed_manifest`, `preview_producer`, `private_*`, `project_tool_sources`, `qualification_resolver`, `required_evidence`, `reuse_stages`, `rust_suites_emit`, `snapshot`, `stack_family_cache`, `task_report`, `validators`, `wire_*`, `workflow*`) | 1–8 each | Steps, emitters, manifests, producers, validators |

## 4. Generated transport (`velnor-actions-workflow-renderer`)

Typed fixed steps only. Derives no policy, executes no processes.

| File | Matches | Role |
|---|---|---|
| `src/cache_steps.rs` | 28 | Core generated cache steps |
| `src/cache_doctor.rs` | 33 | Cache diagnostics |
| `src/cache_elect.rs` | 29 | Family election — removal target (T70) |
| `src/schema2_mbx_qualification.rs` | 27 | MBX qualification — removal scope (T69) |
| `src/dispatch_cache_boundary.rs` | 26 | Cache-mode boundary |
| `src/preseed.rs` | 21 | Preseed rendering (`PRESEED_BUILD_OUTPUT`, T44 helpers) |
| `src/cache_p08*.rs` | 12–18 each | P08 tool-payload/setup/identity/prelude/save-policy |
| `src/cache_steps_tools_restore.rs` | 15 | Mise verifier composition (ARCHITECTURE §2) |
| `src/cache_steps_mbx_preflight.rs` | 15 | MBX preflight — removal scope (T69) |
| `src/schema2_empty_cache.rs` | 14 | Empty-cache path |
| `src/cache_steps_family.rs` | 12 | Family steps |
| `src/render.rs` | 16 | Render core |
| `src/lane_share*.rs` | 1–18 | Lane sharing |
| `src/tofu_cache.rs` | 19 | Tofu cache rendering |
| `src/schema2_classes.rs` | 19 | Schema classes |
| `src/preseed_closure.rs` / `preseed_mbx.rs` | 6 / 3 | Preseed variants |
| `src/cache_prune.rs` | 8 | Pruning — removal target (T70) |
| `src/cache_save_budget.rs` | 9 | Save budget — removal target (T70) |
| `src/cache_plan_outputs.rs` | 8 | Plan outputs |
| `src/cache_steps_mbx_bundle.rs` | 7 | MBX bundle — removal scope (T69) |
| `src/cache_cleanup.rs` | 6 | Cleanup rendering |
| remainder (`closure`, `steps`, `setup`, `support`, `matrix`, `render_*`, `schema2_*`, `document*`, `stack_jobs`, `tool_seed`, `verification_jobs`, `yaml`, scripts, goldens) | 1–10 each | Closure, steps, schema2 jobs, docs, tooling |

## 5. Freshness, lint, and CLI

| File | Matches | Role |
|---|---|---|
| `velnor-actions-freshness/src/inventory.rs` | 2 | Pinned `actions/cache/{restore,save}` versions |
| `velnor-actions-actionlint/src/actions.rs` / `capabilities.rs` | 2 / 3 | Allowed cache actions, `cache_mode` capability |
| `velnor-actions-cli/src/dispatch.rs` | 2 | CLI dispatch binding (T28/T29 scope) |
| `velnor-actions-rust/src/closure.rs` | 21 | Rust closure cache (T06 scope) |
| `velnor-actions-rust/src/tasks.rs`, `lib.rs`, `propose.rs` | 1–2 each | Task selection |
| `velnor-actions-tofu/src/env.rs` | 13 | Tofu provider cache dir |
| `velnor-actions-tofu/src/task_identity.rs` | 3 | Tofu task identity |

## 6. Explicit test roots

Only one committed Rust integration-test file exists today
(`PR #391` reset; `test = false` targets need registration before discovery):

- `crates/velnor-actions-orchestrator/tests/homebrew_adoption_seed_public.rs` (existing)

Planned explicit roots (must be created and registered by their owner tasks;
registration file in parentheses):

- `crates/velnor-actions-contract/tests/gc1_t04.rs`, `gc1_t05.rs`, `gc1_t23.rs`, `gc1_t53.rs` (`velnor_contract.rs`) — T04/T05/T23/T53
- `crates/velnor-actions-rust/tests/gc1_t06.rs` (`velnor_rust.rs`) — T06
- `crates/velnor-actions-orchestrator/tests/gc1_t07.rs` … `gc1_t10.rs`, `gc1_t12.rs`, `gc1_t15.rs`, `gc1_t19.rs` … `gc1_t22.rs`, `gc1_t24.rs` … `gc1_t27.rs`, `gc1_t32.rs` … `gc1_t36.rs`, `gc1_t39.rs` … `gc1_t41.rs`, `gc1_t49.rs`, `gc1_t51.rs`, `gc1_t52.rs`, `gc1_t54.rs`, `gc1_t58.rs` … `gc1_t62.rs`, `gc1_t68.rs` (`velnor_orchestrator.rs`)
- `crates/velnor-actions-mise/tests/gc1_t11.rs`, `gc1_t17.rs`, `gc1_t18.rs` (`velnor_mise.rs`)
- `crates/velnor-actions-workflow-renderer/tests/gc1_t13.rs`, `gc1_t14.rs`, `gc1_t16.rs`, `gc1_t30.rs`, `gc1_t31.rs`, `gc1_t42.rs` … `gc1_t48.rs`, `gc1_t50.rs`, `gc1_t55.rs`, `gc1_t69.rs`, `gc1_t70.rs` (`velnor_workflow_renderer.rs`)
- `crates/velnor-actions-cli/tests/gc1_t28.rs`, `gc1_t29.rs` (`velnor_cli.rs`)
- `scripts/hosted_phase_measurement/tests/test_gc1_observation.py` — T37/T38 (no Rust registration file)

## 7. Exact gate commands

Generator candidate gates (`GENERATE_CANDIDATE`, TASKS.md §Command Sets):

```bash
export CARGO_TARGET_DIR="$PWD/.velnor-owned-verification-target"
cargo build --locked -p velnor-actions-cli --bin velnor-actions
export VELNOR_ACTIONS_BIN="$CARGO_TARGET_DIR/debug/velnor-actions"
test -x "$VELNOR_ACTIONS_BIN"
VELNOR_GENERATE_A="$(mktemp -d "${TMPDIR:-/tmp}/velnor-gc1-a.XXXXXX")"
VELNOR_GENERATE_B="$(mktemp -d "${TMPDIR:-/tmp}/velnor-gc1-b.XXXXXX")"
"$VELNOR_ACTIONS_BIN" generate --output-dir "$VELNOR_GENERATE_A"
"$VELNOR_ACTIONS_BIN" generate --output-dir "$VELNOR_GENERATE_B"
git diff --no-index --exit-code "$VELNOR_GENERATE_A/.github" "$VELNOR_GENERATE_B/.github"
"$VELNOR_ACTIONS_BIN" generate
```

Repository gates (root + nested runner; `docs/implemented/repo-verification.md`):

```bash
export CARGO_TARGET_DIR="$PWD/.velnor-owned-verification-target"
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo nextest run --locked --workspace --no-tests fail
export VELNOR_ACTIONS_BIN="$CARGO_TARGET_DIR/debug/velnor-actions"
alint validate-config
alint check --fail-on-warning
cargo deny check
bash scripts/check-freshness.sh
cargo fmt --manifest-path crates/velnor-runner/Cargo.toml --all -- --check
cargo clippy --manifest-path crates/velnor-runner/Cargo.toml --locked --all-targets -- -D warnings
cargo nextest run --manifest-path crates/velnor-runner/Cargo.toml --locked --no-tests fail
cargo deny --manifest-path crates/velnor-runner/Cargo.toml check
```

T01 verification commands (this task): `git rev-parse HEAD`, `git status --short`,
`rg -n "cache|preseed|prepare_check|source_steps" crates/velnor-actions-*`,
`alint validate-config`, `alint check --fail-on-warning`.
