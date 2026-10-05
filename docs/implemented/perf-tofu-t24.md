# T24 tofu efficiency gates + benchmark matrix + scaling (spec §9)

T24 names the seven deterministic efficiency gates as invariant
tests, records the cold/warm/docs/root/module/lock/mixed/fork matrix
with raw `bench:` evidence, and scales plan/prepare/generate across
1/10/100 synthetic roots. Budgets stay text until T25 measures them:
this doc records matrix numbers, not budget PASSED verdicts.

Commit measured: `f57e0b4` (local T24 measurement commit —
DANGLING, contained in no branch, absent from origin; the pushed T24
commit adds only this doc file over it, code tree identical —
verify with `git diff f57e0b4 <pushed> -- .
':!docs/implemented/perf-tofu-t24.md'`, which must be empty).
Machine: `arm64`, macOS 27.0, 128 GiB RAM.
Toolchain: rustc/cargo 1.98.1 (48a229cea/797e8a9bc), mise
2026.9.16, mbx 1.21.0, nextest 0.9.146 (pinned
`aqua:nextest-rs/nextest/cargo-nextest@0.9.146`), git 2.56.0,
python 3.14.8. Fixture pins: `tofu_repo` N-root builder
(`tofu_perf_fixtures_t24.rs`), `BENCH_ROOTS = 10`
(`impl_tofu_t24_bench.rs`), opentofu selector unchanged (T15 pin,
no install performed locally).

Method: `impl_tofu_t24_bench` plans fixed-shape fixtures per case
(30 obligations at width 10; `digest=` over sorted task IDs proves
the same set). `setup_ms` builds the fixture, `plan_ms` runs
`plan_internal`, `metadata_ms` is a direct-`git ls-files`
lower-bound proxy for the index-subprocess cost inside `plan`,
`rss_kb` is whole-test-binary peak RSS (not isolated plan RSS).
Queue/transfer are local no-ops; CI values need hosted runs. Raw
`bench:`/`perf:` lines via the orchestrator test binary with
`--nocapture`. Five runs each; figures are run1..run5 sorted with
the median. Hosted runs: none (local-only evidence, no run links;
the cache-measurements hosted-evidence pattern applies when CI
runs exist).

## 1. The seven gates (spec quotes → pins → tests)

Spec §9 lists six "Deterministic efficiency gates" bullets; gate 7
is derived from the §9 budget-table docs-only row (the only
remaining deterministic, code-pinnable efficiency clause — the
warm-path/warm-plan rows are measured budgets, T25's surface).

| Gate | Spec quote (abridged) | Pin | Named test |
| --- | --- | --- | --- |
| 1 | "A pure-tofu consumer using a prebuilt generator installs/runs **no Rust toolchain, Cargo metadata, MBX, Nextest, rustfmt, or Clippy** for its stack or Plan job." | `impl_tofu_t17.rs:88` family; plan + stack jobs carry no Rust/MBX setup | `impl_tofu_t24_gates.rs:145` `gate1_...` |
| 2 | "`plan`/`generate` perform **zero tofu init/validate/apply operations**, no provider downloads for discovery, and no modifications to source/tool/lock files." | `impl_orch_f2a.rs:77` + zero-spawn tofu src; snapshot invariance | `impl_tofu_t24_gates.rs:247` `gate2_...` |
| 3 | "One repository index per snapshot, cached parsed facts within that snapshot, one local-module graph traversal per needed analysis; no adapter-specific whole-tree rescans or repeated unbounded `rg`/`find`." | `discover_index.rs:31` single call site; `select.rs:69` single entry; tofu's only walk is bounded `collect_unit_files` | `impl_tofu_t24_gates.rs:293` `gate3_...` |
| 4 | "At most one successful initialization per selected root per verification attempt; formatting visits each intended file once; no duplicate provider archive uploads or unrelated tool setup." | `select_tofu.rs:266` nested-merge; `impl_tofu_t18.rs:223` init-once + `:249` fmt-once; distinct save keys | `impl_tofu_t24_gates.rs:339` `gate4_...` |
| 5 | "Cache misses retain all correctness gates. A cache hit does not replace `validate` execution under the initial policy." | `task_identity.rs:154` `reuse_eligible` (T23: init/validate `tofu_reuse_disabled`, fmt qualified, coverage intact) | `impl_tofu_t24_gates2.rs:46` `gate5_...` |
| 6 | "Existing resource limits bound root fan-out and subprocess budgets. Do not split trivial steps into separate hosted jobs without measured benefit." | `stage_tofu_root_jobs` (`crate_jobs_stage.rs:58`) determinism; `JobTimeout` consts; `RUN_TIMEOUT_SECS = 600`; `max-parallel` render | `impl_tofu_t24_gates2.rs:85` `gate6_...` |
| 7 | Budget-table docs-only row: "No tofu installation/initialization/validation when the plan has proved no relevant tofu obligation needs execution. Generic mandatory workflow checks remain." | Zero `affected_by_change` on docs-only plans; plan + Required checks still render | `impl_tofu_t24_gates2.rs:142` `gate7_...` |

No new scan/fetch counters: none exist in-tree (grep-negative at
T19, still true), and every gate pins structurally (call-site
counts, token scans, behavioral asserts) over existing vocabulary.
No new error/report/merge types. `TaskTiming`/`duration_ms` stay
`None` (T25's surface; untouched). Qual Gates 0–8 files are a
different family (merge/coverage/release qualification) and pin
none of the above.

## 2. Benchmark matrix (n=5 per case)

`digest=1f4af9af486002c4` in all 25 fixed-shape samples (cold, warm,
docs, root, fork): the same 30-obligation set throughout.

| Case | plan_ms (sorted, med) | setup_ms | metadata_ms | rss_kb | Digest |
| --- | --- | --- | --- | --- | --- |
| tofu-cold (first plan) | 471, 485, 501, 521, 531 (501) | 62, 62, 63, 65, 67 | 4, 4, 4, 5, 7 | 410960–412320 | 1f4af9af486002c4 |
| tofu-warm (second plan) | 296, 297, 299, 310, 329 (299), cold 337, 341, 349, 354, 370 | 61, 61, 63, 65, 66 | 3, 4, 4, 4, 5 | 411504–415008 | same |
| tofu-docs (README touch) | 476, 478, 486, 515, 524 (486) | 62, 62, 65, 66, 68 | 4, 4, 4, 8, 8 | 410864–412016 | same |
| tofu-root (r003 touch) | 481, 484, 498, 508, 523 (498) | 57, 59, 61, 64, 66 | 4, 4, 4, 5, 7 | 411168–412304 | same |
| tofu-module (shared touch) | 348, 352, 370, 375, 382 (370) | 56, 58, 62, 62, 63 | 4, 4, 6, 6, 9 | 410432–410960 | bd79216a2db0378c |
| tofu-lock (lockfile touch) | 332, 345, 367, 367, 374 (367) | 56, 57, 60, 62, 67 | 4, 5, 5, 6, 10 | 410240–410864 | c5f4a62ead7e3479 |
| tofu-mixed (rust touch) | 395, 411, 416, 422, 455 (416) | 58, 60, 62, 62, 64 | 5, 7, 7, 7, 12 | 410544–410960 | 7f64b156bd26269e |
| tofu-fork (fork event) | 290, 295, 300, 315, 329 (300) | 60, 60, 61, 66, 68 | 3, 4, 4, 5, 5 | 411472–412352 | same-as-PR |

Findings, not verdicts: warm/fork second-plans cluster ~300 ms
against ~500 ms first-plans (same-process warmth, not cache
behavior); docs/root touches cost the same as cold (plan walls are
discovery-dominated, touch-site independent); module/lock/mixed
shapes differ only by fixture width. Selection asserts hold in
every case (docs zero-affected, root triple-only, module caller,
lock root, mixed rust-only, fork PR-trust with PR digest).

## 3. Synthetic scaling (n=5 per width)

At the measured head, plan scaled to 40 roots (120 obligations, under
the then-current 320 KiB matrix budget); prepare/generate scaled to 100 roots (300
proposals). Walls are local observations, not hosted performance.

| Roots | prepare_ms (sorted, med) | plan_ms (sorted, med) | generate_ms (sorted, med) |
| --- | --- | --- | --- |
| 1 | 18, 22, 27, 30, 30 (27) | 222, 228, 239, 239, 251 (239) | 253, 260, 271, 275, 301 (271) |
| 10 | 31, 31, 34, 42, 43 (34) | 296, 297, 310, 318, 335 (310) | 349, 354, 357, 377, 400 (357) |
| 40 | — | 673, 680, 704, 720, 734 (704) | — |
| 100 | 91, 92, 93, 95, 98 (93) | budget (unrun past 40) | 1548, 1566, 1598, 1606, 1674 (1598) |

Generate splits as prepare+render in the `perf:` lines (100-root
prepare 77–80 ms, render the rest; 100-root render is dominated
by the single actionlint YAML pass, the P13-observed shape).
`perf: op=plan` uses the shared `crates=` label for root count;
the tofu `perf: op=prepare/generate` lines use `roots=`.

## 4. Verdicts

- Gates 1–7 as named invariant tests: PASSED (7/7 green at
  `f57e0b4`; full suite steady, see the T24 report).
- Same obligation set across the fixed-shape matrix: PASSED
  (`1f4af9af486002c4` in all 25 samples).
- Matrix sample counts (≥3 cold, ≥5 warm): PASSED (5 cold + 5 warm
  + 5 each for docs/root/module/lock/mixed/fork, all local).
- 120 s warm path / 10 s warm plan budgets: text only (T25
  measures; no budget PASSED verdict is claimed here).
- CI-hosted walls, queue, cache transfer, provider
  download/restore/save, runner-seconds: UNMEASURED (no hosted
  runs; raw run links: none — local-only evidence).
- In-plan parsed-fact caching ("cached parsed facts within that
  snapshot", gate 3 clause): open — tofu planning re-reads file
  contents per pass (the T25 repeated-scan inventory); no
  behavior was changed to claim it. T25 owns the fix-or-measure.
- Telemetry population (`TaskTiming` slots, `duration_ms`):
  untouched by design (T25's surface; `None` everywhere).
