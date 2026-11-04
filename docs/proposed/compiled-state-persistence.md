# Compiled-state persistence across CI runs

Status: proposed; the design is decided in the Velnor fast CI decision record, and hosted Linux measurements are pending. This page is not an implementation claim, except that the restore hardening landed in #477. It records why compiled state does not carry from one run to the next today, and the design that would carry it.

Labels used for numbers: **measured** (read from a log, `gh` output or source in this repository, or measured with `mbx` 1.29.1 on a local macOS arm64 machine), **reported** (taken from a pull request or issue body, not re-measured), **inferred** (arithmetic or a ratio from other numbers). Sizes labelled "Linux harness" were measured by the Velnor fast CI decision record on a Linux arm64 Docker harness, which is not a hosted runner. Every other size on Linux hosted runners is inferred or unmeasured, except hosted-log lines quoted as measured, until the measurements in [Open measurements](#open-measurements) land.

## Goal

Every sequential CI run reuses the compiled state of the previous run, the same way a second local `cargo` command through `mbx` does. That covers successive pushes to `main`. A pull request reuses the units that `main` already produced, and sequential reuse of a pull request's own output waits for pull-request-scoped state (see [Pull-request-scoped state: last](#pull-request-scoped-state-last)). The state must fit GitHub's default 10 GB repository cache limit and must not store the same bytes once per crate job.

## Why compiled state does not carry today

Locally, `mbx` keeps one persistent store (`MBX_CACHE_DIR`, `~/Library/Caches/mbx` on macOS, `~/.cache/mbx` on Linux). Every compiled unit and its recorded prediction stay there, so a second run finds each unit. A hosted runner is a fresh machine. Velnor sets `MBX_CACHE_DIR=${{ runner.temp }}/velnor/mbx` (`crates/velnor-actions-workflow-renderer/src/cache_steps_mbx_preflight.rs:85`), an empty directory at job start. mbx looks a unit up only when an earlier build recorded a prediction for it. With an empty store, a unit is "not looked up": it compiles and is stored, and the next job starts empty again.

Four causes stack. Fixing one alone changes nothing observable.

### 1. Nothing writes compiled state

- The workflow has two `actions/cache/save` steps, for Cargo sources (`.github/workflows/ci.yml:440`) and Mise tools (`ci.yml:1925`). Neither carries compiled state (measured).
- Crate jobs restore state read-only. Only a job with exactly one `MbxProducerMark` step may export and save (the `marks != 1` branch, `cache_steps_mbx_bundle_publish.rs:250`). The only job that carried the mark was the `Dependency seed` job. PR #279 moved the write from crate jobs to that job, and PR #458 removed the remaining write clones.
- PR #466 turned the seed jobs off (`.velnor/config.toml:40`, `dependency_seed = "off"`). Its body states that each seed job cost 535 to 547 runner-seconds per main push and published nothing, because every save was refused as `over_ceiling` (reported).
- `gh cache list` at 2026-11-03 14:11 UTC showed 19 entries and 3,684,271,824 B in total: 16 `mise-tools-v2` entries (3,461,236,524 B, two sets of 8 created at 12:15 and 13:28 UTC), 3 sources entries (223,035,300 B), and no state entry (measured). The `mise-tools-v2` entries double until prune runs, because it keeps 8 per group (`TOOLS_RETAINED_ENTRIES`, `cache_prune.rs:15`; inferred from that constant and the two creation times).
- Hosted evidence: main push run 41639002674 and pull request run 41641931909. In both runs, 18 of 18 crate jobs logged `Cache not found for input keys: velnor-v2-state-` and skipped `Recover state snapshot`. No log contains `mbx cache import`. A green `Restore state snapshot` step is a miss; read the log line, not the step color (measured).
- In the build lane of each job, `object cache:` reported almost every unit as "not looked up", with 1,567 not looked up in push run 41639002674 and 1,614 in pull request run 41641931909. Each count sums the first `object cache:` line of each of the 18 Rust jobs (measured, recounted from the job logs). The clippy and doc lanes of a job hit only what that job's own build lane stored a moment earlier.

### 2. The old writer's saves were refused by a size gate

The seed jobs exported 5.9 GiB (root workspace) and 3.5 GiB (nested runner workspace) of uncompressed bytes (run 40712054296, issue #437, reported). The family ceiling is 384 MiB (`crates/velnor-actions-contract/src/workflow/cache_family/family.rs:131-137`, `MBX_BUNDLE`). The publish step measures the uncompressed export with `du -sb` against that ceiling (`cache_steps_mbx_bundle_publish.rs:127`), and then measures the packed payload against the same ceiling (`:135`). Both seed exports failed the first check, before any compression. The 10 GB repository limit was not the binding limit for these refusals. The mechanism that refused the saves in the code is this 384 MiB uncompressed family ceiling. On main, no renderer save step uses `OPERATING_CEILING_BYTES`: the orchestrator constant (`crates/velnor-actions-orchestrator/src/cache_archive_envelope.rs:24`) is referenced by the admission API and tests, not by a save step. Issue #431 (fixed by #432) states that 32 entries of about 8.4 GB exceeded the 8 GB operating ceiling and refused every save at that time. That claim is reported and not verified against code.

The two figures are not comparable. PR #279 recorded stored (gzip) sizes of 63.7 MiB (root) and 12.8 MiB (nested). Run 40712054296 recorded uncompressed bytes. No stored size exists for the later exports, so this page makes no growth claim.

### 3. A restored payload did not import (fixed in #477)

Before PR #477, `Recover state snapshot` ran `mbx cache import` on `payload.tar.gz`. mbx 1.29.1 reads an archive argument as an uncompressed tar, so a gzip file failed to import. On failure the script called `corrupt import_failed`, which prints a warning and exits 0, so the job built cold and passed. PR #407 introduced the `tar -czf` wrapper and that call together; at PR #278 the import took the directory. The defect never ran on a hosted runner, because the restore always missed first (tracked in issue #440).

PR #477 changed the restore. It inflates the payload once, screens the plain tar, extracts it into a private stage, and runs `mbx cache import` on the staged directory (`cache_steps_mbx_bundle_recover.rs:154-203`). No hosted run has exercised this path, because no entry exists to restore.

### 4. MBX settings missed registry units inside one run

Velnor ran `MBX_SHARE_OUT_DIR=0`, which keys `OUT_DIR` into each unit, so `serde_core`, `thiserror` and their dependents missed and the misses cascaded. PR #471 set `MBX_SHARE_OUT_DIR=1` (`cache_steps_mbx_preflight.rs:79`, `ci.yml:552`). It keeps `MBX_BUILD_SCRIPT_EXECUTION=0` (`cache_steps_mbx_preflight.rs:83`, `ci.yml:548`), so build-script units may keep missing even with a warm store (inferred).

### Why this stayed hidden

No stage tests the whole chain of export, pack, save, restore, import and hit. Layers 1 to 3 coexisted until #477 fixed layer 3 because the failure path of the restore is silent: it exits 0 with a warning. A cache-key change also starts every run cold, so each of #471, an mbx bump and a runner-image change (`ImageOS`) produces one cold run by design.

## Local compared with CI

| Aspect | Local `cargo` with `mbx` | CI today |
|---|---|---|
| Store | One persistent directory | Empty directory per job |
| Predictions | Kept between commands | None at job start |
| Lanes (build, clippy, doc, doctest) | Share one store | Share the job's own store only |
| Between runs | Store untouched except by `mbx gc` (default `gc.max_size`: 5 % of the cache disk, from 5 GiB to 500 GiB, per `mbx settings ls gc.max_size` on mbx 1.29.1) | Nothing restored, nothing saved |
| Between parallel crate jobs | Not applicable | 18 isolated stores |

Reuse inside a job works; reuse across jobs and runs does not exist.

## GitHub cache rules that bind

Each rule below was checked on 2026-11-04 against the original sources, and [the cache entry in the GitHub Actions reference](github-actions-reference.md#5-cache-rules-verified-against-original-sources-checked-2026-11-04) holds the full evidence with file and line citations. The sources are the GitHub docs pages `dependency-caching`, `workflow-syntax` and `limits`, the REST cache API page, `actions/cache` at the pinned commit `55cc834` (tag `v6`, bundling `@actions/cache` 6.1.0), and the `actions/toolkit` source for that package (commit `0786132e`, `packages/cache/src`). The numbers in brackets point to entries 5.1 to 5.9 of the reference.

- An entry is immutable. A new save needs a new key, and a save to an existing key is refused and logged at info level while the step succeeds [5.2].
- `restore-keys` is a prefix match and returns the most recently created match, not the highest generation. The server does the matching, so only the docs prove the order [5.3].
- A run reads caches from its own ref and the default branch. A `pull_request` run writes to `refs/pull/N/merge`, which the default branch and other pull requests cannot read [5.4]. Whether a later push to the same pull request restores an earlier push's entry is inferred from the ref-keyed scope and not proven.
- Only some events write the default-branch scope. `push`, `workflow_dispatch`, `schedule` and a few others do. `workflow_run` is read-only by default and writes only with an explicit write-capable `cache-mode` [5.9].
- `cache-mode` takes `read`, `write`, `write-only` or `none`, at workflow or job level, and the job value wins. The service enforces it with scoped tokens [5.5].
- The pinned client does not read `ACTIONS_CACHE_MODE`. A save under `read` still tries to reserve, is denied by the service, and logs a warning [5.5].
- Any run that can read a cache restores its bytes as-is. Contents are not signed, and `actions/cache` extracts with `tar -xf -P` inside the restore step, so nothing can inspect the archive before extraction. It checks no uncompressed size [5.7].
- Entries not accessed for 7 days are evicted by default, and at the repository limit the eviction is least-recently-accessed first, whatever the key. The 7-day period and the 10 GB size are defaults that owners can change [5.1].
- The default limit is 10 GB per repository. Owners can raise it since 2025-11-20 at a cost, and a repository over its budget can turn read-only. This project does not plan to raise it, and raising it needs an owner decision [5.1].
- `actions/cache` 6.1.0 compresses with `zstdmt` on Linux and does not pass `--long=30` for saves, because `getCompressionMethod` returns the without-long variant. It passes no level, so zstd's default applies [5.6]. A file that is already compressed gains nothing from this outer pass.
- There is no per-entry size check in the version 2 save path. A 10 GiB per-archive check exists only in the legacy version 1 path [5.8]. The service may have its own limit, which GitHub does not document.
- The repository limits are 200 new-entry uploads per minute and 1,500 downloads per minute. A 429 response fails nothing, because a failed save is a warning [5.8].

## Decided design

The Velnor fast CI decision record fixes the following design. It replaces the earlier proposal on this page. Items marked open are not decided.

1. One union producer per Cargo root. It runs the crate jobs' per-crate lane records by digest, not one `--workspace` build, in two phases: a dependency skeleton at `MBX_BUILD_SCRIPT_EXECUTION=1`, then the real members at `0` (see [The build-script rule](#the-build-script-rule)). Crate jobs run `1`. A registry-only skeleton producer was rejected by measurement.
2. A split into job P (compile) and job A (attest and save).
3. A workflow-level `cache-mode: read` with named writers.
4. Key version 4.
5. A Velnor zstd single-file packer, because the pinned `actions/cache` compresses with `zstdmt` and without `--long=30`.
6. Two-bound ceilings: 1.5 GiB stored and 12 GiB extracted.
7. Retention of 1 generation per root, with prune on `workflow_run` after CI and State seal.
8. `SOURCE_DATE_EPOCH=0` in every mbx task environment and in the compatibility key.
9. Pull-request-scoped state last.

## Budget

The cache policy constants are in `crates/velnor-actions-contract/src/config/workflow_cache_policy.rs:11,18,25`:

| Quantity | Bytes |
|---|---:|
| Accounting ceiling | 10,000,000,000 |
| Operating budget | 8,000,000,000 |
| Minimum reserve | 2,000,000,000 |

Inventory at 2026-11-03 14:11 UTC: 3,684,271,824 B, of which `mise-tools-v2` is 3,461,236,524 B in 16 entries (measured). Prune keeps 8 per group (`cache_prune.rs:15`), and it runs before saves today, which leaves two generations and doubles the tools entries. After prune the tools entries are 1,730,612,999 B and the sources entries are 223,035,300 B, 1,953,648,299 B together (inferred from the two creation times). The renderer's save admission checks elapsed time, not the 8e9 total (`cache_save_budget.rs:48-72`). Because eviction ignores the project's own budget, every state save checks the 8e9 total, so that a burst of large entries cannot evict the default branch's state first. Admission counts every cache writer that the live 8e9 check covers: the sources producer, the tools producers and the state A job of every root. A GitHub concurrency group cannot serialize them. A group keeps one running and one pending member, and a newer pending member replaces an older one even with `cancel-in-progress: false` (`mise-task-suites-contract.md:80-82`), so the group would drop writers. The design uses admission accounting and a `needs:` chain instead:

- Plan outputs which writers are eligible in this push: the sources producer, the tools producers, and state A for each root whose snapshot changed.
- Each writer's admission reserves the outer stored-size bound of every other writer that is eligible in the same push and can run at the same time, plus the live inventory, against 8e9. A save that does not fit is refused as `over_budget`, and the run stays cold.
- No writer is dropped. A writer that `needs:` an earlier writer cannot run at the same time as it, so the earlier writer's ceiling leaves the reservation once that job ends and the later job reads the inventory the earlier save left.
- Worst case with every writer running at once at its ceiling: 1.5 GiB per state root (outer bound 1,616,920,640 B) and the families' ceilings from `cache-families.md` (tools allowance 1.5 GiB, sources 128 MiB). New entries are 2 x 1,616,920,640 + 1,610,612,736 + 134,217,728 = 4,978,671,744 B. The live inventory is 1,953,648,299 B of tools and sources entries plus both old state entries at 3,233,841,280 B, which is 5,187,489,579 B. The total is 10,166,161,323 B, which is 2,166,161,323 B over 8e9. Full concurrency therefore does not fit at these ceilings.
- Required ordering, expressed as `needs:` and not as a concurrency group: the sources and tools producers run first, then the state A job of the first root, then the state A job of the second root. Each writer deletes its superseded generation before the next group starts (see [Budget with these ceilings](#two-bound-ceilings)). The producers' peak is 5,187,489,579 + 1,744,830,464 = 6,932,320,043 B. The A phase peak is in the section below, at 6,729,810,112 B. Both are under 8e9.
- The alternative to the chain for the two state A jobs is an outer state bound of at most 1,530,237,952 B each (an inner file of about 1.42 GiB), because four state entries plus 1,879,048,192 B of tools and sources entries must fit in 8e9. The 1.5 GiB ceiling has an outer bound of 1,616,920,640 B, which is over by 86,682,688 B, so it needs the chain.

The decision record fixes the 8e9 total admission and the split of P and A. It does not state the reservation rule or the chain, so treat both as requirements of the producer unit (unit 6 in the order below).

## Storage layout

Six layouts were compared for 18 crate jobs over two Cargo roots (a root workspace with 12 jobs and a nested runner workspace with 6 jobs). mbx's behavior bounds the options:

- The store is content-addressed, so importing two stores that share units leaves the object union (measured: two 109.6 and 109.9 MiB exports became a 135.2 MiB union).
- An export is the closure of what its builds used, including hits. It is not a delta (measured).
- A bundle whose shared objects were removed is refused at import with `cache export is incomplete or corrupt` (measured). A layered base plus delta therefore cannot be built from mbx exports. A gzip file passed to `mbx cache import` fails with exit code 1 (measured).
- An import creates no exportable build, so a merge job must replay the lanes in the imported store to export a union (measured: 22 of 24 and 23 of 25 units hit on replay).
- `--group` selects whole builds. mbx 1.29.1 has no per-unit filter and no delta export.

| Layout | Bytes per generation | Verdict |
|---|---|---|
| One entry per job | Sum of per-job exports, 3.46e9 B zstd -3 (measured, Linux harness) and 18 writers | Not preferred: about 72 percent of the bytes duplicate, and 18 writers conflict with the read-only crate job rule |
| Shared base plus per-job delta layers | At least the per-job sum, because each delta export includes the base objects it used | Not preferred: impossible with mbx 1.29.1 |
| Crate jobs upload exports, one job merges | One union stored, plus transient artifacts per run | Not preferred: adds 18 uploads to the required path, and the merge must replay lanes |
| One producer per Cargo root, one union entry per generation | Union of all, 2.24e9 B zstd -3 (measured, Linux harness) | Decided |
| Per-crate export groups | Sum of closures, like per job | Not preferred |
| mbx remote cache on an object store | Object-level dedupe, no 10 GB cap | Not preferred for now: needs an external service, and the project uses GitHub caches only |

### Decision

Store one union entry per Cargo root and generation. One non-crate producer per root writes it, and crate jobs restore it read-only and never export, save or mark. The producer runs in its own generated workflow, `.github/workflows/state-seal.yml` (decision X73, the fallback of X57 item 4), triggered by `push` to the default branch only. Run 41831221927 measured Required at 337 s and the producer at about 769 s, so with the producer inside `ci.yml` every Rust-change push kept the CI run open for about 9 more minutes. The workflow has its own Plan job, a copy of the `ci.yml` Plan built by the same code path (same checkout, tool resolution, `Compute cache keys`, `Select attested state`, `plan-v1` and `Publish plan` steps), so the compat, snapshot, tools and sources keys it computes equal the ones the crate jobs in `ci.yml` restore. The copy runs `plan-v1` because each lane step resolves its task from the run's `plan.json` (`task_execution.rs`, `load_plan`); the sealing run therefore has a plan of its own. The producer needs only that Plan, runs with `continue-on-error`, and is skipped on an exact key hit. It runs the crate jobs' lane records by digest without uploading task reports, with the same environment, checkout path and store path. A probe showed that foreign-lane resolution works. Every producer-kind job except the tofu providers producers lives in this workflow: the sources and tools producers, the state producers P and the attest jobs A. The tofu providers producers stay in `ci.yml`, because they are chained into the tofu crate lane by `needs` (`tofu_stage.rs`); a repository with tofu roots can therefore keep its CI run open after `Required` on a provider-cache miss, and its attest jobs no longer wait for them. `ci.yml` keeps the compat, snapshot and select steps in Plan, because the crate jobs restore from them, and its `Required` needs are unchanged. The run of `ci.yml` ends when `Required` ends, except for that tofu case. The workflow's concurrency group never cancels a running sealing run; a newer pending run replaces an older pending one, and the tip check below makes that safe. The producer checks the tip as its first step and fails (it is `continue-on-error`) when its head is behind, so a stale run costs seconds, builds nothing and leaves `produced` unset, which skips its seal job.

Measured on a Linux arm64 Docker harness (source `ced5d482b0`, the CI environment, 18 crate jobs, full debuginfo; not a hosted runner):

| Quantity | Value |
|---|---:|
| Sum of per-job exports, raw / zstd -3 | 17.66e9 B / 3.46e9 B |
| Union of all, raw / zstd -3 / zstd -19 | 12.44e9 B / 2.24e9 B / 1.72e9 B |
| Root union, raw / zstd -3 | 8.31e9 B / 1.39e9 B |
| Runner union, zstd -3 | 0.92e9 B |
| Duplication factor (sum over union) | 1.54 |
| Warm run with no change, restored from the union | 98.3 percent hit, 0 registry misses (bypassed units aside) |
| One-line leaf change | 98.2 percent hit |
| Core-crate change | 94.6 percent hit |
| Registry-only skeleton consumer | 80.2 percent hit, 909 units not looked up across 18 jobs |

The harness stored sizes omit `--long=30`, so they are upper bounds. The decision record corrected an earlier aggregation error that reported 3,636 skeleton units not looked up; the correct count is 909. In the harness, `velnor-runner-cli` and `velnor-runner-host` each showed 19 registry misses in every warm scenario. The harness did not set `SOURCE_DATE_EPOCH`, so this is consistent with the `__DATE__` and `__TIME__` expansion in mimalloc's `options.c` (inferred), which `SOURCE_DATE_EPOCH=0` removes. A local probe with `SOURCE_DATE_EPOCH=0` gave 1,062 hits and 2 misses for `velnor-runner-cli`, and 15.5 s warm against 72.5 s cold (measured, macOS).

A generation export contains only what that generation's builds used, so units no longer used drop out without any `mbx gc` (measured).

Earlier download-time estimates of 77 MB and 215 MB came from a registry-only macOS skeleton and no longer apply. The measured roots are 1.39e9 B and 0.92e9 B stored.

## Size levers

The first three rows are decided or measured on Linux. The other rows come from a local macOS arm64 probe on small registry crates, so their Linux effect is inferred.

| Rank | Lever | Effect | Status |
|---|---|---|---|
| 1 | Velnor packer: one `tar` piped to `zstd -3 --long=30`, saved as a single file | Admission checks the exact inner bytes and reserves the outer bound. A plain tar roughly halved stored bytes on macOS (40.4 MB to 20.5 MB on a 137.5 MB export) | Decided. Touches the restore path and needs security review |
| 2 | Debuginfo policy: `debug = 0` for dependencies | About 43 percent fewer stored bytes | Decided, ships after measured main pushes and before pull request state. Changes every unit key once and removes dependency line numbers from backtraces |
| 3 | `zstd -19` instead of `-3` | 23 percent smaller on the union, at 64 to 76 times the wall time | Rejected |
| 4 | One producer and one store for build, clippy and doc | The union of the three lanes is 149.9 MiB against 207.4 MiB separately (28 percent less) | Measured on macOS; inherent to one producer per root |
| 5 | `debug = "line-tables-only"` everywhere, instead of 2 | 17 percent stored | Measured on macOS |
| 6 | `MBX_CACHE_LINKS=0` | Larger, not smaller (raw 131.6 to 142.7 MiB) | Measured; do not use |
| 7 | `split-debuginfo = "off"` | No gain after zstd (1 percent) | Measured on macOS; the Linux default is already off |
| 8 | Exclude incremental state, or run `mbx gc` before export | Nothing to gain: exports never carry incremental state and are already the closure | Source and measurement |

## Key, retention and prune

Three structural problems sit in the current key and prune rules:

1. The snapshot is the SHA-256 of `Cargo.lock` alone (`cache_steps_mbx_bundle.rs`, `compat` and `snapshot` in the discover step). One lock gives one generation forever, because keys are immutable. New workspace-crate units from code edits are never saved while the lock is unchanged.
2. Prune groups entries by the key minus the final `-s256-<hex>` (`cache_prune.rs`), and compatibility is part of the key. A retired compatibility digest is a different group and keeps its entries until GitHub evicts them (inferred from the grouping code).
3. The ceiling bytes are part of the compatibility digest, so any ceiling change orphans every entry.

Decided key, schema 4 (schema 3 is taken by the `SOURCE_DATE_EPOCH` change):

```text
velnor-v4-state-<trust>-<unit>-<root>-s256-<compat>-s256-<snapshot>-s256-<payload>
```

- `trust` is the trust class of the writer, and `unit` is the state unit (for example compiled state). `root` is a validated, readable name for the Cargo root, so prune groups by trust, unit and root across compatibility digests.
- `compat` covers family, schema, mbx version, toolchain, root, `uname -s` and `-m`, `MBX_SHARE_OUT_DIR`, a literal `MBX_BUILD_SCRIPT_EXECUTION=hybrid` line (the bundle's build-script shape, see [The build-script rule](#the-build-script-rule)), `MBX_CC`, the resolved store path, the checkout path, `SOURCE_DATE_EPOCH` and `ImageOS`. It does not cover ceilings, because bounds are admission policy and not compatibility. The store path is in the key because, with `MBX_SHARE_OUT_DIR=1`, the absolute store path enters the action key of every crate that reads `OUT_DIR` (`serde_core`, `serde`, `thiserror`). A local probe showed a consumer with a different store path missing `serde_core`, and a consumer with the producer's path hitting. The checkout path is in the key because compile paths can enter unit keys (see open measurement 10).
- `snapshot` hashes `Cargo.lock`, the root's Rust inputs and the producer task set. An exact hit on the key skips the producer. A commit SHA alone is not an identity (`cache-contract.md`).
- `payload` is the SHA-256 of the stored file. Job A computes it from the file P produced, so the key binds the bytes (see [Restore hardening](#restore-hardening-and-attestation)).
- Plan selects only an attested, newest entry by prefix, so the newest generation of one compatibility wins. A lock change finds the previous generation by prefix, and mbx borrows predictions from earlier lock states, so unchanged crates still hit (UNCONFIRMED: no source or log read for this claim).
- Retention is 1 generation per root. Today prune runs before saves, which leaves 2 generations and explains the 3.68e9 B inventory. Each root's A job deletes that root's superseded generation only after its new entry is saved and sealed (keep new, then delete old). "Saved and sealed" has a fixed meaning: `gh cache list --key <exact key> --ref refs/heads/main` returns exactly that key with `sizeInBytes` at most the outer bound for the inner file (a positive exact-key lookup through the cache API), and the attestation for the payload is verified. A refused or failed upload, a missing key, a size mismatch or an unverified attestation never deletes the old generation. With the delete rule, a push that changes both roots never holds four state entries at once (see [Budget with these ceilings](#two-bound-ceilings)). A `workflow_run` handler after the CI and State seal workflows complete remains as the backstop, grouped by trust, unit and root, with explicit checks of the event, the branch and the repository. It removes any generation that an A job failed to delete. A reader that loses a race with prune sees a miss and builds cold, because a failed restore is a miss.
- The old `velnor-v2-state-trusted-` prefix joins the retired prefixes pruned to zero (`cache_prune.rs:16`). No state entry exists, so this costs nothing.

A save happens whenever the snapshot changes, which means whenever `Cargo.lock`, the root's Rust inputs, or the producer's task set changes, and the exact key is absent. The save does not depend on byte growth. A change in the closure or the action set can leave the object bytes equal or smaller and still needs a new generation, because the old one is keyed to a different snapshot. The earlier rule of saving only when `object_bytes` grew by 1 MiB is dropped.

## Two-bound ceilings

The one raw-byte gate hid the packer problem. Two bounds limit two different resources:

- A stored bound: the bytes of the packed single file (the inner file). The cache counts a larger outer entry, so admission reserves the outer bound defined below.
- An extracted bound: the uncompressed bytes that Velnor's recover step writes to the runner's disk, which also caps a decompression bomb in that step.

Decided values: stored 1.5 GiB and extracted 12 GiB now. After the debuginfo policy they drop to 1 GiB and 6 GiB. The measured maxima are 8.27e9 B raw and 1.39e9 B stored for the root (Linux harness, `zstd -3` without `--long=30`; the earlier 1.05e9 B figure came from the pre-hybrid export and is superseded by the harness table). The first hosted push measured the stored bytes with `--long=30` (see [First hosted result](#first-hosted-result)). Over either bound, the publish step refuses the save, prints both measured byte counts in the warning, and the run stays cold. The warning `over_ceiling` splits into `over_stored` and `over_extracted`. Velnor writes one `tar | zstd -3 --long=30` file and measures its exact size. The pinned `actions/cache` then wraps that file in its own tar and zstd, and GitHub accounts the resulting `cache.tzst` ([the reference](github-actions-reference.md), section 5.6). Admission therefore reserves an upper bound for the outer entry. For an inner file of S bytes:

- tar = ceil((1,024 + 4,608 + ceil(S / 512) x 512 + 1,024) / 10,240) x 10,240. This is two 512-byte header blocks, a 4,608-byte name reserve, the data padded to a 512-byte block, two 512-byte end-of-archive blocks, and padding to the 10,240-byte tar record (a local `tar` write of a 3,000,000-byte file produced 3,010,560 B, so the record padding is real).
- outer = tar + floor(tar / 256). This is `ZSTD_compressBound` for an input of 128 KiB or more, which is the worst case on incompressible input (the `ZSTD_COMPRESSBOUND` macro in `zstd.h`, [zstd source](https://github.com/facebook/zstd/blob/dev/lib/zstd.h)).

At S = 1.5 GiB (1,610,612,736 B), tar is 1,610,629,120 B and outer is 1,616,920,640 B, which is 6,307,904 B over the inner file. The 4,608 B name reserve is part of that number. The code computes it in `outer_entry_bound` (`crates/velnor-actions-contract/src/workflow/cache_family/outer.rs`) and the save script in `crates/velnor-actions-workflow-renderer/src/state_seal_save.rs` applies the same formula. At S = 1 GiB, outer is 1,077,950,520 B. The budget below reserves this outer bound for every state entry. The sealed check reads the actual stored size from the cache API (`sizeInBytes` of the exact key in `gh cache list`) and requires it to be at most the outer bound computed from the inner file's measured size.

The extracted bound does not cap what `actions/cache` extracts. The outer restore step runs `tar -xf -P` on the entry before any Velnor code runs, with no size check [5.7 in the reference]. The extracted bound limits Velnor's own recover extraction only. The outer extraction is a separate tracked risk (SEC-4), and a full pre-extraction guarantee waits on it. The payload digest in the key narrows it: recover compares the payload's SHA-256 with the key before it runs `zstd -dc --long=30` or imports anything. The `--long=30` flag is required because the packer writes frames with a window log above 27, and the zstd CLI manual states that decompressing such frames needs a matching `--long` or `--memory` setting ([zstd manual](https://github.com/facebook/zstd/blob/dev/programs/zstd.1.md)). The extracted-byte guard stays: recover stops once the output passes the extracted bound.

Budget with these ceilings. A ceiling is per entry, and each root has one entry. The earlier version of this section multiplied by the root count twice. Recomputed:

- Both roots at the stored ceiling: 2 times the outer bound of 1.5 GiB is 3,233,841,280 B.
- Post-CI prune alone is wrong. When one push changes both Cargo roots, both old entries stay until CI ends while both A jobs save, so the peak is 4 entries: 4 times 1,616,920,640 B plus 1,953,648,299 B for the tools and sources entries is 8,421,330,859 B. That is over the 8e9 B admission rule.
- With keep-new-then-delete-old and the A jobs ordered by `needs:`, the peak is the other root's old entry plus this root's old and new entries, which is 3 entries: 3 times 1,616,920,640 B is 4,850,761,920 B, and with the 1,953,648,299 B it is 6,804,410,219 B. That is under 8e9 B with 1,195,589,781 B of headroom, so the 1.5 GiB ceiling holds and needs no change. Against the 10e9 B accounting ceiling the reserve is 3,195,589,781 B.
- At the measured Linux sizes (root 1.39e9 B, runner 0.92e9 B, zstd -3), the steady total is 1.39e9 + 0.92e9 + 1,953,648,299 = 4,263,648,299 B. These are inner file sizes, and the outer entry adds at most 0.4 percent. The peak is the root's save, 1.39e9 + 0.92e9 + 1.39e9 + 1,953,648,299 = 5,653,648,299 B (the runner's save peaks lower, at 5,183,648,299 B). Without the delete step the same push peaks at 6,573,648,299 B.
- After the debuginfo policy, with 1 GiB ceilings, the peak is 3 times 1,077,950,520 B plus 1,953,648,299 B, which is 5,187,499,859 B. The reserve against 10e9 B is 4,812,500,141 B, and the headroom under 8e9 B is 2,812,500,141 B, so the reserve is at least 2e9 B.
- The harness totals (steady 4.26e9 B, and 6.57e9 B for the push without the delete step) are upper bounds because they omit `--long=30`.

Requirements for the producer unit:

- Each root's A job deletes the superseded generation of that root only after the new entry is saved and sealed, as defined in the retention rule of the key section (a positive exact-key `gh cache list --key <exact key> --ref refs/heads/main` result with the expected size, plus a verified attestation). A failed or refused upload never deletes the old generation.
- The state A jobs of the two roots run in a `needs:` chain after the sources and tools producers. A chain, not a concurrency group, because a group drops pending members. Peak at ceilings: 3 state entries (4,850,761,920 B) plus 1,879,048,192 B of tools and sources entries (tools 1.5 GiB, sources two entries at 128 MiB) is 6,729,810,112 B, which leaves 1,270,189,888 B under 8e9. Running both A jobs at once at ceilings reaches 8,346,730,752 B, which is 346,730,752 B over.
- Plan's list of eligible writers and the reservation rule under [Budget](#budget) apply to every writer.

All figures are arithmetic from the decided ceilings and the measured entry sizes (inferred for the worst case, measured for the harness totals).

## The build-script rule

Decision X61 amends X48 (rule K). Registry crates' build scripts run under `MBX_BUILD_SCRIPT_EXECUTION=1` so a consumer at `1` hits them. A workspace member's own build script must never be replayed from a bundle another job wrote, because mbx 1.29.1 has no per-package scope for the setting: a bundle written under `1` and restored in a different job replayed a member's stale build-script output, and the consumer exited 100 where a cold build exited 0 (a local reproduction recorded by the maintainers). The producer therefore builds a hybrid bundle in one store and exports it once:

- Phase 1 stubs every workspace target source (libraries empty, other targets a bare `main`, member build scripts included) and runs the crate jobs' lane records at `MBX_BUILD_SCRIPT_EXECUTION=1`. Only dependencies compile for real.
- The restore step puts the sources back (`git checkout` of the stubbed files, then `git diff --quiet`), removes the store's `incremental/` directory, and sets `MBX_BUILD_SCRIPT_EXECUTION=0`. Without the removal the real members compile incrementally, their results are not stored, and the export fails with `action result is missing`.
- The same step removes this run's phase-1 task reports (`$RUNNER_TEMP/velnor/r<run>-a<attempt>/m-*`). It must, because the pinned controller writes each task report once, and phase 2 would otherwise fail with `report_exists` (#491).
- Phase 2 runs the same lane records at `0`. The publish step refuses to export unless the variable is `0`, and the job environment of every producer is `0`.
- Crate jobs run `MBX_BUILD_SCRIPT_EXECUTION=1` (measured over 18 jobs: registry bypass 216 to 10, registry misses 14 to 6, stale-replay safety passes).
- The compatibility digest binds the bundle's shape as the literal `MBX_BUILD_SCRIPT_EXECUTION=hybrid`, never an environment value, so the producer's `0` and the consumers' `1` produce the same key. A bundle of another shape would need another literal.
- The producer and the consumers run on the same runner class, because cargo passes `NUM_JOBS` to build scripts and a `1` bundle misses its build-script runs when the CPU counts differ.
- `SOURCE_DATE_EPOCH=0` goes in every mbx task environment and in the compatibility key. Without it the `velnor-runner-cli` chain always misses, because `mimalloc` expands `__DATE__` and `__TIME__`.

About 4 registry units stay bypassed as `unsupported-crate-type` (an mbx 1.29.1 limit). If the first hosted push confirms it, acceptance T5 follows the unmeetable-target path with that evidence.

## Workflow-level cache-mode

Today ten jobs write the main cache scope: Plan, alint, deny, machete, jactionlint, maintenance-helpers, both Required jobs, and the tools producers, plus the freshness and job-image-candidate jobs. Most of them run third-party code, and an entry from such a job can be poisoned. The design sets `cache-mode: read` at workflow level and grants write only to jobs that run no third-party code: the sources and tools producers and job A. Job A uses `write-only`, since it checks out nothing and restores nothing.

Two limits from [the reference entry](github-actions-reference.md#5-cache-rules-verified-against-original-sources-checked-2026-11-04) apply. The pinned `actions/cache` does not read `ACTIONS_CACHE_MODE` [5.5], so the denial for a `read` job is a service response, logged as a warning, not an informational skip. A reusable workflow cannot raise its caller's explicit mode [5.5]. Whether `cache-mode` accepts expressions is unconfirmed, so the workflow uses literals.

## Restore hardening and attestation

PR #477 landed the restore hardening. At class level, the restore treats the payload as untrusted bytes: it bounds the inflated and extracted size, rejects archives whose members are not plain files and directories under the expected family directory, extracts into a private stage, imports from the staged directory, runs the import from a working directory outside any checkout, and goes cold with exit 0 on every refusal or failure (`cache_steps_mbx_bundle_recover.rs:154-203`). The save side still packs `payload.tar.gz` and no hosted run has exercised the restore, because no entry exists.

The decided design moves the screen from gzip to `zstd -dc --long=30` under the same bounds. These checks detect corruption but do not prove that a trusted writer produced the payload. The class of finding is a store that passes the self-consistency checks without a trusted writer. No further detail is recorded here.

The attestation closes that gap with the P and A split:

- P compiles with cache read access and no `id-token` permission, packs the single file, and uploads it as one artifact with `retention-days: 1`.
- A checks out nothing and runs, in order: download the artifact; build the v4 key from Plan's compat and snapshot outputs plus the file's SHA-256; admit the save against the 8e9 total; save with `write-only`; bind; attest; retire. The bind step lists the exact key and writes the attestation subject. The retire step re-checks the stored size against the outer bound, verifies the attestation, deletes the root's superseded generation and then deletes the artifact by its ID. A save that fails or is refused deletes neither, and the artifact still expires after 1 day. Every `gh` call names the repository (`-R`), because A has no git remote.
- A runs only when the producer's last step set the job output `produced`, never on `needs.<producer>.result`: the producer carries `continue-on-error`, so its result reads `success` even after a failed lane.
- Artifact storage is billed separately and sits outside the cache budget. The handoff file is at most the stored ceiling (1.5 GiB per root) for at most 1 day, and it does not count against the 8e9 admission total. Placing the ceiling and 8e9 total checks in A is inferred from the split.
- Plan selects only an attested, newest entry. The attested subject is one line, `<exact v4 key> <cache id> <size in bytes>`, followed by a line feed. The bind step writes it after the save, from an exact-key `gh cache list` that returns exactly one entry. The cache id and the size tie the attestation to that one stored generation: a key that is deleted and saved again gets a new cache id, and a key listed with other bytes has another size, so neither matches the subject Plan rebuilds from its own listing. The key's last segment is the SHA-256 of the stored file, so Plan verifies the subject before it downloads 1.5 GiB, and Recover then proves that the downloaded bytes hash to the key's segment and that the restored key is the key Plan selected. A pre-download bound on the bytes `actions/cache` writes is not claimed (SEC-4).
- Recover verifies the payload's SHA-256 against the key's payload segment before it runs `zstd -dc --long=30` or imports. A mismatch is a cold run.

The attestation unit (SEC-7) shipped the P and A split first with a throwaway canary pair. #489 removed the canary and added the union producer P (`state-producer-root`, `state-producer-crates-velnor-runner`) and the attest job A (`state-seal-root`, `state-seal-crates-velnor-runner`) in its place. The signer was the workflow file that ran the jobs (`ci.yml`): the runner overwrites the workflow-level blanks of `ACTIONS_RUNTIME_TOKEN` and `ACTIONS_ID_TOKEN_REQUEST_*` for every JavaScript action step (`src/Runner.Worker/Handlers/NodeScriptActionHandler.cs`, actions/runner main, read 2026-11-04). Since X73 the signer is `.github/workflows/state-seal.yml`, because the A jobs run there. Plan selection (`state_select.rs`) and prune (`cache_prune.rs`) both name that signer through the one constant `STATE_SEAL_WORKFLOW_PATH`; they used to take it from `GITHUB_WORKFLOW_REF` and `ci.yml`. The admit and retire steps of A use the same constant.

**Rollout.** Every existing state entry is signed by `ci.yml`. After the move, selection and prune accept only `state-seal.yml` signatures, so the first push to `main` after the merge selects nothing and the crate jobs build cold. That push produces the first generation signed by `state-seal.yml`, and A's retire step deletes the older `ci.yml`-signed generation once the new one is sealed. One cold generation is accepted. The compat and snapshot keys do not change.

**Tip check.** The producer P checks the tip as its first step, and each A job checks it again before Admit. The check reads the tip of the default branch (`gh api repos/<repo>/git/ref/heads/<branch>`) and compares it with `GITHUB_SHA`. If the head is behind, or the tip cannot be read, P fails before building and A logs `state seal skipped: head <sha> is behind ...` (or a `cache_unavailable` warning) and saves, binds, attests and retires nothing, so no unattested entry is left behind. The A job holds `contents: read` for this read, so it also works on private repositories.

## First hosted result

The main push [run 41798794144](https://github.com/tailrocks/velnor-new/actions/runs/41798794144) (head `c9f5f0a815`) was the first successful hosted writer run. The runs before it at `a7b2d83e62` ([41788881765](https://github.com/tailrocks/velnor-new/actions/runs/41788881765)) and `03f6ef18c7` ([41796317458](https://github.com/tailrocks/velnor-new/actions/runs/41796317458)) failed in the producer with `report_exists`, and #491 fixed that. Run 41798794144 wrote and attested two entries (measured, from the seal job logs). The cache entries have these outer sizes: 1,258,969,937 B for the root and 607,368,491 B for `crates/velnor-runner`. The inner `payload.tar.zst` files hold 1,259,348,365 B (`sha256:fe6fa41a…`) for the root and 607,577,040 B (`sha256:0bee1bd1…`) for `crates/velnor-runner`. The admission lines were, for `crates/velnor-runner`, `usage=1953702570 bound=609963800 reserved=1616920640 budget=8000000000`, and for the root, `usage=2561071061 bound=1264275520 reserved=0 budget=8000000000`. The root admission usage of 2,561,071,061 B equals the runner admission usage of 1,953,702,570 B plus the 607,368,491 B runner entry saved just before it, so it is the usage before the root save and not the total after the run.

## Pull-request-scoped state: last

After the producer exists, a pull request run restores the trusted entry that `main` last saved, because a pull request reads the default branch's scope. Reuse for a pull request is therefore limited to units that `main` already produced. A pull request that changes a workspace crate recompiles that crate and everything that depends on it on every push, and a pull request that changes `Cargo.lock` misses the changed units. This page does not promise sequential reuse across pushes to one pull request until pull-request-scoped state lands.

Sequential reuse of a pull request's own output needs state that pull request runs write. Such an entry is readable only from `refs/pull/N/merge`, so `main` never reads it. It is a new writer class, because the contract allows pull request writes for sources only (`cache-contract.md`, `cache-decisions.md` row 5). It needs:

- same-repository pull request writers only, writing in pull request scope;
- a per-push prune of the pull request's superseded entries, run from a trusted handler after CI that holds `actions: write` (pull request jobs hold none), because today only the `pull_request: closed` handler removes pull request entries (`cache_cleanup.rs`);
- a per-pull-request cap;
- admission against the live inventory total before each save, refusing above 8e9 B, serialized as described under [Budget](#budget);
- verification of restored bytes, because a poisoned entry still runs inside that pull request's later jobs;
- a hosted two-push pull request run that confirms a later push restores the earlier push's entry, which no official page states [5.4].

Budget. A full pull request state costs as much as one main generation, since an export is a closure and no delta exists, and the state is one entry per Cargo root: 1.39e9 B for the root workspace and 0.92e9 B for the runner workspace (harness, `zstd -3`), not the 2.24e9 B union of both. With 8e9 B minus the steady 4.26e9 B, the headroom is 3.74e9 B, which fits one pull request state now (arithmetic, 3.74e9 over 2.31e9 is 1.6). The count rises after the debuginfo policy cuts every entry, and the per-pull-request cap and total admission decide the real limit. This is the last unit in the order, after the debuginfo policy, and the work needs a recorded owner decision before any implementation.

## Alternatives not preferred

- **sccache with the GitHub cache backend.** It cannot cache binary, proc-macro or incremental units. It stores one entry per object, which runs into the 200-uploads-per-minute repository limit [5.8], and it has no trust gate of its own.
- **`Swatinem/rust-cache`.** It rebuilds workspace crates by design and has no trust gate.
- **A persistent self-hosted runner.** It would give local-like reuse, but the workflow contract says "V1 emits no self-hosted labels" (`docs/proposed/workflow-contract.md:205`), and the project uses GitHub caches only.
- **Target mode (a `target/` cache per run).** It already produced 4.4 GB of churn per run against the 10 GB quota (issue #157), and a restored `target/` rebuilds workspace crates because source mtimes change on checkout.
- **Raising the 384 MiB ceiling alone.** Issue #436 withdrew that proposal: a raise cannot produce working reuse without a writer and a readable payload.
- **Turning the old `dependency_seed` back to `produce` unchanged.** The old shape cost 535 to 547 runner-seconds per push and exported 5.9 and 3.5 GiB uncompressed (reported).
- **A registry-only skeleton producer.** Rejected by measurement: a skeleton consumer hit 80.2 percent with 909 units not looked up.
- **Plain tar handed to `actions/cache`.** Not preferred: it leaves compression to `zstdmt` without `--long=30` and hides the stored size from admission.

## Sequenced plan

Each unit ends in a check that a reviewer can run. Gate commands come from [repository verification](../implemented/repo-verification.md). Hosted proof uses ordinary pull request and main-push runs only. A pull request run cannot exercise the producer, which runs on a protected push, so producer proof is the first main push after merge.

1. **Restore hardening** (done, #477): import from a staged directory, working directory outside any checkout, bounded extraction.
2. **`SOURCE_DATE_EPOCH=0`** in every mbx task environment and in the compatibility key. Acceptance: a warm run shows 0 registry misses for `velnor-runner-cli` and `velnor-runner-host`.
3. **Workflow-level `cache-mode: read`** with the named writers.
4. **Key v4, the Velnor zstd packer, two-bound ceilings and post-CI prune.** The recover screen moves to `zstd -dc --long=30`, and the generated prune workflow is regenerated. This amends cache decisions 2, 4 and 5 and `cache-contract.md`, citing the decision record.
5. **Attestation (SEC-7)** with the P and A split.
6. **Producer** in the separate push-triggered `state-seal.yml` (X73; the unit first shipped inside the CI run), with `dependency_seed` renamed to `compiled_state` and no alias, together with the `ci-audit` categories. The save tail adds a head check (the pushed commit equals the default branch tip), admission against the live inventory in every state A job, ordered by a `needs:` chain (the sources producer, then the tools producers, then the state A job of each root, one after the other) with no per-job concurrency group, because a group drops pending members (the workflow-level group of `state-seal.yml` only serializes whole runs), a delete of the root's superseded generation right after the new entry is saved and sealed, and a lint that rejects any export or save step in a crate job. The `ci-audit` qualification changes: a per-push state generation save does not disqualify a push, and only sources, tools, or cold or failed state saves do.
7. **Measured main pushes.** Acceptance: a main push after a warm save shows 0 registry units missed or not looked up in the build, clippy and doc lanes of every crate job, and the cache total stays under 8e9 B with the 2e9 B reserve. Bypassed units are reported and decided separately (open item, see [The build-script rule](#the-build-script-rule)).
8. **Debuginfo policy** in the Cargo manifests, with ceilings dropping to 1 GiB and 6 GiB.
9. **Pull-request-scoped state**, after a recorded owner decision.

A failed hit that reaches `import_failed`, `over_stored` or `over_extracted` must become a measurable signal (a stats line or an audit field) instead of a warning that exits 0, and an end-to-end round trip check must run before step 7.

## Open measurements

The Linux harness answered the layout, ceiling, retention, packer-level and hit-rate questions. These remain, on a hosted Linux runner, per Cargo root, lanes build, clippy and doc in one store, `MBX_BUILD_SCRIPT_EXECUTION=0`, `MBX_SHARE_OUT_DIR=1`, `SOURCE_DATE_EPOCH=0`:

1. Stored bytes with `--long=30`, which the harness omitted, and compression CPU time for `zstd -3 --long=30` over the export.
2. Restore time per job on a 4-core runner: download, extraction and `mbx cache import`.
3. A real hosted round trip: save, restore, import, hit counters.
4. The hybrid bundle (dependency skeleton at `1`, real members at `0`) on Linux, to decide the bypassed units.
5. Producer wall time and new object bytes after a one-crate lock bump, and the producer wall time in `state-seal.yml`, which no longer holds the CI run open.
6. The debuginfo lever matrix and cold compile seconds per lane on Linux.
7. A hosted two-push pull request run, to confirm cross-push restore.
8. A per-entry size limit in the version 2 service and a fine-grained-token delete, which the GitHub pages read do not state.
9. The behavior of `read` mode on the pinned `actions/cache` (the warning text), which no run has shown.
10. Compile-path sensitivity on Linux, although the checkout path is stable (`/home/runner/work/<repo>/<repo>`).
11. Whether a warm import on the runner host still fails with exit 101 (`missing out/compile_identity.rs`) after an import from a matching checkout, which one local run showed.
