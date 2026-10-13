# P14 review-feedback verdict ledger

Audited head: `48af7743e9e3d30f6c87d4484a48e83074a995b6` (`docs/velnor-actions-spec`).
Requirements: `docs/reviews/pr-1.md` §5; adoption: `docs/reviews/pr-1-adoption.md`;
prior skeleton: `docs/reviews/pr-1-disposition.md` (stale in places — see verdicts).
Method: each verdict from implementation/test bodies opened at the audited head,
fixing SHA via blame/log. No code changed for this ledger.

PR #1 state (fetched 2026-09-30): OPEN, head `48af774`, 0 unresolved threads,
no APPROVED/CHANGES_REQUESTED. 3 inline threads, all resolved 2026-09-28
(G01/G03 outdated, G02 current). No new reviewer feedback since `2a162ac`;
issue comments are agent coordination notes (see N01–N02).

Counts: fixed 28 (R26 has an open residual) · superseded 0 · still-open 5.

## Verdict table

| ID | Requirement | Verdict | Commit |
|----|-------------|---------|--------|
| R01 | 7 Rust crate jobs + separate global validators | fixed | `7a2cc3a` |
| R02 | Validators own jobs; no per-task fan-out | fixed | `6d142e4`+`7a2cc3a` |
| R03 | No Policy/Workflow-Lint umbrella | fixed | `ac3d185` |
| R04 | Alint bundles + edition-2024 enforcement | fixed | `f63e04f` |
| R05 | Per-job caches; MBX before Cargo ops | fixed | `9861cdf` |
| R06 | Single Mise cache identity | fixed | `9861cdf` |
| R07 | Measured Rust-cache payload choice | fixed | `9861cdf` |
| R08 | No overlapping cache owners | fixed | `9861cdf` |
| R09 | Race-safe shared-cache writer | fixed | `9861cdf` |
| R10 | Real Cargo home; offline warm; cold fetch | fixed | `9861cdf` |
| R11 | Whole-workflow sizes/transfer/eviction/quota | OPEN | — |
| R12 | Warm run reuses; deltas rebuild | OPEN | — |
| R13 | PR-scoped save; fork read-only | OPEN | — |
| R14 | `ci.yml` + display `CI`; responsibility names | fixed | `16a25a9` |
| R15 | Rust grouping, package display, collision IDs | fixed | `7a2cc3a` |
| R16 | fmt+clippy+test inside crate jobs | fixed | `7a2cc3a` |
| R17 | Nextest config + CI profile + overrides | fixed | `8087fbc`+`755d5a6` |
| R18 | Mise detection → setup in needing jobs | fixed | `9861cdf` |
| R19 | Pinned action + CLI + checksum post-checkout | fixed | `ee8ab98`+`9861cdf` |
| R20 | Standard Mise setup; no duplicate manual | fixed | `9861cdf` |
| R21 | Reproducible install; lockfile policy | fixed | `e2e711f`+`d9fe86f` |
| R22 | rustfmt/clippy present cold AND warm | fixed | `8158403`+`7a2cc3a` |
| R23 | Wrapper selects MBX; absence→Cargo | fixed | `8087fbc` |
| R24 | MBX action/cmds only for MBX | fixed | `8087fbc`+`b911e01` |
| R25 | Explicit MBX+Nextest command/profile rule | fixed | `8087fbc`+`755d5a6` |
| R26 | No task-name auto-execution | fixed* | `8087fbc` |
| R27 | No Velnor prefix in IDs/names | fixed | `16a25a9`+`ac3d185` |
| R28 | Formatting once per scope | OPEN | — |
| R29 | Accurate local/consumer docs | OPEN | — |
| R30 | Proposal-to-contract adoption recorded | fixed | `8529f1d` |
| G01 | Consumer bootstrap w/o Velnor-only lock | fixed | `ac12e92` (+replies) |
| G02 | Self-contained consumer tool catalog | fixed | `ee8ab98` (+replies) |
| G03 | Mise owns subprocess effects | fixed | `fa961ee` (+replies) |

*R26 requirement holds; opt-in-key residual open (see below).

## Fixed evidence (compact)

- R01: 7 `rust-*` jobs + 5 validators + plan + required in
  `.github/workflows/ci.yml`; `build_crate_jobs`
  (`crates/velnor-actions-orchestrator/src/crate_jobs.rs:55`); test
  `groups_obligations_into_one_ordered_job_per_crate` (`crate_jobs_tests.rs:58`).
- R02: exact-set `check_required_evidence`
  (`orchestrator/src/required_evidence.rs:100`); no `strategy:` in `ci.yml`;
  matrix/closure no-ops without `velnor-task`; tests `impl_crate_graph.rs:66`,
  `impl_required_evidence.rs:24`.
- R03: `ValidatorKind` exhaustive match, `Policy` deleted
  (`workflow-renderer/src/support.rs:99`); zero policy jobs in `ci.yml`; tests
  `impl_e2e_wiring.rs:278`, `impl_renderer_tree.rs:199`.
- R04: 5 deduped bundles (`.alint.yml:41`); `alint check` green;
  inheritance-aware edition tests (`p11_toml.rs:240,247`,
  `p11_metadata.rs:268`); literal rule correctly rejected per adoption §4.
  Nit: `.alint.yml:136` gha-pin demote is stale (renderer pins SHA since
  `94f1a59`) — may revert to error.
- R05: restore→MBX→fetch→obligations (`crate_jobs.rs:255`,
  `source_prep.rs:106`, renderer order gates); tests `impl_orch_p08.rs:116`,
  `impl_mise_p08.rs:66`; all 7 crate jobs conform in `ci.yml`.
- R06: `mise_cache_key_for_tools` tool-union digest, no job id
  (`cache_p08.rs:45`); plan+7 rust share one key in `ci.yml`; tests
  `impl_renderer_p08.rs:14`, `impl_orch_p08.rs:66`.
- R07: measured 7-crate table, `QUALIFIED_TRANSPORT=ObjectsPlusSharedSources`
  (`mise/src/cache_transport.rs:3`); applied in all 7 jobs; tests
  `impl_mise_p08.rs:99`, `impl_orch_p08.rs:146`. Static estimates; live numbers
  belong to R11.
- R08: one owner per path (`mise/src/runtime_paths.rs:45`) + subset validation
  wired at render; tests `impl_mise_p08.rs:9`, `impl_renderer_p08.rs:106`.
- R09: plan-only save (`workflow_jobs_cache.rs:27`), readers restore-only
  (`crate_jobs.rs:329`); exactly one `Save Cargo sources` (`ci.yml:170`);
  tests `impl_orch_p08.rs:87`.
- R10: owned-home subset rejecting `~/.cargo`/credentials
  (`mise/src/cache_sources.rs:47`); offline-probe→skip else explicit fetch
  (`source_prep.rs:106`); `--offline` on cargo cmds in `ci.yml`.
- R14: tree is `ci.yml` only, `name: CI`; stale removal by whole-tree swap
  (`generate.rs:208`); test `impl_renderer_tree.rs:116`.
- R15: `Rust / <label>` display, `rust-<slug>`+digest8 on collision
  (`contract/src/workflow/jobs.rs:183,201,216`); tests `crate_jobs_tests.rs:73`,
  `impl_crate_graph.rs:61`. Gap: no dedicated collision unit test.
- R16: all 7 crate jobs hold Format/Clippy/Build/Test/Doctests/Doc steps; 14
  jobs total; tests `crate_jobs_tests.rs:81`, `impl_crate_graph.rs:70`.
- R17: `[profile.ci]` parse (`mise/src/nextest_config.rs:72`) → nearest-first
  select (`rust/src/profile_select.rs:203`), declared>detected (`:55`);
  `--profile` spliced at emission (`rust/src/argv.rs:283`); real
  `nextest_digest` (`orchestrator/src/identities.rs:210`); tests
  `impl_rust_p06.rs:101,125,365`, `impl_rust_argv.rs:142`.
- R18: `ensure_setup_p08` on every job (`render.rs:199`); argv-need superset
  covers detected-Mise case; tests `impl_renderer_setup.rs:66,104`,
  `impl_e2e_wiring.rs:106`.
- R19: full-SHA `uses` + `MISE_VERSION` + verified binary sha256
  (`orchestrator/src/pins.rs:46`); shape validation (`setup.rs:37`);
  post-checkout insert (`cache_p08.rs:285`); tests `impl_renderer_setup.rs:11,33`.
- R20: manual setup removed (`cache_steps.rs:341`); YAML asserted free of
  Restore/Save Mise tools (`impl_renderer_setup.rs:97`, `impl_orch_p08.rs:70`).
- R21: `mise.lock` read-only, never created (`recommendations.rs:43`,
  `tooling-input-contract.md:16`); cold `mise install <tool>@<exact>`
  (`steps.rs:104`); tests `impl_mise_surface.rs:186`, `impl_mise_install.rs:14`.
- R22: unconditional `rustup component add clippy rustfmt` per exact toolchain
  (`mise/src/steps.rs:192`) in plan + all 7 crates; tests
  `impl_mise_steps.rs:246`, `impl_crate_graph.rs:70`.
- R23: exact-`mbx` only (`evidence.rs:187`); absence→Cargo
  (`profile_select.rs:51`); override provenance + fail-closed conflicts; tests
  `impl_rust_p06.rs:57,82,166,195`, `impl_p06_detection.rs:85,318`.
- R24: Cargo→no MBX step, MBX→exactly one (`cache_steps.rs:60,77`); driver
  routing (`vectors.rs:62`, `preflight.rs:27`); tests `vectors_tests.rs:94`,
  `impl_orch_p08.rs:146`, `impl_mise_install.rs:103`.
- R25: `mbx nextest run --profile <resolved>` (adoption §7;
  `nextest.rs:13`, `nextest_shapes.rs:60`, `vectors.rs:62`); tests
  `impl_mise_p06.rs:183,247`, `impl_p06_detection.rs:318`.
- R26: content-only scan, filenames never consulted
  (`evidence_text.rs:58`); misleading names tested
  (`impl_p06_detection.rs:249`, `impl_rust_evidence.rs:163`).
  Residual: no explicit custom-task opt-in key exists — see fix spec R26-R.
- R27: branding gate (`contract/src/workflow/jobs.rs:138`); unbranded IDs/names
  in `ci.yml`; tests `jobs.rs:390`, `crate_jobs_tests.rs:75`. Latent:
  `TASK_JOB_ID="velnor-task"` (`render.rs:46`) never emitted.
- R30: `docs/reviews/pr-1-adoption.md` (89 lines, §§1–7) intact since `8529f1d`.
- G01: consumer embed release version/URL/digest
  (`workflow-contract.md:173`); impl `workflow.rs:86`, `pins.rs:83`,
  `generate.rs:112` (lock Velnor-only), `attach.rs:28`; tests `pins.rs:253`,
  `impl_orch_f2e.rs:80`. Reply `4119261575`.
- G02: compiled-in catalog (`mise/src/catalog.rs:160`); `resolve_mise_setup`
  (`pins.rs:46`); every `Setup Mise` in `ci.yml` carries version+sha256; test
  `impl_orch_f2e.rs:267` (lockless consumer gen). Reply `4119261763`.
- G03: sole `Command` constructor (`mise/src/command.rs:1,327`); zero
  `Command::new` in orchestrator `src/`; forbidden-token sweep + evasion
  fixtures (`impl_orch_f2f.rs:145`). Reply `4119261980`.

## Still-open fix specs

### R11 — whole-workflow cache measurements
No sequential-run size/hit/miss/duration/eviction/quota evidence for the P08
design. `docs/implemented/performance.md` has plan/compile/test walls only;
`parse_service_usage`/`headroom_bytes` (`mise/src/cache_trust.rs:92`) have zero
production callers; no cache integration fixture exists.
Fix: (1) new fixture (e.g.
`crates/velnor-actions-orchestrator/tests/impl_cache_fixtures.rs`) asserting
per-job keys/paths and recording `gh cache list` sizes + run timings;
(2) sequential-run table (stored/transfer/restore/save durations, hit/miss,
eviction, headroom) in `docs/implemented/performance.md`; (3) wire the quota
helpers into a reporting path or document why static.

### R12 — warm-reuse proof
No warm-reuse test: `warm_rerun_reuses_identical_extension`
(`rust/tests/impl_rust_f2a.rs:111`) checks identity eligibility only; gate-4's
47/47 run predates P08 `ci.yml`.
Fix: (1) warm-reuse test (new `impl_cache_warm.rs` or extend
`impl_orch_p08.rs`): render twice with identical inputs, assert identical keys
+ offline-skip branch; (2) hosted evidence — seed run N, run N+1 shows
`sources hit, skipping fetch` with no `Downloading`/dep-compile — recorded in
`performance.md` or the gate-4 doc.

### R13 — PR-scoped save; fork read-only
`Save Cargo sources` emitted unconditionally (`ci.yml:170`, no `if:`);
`pr_save_allowed`/`is_read_only`/`save_after_success`
(`mise/src/cache_trust.rs:18`) have zero production callers; sources key has
no trust component. (Correction to probe notes: contract `Step` HAS
`condition: Option<String>` (`contract/src/workflow/ir.rs:130`), but
`step_to_yaml` never serializes it — `document.rs:194` covers job-level only,
`:272` is a hardcoded upload-artifact `if:`.)
Fix: (1) serialize `step.condition` in `step_to_yaml`
(`workflow-renderer/src/document.rs:255`) for Action/Shell steps;
(2) set a push-only/trust condition on the save step in
`orchestrator/src/source_cache.rs` (+ gate mise `cache_save`);
(3) wire fork→read-only into generation with `impl_orch_p08.rs` assertions on
emitted conditions; (4) document the PR-scoping policy.

### R26-R — custom-task opt-in key (residual; requirement itself holds)
`PartialRustStack` (`orchestrator/src/config.rs:165`) has no tasks key under
`deny_unknown_fields`; no `tasks`/`custom` key in `init.rs` or
`contract/src/config/`.
Fix: (1) define e.g. `[stacks.rust] custom_tasks = [...]` allowlist in
`contract/src/config/stacks.rs` + `orchestrator/src/config.rs` + `init.rs`
template; (2) emit `mise run` steps only for allowlisted names in
`orchestrator/src/crate_jobs.rs` (or `vectors.rs`); (3) negative tests
(undeclared names never emitted; unknown keys rejected).

### R28 — formatting once per scope
8 overlapping executions: plan `Format` (`ci.yml:180`,
`mbx fmt --all --check` on the virtual workspace manifest = union of crates)
+ 7 per-crate `Format` steps (`ci.yml:379…1383`) — every file checked twice.
Single `explicit_fmt` (`orchestrator/src/discover.rs:226`) feeds both
per-package groups (`rust/src/tasks.rs:212`) and the workspace group
(`rust/src/tasks.rs:279`) → plan step (`orchestrator/src/wire_w1.rs:158`).
Fix: in `derive_for_config` (`orchestrator/src/derive_groups.rs:107`) skip the
workspace `Fmt` group when per-package `Fmt` groups exist for the same config
(keep the 7 per-crate steps per pr-1, drop plan `--all`); or return `None` in
`workspace_format_step` (`wire_w1.rs:162`) under the same condition. Add a
regression test: with `rustfmt.toml` present, total `Format` steps == 7, none
in `plan`.

### R29 — accurate docs
(1) `README.md:7` "The proposed CLI is:" → CLI is implemented
(`cli/src/args.rs:18`, `dispatch.rs:58`); document actual
`cargo run -p velnor-actions-cli -- {init,plan,generate --output-dir}` +
`--help`. (2) `README.md:3` "dogfood CI green 47/47 … run 36569723507" is
old-topology stale (47-job matrix deleted; tree is 14 jobs in `ci.yml` only) —
update or drop. (3) `README.md:25-26` blanket "Use MBX…" contradicts the
detection model (`workflow-contract.md:16`: Cargo-vs-MBX and runner selected
independently from evidence). (4) Alint-pin contradiction:
`version-policy.md:71,101,113` claims reviewed mutable-tag exception, but
`workflow-contract.md:308` says "No tag exception exists" and `ci.yml:50`
emits full SHA — one side must change. (5) Add a consumer installation doc
(release asset URL/SHA-256 route per bootstrap §2; honestly state no official
release exists yet — source builds fail consumer generation by design).

## New feedback since the reviewed head

No new reviewer feedback: no reviews beyond the 2026-09-28 Codex pass, no new
inline threads, no new general comments except donbeave agent-coordination
notes. Two coordination-note follow-ups (not review verdicts):

- N01: `non_utf8_path_broadens_explicitly` failed on Linux at `a74dd7f`
  (run 36759633324); no commit since touches
  `orchestrator/tests/impl_git_paths_p10.rs` or the walker guard. Owner:
  verify/fix on Linux; close the loop on the PR thread.
- N02: runner-side `actionlint@1.7.12` missing at `c8b3a89` (run 36751323928).
  `ci.yml:124,343` now installs actionlint via mise in plan/crate jobs —
  likely addressed; needs a hosted green run as proof.
