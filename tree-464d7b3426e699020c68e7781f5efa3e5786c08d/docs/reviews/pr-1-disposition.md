# PR #1 review disposition ledger (P14 skeleton)

Scope: one row per pr-1 §5 checklist item (R01–R30) + GitHub threads (G01–G03).
Requirements source: `docs/reviews/pr-1.md`; adoption: `docs/reviews/pr-1-adoption.md`.
Classes: **D** = accepted design change · **B** = confirmed bug · **V** = pending verification.

| ID | Required outcome (class) | Package | Status | Evidence / commit |
|----|--------------------------|---------|--------|-------------------|
| R01 | 7 Rust crate jobs + 1 job per repo-wide validator (D) | renderer | pending | /tmp/pr1-p05p06-probe.md (47-job graph) |
| R02 | Each validator owns its job; no per-task fan-out (D) | orchestrator | pending | /tmp/pr1-p05p06-probe.md; /tmp/pr1-salvaged-evidence.md W1 |
| R03 | No umbrella Policy/Workflow-Lint groupings (D) | renderer | pending | /tmp/pr1-p05p06-probe.md (merge_support_jobs) |
| R04 | Alint bundles + edition-2024 enforcement (B) | alint | partial | /tmp/p00-repro.log (preserved); fix in flight |
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
| R17 | Test-runner detection: nextest.toml + [profile.ci] (B) | rust-detect | pending | /tmp/pr1-p05p06-probe.md (nextest_digest=None) |
| R18 | Mise detection → jdx/mise-action in needing jobs (D) | renderer | pending | /tmp/pr1-p05p06-probe.md (evidence scan) |
| R19 | Mise action pinned SHA, post-checkout, pinned ver+sha256 (V) | renderer | pending | verification pending (hosted run) |
| R20 | Standard Mise setup; remove duplicate manual setup (B) | renderer | pending | /tmp/pr1-p07p10-probe.md (env overlay) |
| R21 | Reproducible install; cold cache passes; mise.lock rule (V) | renderer | pending | verification pending (hosted run) |
| R22 | rustfmt/clippy present on cold AND warm caches (B) | cache | pending | /tmp/pr1-p07p10-probe.md; upstream jdx/mise-action#215 |
| R23 | MBX via repo wrapper; default Cargo; explicit override (B) | rust-detect | pending | /tmp/pr1-p05p06-probe.md (line-local scan) |
| R24 | MBX mode: action + `mbx test`; Cargo mode: `cargo test` (D) | rust-detect | pending | /tmp/pr1-p05p06-probe.md |
| R25 | Explicit MBX+Nextest combined command (D) | rust-detect | pending | adoption §7 (`mbx nextest run`); impl pending |
| R26 | No Mise-task auto-select from name; explicit config only (B) | orchestrator | pending | /tmp/pr1-p05p06-probe.md; /tmp/pr1-p03p04-probe.md |
| R27 | No Velnor prefix in job names/IDs; purpose names (B) | renderer | pending | /tmp/pr1-p05p06-probe.md (6/6 prefixed) |
| R28 | Formatting runs once per scope (B) | orchestrator | pending | /tmp/pr1-p05p06-probe.md (Plan double-fmt) |
| R29 | README documents local CLI build/run (V) | cli-docs | pending | verification pending |
| R30 | pr-1 accepted as requirements source (D) | contract | complete | 8529f1d + docs/reviews/pr-1-adoption.md |
| G01 | Thread 4119057282 (workflow-contract:175, P1) replied (V) | contract | replied; impl pending | PR head 8ccc60c; 0 unresolved threads |
| G02 | Thread 4119057286 (workflow-contract:251, P1) replied (V) | contract | replied; impl pending | PR head 8ccc60c; 0 unresolved threads |
| G03 | Thread 4119057292 (cli-contract:34, P2) replied (V) | contract | replied; impl pending | PR head 8ccc60c; 0 unresolved threads |

## PR state (2026-09-29, /tmp/pr1-state.md)

Head `8ccc60c`, 0 unresolved threads, no APPROVED/CHANGES_REQUESTED reviews.
Run 36617447350: 45 success + 2 failure (rust nextest, Required aggregator).
No green gates claimed; hosted qualification still pending.
