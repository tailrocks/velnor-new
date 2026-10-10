# P08 cache measurements + warm-reuse proof (R11/R12)

Commit measured: `347976d` (branch `docs/velnor-actions-spec`);
green run below at `695752e`.
Hosted runs: seed
[`36754512444`](https://github.com/tailrocks/velnor-new/actions/runs/36754512444)
(plan-only, 2026-09-30T17:52Z, conclusion `failure`: plan failed at
`Check generated files` AFTER the save), warm
[`36760724180`](https://github.com/tailrocks/velnor-new/actions/runs/36760724180)
(2026-09-30T18:44Z, conclusion `failure`: orchestrator crate failed,
so Required failed — cache evidence below is still valid, but no
green-run verdict may cite it), and green
[`36777030585`](https://github.com/tailrocks/velnor-new/actions/runs/36777030585)
(2026-09-30T21:04Z, conclusion `success`, 41/41 reports,
`status: passed`), all `ubuntu-26.04` (`Image: ubuntu-26.04` in the
provisioner log), `x86_64-unknown-linux-gnu`, rust 1.98.1,
nextest 0.9.146, P08 `ci.yml`. Pins moved mid-day: seed/warm ran
mise 2026.9.16 + mbx 1.19.0, green runs mise 2026.9.18 + mbx
1.21.0 (install lines verified per run). Service listing
pulled 2026-09-30 ~19:25Z and re-pulled 2026-10-01 ~05:55Z (read-only
`gh cache list` + `cache/usage` API); the two pulls are byte-identical
(§1). Local fixture runs on `arm64` macOS, rustup cargo 1.98.1 direct
(generated steps use `mise exec rust@1.98.1`; the cargo-level
probe/fetch behavior is identical — the `mise exec` launch delta is
not isolated here). Every figure below is measured; the single
assumption (quota limit) and every derived aggregate are labeled.
Nothing is fabricated.

## 1. Service inventory (stored sizes, eviction, headroom)

`gh cache list --limit 100 --json` returned 21 entries totaling
**1,325,342,795 B (1263.95 MiB)** — exactly equal to the
`cache/usage` API (`active_caches_size_in_bytes`, 21 caches), so the
listing is complete (no pagination gap). A second pull after the warm
and green runs returned the same 21 entries and the same byte total:
both runs were storage-neutral (sources save skipped/refused, tools
never save, MBX never saves on PR runs). A third pull 2026-10-01T12:59Z
is again byte-identical (21 entries, 1,325,342,795 B; oldest
2026-09-28T23:12Z, newest the 09-30 seed save): ~24 further same-repo
PR runs added zero entries and evicted zero.

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
(plan + 7 crates + validators + Required; orchestrator FAILED —
red run, cache branches only). Green = run 36777030585 (success,
17 m 30 s created→updated, fully accounted in
[performance.md](performance.md)).

| Layer / step | Seed (cold) | Warm (N+1, red) | Green (N+2) |
| --- | --- | --- | --- |
| Mise tools restore | MISS (`mise cache not found for mise-v1-…e9592c9b`) | MISS (same key) | MISS (same key shape, `…-2026.9.18-6270601fa454da91`) |
| Tool install (cold, per job) | plan ~13.8 s | crate ~14.3 s | plan 15 s, crates 12–18 s, Required 2 s (`gh` only) |
| Sources restore | MISS (~0.3 s overhead, no entry) | HIT, 17,568,922 B, exact seed key | HIT, 17,568,922 B, exact seed key, all 8 restoring jobs |
| Sources restore duration | — | plan 1.85 s @ 15.4 MB/s; contract 1.56 s @ 18.4 MB/s (action-reported rates; the old "~0.6 s @ 82.5 MB/s" matched no sampled job and is withdrawn) | sub-second steps; action-reported 16.0–103.4 MB/s across the 8 jobs (plan 85.9, orch 103.4, cli 16.0, contract 36.3, rust 73.2, actionlint 32.4, renderer 24.1, mise 60.5) |
| Fetch probe | `sources miss (source_missing)`, 58 `Downloaded` | `sources hit, skipping fetch`, 0 `Downloaded` | `sources hit, skipping fetch`, 0 `Downloaded` (fetch steps; setup downloads split below) |
| Crate `Downloading`/`Updating` | n/a (skipped) | 0 lines in every sampled crate job | 0 lines in all 7 crate logs |
| Sources save | SAVED in ~2.0 s (17:53:40→42) | SKIPPED (`Unable to reserve cache … another job may be creating`) — immutable entry kept, no duplicate stored | SKIPPED (step skipped: entry exists from seed; inventory byte-identical after) |
| MBX objects | no entry, no hit/miss line in action output | same: setup ~10.3 s, MISS/`no_entry` (P08 MBX action never ran on the default branch, so no seed exists) | `No mbx cache found` in all 7 crate jobs; restore step costs 10–14 s/job returning nothing (~80 s total overhead) |

Aggregate green-run transfer (sources): per-job restore size is
measured in each of the 8 job logs (`Cache Size: ~17 MB
(17568922 B)`); the aggregate 8 × 17,568,922 =
**140,551,376 B (~134.0 MiB)** downloaded for **17,568,922 B stored
once** is their sum — DERIVED, not independently metered (no
per-job byte counter exists outside the restore line). Sharing
removes stored copies, not per-job transfer, exactly as pr-1
predicts.

Setup-vs-fetch download split (green run): the "zero re-download"
claim covers fetch steps only. Plan's `Build helper` step runs
before any restore with its own cargo home and emits 53
`Downloaded` lines (all inside the Build helper step group,
21:06:38Z); fetch-step delta is 58 → 0 (seed likewise: 58 fetch
of 111 plan-log total, rest setup). Run total: 53 downloads, all
tool-provisioning/setup, zero from source fetch. Rust-component
download bytes are UNMEASURED (the 0–1 s `Prepare Rust components`
step logs no byte count).

Warm-crate obligations all ran `--offline` with
`mbx[cache]: 0 B downloaded, 0 B uploaded` (green run sampled all
7: contract clippy 1 hit / 51 not-looked-up, build/test/doc 0–25
hits — every hit is intra-job clippy→build reuse, 7.73 s avoided
in contract, 9.67 s in orchestrator; cross-job compiler reuse is
zero because no MBX seed exists).

Per-goal dimensions (green run), recorded separately: queue 91 s
(created→first-job-start 21:04:36→21:06:07); setup 5–8 s/job
(checkout + mise + helper/plan downloads); tool-install 12–18 s/job cold (checked-out jobs —
the tools MISS dominates job startup, ~114 s run-wide); transfer
above; compile per crate (clippy+build, cold target, warm
sources, no MBX): actionlint 13 s, contract 14 s, mise 16 s, rust
18 s, renderer 28 s, cli 30 s, orchestrator 46 s — ~165 s summed,
recompiled 7× with zero cross-job reuse; test inside those walls
except orchestrator (nextest 610/610 in 679.316 s, 2 slow tests)
and cli (211 in 23.910 s); artifact = 17.57 MB sources snapshot
(+ 39,174 B matrix reports, 22,860 B plan, 2,401,586 B helper);
critical path 17 m 30 s = queue 91 + Plan 88 + plan→crate gap 2 +
crate wave 818 (orchestrator 781 s wall) + crate→Required gap 5 +
Required 46 — residual 0 s; sizes/bytes in §1; reasons:
`source_missing` (seed), `no_entry` (tools/MBX every run),
reservation-refused (warm save — healthy, not an error).

Warm-run wall accounting (the 45 s the old "plan 87 s → slowest
crate 252 s → Required 22 s" sum dropped): created→updated 406 s =
queue 3 + Plan-start wait 36 (validators done 18:44:41, Plan
starts 18:45:01 — scheduling gap, cause undetermined beyond API
timestamps) + Plan 87 + crate gap 3 + orchestrator 252 + Required
gap 2 + Required 22 + tail 1. Residual 0 s. The old text has been
corrected: job walls never sum to run wall without queue and
scheduling gaps.

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
274.42 MB with 4.26 GB of excluded `registry/src`. This full-home
inventory is not the archived payload: the Velnor source archive contains
only `registry/index`, `registry/cache`, and `git/db`.

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
shows the seed/warm/green branches with zero fetch re-download on
warm and green. Local §3 replays both branches verbatim.
UNMEASURED / open: MBX objects seed (needs a default-branch P08 run
— until then every crate job recompiles from scratch and pays 10–14 s
for an empty restore); eviction over time (needs a week-scale
sequential sample — oldest entry 2026-09-28, none 7-day-eligible yet);
fork-PR read-only hosted run (all 134 runs in history are same-repo
`pull_request` events per 2026-10-01 API census — zero fork-origin
runs, so fork read-only has unit evidence only:
`pr_save_allowed`/`is_read_only`/`mode_for_event`); simultaneous-writer
attempts (PARTIAL: warm-run sources save hit backend reservation
refusal, but no controlled two-writer race on one key was run).
ANSWERED: why the Mise built-in cache never saved — the pinned
`jdx/mise-action@v5.0.0` (`9149ea8`) saves only inside its `install`
leg (`src/index.ts:run()` gates `saveCache` on the `install` input;
`action.yml` offers no PR-scoped save input and the source has zero
`pull_request` handling), which Velnor disables (`install: false`),
so the push-gated `cache_save` expression never saved on any event
(all 134 runs to date are `pull_request` per the 2026-10-01 API
census; push triggers only on `main`, unmerged). Historical V1 setups
were restore-only and had no elected tool-cache save; that path has been
retired. Active V2 uses renderer-owned runtime-qualified identity and
explicit elected writers, as described in the V2 sections above. Full
per-action PR-save verdict: gate-4 doc R13 bullet.

## Hosted MBX object-cache round-trip

`qualification.yml` mode `mbx-cache-roundtrip` runs two hosted jobs in one
dispatch. The protected-main writer builds a probe crate and its post step
must export and save the MBX objects before the dependent reader starts. The
reader has only `actions: read`, requires an imported object set, and checks
that MBX reuses a cached compilation. A run-and-attempt-specific generation
prevents a cache from an earlier dispatch from satisfying this check. The
reader disables saving and restores from a prefix, so its primary lookup is
not an exact match for the writer's run-specific save key. The action therefore
reports `cache-hit=false` even when it imports that run-bound object set. The
workflow requires the imported-object count and a cached-compilation reuse
measurement instead of requiring an exact primary-key hit.

Both jobs in the recorded round-trip probe set `MBX_GC_AUTO=1` and
`MBX_SHARE_OUT_DIR=0`. This is the historical probe configuration recorded in
`ac3ab6a3d`, not the current native renderer lifecycle. The native MBX
lifecycle policy in [PR #133](https://github.com/tailrocks/velnor-new/pull/133)
sets `MBX_GC_AUTO=0` while task results are active and runs guarded `mbx clean`
after the final workspace consumer. The historical production measurements
above used `MBX_GC_AUTO=0` on hosted Linux and a Scale Set local backend with a
manual bundle, before the native object action route. Neither those
measurements nor this `GC_AUTO=1` probe measure
the current native lifecycle's post-export peak disk use. The probe uses a
separate candidate action ref, so its result does not qualify the production
action pin, current lifecycle, or either typed production lane. Dispatch once
from protected `main` with mode `mbx-cache-roundtrip`; the writer and reader
run in order at the same SHA.

This is a small end-to-end action and cache round-trip probe. The writer
samples `df -B1 -P` and `df -i -P` on `$RUNNER_TEMP` after the probe build.
The reader prints those same lines. `tee` writes each MBX stats JSON to
the step log and a file. `jq -e` reads the file. It still does not qualify the
affected ChainArgos workload.
A failing stats producer also fails the probe even if `tee` writes valid JSON:
both reader steps enable `pipefail` explicitly because the hosted default shell
does not.
A green probe alone is not an ENOSPC repair verdict. The hosted
round-trip still needs to run against GitHub Actions after the generated
workflow is adopted.

## Generated workflow size with V2 cache identity

The generator enforces a fixed 500,000-byte `ci.yml` cap. After compacting the
runtime identity body into one version-marked script plus one local composite
action per used hosted lane, the deterministic P13 workspace fixture measured:

| `workspace_repo` members | `ci.yml` bytes | Result |
| ---: | ---: | --- |
| 1 | 33,737 | accepted |
| 10 | 110,831 | accepted |
| 29 | 273,585 | accepted |
| 30 | 282,151 | accepted |
| 40 | 367,811 | accepted |
| 60 | 539,131 | rejected by the byte cap |
| 100 | 881,771 | rejected by the byte cap |

The separate T24 ToFu-root fixture measured:

| ToFu roots | `ci.yml` bytes | Result |
| ---: | ---: | --- |
| 1 | 24,016 | accepted |
| 10 | 96,469 | accepted |
| 31 | 265,624 | accepted |
| 32 | 273,679 | accepted |
| 40 | 338,119 | accepted |
| 60 | 499,219 | accepted |
| 100 | 821,419 | rejected by the byte cap |

These are fixture-specific local generator measurements. The 60-root output is
781 bytes below the current limit; this does not promise that other 60-root
workflows fit. A rejected render reports its actual size and leaves no partial
output tree. The cap remains authoritative, and these figures do not measure
hosted cache hits, transfer size, disk usage, or build performance.

## Cache-save cancellation progress semantics

Source review: pinned
[`actions/cache` save-only bundle](https://github.com/actions/cache/blob/55cc8345863c7cc4c66a329aec7e433d2d1c52a9/dist/save-only/index.js)
has blob `b3a8aa37f9f7a608d7d5a63a8990b1fd4c043759`. Its V2 save path forces
the Azure SDK, 64 MiB blocks, and concurrency 8; the SDK uses a 128 MiB
single-shot threshold. The legacy `Uploading chunk ...` marker is in the V1
uploader and is unavailable on this V2 path. V2's ordinary `Sent N of TOTAL`
progress line needs no debug setting; it uses a one-second display timer, with
a final display attempt on cleanup unless completion was already displayed.

Count a cancellation probe only when the final `Save` step is live and its log
contains `Sent N of TOTAL` with `0 < N < TOTAL` before cancellation. For an
archive at or below 128 MiB, this shows partial request-body progress observed
by the SDK; it does not prove server acknowledgement or cache finalization.
For a larger archive, progress advances after successful `stageBlock` calls
for blocks of at most 64 MiB; the final block may be smaller. This does not
prove all blocks completed or V2 `FinalizeCacheEntryUpload` succeeded.

A fast small upload may produce only a final `Sent TOTAL of TOTAL` line. That
does not qualify a cancellation probe: record `NOT_RUN`. Do not add archive
padding or artificial delay to manufacture a partial sample. This section is
source review only: no local upload/cancellation test is recorded, and hosted
cache-save cancellation evidence remains `UNRUN`.
