# T57 architecture review — GitHub-only cache integration

Verdict: **CONDITIONAL PASS**

Date: 2026-10-26. Workdir: `tailrocks/velnor-new` at `8b27898e`.
Plan: `velnor-github-only-cache-plan` (ARCHITECTURE.md, CONTRACTS.md fixed inputs).

The implementation is complete enough for controlled runtime qualification
without invented policies, subject to the one condition in §6.
No measured fit or speedup is declared: live evidence is still pending by
design (T58+ own `run-commands.json`; no hosted commands were executed here).

## 1. Method and inputs

- Rechecked T01 ownership map (`docs/proposed/cache-github-only/caller-map.md`,
  source HEAD `693f3aae`): every cache caller and explicit test root enumerated.
- Rechecked fixed contracts: ARCHITECTURE.md §§1–11, CONTRACTS.md §§1–9,
  CACHE_BUDGET.md ceilings, T02 `baseline.json` / `run-budget.json`.
- Prereqs taken as stated: T53/T54/T55 fully gated PASS, T56 security gate PASS.
- Ran the card's exact verification commands (results in §5).

## 2. Real callers vs fixed contract — holds

- New pure types exist and are registered through their declared parents:
  `cache_identity.rs`, `cache_policy.rs`, `cache_context.rs` in
  `velnor-actions-contract/src/workflow/mod.rs` (mods + re-exports).
- Every explicit test root registers its owner modules:
  `velnor_contract.rs` (`gc1_contract_cases`, `gc1_t04/05/23`),
  `velnor_orchestrator.rs` (26 `gc1_*` incl. `gc1_runtime_cases`),
  `velnor_workflow_renderer.rs` (incl. `gc1_workflow_cases`),
  `velnor_mise.rs`, `velnor_rust.rs`, `velnor_cli.rs` (`gc1_t28/29`).
- CLI reaches typed operations only: `dispatch.rs` /
  `dispatch/operation.rs` route `CACHE_PUBLISH/RECOVER/CLEANUP_OP`
  to `InternalOp::{CachePublish, CacheRecover, CacheCleanup}`.
  No generic shell-execution operation was introduced.
- ARCHITECTURE §2 seams are implemented, not bypassed:
  `check_tool_acquire.rs::acquire` hydrates through
  `qualified_tool_home` / `qualified_source_command` with archive,
  tree, and executable probes; `check_execution.rs::compiler_prefix`
  binds the qualified context; `cache_steps_tools_restore.rs` owns the
  generated Mise verifier composition; `rust_toolchain_script.rs`
  reuses reverified archives before origin fetch.
- Controller bootstrap bridge (`closure.rs::provision_acquire_step`,
  `steps.rs::acquire_velnor_step`, `cache_bootstrap.py`) stages only the
  exact bounded standalone member and verifies it against the
  independently committed release digest before execution. No general
  source/state extraction or admission bypass in the bridge.
- Renderer remains typed fixed steps; no Cargo policy derivation or
  process execution in renderer code.

## 3. Actual command and product equivalence — holds

- `product_release_pins.rs::mbx_build_argv` preserves the exact recipe
  (`build --release --locked --package velnor-actions-cli --bin
  velnor-actions` + explicit Intel target args).
- Release assets (`schema2_generator_release_assets.rs`) bind setup and
  lookup to the shared `CARGO_TARGET_DIR`-owned context; no change to
  `target/release` vs `target/<triple>/release` semantics.
- `preseed.rs::PRESEED_BUILD_OUTPUT` and helper input identity preserved;
  authority-side files (`job_image_authority*.py`, `job_image_authority.rs`)
  are data-only validation/publication with no checkout, build, or
  candidate-built executable path.
- `preview_producer.rs` keeps prepared-check homes, resource limits, and
  profile distinctions with qualified tool receipts.
- Removal targets for the ordered tasks remain intact and unedited:
  `cache_family_mbx.rs`, both `dependency_seed.rs`,
  `cache_elect.rs`, `cache_prune.rs`, `cache_save_budget.rs`,
  `cache_steps_mbx_*`. No premature removal.

## 4. Budget assumptions and unsupported observation — holds

- Ceilings consistent with ARCHITECTURE §9 and CACHE_BUDGET.md:
  hard `10,000,000,000`, provisional operating `8,000,000,000`,
  `2,000,000,000` reserve. No code or doc raises the ceiling.
- Unknown/zero discipline kept: `run-budget.json` (152 unknown markers)
  and `baseline.json` (18) record explicit unknown reasons; new
  observation code (`compiler_observation.rs`, `cache_inventory.rs`,
  `cache_context.rs`) carries `unknown_reason` / `ObservationGap`
  rather than zero.
- Grep over changed sources and `docs/proposed/cache-github-only/`
  finds no measured-fit or speedup claim.
- No TODO/FIXME/`todo!`/`unimplemented!` in any added or changed file.

## 5. Verification (card-exact commands, this review)

- `cargo fmt --all -- --check`: clean.
- `cargo clippy --locked --workspace --all-targets -- -D warnings`: clean.
- `cargo nextest run --locked --workspace --no-tests fail`:
  **103 passed, 0 failed**, including `gc1_contract_cases::*` (T53),
  `gc1_runtime_cases::*` (T54), `gc1_workflow_cases::*` (T55),
  and all `gc1_tNN_preserves_contract` / `rejects_invalid_state` pairs.

## 6. Condition (blocks integration, not qualification prep)

- The T56 security review artifact
  (`docs/reviews/cache-github-only-security.md`) is **absent from the
  tree**, although T56 PASS is stated. T57 requires T56, and the plan
  requires review verdicts to be recorded, not just asserted.
  This review spot-checked trust-relevant seams (outer-archive bridge
  digest check, fixed cache-mode boundaries, authority data-only path)
  and found no architecture-level violation, but it does not substitute
  for the independent security verdict.
- **Condition:** record the T56 security verdict artifact before any
  integration commit. No code change is required by this review.

## 7. Remaining conflicts and explicit decisions

- None requiring a new decision. Known bounded deferrals already have
  owners: live measurement gaps stay `unknown` (T02/T65+), hosted
  command contracts stay blocked labels until T58 writes exact approved
  arrays, and mechanical removals stay with T68/T69/T70.
- `caller-map.md` (second permitted file) needed no update: spot checks
  confirm its caller/role inventory still describes the tree.

Scope respected: only this file was written. No commit made.
