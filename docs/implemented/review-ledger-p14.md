# P14 review-feedback verdict ledger

Audited head: `34550e889b82d5586120fb40da5522cb5e4d0509` (`docs/velnor-actions-spec`).
Requirements: `docs/reviews/pr-1.md` §5; adoption: `docs/reviews/pr-1-adoption.md`;
prior skeleton: `docs/reviews/pr-1-disposition.md` (stale in places — see verdicts).
Prior audit: same file at `48af774` (2026-09-30); this revision re-verifies
every row at the new head and adds the F1–F7 + G1–G7 consumer rounds.
Method: each verdict from implementation/test bodies opened at the audited head,
fixing SHA via blame/log. No code changed for this ledger.

PR #1 state (fetched 2026-10-01): OPEN, head `34550e8`, 0 unresolved threads,
no APPROVED/CHANGES_REQUESTED (4 Codex COMMENTED reviews, 2026-09-28 only).
3 inline threads, all resolved 2026-09-28 (G01/G03 outdated, G02 current).
Two consumer finding comments since `48af774`: F1–F7 (`5921204817`,
2026-09-30) with maintainer reply `5927509300`, and G1–G7 (`5925973373`,
2026-10-01) with maintainer reply `5931252884`. All other issue comments
are agent coordination notes.

Counts: R fixed 30/30 (R26 residual closed) · G01–G03 fixed ·
F fixed 7/7 (F3 keeps an external release-infra residual) ·
G fixed/documented 6 + rejected-with-evidence 1 (G2) · still-open 0.

## Verdict table (R + original G)

| ID | Requirement | Verdict | Commit |
|----|-------------|---------|--------|
| R01 | 7 Rust crate jobs + separate global validators | fixed | `7a2cc3a` |
| R02 | Validators own jobs; no per-task fan-out | fixed | `6d142e4`+`7a2cc3a` |
| R03 | No Policy/Workflow-Lint umbrella | fixed | `ac3d185` |
| R04 | Alint bundles + edition-2024 enforcement | fixed | `f63e04f`+`cf8d762` |
| R05 | Per-job caches; MBX before Cargo ops | fixed | `9861cdf` |
| R06 | Single Mise cache identity | fixed | `9861cdf` |
| R07 | Measured Rust-cache payload choice | fixed | `9861cdf` |
| R08 | No overlapping cache owners | fixed | `9861cdf` |
| R09 | Race-safe shared-cache writer | fixed | `9861cdf` |
| R10 | Real Cargo home; offline warm; cold fetch | fixed | `9861cdf` |
| R11 | Whole-workflow sizes/transfer/eviction/quota | fixed | `33b3178` |
| R12 | Warm run reuses; deltas rebuild | fixed | `33b3178` |
| R13 | PR-scoped save; fork read-only | fixed | `b94fe9e`+`d2eae99` |
| R14 | `ci.yml` + display `CI`; responsibility names | fixed | `16a25a9` |
| R15 | Rust grouping, package display, collision IDs | fixed | `7a2cc3a`+`86a6223` |
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
| R26 | No task-name auto-execution | fixed | `8087fbc`+`442fa8b` |
| R27 | No Velnor prefix in IDs/names | fixed | `16a25a9`+`ac3d185` |
| R28 | Formatting once per scope | fixed | `b94fe9e`+`d2eae99` |
| R29 | Accurate local/consumer docs | fixed | `442fa8b`(+this rev) |
| R30 | Proposal-to-contract adoption recorded | fixed | `8529f1d` |
| G01 | Consumer bootstrap w/o Velnor-only lock | fixed | `ac12e92` (+replies) |
| G02 | Self-contained consumer tool catalog | fixed | `ee8ab98` (+replies) |
| G03 | Mise owns subprocess effects | fixed | `fa961ee` (+replies) |

## Verdict table (consumer rounds F + G)

Source: issue comments `5921204817` (F1–F7) and `5925973373` (G1–G7);
maintainer replies `5927509300` and `5931252884`. Re-verified at `34550e8`.

| ID | Requirement | Verdict | Commit |
|----|-------------|---------|--------|
| F1 | `RUSTDOCFLAGS` on native doc steps | fixed | `e474dd4` |
| F2 | `actionlint.yaml` honors `runner_label` | fixed | `e474dd4` |
| F3 | Release provenance (manifest commit) | fixed* | `e474dd4`+`2922794` |
| F4 | `Swatinem/rust-cache` documented set | fixed | `e474dd4` |
| F5 | Bounded fetch retry, no silent tail | fixed | `e474dd4` |
| F6 | `MISE_LOCKFILE` split documented | fixed | `e474dd4` |
| F7 | `retention-days` on uploads | fixed | `e474dd4` |
| G1 | Fork-PR trust model | documented | `90789f5` |
| G2 | Honor `mise.lock` at install | rejected† | `2922794` |
| G3 | Merge fail-closed contract | fixed | `90789f5`+`2922794` |
| G4 | Per-job `timeout-minutes` | fixed | `2922794` |
| G5 | Zizmor installed-but-never-run | fixed | `ba18ad2`(+regen) |
| G6 | Gate-class variances | documented | `90789f5`+`e474dd4` |
| G7 | Actionlint allowlist drift | fixed | `e474dd4` |

\*F3 in-repo work complete; immutable tags + attestations need release
infrastructure outside the repo (see residuals). †G2 rejected with
counter-evidence after a tried-and-reverted implementation (see below).

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
  The stale gha-pin demote is gone (`cf8d762`); `.zizmor.yml` has no ignore.
- R05: restore→MBX→fetch→obligations (`crate_jobs.rs:255`,
  `source_prep.rs:106`, renderer order gates); tests `impl_orch_p08.rs:116`,
  `impl_mise_p08.rs:66`; all 7 crate jobs conform in `ci.yml`.
- R06: `mise_cache_key_for_tools` tool-union digest, no job id
  (`cache_p08.rs:45`); plan+7 rust share one key in `ci.yml`; tests
  `impl_renderer_p08.rs:14`, `impl_orch_p08.rs:66`.
- R07: measured 7-crate table, `QUALIFIED_TRANSPORT=ObjectsPlusSharedSources`
  (`mise/src/cache_transport.rs:3`); applied in all 7 jobs; tests
  `impl_mise_p08.rs:99`, `impl_orch_p08.rs:146`. Live numbers live in R11.
- R08: one owner per path (`mise/src/runtime_paths.rs:45`) + subset validation
  wired at render; tests `impl_mise_p08.rs:9`, `impl_renderer_p08.rs:106`.
- R09: plan-only save (`workflow_jobs_cache.rs:27`), readers restore-only
  (`crate_jobs.rs:329`); push-gated saves only (`ci.yml`); tests
  `impl_orch_p08.rs:87`, `c11_cache_saves_push_only_prs_and_forks_read_only`.
- R10: owned-home subset rejecting `~/.cargo`/credentials
  (`mise/src/cache_sources.rs:47`); offline-probe→skip else explicit fetch
  (`source_prep.rs:106`); `--offline` on cargo cmds in `ci.yml`.
- R11: per-job cache fixture (`impl_cache_fixtures.rs`: shared sources
  keys/paths, qualified Mise identities, single plan writer,
  restore<MBX<fetch order); sequential-run evidence in
  `cache-measurements.md` (seed `36754512444`, warm `36760724180`, green
  `36777030585` @`695752e`: 21 entries / 1263.95 MiB stored, 12.3% of the
  assumed 10 GiB quota, zero evictions, per-job transfer/durations);
  `summarize_cache_usage` reporting path (`cache_trust.rs`, post-hoc by
  design — render stays hermetic).
- R12: `impl_cache_warm.rs` renders twice with identical inputs and asserts
  byte-identical workflows + identical keys, the offline-skip branch in
  every crate fetch step, `--offline` on every obligation, and disjoint
  MBX/Cargo shapes; hosted warm/green show zero fetch re-download;
  local `cacheprobe` fixture replays both branches verbatim.
- R13: `Step.condition` in contract IR with validation, serialized as
  step-level `if:` in both renderers (`document.rs`); Save Cargo sources
  plus every Setup Mise `cache_save` gated on `github.event_name ==
  'push'`; PRs (same-repo or fork) restore read-only; policy recorded in
  the Gate 4 doc; tests `c11_cache_saves_push_only_prs_and_forks_read_only`,
  `step_conditions_serialize_as_if_with_upload_default`.
- R14: main tree is `ci.yml` only, `name: CI`; stale removal by whole-tree
  swap (`generate.rs:208`); test `impl_renderer_tree.rs:116`. The scheduled
  `freshness.yml` probe (P12-4) stands alongside under the Velnor policy;
  consumers still get exactly `ci.yml` + `actionlint.yaml`.
- R15: `Rust / <label>` display, `rust-<slug>`+digest8 on collision
  (`contract/src/workflow/jobs.rs:183,201,216`); tests `crate_jobs_tests.rs:73`,
  `impl_crate_graph.rs:61`; dedicated collision test added (`86a6223`).
- R16: all 7 crate jobs hold Format/Clippy/Build/Test/Doctests/Doc steps;
  tests `crate_jobs_tests.rs:81`, `impl_crate_graph.rs:70`.
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
  Opt-in key shipped (`442fa8b`): `[stacks.rust] custom_tasks`
  allowlist (contract `stacks.rs` + orch `config.rs` + `init.rs`
  template); `mise run` steps emitted only for allowlisted names
  (`custom_run.rs`, `crate_jobs.rs`); negative tests both sides.
- R27: branding gate (`contract/src/workflow/jobs.rs:138`); unbranded IDs/names
  in `ci.yml`; tests `jobs.rs:390`, `crate_jobs_tests.rs:75`. Latent:
  `TASK_JOB_ID="velnor-task"` (`render.rs:46`) never emitted.
- R28: `derive_for_config` suppresses the workspace `Fmt` group when
  per-package `Fmt` groups exist for the same config. At HEAD: exactly 7
  `Format` steps (`ci.yml:622…2327`, one per crate), none in `plan`
  (`:119–337`); Format-count regression test green.
- R29: `README.md` documents the implemented CLI (`init`/`plan`/`generate
  --output-dir` + `--help`), the stale 47/47 dogfood line is gone, the
  MBX/Nextest detection model matches `workflow-contract.md:16`, the
  Alint pin is full-SHA on both sides (`version-policy.md:71,100` agrees
  with `workflow-contract.md:308`), and a consumer-installation section
  states no official release exists yet (source builds fail
  consumer-policy generation by design). This revision adds the 15-job
  count and tested clean-env local commands.
- R30: `docs/reviews/pr-1-adoption.md` (§§1–7) intact since `8529f1d`.
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
- F1: typed cargo `RUSTDOCFLAGS=-D warnings` env into every `TaskKind::Doc`
  leg (`matrix_step.rs`); 7/7 Documentation steps in `ci.yml` carry it;
  2 tests. Reply `5927509300`.
- F2: configured `runner_label` threaded into the actionlint bridge
  (`workflow.rs::actionlint_input`), fail-closed `InvalidRunnerLabel` on
  unlisted labels, no hardcoded distro in the render path; e2e test
  asserts no `ubuntu-26.04` leak under a 24.04 config. At HEAD all jobs
  run `ubuntu-26.04` and `actionlint.yaml` allows exactly that.
- F3: manifest parse requires the 40-hex commit (`manifest.rs`,
  `check_commit`); recorded as `VELNOR_RELEASE_COMMIT`; lock-backed
  attach path carries `generator.commit` (G-round batch); 3+ tests.
  Immutable-tag/SLSA process documented; tags + attestations need
  release infrastructure (external residual, see below).
- F4: `Swatinem/rust-cache` in both action allowlists, sample config
  (`cli-contract.md:197`), emission rule (`workflow-contract.md:303`),
  pins, policy doc, and freshness inventory; SHA consistent; Cargo-only
  gating tested.
- F5: fetch `continue-on-error` replaced by bounded `MAX_DOWNLOAD_ATTEMPTS=3`
  in-helper retry (`retrieve_retry.rs`); only the plan download retains
  `continue-on-error` so the gap reaches the merge verdict (exactly one
  occurrence in `ci.yml:477`); merge gating untouched; documented in
  `merge-report-contract.md`.
- F6: `MISE_LOCKFILE=0` split documented (`tooling-input-contract.md` §1.1,
  `task-execution-contract.md:144`): fixed steps never read repo config;
  custom tasks use the project lockfile.
- F7: typed `ARTIFACT_RETENTION_DAYS=30` on all upload constructors
  (`steps_artifact.rs` + 4 call sites); 11/11 `upload-artifact` steps in
  `ci.yml` carry `retention-days` (the 90-day baseline value is typed
  `BASELINE_RETENTION_DAYS` by design, not a leak).
- G1: took the "document" branch: `fork-pr-trust-model.md` specifies
  triggers, PR-tree execution, the self-attestation limit with code refs,
  what malicious regen can/cannot do (asset re-bind + sha256 fail-closed,
  push-gated saves, `needs` cross-check, plan-stamp re-check), and the
  trust root (CODEOWNERS + branch protection). No base-pinned validator
  exists; the doc specifies it with 4 acceptance criteria as future work.
  Reply `5931252884`.
- G2: REJECTED with counter-evidence. Config-visible installs (so mise
  would enforce the lock) were tried, then independently probed: mise
  loads 9+ config paths no scanner can enumerate, `[plugins]` shadows
  backends and executes `bin/install` even for untrusted configs,
  installs ignore trust, and a lone `mise.lock` is unenforced (tamper
  ignored). All installs carry `--no-config` + `MISE_NO_CONFIG=1` again
  (the finding's own first suggested fix); the lock audit is rescoped to
  local-dev lock-file hygiene; checksums are documented TOFU
  (`tooling-input-contract.md` §1.1). Proofs:
  `every_emitted_install_argv_carries_no_config`,
  `hostile_config_ignored_by_isolated_installs`.
- G3: fixed + documented. `continue-on-error` exactly once
  (`ci.yml:477`, plan download); missing reports → `planning_failed`;
  `needs` exact-inventory cross-check now derives from
  `required.needs` (also fixing the live bug where downstream
  `publish-baseline` broke every `Required` run), enforced at
  generation time and e2e-pinned. Behavior in `merge-report-contract.md`.
- G4: 15/15 jobs carry typed per-kind `timeout-minutes` (10 infra +
  validators, 30 crates), e2e-pinned.
- G5: zizmor installed in 3 jobs (plan + the 2 crates whose suites spawn
  generate-validation, down from 19) and RUN by the dedicated Zizmor job
  (`ci.yml:2528`); install set is exact and test-pinned
  (`crate_tools_install_exact_pinned_set`); `generate` shells out to all
  three validators via staged validation (`validate.rs:45-46`).
- G6: `gate-class-variances.md`: MSRV PR-exclusion permanent with a
  planned qualification workflow; schedule renders when present
  (emitters currently `None`); ruleset reads specified as a future
  validator shape; per-config doc env shipped (F1: `RUSTDOCFLAGS` on all
  doc legs, all configs).
- G7: same `runner_label` threading as F2, fail-closed
  `InvalidRunnerLabel`, e2e-pinned.

## Accepted residuals (not still-open requirements)

- F3-external: immutable release tags need a repo tag-protection ruleset
  (admin op, no release infra in-repo yet); attested builds ride future
  release automation. In-repo provenance (commit field, `VELNOR_RELEASE_COMMIT`,
  lock-backed commit) is shipped and tested.
- G1-future: base-pinned fork-PR validator is specified with 4 acceptance
  criteria in `fork-pr-trust-model.md`, unbuilt. The finding's either/or
  was satisfied by the documentation branch.

## Prior coordination notes (closed)

- N01: `non_utf8_path_broadens_explicitly` failed on Linux at `a74dd7f`
  (run 36759633324). Closed: green Linux run `36836254328` @`644fdf5`
  (success 14/14) executes the orchestrator suite including that test.
- N02: runner-side `actionlint@1.7.12` missing at `9e81355` (run 36751323928).
  Closed: actionlint installs via mise in plan/validating jobs, and the
  same green run proves it end to end.

## Current hosted runs at this revision

- Green: `36874320163` @`3a98511` (2026-10-01, success: 14 jobs green +
  `Publish baseline` skipped push-only) — last green; carries the
  hosted leaf-edit datapoint (cli job 87 s).
- Previous greens: `36870627159` @`0c9a6a7`, `36865471829` @`93dd3d4`,
  `36862207497` @`34550e8` (each 14 green + `Publish baseline`
  skipped), `36836254328` @`644fdf5` (success 14/14).
- Red: `36860814112` @`395d4bf` (2026-10-01, failure: `Rust /
  velnor-actions-contract` rustdoc intra-doc link + dependent `Required`).
  Cause matches the `34550e8` fix (public `WorkflowIr::validate` link);
  all other 12 jobs green, `Publish baseline` skipped (push-only).
