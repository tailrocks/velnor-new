# Compiled-state persistence across CI runs

Status: proposed; decisions pending; Linux measurements pending. This page is not an implementation claim. It records why compiled state does not carry from one run to the next today, and the design that would carry it.

Labels used for numbers: **measured** (read from a log, `gh` output or source in this repository, or measured with `mbx` 1.29.1 on a local macOS arm64 machine), **reported** (taken from a pull request or issue body, not re-measured), **inferred** (arithmetic or a ratio from other numbers). Every size on Linux hosted runners is inferred or unmeasured until the measurements in [Open measurements](#open-measurements) land.

## Goal

Every sequential CI run reuses the compiled state of the previous run, the same way a second local `cargo` command through `mbx` does. That covers successive pushes to `main` and successive pushes to one pull request. The state must fit GitHub's 10 GB repository cache limit and must not store the same bytes once per crate job.

## Why compiled state does not carry today

Locally, `mbx` keeps one persistent store (`MBX_CACHE_DIR`, `~/Library/Caches/mbx` on macOS, `~/.cache/mbx` on Linux). Every compiled unit and its recorded prediction stay there, so a second run finds each unit. A hosted runner is a fresh machine. Velnor sets `MBX_CACHE_DIR=${{ runner.temp }}/velnor/mbx` (`crates/velnor-actions-workflow-renderer/src/cache_steps_mbx_preflight.rs:85`), an empty directory at job start. mbx looks a unit up only when an earlier build recorded a prediction for it. With an empty store, a unit is "not looked up": it compiles and is stored, and the next job starts empty again.

Four causes stack. Fixing one alone changes nothing observable.

### 1. Nothing writes compiled state

- The workflow has two `actions/cache/save` steps, for Cargo sources (`.github/workflows/ci.yml:440`) and Mise tools (`ci.yml:1925`). Neither carries compiled state (measured).
- Crate jobs restore state read-only. Only a job with exactly one `MbxProducerMark` step may export and save (`cache_steps_mbx_bundle_publish.rs:243`). The only job that carried the mark was the `Dependency seed` job. PR #279 moved the write from crate jobs to that job, and PR #458 removed the remaining write clones.
- PR #466 turned the seed jobs off (`.velnor/config.toml:40`, `dependency_seed = "off"`). Its body states that each seed job cost 535 to 547 runner-seconds per main push and published nothing, because every save was refused as `over_ceiling` (reported).
- `gh cache list` on 2026-11-03 showed 11 entries and 1,953,658,825 B in total: 8 `mise-tools-v2` entries, 3 sources entries, and no state entry (measured).
- Hosted evidence: main push run 41639002674 and pull request run 41641931909. In both runs, 18 of 18 crate jobs logged `Cache not found for input keys: velnor-v2-state-` and skipped `Recover state snapshot`. No log contains `mbx cache import`. A green `Restore state snapshot` step is a miss; read the log line, not the step color (measured).
- In the build lane of each job, `object cache:` reported almost every unit as "not looked up", with 1,496 not looked up per run by the investigator's count (measured; a second count by first matching line gave 1,567 for the push and 1,614 for the PR, so the exact total depends on which line is counted). The clippy and doc lanes of a job hit only what that job's own build lane stored a moment earlier.

### 2. The old writer's saves were refused by a size gate

The seed jobs exported 5.9 GiB (root workspace) and 3.5 GiB (nested runner workspace) of uncompressed bytes (run 40712054296, issue #437, reported). The family ceiling is 384 MiB (`crates/velnor-actions-contract/src/workflow/cache_family/family.rs:131-137`). The publish step measures the uncompressed export with `du -sb` against that ceiling (`cache_steps_mbx_bundle_publish.rs:127`), and then measures the packed payload against the same ceiling (`:135`). Both seed exports failed the first check, before any compression. The 10 GB repository limit was not the binding limit for these refusals. The 8e9 B operating budget had bound earlier, in issue #431 (fixed by #432), which recorded 32 entries of about 8.4 GB refusing every save (reported).

The stored size grew from 63.7 MiB (root) and 12.8 MiB (nested) in PR #279 to 5.9 and 3.5 GiB in run 40712054296. No change records why (unexplained; lanes added after #279 are a plausible factor).

### 3. A restored payload would not import

`Recover state snapshot` runs `mbx cache import "$payload"` on `payload.tar.gz` (`cache_steps_mbx_bundle_recover.rs:101`). mbx 1.29.1 reads an archive argument as an uncompressed tar, so a gzip file fails to import. On failure the script calls `corrupt import_failed`, which prints a warning and exits 0, so the job builds cold and passes. PR #407 introduced the `tar -czf` wrapper and this call together; at PR #278 the import took the directory. The defect has never run on a hosted runner, because the restore always misses first (source-confirmed, tracked in issue #440).

### 4. MBX settings missed registry units inside one run

Velnor ran `MBX_SHARE_OUT_DIR=0`, which keys `OUT_DIR` into each unit, so `serde_core`, `thiserror` and their dependents missed and the misses cascaded. PR #471 set `MBX_SHARE_OUT_DIR=1` (`cache_steps_mbx_preflight.rs:79`, `ci.yml:552`). It keeps `MBX_BUILD_SCRIPT_EXECUTION=0` (`cache_steps_mbx_preflight.rs:83`, `ci.yml:548`), so build-script units may keep missing even with a warm store (inferred).

### Why this stayed hidden

No stage tests the whole chain of export, pack, save, restore, import and hit. Layers 1 to 3 coexisted because the failure path of the restore is silent: it exits 0 with a warning. A cache-key change also starts every run cold, so each of #471, an mbx bump and a runner-image change (`ImageOS`) produces one cold run by design.

## Local compared with CI

| Aspect | Local `cargo` with `mbx` | CI today |
|---|---|---|
| Store | One persistent directory | Empty directory per job |
| Predictions | Kept between commands | None at job start |
| Lanes (build, clippy, doc, doctest) | Share one store | Share the job's own store only |
| Between runs | Store untouched except by `mbx gc` (default budget 5 % of the disk) | Nothing restored, nothing saved |
| Between parallel crate jobs | Not applicable | 18 isolated stores |

Reuse inside a job works; reuse across jobs and runs does not exist.

## GitHub cache rules that bind

Read from GitHub's documentation on 2026-11-03: the dependency-caching page, the REST cache API, the `actions/cache` README and the 2025-11-20 changelog entry. These pages were read through summaries, so check the exact wording before quoting.

- An entry is immutable. A new save needs a new key, and an identical key can never add units.
- `restore-keys` is a prefix match and returns the most recently created match, not the highest generation.
- A run reads caches from its own ref and the default branch. A `pull_request` run writes to `refs/pull/N/merge`, which the default branch and other pull requests cannot read.
- Any run that can read a cache restores its bytes as-is. GitHub does not validate them.
- Entries not accessed for 7 days are evicted. At the repository limit, eviction is least-recently-accessed first, whatever the key.
- The default limit is 10 GB per repository. Paid accounts may raise it since 2025-11-20. This project does not plan that, and raising it needs an owner decision.
- `actions/cache` compresses the archive with zstd (`--long=30`, read from the toolkit source). A gzip file inside the archive gets no benefit from it.
- Not found in the pages read: a per-entry size cap, download rate limits, and whether a fine-grained token with `actions: write` can delete entries. Treat all three as unconfirmed.

## Budget

The cache policy constants are in `crates/velnor-actions-contract/src/config/workflow_cache_policy.rs:11,18,25`:

| Quantity | Bytes |
|---|---:|
| Accounting ceiling | 10,000,000,000 |
| Operating budget | 8,000,000,000 |
| Minimum reserve | 2,000,000,000 |

Inventory on 2026-11-03: 1,953,658,825 B, of which `mise-tools-v2` is 1,730,623,525 B (measured). Headroom under 8e9 B is 6,046,341,175 B (arithmetic). The 384 MiB family ceiling is enforced per family, and save admission checks elapsed time, not the 8e9 total (`cache_save_budget.rs:48-72`). Because eviction ignores the project's own budget, the 8e9 total must be checked on every state save, or a burst of large entries can evict the default branch's state first.

## Storage layout

Six layouts were compared for 18 crate jobs over two Cargo roots (a root workspace with 12 jobs and a nested runner workspace with 6 jobs). Registry crates compile about 5.2 times across the root jobs and 2.25 times across the runner jobs (inferred from crate counts per job). mbx's behavior bounds the options:

- The store is content-addressed, so importing two stores that share units leaves the object union (measured: two 109.6 and 109.9 MiB exports became a 135.2 MiB union).
- An export is the closure of what its builds used, including hits. It is not a delta (measured).
- A bundle whose shared objects were removed is refused at import with `cache export is incomplete or corrupt` (measured). A layered base plus delta therefore cannot be built from mbx exports.
- An import creates no exportable build, so a merge job must replay the lanes in the imported store to export a union (measured: 22 of 24 and 23 of 25 units hit on replay).
- `--group` selects whole builds. mbx 1.29.1 has no per-unit filter and no delta export.

| Layout | Bytes per generation | Verdict |
|---|---|---|
| One entry per job | About 3.0 times one union per root (inferred) and 18 writers | Rejected: duplicates registry units, and 18 writers conflict with the read-only crate job rule |
| Shared base plus per-job delta layers | At least the per-job sum, because each delta export includes the base objects it used | Rejected: impossible with mbx 1.29.1 (measured) |
| Crate jobs upload exports, one job merges | One union stored, plus about 1.7 GB of transient artifacts per run (inferred) | Rejected: crate jobs would export, and the merge must replay lanes |
| One producer per Cargo root, one union entry per generation | One union per root | Recommended |
| Per-crate export groups | Sum of closures, like per job | Only if a measured gap remains; about 3 times the bytes |
| mbx remote cache on an object store | Object-level dedupe, no 10 GB cap | Rejected for now: needs an external service, and the project uses GitHub caches only |

### Recommendation

Store one union entry per Cargo root and generation. One non-crate producer job per root writes it. Crate jobs and the producer restore it read-only before their own work, and crate jobs never export, save or mark. Zero bytes duplicate across the 18 jobs. Each job downloads the whole union for its root, which costs seconds, not quota (inferred: about 77 MB for a root job and 215 MB for a runner job after the size levers below; 0.8 to 3.6 s at the measured 59 to 94 MB/s download rate).

A generation export contains only what that generation's builds used, so units no longer used drop out without any `mbx gc` (measured).

## Size levers

Ranked by effect. Measurements come from a local macOS arm64 probe on small registry crates, so every Linux effect is inferred.

| Rank | Lever | Effect | Status |
|---|---|---|---|
| 1 | Hand a plain tar, not a `tar.gz`, to `actions/cache`, so its zstd long-range matching applies | Stored bytes about halve: 40.4 MB to 20.5 MB on a 137.5 MB export | Measured on macOS; Linux ratio unmeasured. Touches the restore extraction path and needs security review |
| 2 | `[profile.dev.package."*"] debug = 0` (registry dependencies only) | A further 27 percent stored (20.5 to 15.0 MB) | Measured on macOS. Repository policy, not generator code. Changes every unit key once and removes dependency line numbers from backtraces |
| 3 | `debug = "line-tables-only"` everywhere, instead of 2 | 17 percent stored | Measured on macOS |
| 4 | One producer and one store for build, clippy and doc | The union of the three lanes is 149.9 MiB against 207.4 MiB separately (28 percent less); clippy and doc add 14 percent to the build lane | Measured; inherent to one producer per root |
| 5 | Keep the clippy and doc lanes in the state | Saves 14 percent of bytes if dropped, but loses the registry check phase in those lanes | Rejected |
| 6 | Exclude workspace-member links and test executables from the producer | Largest lever by bytes if the producer builds members: the old seed exports were 5.9 and 3.5 GiB | Not measured on the real workspace |
| 7 | `MBX_CACHE_LINKS=0` | Larger, not smaller (raw 131.6 to 142.7 MiB) | Measured; do not use |
| 8 | `split-debuginfo = "off"` | No gain after zstd (1 percent) | Measured on macOS; the Linux default is already off |
| 9 | Exclude incremental state | Nothing to gain: exports never carry incremental state | Source and docs |
| 10 | `mbx gc` before export | Nothing to gain: the export is already the closure | Measured |
| 11 | Higher zstd level | About 25 percent more at 36 s per 137 MB | Not controllable: `actions/cache` picks the level |

Estimated stored bytes per generation for both roots, from a measured skeleton export in a gzip tar (152 MB root, 424 MB runner; a dependency-only skeleton export from a local prototype):

| Scenario | Root | Runner | Both roots |
|---|---:|---:|---:|
| Today's packer | 152 MB (measured) | 424 MB (measured) | 576 MB |
| Plain tar (lever 1) | 77 MB (inferred) | 215 MB (inferred) | 291 MB |
| Levers 1 and 2 | 56 MB (inferred) | 157 MB (inferred) | 213 MB |

The skeleton figures describe an export of registry crates only. A producer that also builds workspace members is larger, and its size is unmeasured.

## Key, retention and prune

Three structural problems sit in the current key and prune rules:

1. The snapshot is the SHA-256 of `Cargo.lock` alone (`cache_steps_mbx_bundle.rs`, `compat` and `snapshot` in the discover step). One lock gives one generation forever, because keys are immutable. New workspace-crate units from code edits are never saved while the lock is unchanged.
2. Prune groups entries by the key minus the final `-s256-<hex>` (`cache_prune.rs`), and compatibility is part of the key. A retired compatibility digest is a different group and keeps its entries until GitHub evicts them after 7 unused days (inferred from the grouping code).
3. The ceiling bytes are part of the compatibility digest, so any ceiling change orphans every entry.

Proposed key, version 3:

```text
velnor-v3-state-trusted-<root-slug>-s256-<compat>-s256-<generation>
```

- `root-slug` is a validated, readable name for the Cargo root. It lets prune group by root across compatibility digests.
- `compat` covers family, schema, mbx version, toolchain, root, `uname -s` and `-m`, `MBX_SHARE_OUT_DIR`, the bundle's `MBX_BUILD_SCRIPT_EXECUTION` value, `MBX_CC`, `MBX_CACHE_DIR` and `ImageOS`. It does not cover ceilings, because bounds are admission policy and not compatibility. `MBX_CACHE_DIR` is in the key because, with `MBX_SHARE_OUT_DIR=1`, the absolute store path enters the action key of every crate that reads `OUT_DIR` (`serde_core`, `serde`, `thiserror`). A local probe showed a consumer with a different store path missing `serde_core`, and a consumer with the producer's path hitting. On hosted Linux the path is constant by construction (`cache_steps_mbx_preflight.rs:104-108`).
- `generation` is the SHA-256 of the lock digest, a producer-scope digest, and, only when the scope includes workspace units, the root tree identifier. A commit SHA alone is not an identity (`cache-contract.md`).
- Restore uses the prefix up to and including `compat`, so the newest generation of that compatibility wins. A lock change finds the previous generation by prefix, and mbx borrows predictions from earlier lock states, so unchanged crates still hit.
- Retention is 2 generations per root across all compatibility digests (today 1 per unit, `family.rs:136`). A concurrent job may still be downloading generation N-1 when generation N is saved, and N-1 is the rollback if N is bad. Prune groups by family and root slug, so a retired compatibility digest dies on the next main push instead of after 7 days.
- The old `velnor-v2-state-trusted-` prefix joins the retired prefixes pruned to zero (`cache_prune.rs:16`). No state entry exists, so this costs nothing.

Save only when the generation holds new units. The producer reads `mbx cache stats --json` before and after its lanes and saves when `object_bytes` grew by at least 1 MiB, or when no entry exists for the compatibility digest. Noise on an all-hit rerun was about 9.7 KB and a real new unit about 25.8 MB (measured, macOS).

## Two-bound ceilings

The one raw-byte gate hid the packer problem. Two bounds limit two different resources:

- A stored bound: the compressed bytes GitHub counts against the quota.
- An extracted bound: the uncompressed bytes the restore writes to the runner's disk, which also caps a decompression bomb.

Over either bound, the publish step refuses the save, prints both measured byte counts in the warning, and the run stays cold. The warning `over_ceiling` splits into `over_stored` and `over_extracted`. The stored bound is measured as `zstd -T0 -3 --long=30` over the tar, the codec `actions/cache` uses (equal to the stored size within a few percent, inferred).

Interim values, to be replaced from hosted measurements: stored 128 MiB (root) and 320 MiB (runner); extracted 768 MiB and 2 GiB. Final rule: stored bound is 1.5 times the median stored size of the chosen variant, rounded up to 16 MiB; extracted bound is 1.4 times the raw size. A packed runner skeleton at 424 MB exceeds the current 384 MiB family bound by about 5 percent, so the runner root cannot be stored under today's single bound.

Worst-case budget with these bounds: 2 roots times 2 retained generations times 2 compatibility digests times (128 + 320) MiB is about 1.88 GB. State per generation for both roots is 213 to 576 MB, so main-only state with 2 retained generations and one superseded digest (4 generations) costs 0.85 to 2.3 GB beside the 1.95 GB already stored. That leaves 3.7 to 5.2 GB under the 8e9 budget (arithmetic from the figures above; inferred).

## The build-script rule

Every writer compiles with `MBX_BUILD_SCRIPT_EXECUTION=0`. A bundle written under `1` and restored in a different job replayed a workspace member's stale build-script output: the consumer exited 100 where a cold build exited 0 (reported from a local reproduction recorded by the maintainers). mbx 1.29.1 has no per-package scope for this setting. The rule is therefore:

- A job that exports a bundle runs with `MBX_BUILD_SCRIPT_EXECUTION=0`, set by role and not by a free constant, so no code path builds a producer environment with `1`.
- The publish step refuses to export unless the variable is `0`, and the renderer rejects a producer job whose environment says otherwise.
- The compatibility digest carries the bundle's value as a literal, so writer and reader keys match whatever the consumers use.
- Consumers keep `0` until a bundle exists that cannot replay stale output. Measured on one root, a `1` consumer over a `0` bundle gets the same registry hits and 23 more "not looked up" units in the build lane (52 against 29), with no wall-time gain.

A side effect of `0`: crates with build scripts may keep recompiling in the consumer.

## Restore hardening

A separate change, in progress and not described here as done, repairs the restore path: it imports from a directory or a plain tar instead of a gzip file, runs the import from a working directory outside any checkout, and bounds extraction. Persistence depends on it. A writer without it would produce entries that restore as `import_failed` and go cold with exit 0.

Poisoned cache bytes are in scope: the objects include proc-macro artifacts that run in the consumer. The checks in the restore script today (member names, symlink and special-file refusal, a manifest, a readback digest) verify consistency of the restored bytes with digests computed from those same bytes. They detect corruption. They do not prove that a trusted writer produced the payload. The `actions/cache` restore also extracts the archive before any of these checks run. The class of finding is a forged or tampered store that passes self-consistency checks. The fix direction is the next section plus the hardening change; no further detail is recorded here.

## Future work: bind the payload digest to the trusted writer

Record the payload digest and the compatibility digest at save time in a place that only the trusted producer job can write, and have the restore compare the restored payload against that record before import. Until this exists, restored state relies on GitHub's rule that only runs of the default branch write the entry a pull request can read. The record format, its storage location and its trust root are not designed yet.

## Pull-request-scoped state: open option

After a producer exists, a pull request run restores the trusted entry that `main` last saved (the pull request reads the default branch). Successive pushes to a pull request then never recompile registry crates, unless the pull request changes `Cargo.lock`. What no run reuses is workspace-member compile output between sequential runs.

Sequential reuse of that output needs state written by pull request runs. A pull request entry is readable only from `refs/pull/N/merge`, so `main` can never read it. It is a new writer class, because the contract allows pull request writes for sources only (`cache-contract.md`, `cache-decisions.md` row 5), and it needs:

- a per-push prune of the pull request's superseded entries, run in a separate workflow that holds `actions: write` (pull request jobs hold none), because today only the `pull_request: closed` handler removes pull request entries (`cache_cleanup.rs`);
- a per-pull-request cap;
- admission against the live inventory total before each save, refusing above 8e9 B;
- verification of restored bytes, because a poisoned entry still runs inside that pull request's later jobs.

Budget. A full pull request state costs as much as one main generation, since an export is a closure and no delta exists. At 213 to 576 MB per generation of registry-only state, with 3.7 to 5.2 GB of headroom after main's retained generations (inferred), 6 to 24 open pull requests fit. These counts are upper bounds: they assume no other growth and a fresh admission check on every save. A smaller overlay made from only the crates a pull request changed is cheaper, but its size is the closure of those crates, which for the largest crates is most of the registry union, so its saving is unmeasured. Writes limited to the first push per lock digest avoid the pruning requirement but give no rolling reuse.

Proposal: leave pull-request-scoped state off. A pull request reads main's state. Revisit after hosted data shows how often pull requests change `Cargo.lock`, and after a measurement of member-only bytes and time per root. This needs a recorded owner decision before any implementation.

## Alternatives ruled out

- **sccache with the GitHub cache backend.** It cannot cache binary, proc-macro or incremental units. It stores one entry per object, which runs into the 200-uploads-per-minute repository limit, and it has no trust gate of its own.
- **`Swatinem/rust-cache`.** It rebuilds workspace crates by design and has no trust gate.
- **A persistent self-hosted runner.** It would give local-like reuse, but the workflow contract forbids it: `docs/proposed/workflow-contract.md:205` says "V1 emits no self-hosted labels", and the project uses GitHub caches only.
- **Target mode (a `target/` cache per run).** It already produced 4.4 GB of churn per run against the 10 GB quota (issue #157), and a restored `target/` rebuilds workspace crates because source mtimes change on checkout.
- **Raising the 384 MiB ceiling alone.** Issue #436 withdrew that proposal: a raise cannot produce working reuse without a writer and a readable payload.
- **Turning `dependency_seed` back to `produce` unchanged.** The old shape cost 535 to 547 runner-seconds per push and exported 5.9 and 3.5 GiB.

## Sequenced plan

Each unit ends in a check that a reviewer can run. Gate commands come from [repository verification](../implemented/repo-verification.md). Hosted proof uses ordinary pull request and main-push runs only. A pull request run cannot exercise the producer, which runs on a protected push, so producer proof is the first main push after merge.

1. **Restore hardening** (separate change, in progress): import format, working directory, bounded extraction.
2. **Measurements** on a Linux runner, with no code in `main` (see below).
3. **Packer and two-bound admission**: plain tar, `over_stored` and `over_extracted`, schema 3.
4. **Key version 3, retention 2 and root-aware prune**, with the generated prune workflow regenerated.
5. **Producer per root**: a dependency-only skeleton export built with `MBX_BUILD_SCRIPT_EXECUTION=0`, one writer per root, gated by a lookup-only probe so a warm push skips it. The save tail adds the new-unit predicate, a head check (the pushed commit equals the default branch tip), a per-root concurrency group with `cancel-in-progress: false`, and a lint that rejects any export or save step in a crate job.
6. **Debug-info policy** in the Cargo manifests, after Linux compile-time numbers.
7. **Acceptance**: a main push after a warm save shows 0 registry-unit misses in the build, clippy and doc lanes of every crate job, and the cache total stays under 8e9 B with the 2e9 B reserve.
8. **Decisions** before the pull-request option or any change to the hard decisions or the cache decisions page: this page does not edit either.

A failed hit that reaches `import_failed` or `over_ceiling` must become a measurable signal (a stats line or an audit field) instead of a warning that exits 0, and an end-to-end round trip check must run before step 7.

## Open measurements

All on a hosted Linux runner, per Cargo root, lanes build, clippy and doc in one store, `MBX_BUILD_SCRIPT_EXECUTION=0`, `MBX_SHARE_OUT_DIR=1`:

1. Raw bytes (`du -sb`), object count and action count of the export, for a registry-only skeleton, a skeleton replaying each crate job's exact arguments, members without links, and members with links.
2. Stored bytes under today's gzip packer and under `zstd -3 --long=30`. The plain-tar lever is worth its security review only if the ratio is 0.6 or lower; above 0.8, drop it.
3. Per crate job: closure raw and stored bytes, and the duplication factor across jobs for each root. This replaces the 3.0 times estimate.
4. Hit rate per job when restoring the union, with registry misses by cause (`mbx explain`).
5. Share of linked outputs in a member export.
6. The debug-info lever matrix and cold compile seconds per lane.
7. New object bytes and producer wall time after a one-crate lock bump.
8. Restore time per job on a 4-core runner: download, extraction and `mbx cache import`.
9. Compression CPU time for `zstd -3 --long=30` over the export.
10. A real hosted round trip: save, restore, import, hit counters. Compile-path sensitivity is untested on Linux, although the checkout path is stable (`/home/runner/work/<repo>/<repo>`).
11. A per-entry size limit and a fine-grained-token delete, which the GitHub pages read do not state.
12. Whether a warm import on the runner host still fails with exit 101 (`missing out/compile_identity.rs`) after an import from a matching checkout, which one local run showed.
