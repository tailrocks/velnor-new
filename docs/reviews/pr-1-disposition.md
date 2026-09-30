# PR #1 review disposition ledger (P14 skeleton)

Scope: one row per pr-1 §5 checklist item (R01–R30) + GitHub threads (G01–G03).
Requirements source: `docs/reviews/pr-1.md`; adoption: `docs/reviews/pr-1-adoption.md`.
Classes: **D** = accepted design change · **B** = confirmed bug · **V** = pending verification.

| ID | Required outcome (class) | Package | Status | Evidence / commit |
|----|--------------------------|---------|--------|-------------------|
| R01 | 7 Rust crate jobs + 1 job per repo-wide validator (D) | renderer | pending | /tmp/pr1-p05p06-probe.md (47-job graph) |
| R02 | Each validator owns its job; no per-task fan-out (D) | orchestrator | partial | merge side done in 6d142e4 (exact-set required-evidence, VELNOR_NEEDS_JSON); validator-job rendering pending P05 |
| R03 | No umbrella Policy/Workflow-Lint groupings (D) | renderer | pending | /tmp/pr1-p05p06-probe.md (merge_support_jobs) |
| R04 | Alint bundles + edition-2024 enforcement (B) | alint | partial | P00 done in 1748495; P11-policy f63e04f: 5 bundles deduped (53 rules, check exit 0, 4 info-only), same-id gha-pin demote pinned in tests; literal edition rule REJECTED per adoption §4 (would fail edition.workspace=true); inheritance-aware enforcement pending |
| R05 | Per-job cache restore; MBX setup before any Cargo cmd (B) | cache | pending | verification pending (hosted run) |
| R06 | Single Mise cache identity; drop role duplicates (B) | cache | pending | verification pending (hosted run) |
| R07 | Measured Rust cache design choice (V) | cache | pending | verification pending (hosted run) |
| R08 | No two caches archive the same paths (V) | cache | pending | verification pending (hosted run) |
| R09 | Shared-cache single-writer policy (V) | cache | pending | verification pending (hosted run) |
| R10 | Cache paths = real Cargo home; offline on restore (B) | cache | pending | /tmp/pr1-p07p10-probe.md (toolchain homes) |
| R11 | Measured sizes/hits/durations/eviction, quota headroom (V) | cache | pending | verification pending (hosted run) |
| R12 | Warm run reuses registry/artifacts; only deltas rebuild (V) | cache | pending | verification pending (hosted run) |
| R13 | PR-scoped cache save; fork PRs read-only (V) | cache | pending | verification pending (hosted run) |
| R14 | `ci.yml` + display `CI`; no vendor in filenames (D) | renderer | pending | /tmp/pr1-p05p06-probe.md (2-file tree) |
| R15 | Rust-grouped jobs named from Cargo package (D) | renderer | pending | /tmp/pr1-p05p06-probe.md (Velnor-prefixed IDs) |
| R16 | Crate jobs hold fmt+clippy+test steps; no fan-out (D) | renderer | pending | /tmp/pr1-p05p06-probe.md (1-step vs 12-step gap) |
| R17 | Test-runner detection: nextest.toml + [profile.ci] (B) | rust-detect | partial | P06 8087fbc: structural [profile.ci] inspector + NextestProfile, independent runner dimension; residual: generated argv lacks --profile, nextest_digest None |
| R18 | Mise detection → jdx/mise-action in needing jobs (D) | renderer | pending | /tmp/pr1-p05p06-probe.md (evidence scan) |
| R19 | Mise action pinned SHA, post-checkout, pinned ver+sha256 (V) | renderer | pending | verification pending (hosted run) |
| R20 | Standard Mise setup; remove duplicate manual setup (B) | renderer | pending | /tmp/pr1-p07p10-probe.md (env overlay) |
| R21 | Reproducible install; cold cache passes; mise.lock rule (V) | renderer | pending | verification pending (hosted run) |
| R22 | rustfmt/clippy present on cold AND warm caches (B) | cache | pending | /tmp/pr1-p07p10-probe.md; upstream jdx/mise-action#215 |
| R23 | MBX via repo wrapper; default Cargo; explicit override (B) | rust-detect | complete | P06 8087fbc: exact-`mbx` wrapper evidence, absence→Cargo, explicit override with provenance; 4-combo argv tests |
| R24 | MBX mode: action + `mbx test`; Cargo mode: `cargo test` (D) | rust-detect | partial | P06 8087fbc: driver dimension resolves all 4 combos; actual command emission pending argv.rs/tasks.rs |
| R25 | Explicit MBX+Nextest combined command (D) | rust-detect | partial | P06 8087fbc: MBX+Nextest+CI-profile selection rule; emission pending argv.rs |
| R26 | No Mise-task auto-select from name; explicit config only (B) | orchestrator | partial | P06 8087fbc: Mise-task names never select (tested); explicit custom-task opt-in key still undefined |
| R27 | No Velnor prefix in job names/IDs; purpose names (B) | renderer | pending | /tmp/pr1-p05p06-probe.md (6/6 prefixed) |
| R28 | Formatting runs once per scope (B) | orchestrator | pending | /tmp/pr1-p05p06-probe.md (Plan double-fmt) |
| R29 | README documents local CLI build/run (V) | cli-docs | pending | verification pending |
| R30 | pr-1 accepted as requirements source (D) | contract | complete | 8529f1d + docs/reviews/pr-1-adoption.md |
| G01 | Thread 4119057282 (workflow-contract:175, P1) replied (V) | contract | replied; impl pending | PR head 8ccc60c; 0 unresolved threads |
| G02 | Thread 4119057286 (workflow-contract:251, P1) replied (V) | contract | replied; impl pending | PR head 8ccc60c; 0 unresolved threads |
| G03 | Thread 4119057292 (cli-contract:34, P2) replied (V) | contract | replied; impl pending | PR head 8ccc60c; 0 unresolved threads |

## Package evidence

- P01+P02 (`6d142e4`, pushed): closed required-evidence contract
  (`required_evidence.rs`, `needs_channel.rs` VELNOR_NEEDS_JSON), obligation
  universe with dispositions (`plan_obligation.rs`), checkout-must-match-head,
  NoWork-never-pass, qualification/promotion split. Standalone-verified:
  orchestrator 66+6+295 green, CLI 112 green, `cargo fmt --check` clean.
  Open gaps: renderer must emit VELNOR_NEEDS_JSON + download
  candidate-report/baseline artifacts (P05); `HEAD^2` checkout assumption
  needs W5 confirmation.
- P07+P09 (`093b8ad`, pushed): EnvPolicy per purpose, RepoTask env_clear,
  reserved-key rejection, bounded subprocess execution with real
  exit/signal; generated steps share the validated env contract
  (STEP_CREDENTIAL_DENYLIST). Preview/generate validates before creating,
  reports commit/rollback distinctly, refuses concurrent generates.
  Standalone-verified: mise 174, renderer 156, orchestrator 75+6+308
  green; clippy `-D warnings` clean; fmt clean. Residual: two-rename
  visibility gap; rollback_failed unreachable single-threaded.
- Independent review of `6d142e4`: FAIL (R02-merge-side) with must-fixes
  F1 (Execute obligations without matrix legs pass: needs exact-set
  Execute⊆matrix check), F2 (planning_failed sites with empty miss
  tokens), F4 (HEAD^2 accept path untested); should-fix F3 (Push
  broadening silent), F5 (disposition checks by equality). Fix agent
  spawned; P05 must not build on merge until F1 lands.
- Review fix (`9626071`, pushed): F1 exact-set Execute⊆matrix check,
  F2 miss tokens at all 4 sites, F4 HEAD^2 accept/reject tests, F3 Push
  warning, F5 exhaustive matches. 8 new tests; orchestrator 75+6+316
  green standalone. R02-merge-side now PASS; P05 unblocked on merge.
- P12 (`08bfd4c`, pushed): 4-namespace freshness script, validated
  exceptions, 26-glob pinned mutation scope. 35 p12_ tests green
  standalone; shellcheck/shfmt/clippy/fmt clean. Real-root gate
  honestly red: no scheduled producer (17 stale-evidence rows) + 3
  genuinely stale pins (mise 2026.9.17, mr-boxington v1.21.0,
  mise-action v5.0.0) — follow-ups.
- Red flag: `size_limits_hold` failing on 6 files (landed: command.rs
  440, merge.rs 429, impl_merge.rs 552, impl_select.rs 485; P06
  in-flight: nextest.rs 481, toml_scan.rs 507). Split agent spawned
  for landed files; P06 warned to split + fix clippy before handoff.
- Size split (`abb5866`, pushed): command.rs 440->312 (+command_env,
  command_output), merge.rs 429 split (+merge_checks), impl_merge 552
  + impl_select 485 split by topic (+impl_merge_plan, +impl_select_base).
  Behavior-preserving; mise allowlist updated. Standalone: mise 174,
  orch 75+6+316, size_limits_hold green, clippy -D clean, fmt clean.
- P10 (`d8a26a6`, pushed): origin identity via `git config --get
  remote.origin.url` in-tree (linked worktrees, includes, worktree
  config); NUL `-z` path bytes via `split_nul_paths`, non-UTF-8 ->
  explicit `non_utf8_path` broaden; mise git allowlist gains `config`.
  20 new tests (7 true fix-proofs). Standalone: mise 178, orch
  75+6+332, clippy -D clean, fmt clean. Residual: select_edges +
  validators still line-parse; select.rs at 398 lines.
- P06 (`8087fbc`, pushed): structural Mise TOML-subset parser +
  `wrappers.cargo.command` (exact-`mbx` only) and `[profile.ci]`
  inspectors in mise; independent driver/runner dimensions,
  `NextestProfile`, ambiguity/conflict fail-closed codes in rust;
  per-workspace nearest-first discovery in orch. 45 new tests incl.
  4-combo argv. Standalone: mise 195, rust 124, orch 75+6+344,
  clippy -D clean, fmt clean. Residual: Nextest argv lacks
  `--profile` (needs argv.rs/tasks.rs); `nextest_digest` None;
  custom-task opt-in key undefined.
- P10 test split (`e64d8c9`, pushed): 403-line
  impl_git_authority_p10 tripped size_limits_hold (P10 verify never
  ran the CLI suite — process gap, now closed). Split by
  responsibility: identity cases stay (203 lines), NUL-path cases
  to impl_git_paths_p10 (211 lines), zero helper duplication. All
  16 tests preserved; orch 344 green.
- P11-policy (`f63e04f`, pushed): five R04 bundles deduped in
  .alint.yml (53 rules on pinned alint 0.16.1, independently
  re-verified: `check --fail-on-warning` exit 0, 4 info-only);
  same-id gha-pin override demoted to info with exact
  paths/query/regex pinned in tests; edition decision documented
  (adoption §4). Structured enforcement: p11_toml, p11_metadata
  (cargo metadata effective edition/MSRV), p11_compiler
  (baseline-clean + deny-flag rejects), p11_alint. AGENTS.md -> 46
  lines, V1 boundaries first, runner pointer to docs/deferred.
  Standalone: CLI 170 incl. size green, clippy -D clean, fmt
  clean. Type-hardening deferred to later P11 step.
- P13-perf (`32bf67e`, pushed): `MemberIndex` metadata reuse
  (1 fetch/workspace after membership validation; explicit
  roots never reused; lanes stay 1), 1/10/100-crate fixture
  generator + timing/unit harness, `scripts/verify-local.sh`
  entrypoint, performance.md P13 section with raw evidence
  (historical §1 labeled partial, unmeasured budgets
  unpassed). 3 unit + 8 integration tests (handoff claimed
  4 unit — 1 pre-existing). Standalone: orch 78+6+352
  green, clippy `-D` clean (parent fixed 1
  format_push_string), fmt/shellcheck/shfmt clean. Deferred:
  e2e negative pipeline tests (need P05 graph).
- In flight: P03-P04 identities/baselines (cover_baseline,
  cover_identity, internal_plan, closure, generator,
  provenance_check, snapshot, identities, reuse_stages,
  wire_w2, rust identity). Queued behind P03: P05 crate
  graph (needs internal_plan.rs), P11-types.
- P08-cache-probe (done, research-only, zero repo writes):
  /tmp/p08-probe/P08-PLAN.md (257 lines, 10 sections). Measured
  runtime path inventory; broken-symlink failure structurally
  confirmed; quota 576MiB/10GB (~94% headroom); target
  ownership table; race-safety argument (immutable entries +
  plan-only writer); transport choice objects+shared-source;
  red/green targets; 6 TDD work packages with named failing
  tests; 8 open questions. P08 builder queued AFTER P05 +
  P03/P04 proofs (restore verification, toolchain_id) per plan
  §10 — no cache optimization before its correctness proofs.
- Hosted CI (run 36653922919 @ a40608f): Alint green after the
  P10 test split; Plan freshness still red on velnor.yml drift
  (reproduced locally: diff is exactly the P07 env-contract
  vars; generation deterministic across two runs). Regeneration
  deferred to P05's ci.yml migration — no velnor.yml refresh.

## PR state (2026-09-29, /tmp/pr1-state.md)

Head `8ccc60c`, 0 unresolved threads, no APPROVED/CHANGES_REQUESTED reviews.
Run 36617447350: 45 success + 2 failure (rust nextest, Required aggregator).
No green gates claimed; hosted qualification still pending.

## P01-V4 caveat (exact-head propagation vs independent validators)

The exact-head F2b failure propagating to a red Required exercises only the
crate-report path (failed task report -> nonzero merge). It does NOT validate
the independent-validator case: passing crate reports combined with a
failed, missing, skipped, or cancelled *global* validator (Alint, Cargo
Deny, Cargo Machete, Actionlint, Zizmor) must each force Required nonzero on
its own evidence. That case needs dedicated fixtures per validator and stays
unproven until they exist; do not cite F2b propagation as validator proof.
