# mbx vs plain-cargo target clone on the k3s cluster (namespace mbx-cache-exp)

Run 2026-11-18 21:35-22:00 UTC (about 25 min of the 3 h budget). Raw per-scenario logs: `logs/<SCEN>.log` (full pod logs, `RESULT` lines), `logs/<SCEN>.top-before.txt|top-after.txt` (kubectl top nodes), `logs/load.log`; manifests in `manifests/`; guard audit in `kx-audit.log`.

## Setup

- Nodes: A = velnor-190 (build + mbx-cache server), B = velnor-5. 96 vCPU, 125 GiB RAM each, kernel 6.12.94 (Debian 13 host). Not checked whether the pair is in FI.
- Images: `rust:1.99.1-bookworm` for everything (rustc 1.99.1 58cf8f9c3, cargo 1.99.1). `RUSTUP_TOOLCHAIN=1.99.1` is set so the repo's `rust-toolchain.toml` (extra components/targets) triggers no rustup download. No system packages needed for the build (aws-lc-sys, turso built with the image's gcc/cmake). `socat` was apt-installed in mbx jobs only (see deviation 1).
- Source: velnor-new `90ce2ad4b14c86f772e0b9af8732844f94e90098` via codeload tarball, extracted to `/w/velnor-new` in every job (`/w` = PVC `subPath w-<job>`, so a different directory on the PVC, same path in the pod). Build: `cargo build --locked --workspace --all-targets` in `crates/velnor-runner`, `CARGO_BUILD_JOBS=32`, `CARGO_INCREMENTAL=0`. `cargo fetch --locked` runs before the timed build (about 4-5 s, CARGO_HOME on an emptyDir, not in the wall time). Each pod: requests 8 CPU / 16 Gi, limits 32 CPU / 48 Gi.
- mbx 1.29.2 from the jdx/mr-boxington v1.29.2 release (`mbx-x86_64-unknown-linux-gnu.tar.gz`, sha256 `73e135d3...0d6` verified against the release `SHA256SUMS`, OK). Config via env: `MBX_CACHE_DIR=/store/mbx MBX_GC_AUTO=false MBX_SUMMARY=full MBX_REMOTE_URL=http://127.0.0.1:8080 MBX_REMOTE_NAMESPACE=exp/velnor-runner MBX_REMOTE_MODE=read-write MBX_REMOTE_TRUST_WRITES=true MBX_REMOTE_TOKEN` (from the Secret). Build command `mbx build --locked --workspace --all-targets`.
- Server: mbx-cache 0.0.1 built in-cluster (`cargo install --locked --git ... --rev f9adcc6`, 56 s), Deployment on node A, filesystem blobs on its own PVC, in-memory metadata, token auth from `MBX_CACHE_TOKENS_JSON` (read+write on `exp/velnor-runner` only, no anonymous), Service `mbx-cache:8080`, readiness `/v1/status`. Never restarted.
- Filesystems (identical on A and B, see `logs/probe-a.log`, `probe-b.log`):

| mount | fs | reflink (`cp --reflink=always` of 100 MB) |
|---|---|---|
| local-path PVC (`/var/lib/rancher/k3s/storage`, device `/dev/mapper/data-rancher`, 3.7 TB, noatime) | **xfs** | **yes** (rc=0, df unchanged after the copy) |
| emptyDir (kubelet OS disk `/dev/nvme0n1p3`, 3.7 TB) | **ext4** | **no** (`Operation not supported`) |
| container root | overlayfs on the same xfs | not tested |

Deviations from the brief:
1. mbx refuses a plain-HTTP remote URL when a token is set unless the host is loopback (documented in cache-server.md). Jobs therefore run `socat TCP-LISTEN:127.0.0.1:8080 -> mbx-cache.mbx-cache-exp.svc:8080` inside the pod and mbx uses `http://127.0.0.1:8080`. Traffic still crosses the cluster network to the server.
2. `MBX_REMOTE_TRUST_WRITES=true` is required: outside GitHub/GitLab push-to-protected-branch mbx forces the remote read-only.
3. `df -k` is the whole node XFS (`data-rancher`, shared with CI/containerd). Concurrent CI writes add noise (the nodes were idle, 0-15% CPU, but df "start" values drift by about 100-400 MB between jobs, e.g. P1 end 94304328 vs P2 start 93952736). Where noise matters I also give `du` of my own directories. "Δ build" is df before to after the build and includes only a few seconds of noise.
4. The checkout `/w/velnor-new` is 110 MiB per job (w_kb about 112000) and is included in "total Δ".
5. E1b/E1c (first concurrent attempt) are invalid: both pods used the same reflink probe filename and E1c fell back to a full copy. They were replaced by E1d.

## Scenario table

All rows: build rc=0. Wall = timed `cargo build`/`mbx build` only. Sizes in GiB (KiB/1048576). "Compiling" = number of `Compiling` lines (for mbx this is cargo's print, which also shows restored crates).

| id | node | what | wall s | Compiling (ws crates) | mbx hits / misses / not looked up | mbx up / down | build Δ df | total Δ df (incl. checkout, clone) | store (excl. targets) | target/ du -L | files nlink>1 in target |
|---|---|---|---:|---:|---|---|---:|---:|---:|---:|---:|
| P1 | A | plain, cold, then `cp -a --reflink=always` target to `/store/plain/target-warm` | 92.0 | 357 (8) | - | - | 10.14 GiB | 10.25 GiB (save-warm: 0.26 s, +2.3 MB) | - | 10.64 GiB | 132 |
| P2 | A | plain, fresh checkout, reflink clone of warm target | 0.31 | 0 | - | - | 0.0007 GiB | 0.114 GiB (clone alone: 0.25 s, +2.5 MB) | - | 10.64 GiB | 132 |
| P3 | A | plain, emptyDir (ext4), full copy of warm target | 0.23 | 0 | - | - | 0 | 0 on PVC; 10.75 GiB on OS disk (w_kb 11278636) | - | 10.65 GiB | 132 |
| M1 | A | mbx + remote, empty local store, empty server | 93.2 | 357 (8) | 2 / 0 / 870+ (progress line; 18 to 49 bypassed) | 8.7 GiB up / 0 | 19.48 GiB | 19.64 GiB | 8.81 GiB (store total 19.47 GiB incl. managed target) | 10.66 GiB | 132 |
| M2 | A | mbx, fresh checkout same path, warm local store | 0.32 | 0 | 0 / 0 (3 bypassed) | 0 / 0 | about 0 | 0.154 GiB (checkout) | 8.81 | 10.66 | 132 |
| M2b | A | as M2 but `/store/mbx/targets` deleted first (store kept) | 7.2 | 357 (8) | 496 / 0 / 0 | 0 / 0 | +1.94 GiB | -1.74 (incl. -3.84 GiB target removal) | 15.63 | 10.66 | 132 |
| M3 | B | mbx, empty local store, warm server | **39.9** | 357 (8) | 496 / 0 / 0, 486 prefetched | 0 / **8.7 GiB down** | 10.70 GiB | 10.85 GiB | 8.80 | 10.66 | 132 |
| M4 | B | mbx, fresh checkout same path, warm local store | 0.32 | 0 | 0 / 0 (managed target reused) | 0 / 0 | about 0 | 0.154 GiB | 8.80 | 10.66 | 132 |
| L1p | A | plain, clone of warm main target + `cargo update -p fastrand --precise 2.4.1` (2.5.0 -> 2.4.1) | 67.4 | 11 (3) | - | - | 8.71 GiB | 8.83 GiB (clone 0.25 s, +2.4 MB) | - | 19.35 GiB (stale + new artifacts) | 132 |
| L1m | A | mbx, warm local store, same lock change | 88.4 | 11 (3) | 0 / 0 / 11 (2 bypassed) | 6.8 GiB up / 0 | 15.53 GiB | 15.68 GiB | 15.63 | 19.37 | 132 |
| L1mb | B | mbx, local store from M3, remote warmed by L1m | 43.4 | 11 (3) | 29 / 0 / 0, 29 prefetched | 0 / **6.8 GiB down** | 8.72 GiB | 8.88 GiB | 15.61 | 19.38 | 132 |

Notes on the table:
- L1 package: `fastrand` (leaf under `tempfile`), downgrade 2.5.0 -> 2.4.1 (also lists `Cargo.lock` diff in logs). 11 crates recompiled, 3 of them workspace crates (`velnor-runner-host` 59 s, `velnor-runner-cli`, ...), plus `turso_core` 32 s. Plain and mbx both compile the same 11.
- P2 and M2/M4 both end with 0 compiled crates. The plain clone at the same absolute path produced a clean fingerprint match, so `CARGO_LOG` tracing was not needed.
- M2/M4 are no-ops because mbx's managed target dir `/store/mbx/targets/v1/<hash>` is keyed on the workspace path (`/w/velnor-new/...`), which is identical in every job; the new checkout's `target` symlink points straight at the already-built dir. M2b is the case where that dir is gone (pruned, or a different mbx cache dir) but the store is warm.
- 132 files with nlink>1 are cargo's own hardlinks (deps/ to final binaries). mbx adds none (materialization is by reflink: M3 778 outputs 8.7 GiB reflinked, 947 outputs 41.9 MiB copied; M2b the same counts).
- Server (node A, `/data` on xfs): 16.3 GB, 4610 files after all runs; blobs uploaded 4611, pack uploads 74, action commits 911; blob hits 3836, action hits 572, misses 475. The 8.7 GiB (M1) + 6.8 GiB (L1m) = 15.5 GiB uploaded sum matches the server data size: no compression benefit on disk visible.

## Marginal cost per extra checkout (same path, XFS reflink node)

| strategy | extra disk | time | crates compiled |
|---|---|---|---|
| plain + reflink clone of warm target (P2) | 2.5 MB clone + 110 MiB checkout | 0.25 s clone + 0.31 s build | 0 |
| plain + full copy onto ext4 emptyDir (P3, today's runner setup) | 10.75 GiB on the OS disk | 8.6 s copy + 0.23 s build (page cache hot, ext4 NVMe) | 0 |
| mbx, same path, managed target present (M2, M4) | about 0 + checkout | 0.32 s | 0 |
| mbx, store warm, managed target gone (M2b) | +1.94 GiB new data (store 8.8 GiB stays, outputs reflinked) | 7.2 s | 0 compiled (496 hits) |
| plain cold build (P1) / mbx cold (M1) | 10.1 GiB / 19.5 GiB (M1 = managed target 10.7 + store 8.8) | 92 s / 93 s | 357 |

Cross-node remote (M3, B, empty local store, warm server): 39.9 s wall vs 92 s cold; 496 hits, 0 misses; 8.7 GiB downloaded (about 215 MB/s effective; cumulative blob transfer 7m12s across parallel streams); node B ends with 8.8 GiB store + 10.7 GiB target, df +10.7 GiB (outputs reflinked from the store, so the real new data is the store download plus some copies). With L1 (L1mb): 43.4 s, 6.8 GiB down, 29 hits, vs 67-88 s locally.
Cost of the remote: mbx uploads everything it builds, with no sign of dedupe of unchanged crates in L1: L1m uploaded 6.8 GiB for an 11-crate change (large debug test binaries).

## Lockfile change (L1)

| | wall | recompiled | new disk |
|---|---|---|---|
| plain, cloned target | 67.4 s | 11 (3 ws) | +8.7 GiB (target 10.6 -> 19.35 GiB: old artifacts stay, new ones added) |
| mbx, warm local store (A) | 88.4 s | 11 (3 ws) | +15.5 GiB (6.8 GiB store + 8.7 GiB target) |
| mbx on B, remote warm | 43.4 s | 11 (3 ws) | +8.7 GiB, 6.8 GiB downloaded |

mbx looked up none of the changed crates before they were built on A (0 hits, 0 misses, 11 not looked up) and then published them. On B the prefetch manifest delivered them (29 hits). Plain cargo with the cloned target is the fastest on the node that has the warm target; mbx's win is B (cold node): 43 s vs about 92 s cold, and compute is not needed for any crate once any node has built it.

## Page cache (E1)

E1d (one pod, two reflink clones of the 14.3 GB apparent / 10.6 GiB du target, `fincore` per file set):
- before any read: warm snapshot 10.0 GB resident, clone1 0, clone2 0.
- after reading clone1: clone1 14.29 GB resident, clone2 **0**, warm unchanged to slightly less (9.7 GB). Node `Cached` +7.0 GB (109.1 vs 102.1 GB; node cache was already near full, evicting).
- after reading clone2 too: clone2 14.29 GB resident, clone1 still 14.29 GB; warm fell to 4.9 GB (evicted).
So reflinked files on XFS do **not** share page cache: each clone has its own pages when read, and N clones that are read cost N x size of RAM cache (here 14.3 GB each). Disk is shared, cache is not. E1a (single clone read, node Cached 83.9 -> 99.2 GB = +15.3 GB) agrees. Only files actually read matter: a no-op cargo build reads metadata only.

## Caveats

- CI load: the nodes were nearly idle (kubectl top: node A 0-12% CPU, B 0-15%, 48% on A during E1d from my own cat), so builds were not slowed by CI. The load gate (above 75% wait) never triggered, and node A was used for A-scenarios as planned. Earlier (21:35) A showed 92% and B 56%; by 21:39 A was at 12%.
- Wall times come from a 96-core node; 92 s cold is far faster than a typical runner.
- df is a whole-node filesystem; use `du` columns (store, target) for exactness. The clone-alone deltas are exact to a few MB.
- One workload, one commit. mbx's own stats say only about 100-500 compilations are cacheable here (bypassed 49 incl. build-script and cc-queries; 870 progress "not looked up" in M1 is cold).
- M2/M4 results depend on the mbx managed-target directory persisting and the path staying fixed (as in real CI with a fixed work path). M2b shows the other case.
- GC was disabled (`MBX_GC_AUTO=false`), so the mbx store only grows: 8.8 GiB after main, 15.6 GiB after one lock change (each distinct change adds the new workspace-crate outputs).
- Server metadata is in memory; all numbers assume it was never restarted (it was not).
- P3 used a `cp -a` from XFS PVC to ext4 emptyDir in 8.6 s because the source was in page cache; a cold read would be slower.
- Probes: mbx doctor not run; `mbx stats` printed "compiler time avoided 0s" because it counts cumulative cache hits only.

## Cleanup proof

Created (all inside namespace `mbx-cache-exp`, list in `logs/created-objects.txt`): namespace; Secret `mbx-cache-token`; ConfigMap `scripts`; PVCs `pvc-a` (node A), `pvc-b` (node B), `pvc-srv` (node A); Deployment+ReplicaSet+Pod `mbx-cache`, Service `mbx-cache`; pods `probe-a`, `probe-b`; 15 Jobs/pods (P1, P2, P3, M1, M2, M2b, M3, M4, L1p, L1m, L1mb, E1a, E1b, E1c, E1d). Dynamic PVs (local-path, reclaim Delete): `pvc-2274d6b7-...` (srv), `pvc-ccb4319c-...` (a), `pvc-f00207b6-...` (b) (`logs/pv-before-delete.txt`).
Deleted: `kx ns-delete` (namespace "mbx-cache-exp" deleted). Afterwards: `kubectl get ns mbx-cache-exp` NotFound; `kubectl get pv` -> "No resources found" (the three PVs are gone, and there were no other PVs); namespace list is back to arc-runners, arc-runners-chainargos, arc-runners-jackin, arc-systems, default, kube-node-lease, kube-public, kube-system, velnor-system. No local processes were left running (`go.sh` watchers finished). No GitHub writes; no `rex`; mise untouched; the token was generated with `openssl rand -hex 32` into a shell variable, put only into the Secret, and never printed. Scratch dir keeps manifests, logs, `kx-audit.log`.
