# P08 cache measurements + warm-reuse proof (R11/R12)

Commit measured: `347976d` (branch `docs/velnor-actions-spec`).
Hosted runs: seed
[`36754512444`](https://github.com/tailrocks/velnor-new/actions/runs/36754512444)
(plan-only, 2026-09-30T17:52Z) and warm
[`36760724180`](https://github.com/tailrocks/velnor-new/actions/runs/36760724180)
(2026-09-30T18:44Z), both `ubuntu-26.04`, `x86_64-unknown-linux-gnu`,
rust 1.98.1, P08 `ci.yml`. Service listing pulled 2026-09-30 ~19:25Z
(read-only `gh cache list` + `cache/usage` API). Local fixture runs on
`arm64` macOS, rustup cargo 1.98.1 direct (generated steps use
`mise exec rust@1.98.1`; the cargo-level probe/fetch behavior is
identical — the `mise exec` launch delta is not isolated here).
Every figure below is measured; the single assumption (quota limit) is
labeled. Nothing is fabricated.

## 1. Service inventory (stored sizes, eviction, headroom)

`gh cache list --limit 100 --json` returned 21 entries totaling
**1,325,342,795 B (1263.95 MiB)** — exactly equal to the
`cache/usage` API (`active_caches_size_in_bytes`, 21 caches), so the
listing is complete (no pagination gap).

| Layer | Entries | Stored bytes | Share |
| --- | ---: | ---: | ---: |
| Shared sources (`velnor-v1-sources-*`, P08) | 1 | 17,568,922 | 1.3% |
| Legacy tools (`mise-tools-v1-*`, pre-P08 role archives) | 20 | 1,307,773,873 | 98.7% |
| Built-in tools (`mise-v1-*`, P08) | 0 | 0 | — |
| MBX objects (`mbx-*`) | 0 | 0 | — |

Headroom: GitHub documents a 10 GB default repository limit. Taking
10 GiB (10,737,418,240 B — assumption, labeled) minus 1,325,342,795 B
gives **9,412,075,445 B (~8.77 GiB free, 12.3% used)** — the same
subtraction `headroom_bytes` performs (test-pinned; over-quota input
errors instead of wrapping). `summarize_cache_usage` parses this
listing shape end to end (format pinned by
`service_report_parses_live_shape_for_sequential_runs`).

Sequential delta: the R07 snapshot earlier the same day (16 entries,
982.26 MiB, see `cache_transport.rs` docs) grew to 21 entries /
1263.95 MiB — **+5 entries, +281.69 MiB, zero evictions** in the
window. No entry is older than ~2 days, so none is 7-day-eligible;
LRU pressure is absent (12.3% of the assumed quota).

## 2. Hosted sequential runs (hit/miss, transfer, durations)

Seed = run 36754512444 (plan ran, crates skipped: plan failed at
`Check generated files` AFTER the save). Warm = run 36760724180
(plan + 7 crates + validators + Required).

| Layer / step | Seed (cold) | Warm (N+1) |
| --- | --- | --- |
| Mise tools restore | MISS (`mise cache not found for mise-v1-…e9592c9b`) | MISS (same key) |
| Tool install (cold, per job) | plan ~13.8 s | crate ~14.3 s |
| Sources restore | MISS (~0.3 s overhead, no entry) | HIT, 17,568,922 B, exact seed key |
| Sources restore duration | — | plan 1.85 s @15.4 MB/s; crate ~0.6 s @82.5 MB/s |
| Fetch probe | `sources miss (source_missing)`, 58 `Downloaded` | `sources hit, skipping fetch`, 0 `Downloaded` |
| Crate `Downloading`/`Updating` | n/a (skipped) | 0 lines in every sampled crate job |
| Sources save | SAVED in ~2.0 s (17:53:40→42) | SKIPPED (`Unable to reserve cache … another job may be creating`) — immutable entry kept, no duplicate stored |
| MBX objects | no entry, no hit/miss line in action output | same: setup ~10.3 s, MISS/`no_entry` (P08 MBX action never ran on the default branch, so no seed exists) |

Aggregate warm-run transfer (sources): 8 restoring jobs × 17,568,922 B
= **140,551,376 B (~134.0 MiB)** downloaded for **17,568,922 B stored
once** — sharing removes stored copies, not per-job transfer, exactly
as pr-1 predicts. Warm plan `Downloaded` lines: 53, all in the
pre-restore `Build helper` step (own cargo home); fetch-step delta is
58 → 0. Warm crate obligations all ran `--offline` with
`mbx[cache]: 0 B downloaded, 0 B uploaded` (sampled: clippy 1 hit /
44 not-looked-up, build/test/doc 0 hits — compiler reuse stays local).

Per-goal dimensions, recorded separately: queue seed ~6 s / warm ~3 s
(created→first-job-start); setup ≈ checkout + mise download ~2 s/job;
tool-install ~14 s/job cold (both runs — the tools MISS dominates job
startup); transfer above; compile warm-crate 1–4 min/job (hosted,
source-dominated); test inside those walls; artifact = 17.57 MB
sources snapshot (+ legacy tools archives); critical path warm 6 m 46 s
(plan 87 s → slowest crate 252 s → Required 22 s); sizes/bytes in §1;
reasons: `source_missing` (seed), `no_entry` (tools/MBX every run),
reservation-refused (warm save — healthy, not an error).

## 3. Local cold-vs-warm (`cacheprobe` fixture, 6 locked packages)

Empty `CARGO_HOME` + fixed lockfile → `cargo fetch --locked`:
**1.33 s**, 6 `Downloaded`. Subset bytes: `registry/cache` 504,114 +
`registry/index` 939,782 = **1,443,896**; excluded `registry/src`
3,326,811 + `.global-cache` 57,344 (re-extracted, never archived).
Save proxy (`tar`+zstd of the subset): **0.58 s → 557,269 B artifact
(38.6%)**. Restore proxy (extract into a fresh home): **0.35 s**.
Verbatim probe script: empty home → exit 101
(`no matching package … offline mode`) = MISS branch in 0.05 s;
restored home → exit 0, `velnor: sources hit, skipping fetch`, in
0.05 s. Warm `build --locked --offline` (cold target): **4.10 s, zero
downloads**; rerun **0.03 s** (no recompile); `test --locked
--offline` **0.57 s, 1 passed**. Reference (machine-shared, NOT the CI
subset): `~/.cargo` holds cache 281.37 MB + index 86.51 MB + git/db
274.42 MB with 4.26 GB of excluded `registry/src`.

## 4. Reporting path (quota helpers are live, render stays hermetic)

`parse_service_usage` / `headroom_bytes` / `stored_vs_transfer`
(`mise/src/cache_trust.rs`) previously had zero production callers.
They are now composed by `summarize_cache_usage` into
`CacheUsageReport` (unit-pinned by `c13`, fixture-pinned by
`service_report_parses_live_shape_for_sequential_runs`, which prints
`cache:` lines under `--nocapture` like the P13 `perf:` harness).
The generator still never calls the live service at render time —
render must stay hermetic and deterministic, and service data exists
only after hosted runs — so the reporting path is post-hoc by
design: `gh cache list --json` → `summarize_cache_usage` → this doc.
Over-quota input fails the report instead of wrapping headroom.

## 5. Warm-reuse proof (R12) and open items

Static: `impl_cache_warm.rs` renders twice with identical inputs and
asserts byte-identical workflows + identical keys, the offline-skip
branch in every crate fetch step, `--offline` on every obligation,
and disjoint MBX/Cargo cache shapes with lock-content re-keying via
`hashFiles` at runtime; `impl_cache_fixtures.rs` asserts per-job
keys/paths, one shared sources key, one tools identity per tool
union, a single plan writer, and restore<MBX<fetch order. Hosted §2
shows the seed/warm branches with zero re-download on warm. Local §3
replays both branches verbatim. UNMEASURED / open: why the Mise
built-in cache never saved (no `mise-v1-*` entries; no save lines in
any log — restore MISS lines are the only evidence); MBX objects seed
(needs a default-branch P08 run); eviction over time (needs a
week-scale sequential sample).
