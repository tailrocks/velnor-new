# mbx vs "plain cargo + clone of warm target/": disk and rebuild experiment

Facts measured on this Mac on 2026-11-19 (macOS, APFS host, machine heavily loaded by other agents; wall times are indicative, disk numbers are exact per dedicated volume, `df -k <mountpoint>` "Used", after `sync`).

## Setup (exact)

- Source: `git -C ~/Projects/tailrocks/velnor-new archive HEAD ci-helper generator > src.tar` (HEAD 09f030b542b2d88bf2c822166368bb2d42f5c47f). **Deviation:** `ci-helper/` alone does not build: `ci-helper/crates/velnor-ci` has path dependencies on `../../../generator/crates/velnor-{gen,model,release,rust}`, so `generator/` was extracted next to it (`<checkout>/{ci-helper,generator}`). The build runs in `<checkout>/ci-helper`. Workspace units (path crates: velnor-ci, velnor-gen, velnor-model, velnor-release, velnor-render, velnor-rust = 6 crates, 10 compilation units with bin/test targets) are the only crates that are not registry crates. Each "fresh extraction" is `tar -x -f src.tar -C <volume>/<name>` (mtimes = commit time).
- Toolchain: `PATH=$HOME/.cargo/bin:/usr/bin:/bin:/usr/sbin:/sbin RUSTUP_TOOLCHAIN=1.99.1 RUSTUP_HOME=<real ~/.rustup> CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0`, shared `CARGO_HOME=~/.cache/ci-ambition/mbx-exp/cargo-home` (`cargo fetch --locked` once, 187 MiB, outside the volumes).
- Build command everywhere: `cargo build --locked --workspace --all-targets` (strategy C) and `mbx build --locked --workspace --all-targets` (strategy M; mbx 1.29.2 binary run by full path, the documented zero-config explicit form `mbx <cargo-command>`; `mbx cargo build` is not the documented form). `mbx doctor` in the isolated env reports "Cargo shim is not installed; explicit `mbx <cargo-command>` still works", so nothing global was used.
- Volumes: four sparse images (80 GB) `apfsM apfsC` (`hdiutil create -size 80g -type SPARSE -fs APFS`) and `hfsM hfsC` (`-fs JHFS+`), attached with `hdiutil attach -nobrowse -mountpoint ~/.cache/ci-ambition/mbx-exp/mnt/<name>`. Strategy M and C each have their own volume per filesystem; volumes were emptied (`rm -rf` contents, `df` rechecked) before the final runs, so no measurement mixes. Extra APFS volumes S, N1, N2 for scenario R.
- mbx isolation env (strategy M and R): `HOME=~/.cache/ci-ambition/mbx-exp/home XDG_CACHE_HOME/XDG_CONFIG_HOME/XDG_STATE_HOME=<that home>/.cache|.config|.state MBX_CACHE_DIR=<volume>/mbxcache MBX_GC_AUTO=false`. mbx finds its store via `cache_dir` setting / `MBX_CACHE_DIR` (default `~/Library/Caches/mbx` on macOS), managed targets default to `<cache_dir>/targets`, global config is `~/Library/Application Support/mbx/config.toml` (macOS; under redirected HOME it does not exist, so defaults apply). Auto-GC was disabled so eviction cannot change sizes (default budget on an 80 GB volume would be 5 GiB).
- `target/` under mbx is a symlink to `<cache_dir>/targets/v1/<hash>` (managed target, `target.views=true` default). `target_kb` uses `du -skL`.
- Strategy C: plain `cargo build`; clone of A's target into B..F with `cp -c -R -p` (APFS, clonefile) or `cp -R -p` (HFS+, full copy) immediately after extraction.
- Strategy R: server `mbx-cache 0.0.1` built with `CARGO_TARGET_DIR=~/.cache/ci-ambition/mbx-exp/server-target cargo install --locked --root ~/.cache/ci-ambition/mbx-exp/bin --path ~/.cache/ci-ambition/refs/mr-boxington-cache` (own CARGO_HOME `cargo-home-server`; no docker needed). Started: `mbx-cache --allow-anonymous --data-dir <volS>/data --listen 127.0.0.1:18080` (PID 7873, stopped by `kill 7873`). Clients: `MBX_REMOTE_URL=http://127.0.0.1:18080 MBX_REMOTE_NAMESPACE=exp/ci-helper MBX_REMOTE_MODE=read-write MBX_REMOTE_TRUST_WRITES=true MBX_SUMMARY=full` (TRUST_WRITES because mbx treats non-GitHub/GitLab CI as read-only by default), with N1 and N2 each its own `MBX_CACHE_DIR` on its own volume.
- M3/C3/R4 change: `printf '\npub fn __exp() {}\n' >> generator/crates/velnor-render/src/lib.rs`. velnor-render has 1 direct dependent (velnor-gen), which velnor-ci uses; chosen as a low-fan-out crate. (velnor-ci itself has none, but is the largest.)
- Not run: optional M4/C4 (lockfile change).

## Isolation proof

- Every mbx invocation ran with the env above. `mbx doctor` under it printed `cache /Users/.../mbx-exp/mnt/apfsM/mbxcache is writable`, and `mbx stats` printed `store: .../mnt/apfsM/mbxcache/actions`, `managed targets 6`.
- The user's real mbx dirs were snapshotted before/after (`logs/real-before.txt`, `logs/real-after.txt`, script `logs/snap-real.sh`; a recursive du/find was not possible: the real cache is about 485 GiB and in constant use). Result: `~/Library/Application Support/mbx/config.toml` unchanged (0 bytes, mtime 2026-11-04 13:20:48, sha1 da39a3ee...); `~/.cache/mbx`, `~/.config/mbx` absent before and after. Top-level mtimes of `~/Library/Caches/mbx/{incremental,scheduler}` and `~/Library/Application Support/mbx/bin` DID change (incremental 03:35:40 -> 04:24:12, scheduler 03:39 -> 04:25:45, bin 04:15:32), but those are being written by other agents' live processes: during the runs `ps` showed other mbx processes (`mbx gc`, clippy/rustc under `~/Library/Caches/mbx/shims`) using the real cache, and the bin change is the real wrapper rewriting `cargo`/`mbx-target` (04:15:32, content = path of the mise mbx binary). Evidence that none of it is mine: `grep -c mbx-exp` over the real `scheduler/{decisions.jsonl,decisions.jsonl.1,observations.jsonl,memory.json}` = 0 in each, `cargo-roots`/`out-dirs` hold 5 entries total, and my redirected `home/` contains no mbx files. I cannot show byte-identical real dirs, only that the observed changes are attributable to other users and that no record of my paths exists in them.
- `~/.local/share/mise` was never written; the mbx binary was only executed.
- Side effect noticed: when mbx starts under the redirected HOME it spawns a shell whose rc files print `mise ERROR ... config not trusted` / `rtk: command not found` into build logs. That is read-only noise from the user's shell init, not from mbx.

## Key observations (read these before the tables)

1. mbx did **not** cache the workspace (path) crates in this layout: every extra checkout showed `144 hits, 0 misses, 10 not looked up` (all 144 registry units hit; the 10 workspace units, 6 crates, were "unconsulted" and compiled by real rustc, 116 `Compiling` lines are printed by cargo even for hits, so that line count is not "compiled"). Warning in the log: `result was not stored: absolute path has no stable cache mapping: .../generator/samples/helper-lock/view/.velnor/rust-ci.lock`. Each such build still stores its 10 unconsulted results: `229.7 MiB stored locally` per checkout (store grew about 238 MB per checkout).
2. Plain cargo with a cloned target also recompiled exactly the 6 workspace crates in every extra checkout (`Compiling` lines = 6; 116 only in the cold build), although sources/mtimes were equal. Cause: the new checkout's workspace-unit fingerprint/metadata hashes differ (path packages; velnor-gen etc. live outside the ci-helper workspace root, so their identity includes the absolute checkout path). `target/debug/.fingerprint` in a cloned checkout holds both A's old hash directories (cloned, now dead) and new ones for velnor-{ci,gen,model,release,render,rust}. `CARGO_LOG=cargo::core::compiler::fingerprint=info|debug` printed nothing with cargo 1.99.1, so the exact dirty reason line was not obtained; the evidence is the differing fingerprint directory names and the 6 `Compiling` lines (`logs/diag-apfsC-H.log`). Mtimes were not the cause (git archive mtimes are identical in all checkouts).
3. The recompiled workspace units dominate the per-checkout disk: libvelnor_ci.rlib 75 MiB, libvelnor_rust.rlib 67 MiB, velnor-ci binary 43 MiB, test binary `it` 37 MiB, etc. (`logs/big.out`). With C the stale cloned copies from A stay in the target as well (target 1.0 GB -> 1.46 GB logical).

## Tables (raw TSV in logs/*.tsv)

Columns: `store` = mbx cache dir minus `targets/` (the managed targets are what `target/` symlinks to, so they are counted in the target column). Baselines: APFS volumes 20-24 KiB used when empty; HFS+ volumes 207364 KiB used when empty (journal+catalog). The Δ column excludes source extraction (about 7 MB tar, extracted before the "before" reading) except nothing else.

### APFS (clonefile available): M = mbx, C = plain cargo + `cp -c -R -p` clone

| step | checkout | df Used after (KiB) | Δ disk this step (KiB) | store excl. managed targets (KiB) | target/ size (KiB, du -L) | files nlink>1 / total | cargo `Compiling` lines (workspace crates) | wall s | clone s |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|
| M1 | A | 890768 | 883468 | 525088 | 992740 | 0 / 1489 | 116 (6) | 213.383 | 0 |
| M2-B | B | 1395532 | 497472 | 763236 | 909088 | 0 / 1230 | 116 (6) | 116.367 | 0 |
| M2-C | C | 1899240 | 496420 | 1001368 | 919472 | 0 / 1230 | 116 (6) | 125.439 | 0 |
| M2-D | D | 2413932 | 507424 | 1240328 | 904164 | 0 / 1230 | 116 (6) | 92.676 | 0 |
| M2-E | E | 2858636 | 437412 | 1477556 | 891984 | 0 / 1230 | 116 (6) | 64.452 | 0 |
| M3 | F | 3365564 | 499660 | 1715752 | 905416 | 0 / 1230 | 116 (6) | 93.477 | 0 |
| C1 | A | 875404 | 868108 | 0 | 1000104 | 0 / 1454 | 116 (6) | 211.319 | 0 |
| C2-B | B | 1349956 | 468288 | 0 | 1466616 | 0 / 1626 | 6 (6) | 108.861 | 1.091 |
| C2-C | C | 1841212 | 483964 | 0 | 1482572 | 0 / 1626 | 6 (6) | 119.285 | 1.336 |
| C2-D | D | 2335196 | 486700 | 0 | 1463504 | 0 / 1626 | 6 (6) | 94.153 | 1.385 |
| C2-E | E | 2776724 | 434220 | 0 | 1448764 | 0 / 1626 | 6 (6) | 57.128 | 1.064 |
| C3 | F | 3246780 | 462756 | 0 | 1460296 | 0 / 1626 | 6 (6) | 97.447 | 0.968 |

APFS clone-alone deltas (C strategy; first reading is noise from a concurrent unrelated writer, df of the volume went down 20956 KiB):

    clone_B delta_kb=-20956 s=1.091
    clone_C delta_kb=556 s=1.336
    clone_D delta_kb=564 s=1.385
    clone_E delta_kb=560 s=1.064
    clone_F delta_kb=560 s=0.968

### HFS+ (no clonefile): M = mbx (hardlink fallback), C = plain cargo + `cp -R -p` full copy

| step | checkout | df Used after (KiB) | Δ disk this step (KiB) | store excl. managed targets (KiB) | target/ size (KiB, du -L) | files nlink>1 / total | cargo `Compiling` lines (workspace crates) | wall s | clone s |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|
| M1 | A | 1716516 | 1502252 | 524080 | 978164 | 0 / 1489 | 116 (6) | 161.479 | 0 |
| M2-B | B | 2578016 | 854600 | 494868 | 883804 | 212 / 1230 | 116 (6) | 91.634 | 0 |
| M2-C | C | 3439424 | 854508 | 732980 | 883728 | 212 / 1230 | 116 (6) | 143.872 | 0 |
| M2-D | D | 4301196 | 854872 | 971160 | 884024 | 212 / 1230 | 116 (6) | 87.734 | 0 |
| M2-E | E | 5162752 | 854656 | 1209312 | 883836 | 212 / 1230 | 116 (6) | 71.858 | 0 |
| M3 | F | 6023788 | 854136 | 1447288 | 883492 | 212 / 1230 | 116 (6) | 57.053 | 0 |
| C1 | A | 1191912 | 977648 | 0 | 977648 | 0 / 1454 | 116 (6) | 152.932 | 0 |
| C2-B | B | 2643376 | 1444564 | 0 | 1444564 | 0 / 1626 | 6 (6) | 93.817 | 5.425 |
| C2-C | C | 4094720 | 1444444 | 0 | 1444444 | 0 / 1626 | 6 (6) | 121.813 | 3.444 |
| C2-D | D | 5546192 | 1444572 | 0 | 1444572 | 0 / 1626 | 6 (6) | 84.919 | 5.125 |
| C2-E | E | 6997468 | 1444376 | 0 | 1444376 | 0 / 1626 | 6 (6) | 72.810 | 3.275 |
| C3 | F | 8448700 | 1444332 | 0 | 1444332 | 0 / 1626 | 6 (6) | 57.683 | 3.860 |

HFS+ clone-alone deltas (full copy):

    clone_B delta_kb=977648 s=5.425
    clone_C delta_kb=977648 s=3.444
    clone_D delta_kb=977648 s=5.125
    clone_E delta_kb=977648 s=3.275
    clone_F delta_kb=977648 s=3.860

### R: mbx with remote cache server (APFS nodes N1 and N2, server data on APFS volume S)

| step | checkout | df Used after (KiB) | Δ disk this step (KiB) | store excl. managed targets (KiB) | target/ size (KiB, du -L) | files nlink>1 / total | cargo `Compiling` lines (workspace crates) | wall s | clone s |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|
| R1 | A | 878284 | 870984 | 524864 | 980048 | 0 / 1489 | 116 (6) | 69.143 | 0 |
| R4-N1 | F | 1887136 | 1001572 | 1273180 | 890508 | 0 / 1230 | 116 (6) | 56.394 | 0 |
| R2 | A | 1075160 | 1067860 | 777712 | 902744 | 0 / 1248 | 116 (6) | 80.548 | 0 |
| R3 | B | 1582044 | 499604 | 1016808 | 896244 | 0 / 1230 | 116 (6) | 73.537 | 0 |
| R4-N2 | F | 2297256 | 713364 | 1487780 | 887760 | 0 / 1230 | 116 (6) | 50.103 | 0 |

## Marginal disk per additional checkout (mean of B..E; F in brackets)

| filesystem | strategy | marginal disk per extra checkout | clone alone | cargo `Compiling` / real compiles per extra checkout |
|---|---|---:|---:|---|
| APFS | M mbx (reflink restore) | 484,682 KiB = 473 MiB (F: 487.9 MiB) | n/a | 144 hits, 0 misses, 10 units not cached (6 crates) |
| APFS | C plain + `cp -c` clone | 468,293 KiB = 457 MiB (F: 451.9 MiB) | about 0.55 MiB, 1.0-1.4 s | 6 crates |
| HFS+ | M mbx (hardlink restore) | 854,659 KiB = 835 MiB (F: 834.1 MiB) | n/a | 144 hits, 0 misses, 10 units not cached (6 crates) |
| HFS+ | C plain + `cp -R` copy | 1,444,489 KiB = 1,411 MiB (F: 1,410 MiB) | 977,648 KiB = 955 MiB, 3.3-5.4 s | 6 crates |

First (cold) checkout A: APFS M1 883,468 KiB (store 525,088 + managed target ~ rest) vs C1 868,108 KiB; HFS+ M1 1,502,252 KiB vs C1 977,648 KiB (HFS+ M1 pays for store plus a hard-linked/copied target, 524 MB of store that the first checkout does not reuse).
Wall times (indicative, load varied): APFS M1 213 s, C1 211 s; later checkouts M 64-125 s, C 57-119 s; HFS+ M1 161 s, C1 153 s, M later 57-144 s, C later 57-122 s. mbx shows no wall-time advantage here because the 10 workspace units dominate and are compiled in both strategies.
mbx output materialisation (APFS): 0 of 1,230 files in target/ have nlink>1 (reflinked or copied: per R logs `229 outputs (271.6 MiB) reflinked, 0 hard linked, 158 outputs (1.8 MiB) copied`). HFS+: 212 of 1,230 files nlink>1 (hard links to the store). Cold checkout A has 0 linked on both (outputs are created by the compiler, then stored).

Answer to the question, on this workload: on APFS the two strategies are within about 3 percent (mbx +16 MiB per extra checkout, because it additionally stores a never-reused 230 MiB copy of the path-crate results in the store, while the cloned target carries dead A-hash artifacts for free via clonefile). On a filesystem without reflink (HFS+ as ext4 stand-in) mbx uses 835 MiB vs 1,411 MiB per extra checkout for a full-copy target, i.e. 41 percent less, mostly because registry outputs are hard-linked (212 files) instead of copied (955 MiB). Neither avoids recompiling the 6 workspace crates per checkout.

## R: remote cache server (nodes N1, N2 on APFS; server volume S)

Proof both nodes use the remote (client counters from `MBX_SUMMARY=full`, server counters from `/metrics`, `logs/R-server-metrics.txt`, `logs/fin.out`):

- R1 N1 (cold, empty server): `0 hits, 0 misses; 0 B downloaded, 503.7 MiB uploaded`, `uploads: 1235 published (957 of them in 144 packs)`. Server after R1: blob_uploads 1057, action_commits 178, data dir 518,760 KiB / 1,057 files.
- R2 N2 (empty local store, warm server): `164 hits, 6 misses, 178 prefetched; 503.7 MiB downloaded, 250.9 MiB uploaded`; server blob_hits 1056, action_hits 178. 6 misses + 10 unconsulted units were compiled locally (the workspace crates; `slowest uncached crates: velnor_ci 36.6s ...`). So hits for registry crates work across nodes and across checkout paths, workspace crates are rebuilt.
- R3 N2 second checkout (local store hit): `144 hits, 0 misses, 0 prefetched; 0 B downloaded, 229.9 MiB uploaded`.
- R4 N1 with the leaf change: `144 hits, 0 misses, 18 prefetched; 459.4 MiB downloaded, 229.6 MiB uploaded`, 10 unconsulted compiled. R4 N2 same change: `144 hits, 0 misses, 9 prefetched; 229.6 MiB downloaded, 229.7 MiB uploaded`, 10 unconsulted compiled (no cross-node hit for the changed or path crates).

Disk per volume (KiB, df Used):

| step | node | node df Used after | node Δ | server df Used after | server Δ | crates actually compiled | wall s |
|---|---|---:|---:|---:|---:|---|---:|
| R1 | N1 A | 878,284 | 870,984 | 520,008 | +519,984 | 6 crates (10 units) + 180 not-looked-up (cold: all) | 69.1 |
| R2 | N2 A | 1,075,160 | 1,067,860 | 755,404 | +235,396 | 6 misses + 10 unconsulted, 164 hits | 80.5 |
| R3 | N2 B | 1,582,044 | 499,604 | 991,000 | +235,596 | 10 unconsulted, 144 hits | 73.5 |
| R4 | N1 F | 1,887,136 | 1,001,572 | 1,226,332 | +235,332 | 10 unconsulted, 144 hits | 56.4 |
| R4 | N2 F | 2,297,256 | 713,364 | 1,461,776 | +235,444 | 10 unconsulted, 144 hits | 50.1 |

(Wall times in this block were measured when the machine was less loaded than in M/C runs, so do not compare them directly.)
Remote vs local vs clone: a node pulling from the server for an empty store downloads the whole 503.7 MiB set (R2 node Δ 1,067,860 KiB, more than a cold local build's 870,984 because it also stores 250 MiB of its re-uploaded workspace results and reflinked targets); a second checkout on a node with a warm local store costs 499,604 KiB, like local M2 (about 485,000) and like C2 (about 468,000). The server grows about 235 MB per checkout/node build (never-reused path-crate results), plus 519 MB for the first full build. Per extra checkout the total (node + server) is about 735 MB for remote vs 473 MiB for local mbx vs 457 MiB for clone-C on APFS. Recompiled crates per additional checkout stay at the 6 workspace crates in all three.

## Caveats

- One workspace, one commit; the cache-unfriendly layout (path deps outside the workspace root, a build script/test referencing an absolute path) drives the 10 uncached units. A workspace whose crates are cacheable would show mbx rebuilding about 0 crates, not measured here.
- Hits: mbx's own summary counts; the "crates compiled" for M is cargo's `Compiling` lines (always 116 when outputs are restored) so the real rustc work is read from `hits/misses/not looked up` and `slowest uncached crates` lines.
- `df` includes small journal/metadata noise; APFS C first clone delta was -20,956 KiB (unrelated write elsewhere in the container). HFS+ empty baseline 207,364 KiB. Differences under about 1-2 MiB are not meaningful.
- Auto-GC off (MBX_GC_AUTO=false); with GC on, eviction would bound the store, and the 230 MiB/checkout of never-reused entries would be collected eventually.
- Sparse images keep the volumes independent, but APFS volumes inside images share no clones with the host APFS. Sparse-image band overhead is not counted in df.
- Wall times are from a machine with other agents building; M and C pairs were run concurrently, so they are comparable to each other only roughly. Scenarios M and C ran 2 at a time.
- M4/C4 (lockfile change) was not run. CARGO_LOG fingerprint reason not captured (see observation 2).
- `mbx doctor` warns about atime updates not being enabled on these volumes (eviction/seed filtering only).
- HFS+ stands in for ext4 only for "no reflink"; ext4 hard-link behaviour is the same as here, but timing and block allocation differ.

## Cleanup performed

Server PID 7873 stopped with `kill` (confirmed gone). Detached (`hdiutil detach`) and deleted: apfsM.sparseimage (3.5 GiB on disk), apfsC (4.2 GiB), hfsM (5.8 GiB), hfsC (8.2 GiB), apfsS (1.4 GiB), apfsN1 (1.9 GiB), apfsN2 (2.3 GiB); removed `mnt/` mount points, `server-target/` (1.1 GiB cargo target of mbx-cache), `cargo-home-server/` (339 MiB). Kept in `~/.cache/ci-ambition/mbx-exp/`: results.md, logs/ (TSVs, build logs, snapshots), scripts, src.tar, cargo-home (fetched deps, 187 MiB), bin/bin/mbx-cache, home/ (empty redirected HOME). Real mbx cache, real mbx config and ~/.local/share/mise were not modified by me.
