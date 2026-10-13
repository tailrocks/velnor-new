# P13 inventory reuse + bench-harness measurements

Split from [performance.md](performance.md) (file-size gate); budgets in the parent doc govern.

## P13 inventory reuse + perf foundations

Commit measured: `c938a8c` (branch `docs/velnor-actions-spec`;
working tree carried concurrent-builder uncommitted changes).
Machine: `arm64`, macOS 27.0, Apple M5 Max, 128 GiB RAM.
Toolchain: rustc/cargo 1.98.1, mise 2026.9.16, mbx 1.20.0,
nextest 0.9.143, git 2.56.0, python 3.14.7.
`CARGO_TARGET_DIR=/tmp/t-p13` for every build below.

Method: `run_inventories` reuses one fetched `cargo metadata`
record per member manifest after membership validation (own
`[workspace]` roots never reused); lanes stay 1 by design
(parallel metadata contends on Cargo's package-cache lock).
`perf_fixtures_p13` builds 1/10/100-crate workspaces,
`perf_harness_p13` times `prepare`/`plan`/`generate` (`perf:`
lines); subprocess counts come from a unit counting harness
(one stub load = one subprocess — exact, not sampled).

Counts (exact): 1 member → 2 legacy / 1 reused; 10 → 11 / 1;
100 → 101 / 1. Root- vs member-manifest metadata differ only in
`workspace_default_members`, which the parser never reads
(identical `WorkspaceRecord`s asserted); legacy-vs-reuse
outcomes/inventories asserted identical incl. nested,
independent, and malformed cases.

Wall times, two runs each (`prepare`/`plan`/`generate` in ms;
generate split as prepare+render; obligations/matrix in parens):

| Crates | prepare | plan | generate |
| --- | --- | --- | --- |
| 1 | 52, 69 | 592, 611 (8 obl) | 43+183, 41+183 |
| 10 | 52, 62 | 686, 718 (44 obl) | 45+311, 45+314 |
| 40 | — | 1161, 1247 (164 obl) | — |
| 100 | 86, 104 | budget (see below) | 110+1742, 107+1713 |

Generate column re-measured post-shellcheck-batch (local arm64,
rustc 1.98.1, n=2; prepare/plan columns older). The old ~770 ms
flat row dated from single-matrix-job renders; P05 grew staged
bodies 16→618 while validation spawned one shellcheck per body
(~27 s storm at 100, bisected to `7a2cc3a`). One argv restored
sub-2 s; residual ~1.7 s is actionlint's single YAML pass.

Direct `cargo metadata --no-deps --offline` on the 100-crate
fixture: 0.04, 0.03, 0.04 s — a lower bound per eliminated
subprocess; the mise-wrapped cost is UNMEASURED. A 100-crate
full plan fails closed with `matrix_budget_exceeded:595321`
(it exceeded the then-current 320 KiB cap and also exceeds the current
512 KiB artifact cap, never truncates), so plan scales to 40 while
`prepare` scales to 100. Counts are O(workspaces) vs
O(manifests) by construction; wall shapes (prepare 52→104 ms,
plan rising with obligations, generate ~180 ms → ~1.7 s across
1→100 crates after the shellcheck batching) are n=2
observations, not fitted complexity (UNMEASURED — the old
"sublinear/linear/flat" wording overclaimed).

Budgets status: no P13 budget PASSED — legacy-vs-reuse wall
comparison explicitly UNPASSED (legacy path removed; counts
exact, walls optimized-path only); mise-wrapped subprocess
cost UNPASSED; CI-hosted numbers UNPASSED (no runs). Earlier
budget verdicts above are unchanged and were not re-measured.

`scripts/verify-local.sh` (sole P13 entrypoint) runs fmt, repo
policy, generated-tree freshness, per-crate clippy/tests, and a
nextest-`ci` integration pass. A full pass was blocked by
concurrent-builder drift at measure time (historical, superseded:
lane run at `69dcc90` is `verify-local: PASS`, 33 stages).
Deferred: P05-gated negative pipeline tests (no P13 code depends
on it); Nextest-archive/sharding stays off, unqualified.

## P13 benchmark cases 1–7 (bench harness)

Commit measured: P13 working tree atop `444c88e` — DANGLING (local-only commit, contained in no branch, absent from origin; figures below are historical and not reproducible from any branch head).
Machine: `arm64`, macOS 27.0, Apple M5 Max, 128 GiB RAM.
Toolchain: rustc/cargo 1.98.1 (48a229cea/797e8a9bc), mise 2026.9.16,
mbx 1.19.0, nextest 0.9.143, git 2.56.0, python 3.14.7; case 7 second
toolchain: rustc 1.97.1 (8bab26f4f).

Method: `impl_bench_p13` plans the same 10-crate fixture in
every case (44 obligations; `digest=` over sorted task IDs proves
the same set). `setup_ms` builds the fixture, `plan_ms` runs
`plan_internal`, `metadata_ms` is a direct-`cargo metadata`
lower-bound proxy, `rss_kb` is whole-test-binary peak RSS (not
isolated plan RSS). Queue/transfer are local no-ops; CI values
need hosted runs. Raw `bench:` lines via the orchestrator test
binary with `--nocapture`. Two runs each; figures are run1/run2.

| Case | plan_ms | setup_ms | metadata_ms | rss_kb | Digest |
| --- | --- | --- | --- | --- | --- |
| 1 cold (empty caches) | 1095, 1081 | 73, 40 | 16, 19 | 343520, 343904 | cd8b34ba18f0982d |
| 2 warm restore | 952, 969 (cold 996, 990) | 73, 40 | 15, 15 | 345312, 345728 | same |
| 3 unchanged repeat | 945, 961 | 72, 41 | 16, 16 | 345264, 345760 | same |
| 4 leaf edit (c009) | 1096, 1080 | 73, 41 | 18, 19 | 343232, 343216 | same |
| 5 API edit (c000) | 991, 987 | 74, 41 | 16, 16 | 345312, 345488 | same |
| 7 dep edge c009→c008 | 1079, 1079 | 72, 40 | 15, 16 | 345312, 345648 | same |

Case 6 lanes (sequential sum vs parallel wall; lanes execute
`plan_internal` analysis, NOT fmt/clippy/test validation):

| Lanes | sequential_ms | parallel_ms | lane_ms | contention |
| --- | --- | --- | --- | --- |
| 2 | 1931, 1949 | 944, 951 | [943,944], [950,951] | 0% |
| 4 | 3626, 3689 | 945, 957 | [945,943,942,942], [956,955,957,956] | 0% |

Speedup 2.0x/3.9x; every lane matches its sequential digest.

Case 7 toolchain: `cargo metadata` under 1.97.1 vs 1.98.1 parses to
identical `WorkspaceRecord`s; the dep half asserts 0→1 edges on a
real manifest change. Findings, not verdicts: selection marks all
44 obligations in every mutation case (fail-open broad); plan
walls cluster ~1 s regardless of touch site (discovery-dominated).

Budgets:

- Same obligation set across all 7 cases: PASSED (digest
  `cd8b34ba18f0982d` in all 16 samples).
- B1 true-cold/install, B7 resolution effects, P13-3 hosted negatives: UNPASSED/UNMEASURED, no controlled runs (bench cases are planner-level; 134 hosted runs, all same-repo `pull_request`, 0 fork-origin, per 2026-10-01 API census).
- Inventory reuse op counts: PASSED (exact: 1/10/100 members →
  2/11/101 legacy subprocesses vs 1 reused).
- Concurrent validation lanes without contention: UNPASSED as a
  validation claim — the 0%-contention observation (2.0x/3.9x at 2
  and 4 lanes) covers `plan_internal` analysis lanes only; lanes
  running fmt/clippy/test validation were never measured (the old
  PASSED graded validation on analysis walls). Metadata lanes stay
  1 by design; plans parallelize above that.
- Toolchain-change inventory stability: PASSED (identical records
  1.97.1 vs 1.98.1).
- Per-stage wall/memory split: PARTIAL — setup/plan/metadata-proxy
  recorded; RSS is test-process peak, not isolated plan RSS; the
  in-plan compiler cost is a proxy, not traced.
- CI-hosted walls, queue, cache transfer: UNMEASURED (no runs).
- Two-minute warm SMALL-FIXTURE path: still UNPASSED (unchanged).

Negative pipeline: `impl_neg_pipeline_p13` (9 tests) drives
config→reports; every stage fails closed (the P05-gated deferral
is lifted: no P05 crate graph was needed).
`scripts/verify-local.sh` also pins per-crate `cargo test --doc`,
per-crate `cargo doc --no-deps`, and the named fixture suite.
