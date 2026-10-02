# OpenTofu Phase B final evidence summary (T36)

Identity: producer lane branch `impl/phaseb-postmerge` at
`f2021332ac099034cad250bbab60589766ea9eec` (tree clean, detached HEAD
verified); consumer repo `tailrocks/github-terraform`.
Required sections per ws7 Q7 (`velnor-ws7-e2e-qual.md:138-145`, pinned
`619ad6f1...40ace9`) + the T36 ledger row. Every item evidenced; nothing
invented. Gaps say MISSING or cite the open item explicitly.

## 1. Per-T-item table (T01–T36)

Columns: owner, status, paths (git-derived scopes), commit, commands+results,
reviewer, run/artifact links. Producer branch CI ran zero times on any
`feat/*` branch (PR/main-only triggers; `gh run list` empty per T24/T28
reports) — per-commit CI links are therefore honestly "none"; local gates
are the signal. Fn counts are `#[test]`-fn greps from the ledger.

| ID | Owner | Status | Paths / commit | Commands + results | Reviewer | Run / artifact links |
|----|-------|--------|----------------|--------------------|----------|----------------------|
| T01 | docs-adopter | complete | no commit (read-only inventory; `/tmp/velnor-ws1-inventory.md` §§2–7, `/tmp/velnor-ci-release.md`, `/tmp/velnor-t01t36.md`, `/tmp/velnor-mergev1-diag.md`) | inventory recorded in ledger | coordinator-accepted (no verify thread) | producer main runs #1/#5/#7 green; consumer main run 36889650178 RED + PR run 36859376510 RED |
| T02 | docs-adopter | complete | no commit (read-only; AGENTS.md + pr-1 triple + 16 proposed files + release interfaces) | failures classified in ledger | coordinator-accepted (no verify thread) | same runs as T01; #8 push run 36916298152 in-progress at check |
| T03 | docs-adopter | complete | no commit (read-only; consumer `.github` tree + O1–O16 map in `/tmp/velnor-obligations.md`) | baseline coverage proven (4 command sets byte-identical) | coordinator-accepted (no verify thread) | none (static proof) |
| T04 | phase-A impl | complete | 52 files +5543/-1, `f549983f997e747ed697da14507886de14c8df53` (parity corpus + opentofu-goldens + capture script) | parity 2/2, goldens check 5/5 | pending independent Phase-A review (ledger-stated) | none |
| T05 | phase-A impl | complete | no commit (WS2 tables re-grounded at 106bfd7, evidence in `/tmp/velnor-phaseA-impl.md`) | owner-per-fact assigned | coordinator-accepted (no verify thread) | none |
| T06 | phase-A impl | complete | 7 commits `f45a78c`..`a998b44` (M1 registry, M2 probes −114 net, M3 TOML edge, M4 selection, M5 edge pairs, M6 waist spike, M4 size followup) | each move: affected tests + clippy/fmt + both golden brackets green | coordinator-accepted (no verify thread) | none |
| T07 | phase-A impl | complete | `6fb0a5d13a51bad17ade2aff6411d7357973f07` ledger record (1 file); behavior proved at `a998b44` | verify-local.sh exit 0, nextest 2020/2020, alint rc 0, deny ok; goldens byte-identical after every move | coordinator-accepted (no verify thread) | none |
| T08 | phase-B impl | complete | 56 files +1175/-233, `e23d77032b344c50590169af07980eefbfd009bd` (8th member + arch/registry/lint amendments) | nextest 2031/2031; 2036 fns; fmt/clippy/goldens/deny/freshness/alint green | verify-t08 PASS 5/5 + coordinator | none (branch CI absent) |
| T09 | tofu | complete | 31 files +1888/-199, `e340a92d9cc224c60dc007553e0b4f50d24c0b6d` (strict `[stacks.tofu]` + evidence rules) | nextest 2101/2101; 2106 fns (+70); all gates green | verify-t09 PASS 5/5 + coordinator | none (branch CI absent) |
| T10 | tofu | complete | 37 files +3646/-230, `37838f555d19a23189642b340d69781df8fb7ff3` (hcl-rs 0.19.8 + caps + precedence + fmt scope) | nextest 2215/2216 (sole p12_live 403 environmental, base-identical, accepted); 2221 fns (+115) | verify-t10 PASS 5/5 + coordinator | none (branch CI absent) |
| T11 | tofu | complete | 24 files +2346/-20, `383a8c3020dad33ac5428623f6e2b016102dd4b0` (closure + selection + fallback) | nextest 2275/2275; 2280 fns (+59); all gates green | verify-t11 PASS 5/5 + coordinator | none (branch CI absent) |
| T12 | tofu | complete | 50 files +2509/-80, `494af593b7a2956eb26b2aac57a8031cd0a946cd` (task kinds + IDs + diagnostics + base-graph) | nextest 2318/2318; 2323 fns (+43); all gates green | verify-t12 PASS 5/5 + coordinator | none (branch CI absent) |
| T13 | tofu | complete | 21 files +863/-159, `1270226c77e12d5fbd7d80ebb853990fa08c8724` (typed argv payloads) | nextest 2336/2336; 2341 fns (+18); all gates green | verify-t13 PASS 5/5 + coordinator | none (branch CI absent) |
| T14 | tofu | complete | 15 files +1467/-18, `95833a02ee96dfbe6fdd9f53c25ae4309c5913ab` (lock + version diagnostics, read-only) | nextest 2383/2383; 2388 fns (+47); all gates green | verify-t14 PASS 5/5 + coordinator | none (branch CI absent) |
| T15 | mise | complete | 4 files +139/-18, `b0ec8edeaead60d5360e12fd8c433b2635d20ff3` (OPENTOFU_VERSION 1.13.1 + selector + digests + freshness) | nextest 2386/2386; 2391 fns (+3); all gates green | verify-t15 PASS 5/5 + coordinator | none (branch CI absent) |
| T16 | mise | complete | pair `caeb8c2823fb6837826ed4dbf824e2605803fcd0` (23 files +1112/-31, spawn path + isolation) + `de478d9780fb07db51872e2adab2d1804a664bc5` (1 file fmt) | fmt-commit byte-equals rustfmt; 2418 fns (+27); all gates green | verify-t16 PASS 5/5 + coordinator | none (branch CI absent) |
| T17 | orchestrator | complete | 18 files +1114/-313, `d36e81a08ea22179289cd65fa0532826af65242f` (per-role tools; pure-tofu drops Rust) | nextest 2422/2422; 2427 fns (+9); all gates green | verify-t17 PASS 5/5 + coordinator | none (branch CI absent) |
| T18 | orchestrator | complete | 21 files +1147/-94, `23bd0ccc958c77121c0a13b5ef0709659ce53384` (root-grouped jobs + init-once + fan-out) | nextest 2443/2443; 2448 fns (+21); all gates green | verify-t18 PASS 5/5 + coordinator | none (branch CI absent) |
| T19 | orchestrator | complete | 21 files +964/-159, `8f8a39013a6920aa09fd973410ff3d197b0ec66a` (exact-set + fail-closed merge) | nextest 2463/2463; 2468 fns (+20); all gates green | verify-t19 PASS 5/5 + coordinator | none (branch CI absent) |
| T20 | renderer | complete | 14 files +767/-22, `6c7b7a2dab4f946bfa9123bd45cc1f7124ab2007` (stable names + determinism + preservation) | nextest 2476/2476; 2481 fns (+13); all gates green | verify-t20 PASS 5/5 + coordinator | none (branch CI absent) |
| T21 | cache | complete | 44 files +1678/-110, `b9628a022cff63cc53ac807610dabae24096cb58` (provider cache identity/transport/trust) | nextest 2504/2504; 2509 fns (+28); all gates green | verify-t21 PASS 5/5 + coordinator | none (branch CI absent) |
| T22 | cache | complete | 13 files +935/-125, `df54abb82df30448118de45c64294ef1c0b32129` (never-archive + negative battery + cold recovery) | nextest 2530/2530; 2535 fns (+26); all gates green | verify-t22 PASS 5/5 + coordinator | none (branch CI absent) |
| T23 | cache | complete | pair `fc864885f228d468b89a41ac23f73b615097619f` (6 files +389/-3, reuse OFF + hit-executes) + `e2ecde4a3d15dd2da800b5e1467ce3e58e25a1c2` (1 file fmt) | nextest 2536/2536; 2541 fns (+6); 3 listed equal-or-stronger flips; all gates green | verify-t23 PASS + coordinator | none (branch CI absent) |
| T24 | perf | complete | 7 files +1273/-0, `9a1b297afdd1c1c0d5c0e914b2299fc8ba8b515f` (7 gate tests + bench matrix + scaling + perf doc) | nextest 2554/2554 (+18); cold n=5 + warm n=5; all gates green | verify-t24 PASS 5/5 + coordinator | none (local-only evidence, stated) |
| T25 | perf | complete | 35 files +1321/-322, `35104bdd1f1cdee50aef5bc7525dd7d378dcdd74` (FileCache + task_ms + corrupt-cache bench + perf doc) | nextest 2561/2561 (+7); warm-plan PARTIAL (local PASS/hosted UNMEASURED); all gates green | verify-t25 PASS 5/5 + coordinator | none (local-only evidence, stated) |
| T26 | quality | complete | 7 files +306/-56, `b2f1ed77f47a8ad22a5df4fa523f0cb7cbd91c0d` (spec §3 + invariants 10–12 into quality contract) | nextest 2568/2568 (+7); 7/7 new pins; all gates green | verify-t26 PASS 5/5 + coordinator | none (branch CI absent) |
| T27 | quality | complete | 9 files +1705/-0, `667b692aafb7a8f2b00d9bc3bf9544dbf8b4d324` (§10 matrix: 49 tests, real-binary hermetic + 1 ignored live) | nextest 2616/2616 + 1 skipped; all gates green | verify-t27 PASS 5/5 + coordinator | none (branch CI absent) |
| T28 | quality | complete | 5 files +118/-4, `e32df58cf2b6631aaa36ed6ccc5a67f70a2c49e1` (comment-only doc/shellcheck fixes + qual record) | nextest 2616/2616 + 1 skipped; verify-local 37 ok; Gates 0–8 keys 54/54; actionlint/shellcheck/zizmor/machete/realbin 9/9 green | verify-t28 PASS 5/5 + coordinator | branch `gh run list` empty (PR/main-only triggers) |
| T29 | review | complete | 9 fixing commits `1c2f3b2`..`549b82f` + close `c2857db` (ledger `review-ledger-t29.md`: 22 fixed / 9 accepted / 0 open) | per-commit nextest 2617→2634 (+18 pins), each green; c4/c5 sole env-403 accepted | review-t29-{arch,sec,perf} + verify-dispo-c1/c23/c4/c5/c6/c7(FAIL→C9 fix-forward)/c8/c9, all PASS at end + coordinator | none (branch CI absent) |
| T30 | review | complete | 1 file 1 line, `48a33f39475270941ca370e65c3d40cb8bc95f66` (ledger row flip) | re-fetch at 7a0286e: 0 reviews, 0 threads, 1 bot notice (not a finding); nextest 2646/2646 + 1; zero drift | impl-t30 + coordinator-accepted (no verify-t30 thread) | PR #10 run 36991743365 (in_progress at re-fetch; later mise-job red → fixtofu-ci) |
| T31 | release | complete | no repo commit (NO PUSH honored); artifact + scratch manifest under impl-t31 `t31-scratch/` | source 95c1d6f + run 36998679726 re-verified; build exit 0; `--version 0.1.0`; DL+shasum+smoke OK; mutation RED 1 / GREEN 0 | impl-t31 + coordinator independent re-runs (no verify-t31 thread) | producer push run 36998679726 success 16/16 on exact 95c1d6f; artifact file:// URL + sha (UNPUBLISHED) |
| T32 | consumer | complete | 2 test files +274/-0 `516407309b3c12cc4b38ab18770811a73514cbb9` + row flip `22a7360d6fd015b9c6fcf9370ca105342fa8898b` | preview B EXIT 0 ×2 byte-identical; O-table closed O3/O7, O10 flag stands; 6 negatives (5/5 red-before, 6/6 green-after); nextest 2656 + 1 | impl-t32 + coordinator (independent preview-B regen IDENTICAL; no verify-t32 thread) | none (no CI-acquisition claim; linux asset unpublished) |
| T33 | consumer | complete | consumer commit `3abf81e1b972d0362e5ef60de25a5f6ee2932e5f` (parent f0eedf3, 5 files +323/-37) + producer row flip `690f3eec0d73f41e9065f09d66ef3d1f18df507b` | 6 preserved files byte-identical; `.github` IDENTICAL to preview B; 5× ubuntu-26.04; markers head all 3 | impl-t33 + coordinator (independent regen IDENTICAL; no verify-t33 thread) | consumer PR tailrocks/github-terraform#37 |
| T34 | consumer | complete | producer row flip `fa336617027b781a3ed0dcd88edc5591ae2701f8` (verification complete; PR CI verdict FAIL, honest) | run 37002672312 on exact 3abf81e: failure (Acquire 404 ×2, tofu lanes skipped, 0 artifacts); manifest byte-identical (sha `16394c54…`); 0 reviews/threads | impl-t34 + coordinator independent re-runs (NO verify-t34 thread exists; ledger "verify-t34" label is imprecise) | consumer PR run 37002672312 failure |
| T35 | consumer | pending | producer row flip `f2021332ac099034cad250bbab60589766ea9eec` (BLOCKED record + push-run recipe) | 0 unaddressed feedback; checks red-on-final-SHA; waiver search: none; merge NOT performed | impl-t35 + coordinator (NO verify-t35 thread exists; ledger "verify-t35" label is imprecise) | no push run (no merge; main stays f0eedf3) |
| T36 | docs-adopter | complete | this doc + 1-line T36 row flip (this commit) | fmt + freshness + full nextest at head (see §2) | impl-t36 + coordinator (no verify-t36 thread by design) | none (docs-only) |

### Non-T producer commits verified in-lane

- `58997d4c1660e3b68dc9f09dd711b3cd85381c04` fixtofu-ci (11 files
  +297/-14): mise Prepare gains `opentofu@1.13.1` for tofu-spawning
  suites; nextest 2649/2649 + 1. PASS 5/5 (verify-fixtofu-ci) +
  coordinator. Fix content present in-lane and in 95c1d6f
  (`matrix_tools.rs`/`matrix_step.rs` zero-diff vs lane head;
  `opentofu@1.13.1` Prepare line in both ci.ymls; surrounding files
  evolved further via merge2/alint-fix).
- `141693be57c54b965e80f61e217e1e252c404434` fixtofu-alint (14 files
  +106/-175): golden capture `cp -RP` + link-pinned hashes; nextest
  2650/2650 + 1; alint 5 errors → 0. PASS 5/5
  (verify-fixtofu-alint) + coordinator. Fix content present in-lane
  and in 95c1d6f (capture script zero-diff vs lane head; `link`
  lines in 95c1d6f tree.sha256).
- `6435b71` merge2 (PR #11 agent docs into feat): verify-merge2 FAIL
  (F4 alint red on recaptured previews; F5 message alint claim
  inaccurate; F1/F2/F3 green). Superseded: equivalent content +
  the alint fix are in 95c1d6f; the FAIL is recorded, not hidden.
- `c2857db` T29 close (T08–T29 ledger complete) and `7a0286e` main-merge
  are recorded under T29/T30; `da4f2ac` (PR #11) and `aaae477` (PR #9)
  are main-line parents, not Phase-B deliverables.

## 2. Source SHAs, artifact, toolchain, local gates

- Producer T31 source (PR #10 squash): `95c1d6f0f1056881e42d53846dac8ffccd7aa6a5`.
- Producer lane head: `f2021332ac099034cad250bbab60589766ea9eec`
  (`impl/phaseb-postmerge`; = T35 record commit; code tree identical
  to 95c1d6f plus T32 tests + evidence rows).
- Consumer main SHA: `f0eedf3c14ca78ab2967636d1d4dde613f16578d`
  (UNCHANGED — no merge performed).
- Consumer PR: tailrocks/github-terraform#37, head
  `3abf81e1b972d0362e5ef60de25a5f6ee2932e5f` (OPEN, MERGEABLE).
- Generator artifact: `velnor-actions-0.1.0-aarch64-apple-darwin`
  (Mach-O arm64, 7609536 bytes),
  sha256 `c8759f3e4f486900dafc13c75ce4e116d403d0ecd3ac98efd1b112d91a4064c2`,
  `velnor-actions 0.1.0`, built per bootstrap §4
  (`mbx build --release --locked`, rust 1.98.1 / mbx 1.21.0).
  URL: `file:///…/velnor-phaseb-impl-t31/t31-scratch/artifact/velnor-actions-0.1.0-aarch64-apple-darwin`
  (content-addressed LOCAL path — UNPUBLISHED, never relabeled).
  Would-be release URL
  `https://github.com/tailrocks/velnor-new/releases/download/v0.1.0/…`
  does NOT exist (no v0.1.0 tag; 9 seed-only releases).
- Scratch manifest (UNPUBLISHED): commit == source 95c1d6f, version
  0.1.0; host record real measured sha; linux record all-zeros
  fixture (no linux bytes); manifest bytes sha256
  `16394c54197c82ed37f5f1818963ebb320ab2803ade375901808b6df00e94add`
  (consumer copy byte-identical).
- Exact tofu version: 1.13.1 (`OPENTOFU_VERSION`, T15; consumer lanes
  install `opentofu@1.13.1`; consumer mise lock still 1.12.5 —
  recorded `version_drift`, T32/T33 input). Linux tarball digest
  `378ada19d4bc70c43732004e8159be771b23b9a5afdf059e5f8a2b3fa2c70a69`
  (release-API channel; no SHA256SUMS asset exists).
- Local gates at lane head f202133 (this session):
  `cargo fmt --all -- --check` clean; `bash scripts/check-freshness.sh`
  PASS; `cargo nextest run --locked --workspace` (pinned aqua 0.9.146)
  2656 run, 2656 passed, 1 skipped (the `#[ignore]` live-registry
  test), exit 0.

## 3. Producer + consumer run IDs and results

Producer (`tailrocks/velnor-new`):
- 36998679726: CI, event push, headSha == 95c1d6f (exact T31 source),
  conclusion success, 16/16 jobs (Actionlint, Machete, Alint, Zizmor,
  Deny, Plan, 8× Rust crates, Required, Publish baseline).
- 36991743365: PR #10 run on exact head 7a0286e; in_progress at T30
  re-fetch (6 pass, Plan pending); later red on the mise job
  (missing opentofu install — the fixtofu-ci cause).
- 36992365509: repro run at base 48a33f3, FAILURE (mise job:
  `opentofu@1.13.1 is not installed`, real_tofu_* FAILs).
- Zero runs on any `feat/*` branch (PR/main-only triggers).
- Phase-A era (T01): producer main runs #1/#5/#7 green; #8 PR lane
  red→green then success; #8 push run 36916298152 in-progress at check.

Consumer (`tailrocks/github-terraform`):
- 37002672312: CI on exact PR #37 head 3abf81e, conclusion failure.
  Actionlint SUCCESS, DCO SUCCESS, Plan FAILURE, Required FAILURE,
  `OpenToFu — .` SKIPPED (`needs: [plan]`), Publish baseline SKIPPED.
  Cause: Acquire 404 on the unpublished v0.1.0 linux asset (exit 22),
  0 artifacts. The predicted honest result.
- 36889650178: consumer main (f0eedf3) RED — Required merge-v1 silent
  exit 1 (`final-report.json` status `no_work`); 36859376510: same on
  the old PR (both pre-migration, T01).
- No push run on a merged main SHA: no merge occurred (T35 BLOCKED).

## 4. Performance samples vs budgets (harness-measured only)

Budgets (contract §4.5/§9 text): 120 s warm critical path, 10 s warm
plan; ≥3 cold + ≥5 warm samples; matrix incl. corrupt-cache/mixed/fork;
1/10/100-root synthetic scaling. Tofu `TaskTiming` slots were all-None
pre-T25 (`qualification.rs:344`, `decisions.rs:228` at df54abb); post-T25
only `task_ms` is measured (rest zero/unmeasured) — NO timing-slot value
is cited below as a measured sample. All numbers are nextest-harness
walls from `perf-tofu-t24.md` (measured at DANGLING `f57e0b4`) and
`perf-tofu-t25.md` (DANGLING `c2a77b7`; doc-only-delta proof commands in
both docs). Env per number: arm64, macOS 27.0, 128 GiB RAM; rustc/cargo
1.98.1, mise 2026.9.16, mbx 1.21.0 (T24), nextest 0.9.146 (pinned aqua),
git 2.56.0, python 3.14.8; fixtures `tofu_repo` N-root builder,
`BENCH_ROOTS = 10`; opentofu T15 pin, no install performed locally.
T24 method: 5 parallel runs each, sorted+median; T25: 5 serial
(`--test-threads=1`) runs each + same-machine A/B base control.
Sample counts: 5 cold + 5 warm + 5 each docs/root/module/lock/mixed/fork
(T24) and 9-case ×5 incl. corrupt-cache (T25) — thresholds MET (local).

| Case | T24 plan_ms med (n=5) | T25 plan_ms med (n=5) | Digest (same set) |
|------|----------------------|----------------------|-------------------|
| cold (first plan) | 501 | 375 (base 352) | 1f4af9af486002c4 |
| warm (second plan) | 299 | 280 (base 298) | same |
| docs touch | 486 | 352 (base 346) | same |
| root touch | 498 | 351 (base 348) | same |
| module touch | 370 | 261 (base 259) | bd79216a2db0378c |
| lock touch | 367 | 260 (base 247) | c5f4a62ead7e3479 |
| mixed touch | 416 | 304 (base 302) | 7f64b156bd26269e |
| fork event | 300 | 282 (base 292) | same-as-PR |
| corrupt-cache | — | 247 (plan+fixture-commit wall) | c5f4a62ead7e3479 |

Scaling medians (n=5): plan 1/10/40 roots 239/310/704 ms (T24),
205/298/618 ms (T25); prepare 1/10/100: 27/34/93 (T24), 17/21/77 (T25);
generate 1/10/100: 271/357/1598 (T24), 213/361/2009 (T25). Plan fails
closed past 40 roots on the 320 KiB matrix budget (pinned, C3).

Budget verdicts (kept open, never redefined):
- 10 s warm plan: PARTIAL — local analysis PASSED with 30× headroom
  (warm med 280 ms); hosted plan-job wall UNMEASURED (no
  tofu-consumer CI exists; producer rust-plan runs are not a proxy).
- 120 s warm critical path: UNMEASURED (no hosted runs).
- Two-minute warm SMALL-FIXTURE path: still UNPASSED (no
  small-fixture CI run exists). STAYS OPEN per Q7.
- CI-hosted walls, queue, cache transfer, provider download/restore/
  save, runner-seconds: UNMEASURED (no hosted runs; no run links).

## 5. Parity evidence + removed legacy paths

Parity: obligations map `velnor-obligations.md` (pinned
`6f5dc527...21bb6a8`); O1–O16 verdicts in the T32 report §3 against
preview B (f0eedf3 + scratch manifest + 3-line `[stacks.tofu]
roots=["."]`, EXIT 0, deterministic regen byte-identical, plan↔generated
step counts 4/9/5/9/11, task IDs 3–5×): O1 replaced-inputs drive
preview; O2 unchanged consumer file; O3 CLOSED (native fmt/init-
readonly/validate lanes, ci.yml:368/398/427); O4 no state file; O5
triggers + drift check + full-SHA pins; O6 Required needs + exact-set
merge; O7 CLOSED (mise opentofu@1.13.1 + lock-keyed restore +
push-gated save); O8a drift check ✓ / O8b actionlint ✓ / O8c trust
still DROPPED; O9 still absent (same justification); O10 still DROPPED
WITHOUT justification — no `schedule:` trigger (FLAG STANDS); O11
actionlint.yaml retained (7 lines, zero-byte delta); O12 generated
agent docs coexist with untouched root docs; O13 all-hosted
(5× ubuntu-26.04); O14 `Required` + external DCO; O15 local mise tasks
byte-identical, CI touches no secrets; O16 verify-policy.sh (100755)
byte-identical, never CI-wired. Negatives: malformed→
`detection: malformed_manifest:<file>` plan-closed; failed plan→
`Failed`; unformatted→fmt exit-3 leg→`Failed` + published red;
module/lock changes select exactly the root triple (6 hermetic E2E
tests, 5/5 red-before / 6/6 green-after).
Removed legacy paths: at consumer base f0eedf3 the O1/O2/O4/O13/O14
legacy paths were ALREADY removed (by `27fae6d`); T33 verified absence
(`github-gen|github/ci|ci-main|ci-pr|ci-policy|ci-unit|maintenance|
nightly|fleet|generator-state|visibility` → 0 matches; `.github-gen/`,
`.github/ci/`, `config/fleet/` absent). Nothing further deleted; no
shims, aliases, or deprecation periods (AGENTS.md:7). Superseded
consumer PRs #34/#31 noted, not touched.

## 6. Review dispositions

- T29 independent reviews at audited head
  `e32df58cf2b6631aaa36ed6ccc5a67f70a2c49e1`: arch 1 blocker / 2 major
  / 4 minor / 3 notes (A1–A10, CONDITIONAL); sec 0 / 3 / 5 / 9
  (B1–B17); perf 0 / 0 / 0 / 4 (C1–C4). 31 findings, 1 overlap
  (A1==B1) → 30 distinct.
- Disposition: `review-ledger-t29.md` — 22 fixed with SHAs, 9 accepted
  with evidence (B8, A9, A10, B10, B12, B14, B15, B17, C4), 0 open.
  9 fixing commits c1–c9 (`1c2f3b2`..`549b82f`), each dual-verified;
  C7 FAIL (A8 pin gap) closed by C9 fix-forward PASS. Residuals owned:
  B3 live-path wiring (coordinator/future lane), B8 warn (coordinator),
  A10 E2E observation (T32 lane — see §7: not explicitly recorded).
- T30 re-fetch at final head 7a0286e (PR #10 OPEN MERGEABLE): 0
  reviews, 0 inline threads (0 unresolved/outdated), 1 issue comment =
  Codex bot quota notice (not a finding). Zero new findings; all 31
  dispositions re-verified current (22 behaviors green, 9 rationales
  re-verified in code).
- T34/T35 feedback tables (PR #37): 0 formal reviews, 0 inline
  comments/threads (incl. outdated), 1 issue comment = Codex
  usage-limits bot notice (not feedback), timeline +1 bodiless
  cross-repo `referenced` event (not feedback). 0 unaddressed; 0
  replies/resolves made; 0 waivers sought or given. T35 waiver search
  over title/body/reviews/comments/threads/timeline: none.
- Producer adoption-era feedback (T01/T02): PR #1 squash-merged
  `8b268d1` (279/279 DCO, final run 36881271764 all 16 pass);
  follow-up PRs #2/#3/#4 all merged; no open producer PRs at adoption.

## 7. Every remaining limitation (nothing waived)

UNRUN / UNMET / BLOCKED:
1. T35 BLOCKED — no merge authorized (T34 FAIL + no human waiver + no
   MERGE-GO): PR #37 OPEN at 3abf81e, main unchanged at f0eedf3, zero
   mutations. No push/main-CI run on a merged SHA exists. Unblock =
   explicit PR-specific MERGE-GO (or T34 READY via release + green PR
   CI) AND a human waiver or satisfied gates. Push-run inspection
   recipe recorded in the T35 report §6 (unexecuted).
2. v0.1.0 release UNPUBLISHED — linux asset 404 is the root cause of
   consumer CI red. T31 NEEDS-HUMAN ×6: official publication
   (per-target assets + manifest + Sigstore/SLSA; release manager),
   2 admin approvals (2 repo admins), independent rebuild (second
   builder), protected release job + environment (repo admin), branch
   protection on velnor-new + velnor mains (repo admin), manual build
   review (repo admin). T34 missing-steps ×3: publication, attestation
   (release manager), second-reviewer byte-identity (repo admin).
3. Budgets: 120 s warm path UNMEASURED; hosted plan-job wall
   UNMEASURED (10 s verdict PARTIAL); small-fixture 2-min path still
   UNPASSED — STAYS OPEN. Queue/transfer/provider/ runner-seconds
   UNMEASURED. No hosted tofu-consumer CI exists.
4. A10 carried observation (`max-parallel` E2E behavior in the T32
   preview) is not explicitly recorded in the T32 report — open
   follow-up, owner coordinator.
5. One `#[ignore]`d test: `live_tofu_registry_init_downloads_provider`
   (registry network; opt-in via `VELNOR_LIVE_TOFU=1`).

ACCEPTED-WITH-EVIDENCE (complete, not open):
6. T04 reviewer still "pending independent Phase-A review"
   (ledger-stated); T01–T03/T05–T07 coordinator-accepted without
   dedicated verify threads.
7. No verify-t30..t35 threads exist: T30–T35 rely on impl-thread
   evidence + coordinator independent re-runs. The T34/T35 ledger rows
   name "verify-t34"/"verify-t35" reviewers — imprecise labels; the
   reviewers were coordinator re-runs (this summary corrects it).
8. T10 nextest 2215/2216 (sole p12_live live-API 403, base-identical,
   accepted by impl + verify); dispo c4/c5 same single env-403
   (accepted, clean c6–c9).
9. B3 residual (no live-path post-restore verification; downgrade
   complete), B8 deferred warn (V1 accepts recorded-but-unpinned),
   B12/C4/B10/B14/B15/B17/A9 acceptances — all recorded in the ledger.
10. O10 flag stands (no schedule trigger); O8c trust still dropped;
    O9 dropped-justified; consumer mise lock 1.12.5 vs catalog 1.13.1
    drift flag stands.
11. T31 artifact is single-host-target; linux record is a zeros
    fixture; scratch manifest unpublished; artifact lives in a sibling
    worktree path (outside any repo); no seed relabeled (seed6
    `517c1d6` still the consumer pin; seed9 `09b443e` already stale at
    T01 — seed supersession recorded, unresolved by design).
12. Measurement commits `f57e0b4`/`c2a77b7` DANGLING (doc-only-delta
    proof commands in the perf docs); qual-input long material lives
    in `/tmp` (`/tmp/velnor-phaseb-research-tmp/q1-parser-prequal.md`,
    `q2-t08-checklist.md`, `q4-digest.md`, ws1–ws7 reports) — ephemeral
    paths, contents quoted in reports/ledger.
13. merge2 FAIL recorded (alint 5 errors on recaptured previews +
    message inaccuracy); superseded by in-lane fixed content, not waived.
14. Doc staleness (T08-era, evidence-only, flagged by T26, untouched):
    `requirements-evidence.md:24` seven_members (+:34 tree_policy fn),
    `.alint.yml:15` seven manifests.
15. Toolchain skews: local alint 0.17.0 vs CI 0.16.1 (verdicts transfer;
    findings are config-rule based); `AGENTS.md` deny `--locked` order
    stale for cargo-deny 0.20.2 (plain `cargo deny check` used
    throughout); some verifies used nextest runner 0.9.143 (pinned
    0.9.146 unavailable offline) with identical counts.
16. Goldens: dogfood `Repository:` worktree-path line is a standing
    artifact (5/5 only in the canonical checkout; 4/5 + 1-line diff
    elsewhere); CI does not run the golden check.
17. Freshness: 3 temporary holds expire 2026-10-15 (verified at T28;
    re-check at any later identity or exception-expiry fails).
18. Consumer main protected by ruleset `protect-main` ONLY (squash-only,
    linear, thread-resolution, 0 approvals, NO required checks);
    classic protection 404. DCO observed-only (external).

## 8. Provenance (section → evidence)

- §1 rows T01–T03: ledger + `/tmp/velnor-ws1-inventory.md`,
  `/tmp/velnor-ci-release.md`, `/tmp/velnor-t01t36.md`,
  `/tmp/velnor-mergev1-diag.md`, `/tmp/velnor-obligations.md`;
  T04–T07: ledger + commits a12efd7/f549983/M1–M6/a998b44/6fb0a5d;
  T08–T28: ledger + impl-tXX/verify-tXX reports + commits (full SHAs
  via `git rev-parse`); T29: ledger + review/dispo/verify reports;
  T30–T36: ledger + impl-tXX reports + row-flip commits.
- §2: T31 report §§1–5 (source SHA, run, artifact, manifest, tofu
  version), research1 Q4 (linux digest), T32/T33 reports (drift,
  versions), this session's gate runs at f202133.
- §3: T31 §1 (36998679726), T30 (36991743365), verify-fixtofu-ci F2
  (36992365509 + mise-job red), T01 rows (36881271764, 36889650178,
  36859376510, #1/#5/#7), T34 §§1–4 (37002672312), T35 §2.
- §4: `perf-tofu-t24.md` + `perf-tofu-t25.md` (numbers, env, method,
  verdicts) + impl/verify-t24/t25 reports; contract §4.5/§9 (budgets).
- §5: pinned `velnor-obligations.md` + T32 §§2–4 + T33 §§2–6.
- §6: `review-ledger-t29.md` + review/dispo/verify reports + T30/T34
  §6/T35 §§1–3 + T01/T02 adoption rows.
- §7: T35 (1), T31 §8 + T34 §5 (2), perf docs §3/§4 (3), ledger §4 +
  T32 (4), T27/T28 (5), ledger + thread listing (6–7), T10/dispo (8),
  ledger §§3–4 (9), T32/T33 (10), T31 + T01 (11), perf docs + research1
  (12), verify-merge2 + §1b (13), impl-t26 (14), T32/verify-t28/
  verify-t10 (15), verify-merge2/impl-fixtofu-alint (16), verify-t28
  (17), T34 §7/T35 §2 (18).
