# RustFS as the per-site storage tier of a fast CI system: research and measurements

Date: 2026-11-19. All measurements were taken on one Apple M5 Max (18 cores, 128 GiB) Mac, arm64.
That machine was shared with many other agents. The load average was 60-117 during the runs, so every
time below is **indicative only**. Ratios are more trustworthy than absolute numbers.

## 0. Setup (what was actually run)

| Item | Value |
|---|---|
| RustFS binary | GitHub release 1.0.6 (`rustfs-macos-aarch64-v1.0.6.zip`, published 2026-11-15). SHA-256 matched `SHA256SUMS`. The binary reports git commit `de9009d`. |
| Source clone used for reading | `~/.cache/ci-ambition/refs/rustfs` at commit `a3d6b59d03` (Cargo.toml says 1.0.5, so it is slightly ahead of 1.0.5 and slightly behind or near 1.0.6) |
| Bind | `127.0.0.1:19001`, `19002`, `19003` (path-style, HTTP), one server at a time per run |
| Data | A fresh 40 GiB APFS sparse image (`hdiutil create -type SPARSE -fs APFS`), mounted under the experiment dir. It was detached and deleted at the end. |
| Credentials | Generated per run (`rfsexp...` access key, random 40-hex secret). They were never printed, and the file was deleted. |
| Topologies tried | `ec4`: one process, 4 directories, so 4 "drives" and EC:2 (default parity for 4 drives). `sd1`: one drive, parity 0 (no erasure coding). |
| Disk check | RustFS refuses several erasure endpoints that share one physical disk ("local erasure endpoints must use distinct physical disks", `crates/ecstore/src/layout/endpoints.rs:1731`). The test used the documented single-machine escape hatch `RUSTFS_UNSAFE_BYPASS_DISK_CHECK=true` (`docs/operations/cluster-lifecycle-operations.md:51`). The 4 drives are therefore on one image and give no failure isolation. |
| Durability modes | `strict` (the default), `relaxed`, `none`, chosen by `RUSTFS_DURABILITY_MODE` (`docs/operations/durability-modes.md`) |
| S3 client | `aws` CLI 1.46.3 (pip, in a venv) for the 2 GiB object and the R2 tarballs. `boto3` with a 16-thread pool for many-object tests (`scripts/r1.py`). |
| Other tools | zstd (homebrew), bsdtar, `cp -c` (APFS clonefile, the L0 stand-in) |
| mbx-cache | v0.0.1 built from `refs/mr-boxington-cache` (commit `f9adcc6`) with `cargo install --locked` on rustc 1.99.1, 5m40s under load |
| Not tested | Multi-node clusters, any real network (WireGuard, 35 ms RTT), Linux/XFS/NVMe, site replication, ILM, STS. All of those are doc/code reading only. |

Scripts and logs are in `scripts/` and `logs/` next to this file.

## 1. Research facts

### 1.1 Maturity and cadence

- Repo `rustfs/rustfs`: created 2023-11-23. About 36k stars, 88 open issues, 212 contributors (GitHub API, 2026-11-19).
  About 835 commits in the 31 days before 2026-11-19, roughly 27 a day.
- First release was 2025-07-02. There are 127 releases in total: 98 alphas, 15 betas, rc.1 to rc.6, and 7 stable.
  **1.0.0 was published 2026-09-16.** Since then: 1.0.1 (10-03), 1.0.2 (10-16), 1.0.3 (10-17), 1.0.4 (10-25), 1.0.5 (11-10), 1.0.6 (11-15).
  That is 6 patch releases in 60 days, about one every 10 days.
- 1.0.6 notes: MRF (retry-queue) and scanner fixes, backports.
- The operator, `rustfs/operator`, is at 0.0.8 (2026-10-03). It has 148 stars and Apache-2.0 licence, and the last push was 2026-11-12.
  The Helm chart in the main repo (`helm/rustfs`) is at chart/app version 1.0.5.
- Open issues touching data safety or availability (title and state as of 2026-11-19):
  - #9480: 3 nodes x 1 drive at EC:1. A node that was unreachable for a minute comes back with a stale `pool.bin`. If a different drive is then replaced, the set cannot start. The issue itself says no data is lost, but the set ends up down.
  - #8768: bucket replication "existing-object backfill" can stall for good on some buckets while the job shows as active.
  - #8564: ListObjectsV2 returns FileCorrupt during native pool decommission.
  - #9161: 1.0.4 has slower small PUT than beta.8 on the same VM.
  - #9483: bucket-policy wildcard evaluation is very slow for non-matching keys.
- Recently closed examples: #9154 (parent heal could delete a valid child under an inline rollback UUID in 1.0.4), #9474 (DELETE of a non-existent key returned 500 with one drive degraded), #8236 (new buckets defaulted to relaxed durability), #6025 (transitioned SSE object GET returned corrupt bytes).
  I found no open issue with the plain words "data loss" or "corrupt" as an unresolved silent-corruption report in the searches I ran (search is by text, so this is not proof of absence).
- The repo contains a "normative" erasure-coding spec that states: "a regression here can silently corrupt or lose all user data" (`docs/architecture/erasure-coding.md`). Erasure code, quorum, and the on-disk format are labelled high-risk in `AGENTS.md`.
- Postmortem example: `docs/postmortems/2026-09-03-replication-checksum-default-regression.md` shows a 3-day path from an own-lab finding to a release candidate that broke Object Lock targets.

### 1.2 Available vs Preview (README.md:40-77)

- Status legend: "Available - shipped and covered by CI gates"; "Preview - shipped behind an opt-in flag or with a bounded compatibility claim".
- **Preview**: S3 Tables (Iceberg REST) and MinIO on-disk compatibility (behind the `rio-v2` feature, not in the default build).
- **Available** includes: S3 core, versioning, Object Lock, SSE, ILM, ILM tiering to remote S3, IAM and policies, OIDC/SSO, distributed mode, single node mode, bitrot protection, healing and scanner, pool expansion and decommission, **bucket replication, site replication, bucket quota**, event notifications, audit, console, **Helm charts**.
- S3 compatibility claim (`docs/architecture/s3-compatibility-matrix.md:9-11`): "broad S3 API compatibility for supported features. It does not claim complete coverage". The matrix counts are in the s3-tests lists, not in the doc.
  Supported rows: bucket and object CRUD, copy (with 10 checksum algorithms), ListObjectsV2, multipart, tagging, bucket policy, public access block, presigned GET/PUT, range and conditional reads, user metadata, SSE-C and selected SSE-KMS, selected versioning and object-lock and conditional-write behaviour.
  Not yet passing: bucket logging, POST-object form checksum handling, bucket ownership controls, some multipart listing edge cases, IAM-account dependent cases. ACL authorization is intentionally unsupported.
  Deliberate deviations: object keys with `.`, `..` or empty path segments are rejected with 400 (the key is a file path on disk). Keys with C0 control characters are rejected. `PutObject` bodies slower than 64 KiB per 60 s are rejected.
  First write after setting a quota returns 503 SlowDown until a background scan builds a usage ledger.
  These deviations matter for cache keys: a key like `a//b` or `a/../b` will fail.

### 1.3 Erasure coding and topology rules

- Reed-Solomon over GF(2^8) with a Vandermonde matrix, 1 MiB erasure blocks, HighwayHash-256 bitrot checksums. This is the same family as MinIO (`docs/architecture/erasure-coding.md` section 1).
- Geometry (section 2): a set has 2 to 16 drives. Default parity by drive count: 1 drive: 0, 2-3: 1, 4-5: 2, 6-7: 3, 8 or more: 4. Parity is at most N/2.
  Read quorum is K = N - parity. Write quorum is K, or K+1 when K = parity (`docs/operations/cluster-lifecycle-operations.md` section 4.2).
  Example from the doc: 2 drives at EC:1: one drive down still reads but no longer writes.
- Small objects: an object whose shards are each at most 128 KiB (one eighth of that on a versioned bucket) is stored inline in `xl.meta` (section 4.3). Our 16 KiB objects are inline; 1 MiB ones are not.
- Layout is immutable (section 1 of the ops doc). Drive count, set width, and pool order are persisted. A mismatch fails permanently with `PoolTopologyMismatch`.
  Expansion means appending a new **pool**. A pool needs at least 2 drives (ellipsis form). A single-node single-drive (SNSD) deployment can never expand or join a pool: migrate through S3 (README.md:105-119).
- Failure domains: the docs say "a single-node pool loses every shard with its host". At startup RustFS warns `host_failure_data_unavailable` for multi-drive pools on one host. One drive per node is allowed. Admission is not a promise of host-failure tolerance.
- Rolling restarts: "Any maintenance that takes a drive or its node offline is a read and write outage for ordinary objects" only for `EC:0`. With parity at least 1 the set stays available as long as quorum holds (`rolling-restart.md` exists).
- `EC:0` is accepted but means no degraded reads at all and no heal for zero-parity objects.
- Distributed endpoints use an ellipsis syntax, e.g. `rustfs server http://node{1...4}:9000/data{1...4}`. All nodes need the same command line.
- Startup requires distinct physical disks per endpoint, and an unsupported-filesystem policy applies. The README lists XFS as the recommended filesystem and says no NFS/SMB.
- Measured here: with 2 of 4 drives' copy of a bucket deleted (EC:2), a GET of a 1 MiB object and of the 2 GiB object both succeeded and were byte-identical. The 2 GiB degraded GET took 11.07 s (`logs/shard-loss.txt`). This only shows single-process shard reconstruction. It says nothing about node loss over a network.

### 1.4 Replication semantics

- Two different mechanisms (`docs/operations/site-replication-operations.md:1-15`): a **control plane** (buckets, bucket metadata, IAM, topology) and ordinary **bucket replication** that moves object versions and deletes.
- Object replication is **asynchronous and queue/scan driven**. From the doc:
  - control-plane retry drain every 30 s; heavyweight reconciler at startup and every 600 s;
  - object MRF entries persisted every 10 s; target health probed every 5 s; persisted MRF journal replayed every 5 min;
  - objects the event path missed are rediscovered by the scanner, with an upper bound of `RUSTFS_DATA_USAGE_UPDATE_DIR_CYCLES x max(RUSTFS_SCANNER_CYCLE, scanner cycle duration)`. The default re-descent is every 16 cycles, and the doc suggests 4 for a tighter objective.
  - "These are scheduling bounds, not delivery SLAs". `Metrics.retries` is "always zero today".
  - A manual `rc admin replicate resync start` exists. `mc admin replicate resync` does not work against RustFS (the `rc` CLI is needed).
- Requirements: versioning must be Enabled on any bucket that exists on more than one site; a shared unversioned bucket is rejected at pairing. Object Lock enablement must match.
- Conflict handling: I found **no documented multi-writer conflict policy** in the docs I read. The only conflict-like logic I found is a guard that skips existing-object replication when the target already has a newer null version (`crates/replication/src/object.rs:144`). Treat site replication as "one writer per key, or content-addressed keys" until tested.
- Replication to a generic S3 target is supported as bucket replication only; site replication needs RustFS peers.
- Site replication copies KMS key names, not keys, so every site needs the same key names.
- Known gaps: #8768 above (backfill stall) and the endpoint-change procedure is manual and multi-step.

### 1.5 Lifecycle, quota, IAM

- ILM (lifecycle) is "Available": expiry and transitions, with a scanner-driven evaluator (`crates/lifecycle`). Standard S3 lifecycle is **day-granular**. A debug env `RUSTFS_ILM_DEBUG_DAY_SECS` shortens a "day" for tests only. Expiry therefore happens on the scanner schedule (default max cycle duration 30 minutes, `DEFAULT_SCANNER_CYCLE_MAX_DURATION_SECS`), not instantly. mbx-cache itself relies on the store's lifecycle rule to bound blob storage (its README, "Bounding storage").
  I did not run ILM here.
- Quota: bucket hard quota exists (durable reservation protocol v2 in `docs/operations/durable-quota-reservation-v2.md`). The first write after setting or changing a quota returns 503 SlowDown until a ledger scan completes (compat matrix).
- IAM/STS: `AssumeRoleWithWebIdentity` is implemented (`rustfs/src/admin/handlers/sts.rs`). `DurationSeconds` is clamped to 900-43200 s, default 3600 (lines 64-80). A session policy is applied (`populate_session_policy`). The OIDC doc has a section "STS workload providers" for workload tokens such as Kubernetes service-account tokens. It needs a discovery document with `issuer`, `jwks_uri`, and `id_token_signing_alg_values_supported`. Signature, issuer, audience, and expiry are verified, and the policy is mapped from a claim or from `RUSTFS_IDENTITY_OPENID_ROLE_POLICY`.
  So scoped short-lived credentials per job are **possible by design**. I did not test it, and I did not test GitHub's OIDC issuer.
- No licence key was needed to run the release binary. There is a `--license` flag and a `rustfs-license` crate with a "server entitlement" gate, and a separate "RustFS Connect" integration. The default binary started and served without either.

### 1.6 Helm and operator

- Helm chart `helm/rustfs` (version 1.0.5): standalone mode (one pod, one PVC) or distributed mode (default, StatefulSet). Topology = `replicaCount` x `drivesPerNode` (default `replicaCount: 4`). Changing `drivesPerNode` after install means deleting the StatefulSet (volumeClaimTemplates). There is a bring-your-own-drives mode (JBOD via `extraVolumes`) and optional pools.
  The chart ships templates for PDB, ingress, Gateway API, Contour, and cert-manager mTLS (`helm/README.md`, `helm/rustfs/templates`).
- Operator (`rustfs/operator`) is a separate repo at 0.0.8, so pre-1.0.

### 1.7 Small-object vs large-object claims

- README claims "high performance" and shows a video vs MinIO on 2 vCPU, 4 GB, 4 x 40 GB drives. It gives no numbers in text. I found no written small-object throughput claim in README.
- The repo has tuning and benchmark runbooks (`docs/operations/object-io-tuning-ab-matrix.md`, `hotpath-warp-ab-runbook.md`, `scanner-benchmark-runbook.md`). Issue #9161 is a user report that small PUT got slower in 1.0.4 than in beta.8 (MinIO 14.75 ms vs RustFS higher for 128 KiB PUT in the issue's table).
- Documented fixed cost for small objects: cross-disk `xl.meta` read plus quorum ("metadata_fanout" in the tuning matrix). Our measurements below agree: the cost is per object, not per byte.

### 1.8 Licence

- `LICENSE` is the standard Apache License 2.0 text (201 lines), with the appendix "Copyright 2024-2026 RustFS, Inc."
- `Cargo.toml:78`: `license = "Apache-2.0"`. Source files carry an Apache-2.0 header.
- The GitHub API now returns `license.spdx_id = "Apache-2.0"` for `rustfs/rustfs` and for `rustfs/operator` (checked 2026-11-19), so the missing-SPDX note in the brief no longer holds.
- **Confirmed: Apache-2.0.**

## 2. Measurements

### R1. Throughput (single node, local loopback)

Method: `aws s3 cp` for the 2 GiB object (default multipart settings). `boto3` threaded (16 threads) for many-object sets. Data are random bytes (incompressible). GETs read data written moments earlier, so some come from the OS page cache.
Sizes: 2048 MiB, 1000 x 1 MiB (1000 MiB), 20000 x 16 KiB (312.5 MiB).
Baseline (L0 proxy): plain `cp -R` and `cp -c -R` (APFS clone) of the same data on the host disk (not the image), and `cat` of everything.

PUT (higher is better; MiB/s, and objects/s for the many-object sets):

| Config | 2 GiB PUT | 1000 x 1 MiB PUT | 20000 x 16 KiB PUT |
|---|---|---|---|
| EC 4 drives, `strict` (default) | 43.9 MiB/s (46.7 s) | 8.3 MiB/s = 8 obj/s (121 s) | aborted after ~4 min at about 9 obj/s (3402 to 3667 objects in 30 s) |
| EC 4 drives, `relaxed` | 101.5 MiB/s (20.2 s) | 26.7 MiB/s = 27 obj/s (37 s) | 166 obj/s = 2.6 MiB/s (120 s) |
| EC 4 drives, `none` | 288 MiB/s (7.1 s) | 69.9 MiB/s = 70 obj/s (14.3 s) | 121 obj/s (166 s), see note |
| 1 drive (parity 0), `relaxed` | 202 MiB/s (10.1 s) | 70.0 MiB/s = 70 obj/s (14.3 s) | 385 obj/s = 6.0 MiB/s (52 s) |
| Baseline `cp -R` | 1.4 s (about 1460 MiB/s) | 1.58 s (about 630 MiB/s) | 40.3 s = 496 files/s |
| Baseline `cp -c -R` (clone) | 0.07 s | 0.36 s | 8.66 s = 2310 files/s |

GET:

| Config | 2 GiB GET | 1000 x 1 MiB GET | 20000 x 16 KiB GET |
|---|---|---|---|
| EC 4 drives, `strict` | 206 MiB/s (9.9 s) | 493 MiB/s = 493 obj/s | not run |
| EC 4 drives, `relaxed` | 232 MiB/s (8.8 s) | 594 MiB/s = 594 obj/s | 688 obj/s = 10.8 MiB/s (29 s) |
| EC 4 drives, `none` | 420 MiB/s (4.9 s) | 507 MiB/s = 507 obj/s | 196 obj/s, **invalid**: a concurrent `rm -rf` of 20k dirs on the same image ran at the same time |
| 1 drive, `relaxed` | 313 MiB/s (6.6 s) | 516 MiB/s = 516 obj/s | 718 obj/s = 11.2 MiB/s (28 s) |
| Baseline `cat` of all files | 0.8 s | 1.29 s | 3.5 s = 5700 files/s |

What limits these numbers (explain the number):

- **PUT is write-sync bound on this host.** An isolated microbenchmark on the idle image (`logs/fsync-bench-image-idle.txt`, 200 x 16 KiB files) gave `fsync` 0.54 ms per file but `F_FULLFSYNC` 14.5 ms per file. On the plain host disk, `fsync` was 1.2 ms and `F_FULLFSYNC` 12.5 ms. Rust's `File::sync_data` and `sync_all` use `F_FULLFSYNC` on macOS (from my knowledge of the Rust standard library; I did not verify it in the RustFS binary). RustFS in `strict` mode syncs shards, `xl.meta` and the directory entries (`docs/operations/durability-modes.md` table). That fits the strict result of 8 objects/s and 46 s for 2 GiB.
  Moving from `strict` to `relaxed` to `none` raised the 2 GiB PUT 2.3x then 6.6x, which is the same direction.
  A busy-run reading taken while RustFS wrote to the image (`logs/fsync-bench.txt`) shows 115-167 ms per file, i.e. the image was contended by the PUT workload itself.
- **Linux/XFS/NVMe will differ.** There `fdatasync` is cheap on NVMe (the Hetzner cluster has not been measured). Do not use the strict-mode macOS numbers as a prediction. The `relaxed`/`none` rows and the single-drive row are closer to a CPU, loopback, and metadata ceiling than to a disk-sync ceiling.
- The single-drive row beats EC 4 on small objects (385 vs 166 obj/s) because EC writes shards and `xl.meta` to 4 directories on one disk instead of one. So here EC costs about 2x on small PUTs. On real separate NVMe drives and nodes, parallel shard writes may change that. Not measured.
- The `none` 16 KiB PUT (121 obj/s) is slower than `relaxed` (166 obj/s), which cannot be a real ordering. It is load noise (load average 87 vs 89, and other agents' jobs). So I cannot rank `relaxed` against `none` for small objects.
- The 20k-file `cp -R` baseline (40 s) also reflects the load: clones took 8.7 s for the same tree.
- 16 client threads were used for many-object tests. I did not sweep concurrency.

Take-aways from R1: RustFS handles large objects at a few hundred MiB/s even on this loaded Mac. Per-object cost is high: about 27 to 70 obj/s for 1 MiB and about 120 to 385 obj/s for 16 KiB PUTs in relaxed-ish modes, against about 500 files/s for a plain `cp -R` and 2300 files/s for APFS clone on the same data. GETs were 500-700 obj/s.

### R2. Cache-shaped workload (cargo registry)

Input: `cargo fetch --locked` for `ci-helper` and `generator` (extracted from `velnor-new` HEAD with `git archive`) into a scratch `CARGO_HOME`, rustc/cargo 1.99.1. Result: 131 crates, 6,053 files, 190 MiB (194,520 KiB) under `registry/` (cache `.crate` files, extracted sources, and the index).
Server: EC 4 drives, `relaxed`, the same as R1. Times are single runs for compression and 3 runs (listed) for the rest.

Size:

| Form | Bytes |
|---|---|
| tar (uncompressed, bsdtar) | 205,304,320 |
| tar + zstd -1 | 46,970,589 (12.6 s) |
| tar + zstd -3 | 44,475,030 (12.9 s) |
| tar + zstd -9 | 40,612,459 (14.8 s) |
| tar alone, to a file | 17.4 s (so the 12-14 s above is bound by reading 6k small files, not by zstd) |

Save and restore, zstd -3 tarball (44.5 MB):

| Step | Times (3 runs) | Notes |
|---|---|---|
| Save, streamed `tar | zstd -3 | aws s3 cp -` | 17.3, 20.7, 13.8 s | compress + PUT in one pipe |
| PUT of the finished tarball only | 3.30, 3.35, 2.26 s | 13-20 MB/s over loopback with the aws CLI |
| GET of the tarball only | 1.28, 1.26, 1.06 s | 35-42 MB/s |
| `zstd -d` only | 0.35, 0.35, 0.28 s | |
| Extract (`zstd -d | tar -x`) from a local file | 17.7, 17.1, 16.3 s | this is the dominant restore cost |
| Restore, streamed `aws s3 cp - | zstd -d | tar -x` | 17.0, 25.0, 26.0 s | |
| L0 restore: `cp -c -R` of the extracted tree | 3.66 (first, in an interrupted run), 4.67, 5.22, 4.0 s | APFS clone, about 6k clonefile calls |
| L0 restore: plain `cp -R` | 10.7, 12.6, 15.9 s | |

Reading: the S3 part of a restore (about 1.2 s GET) is small. About 17 s of the roughly 18 s is untarring 6,053 small files, which was 3.5-4x slower than an APFS reflink copy (about 4.7 s median) and similar to a plain copy.
So moving the registry through RustFS as one zstd tarball costs about 18 s restore versus about 5 s for a local reflink on this host. Both are inflated by load.
The result suggests that the object store is not the bottleneck for this shape of data. File creation is.

### R3. Content-defined chunking value (optional)

Method (`scripts/r3.py`): version A = the registry above. Version B = a fresh `cargo fetch` after downgrading one crate in `ci-helper/Cargo.lock` (`rustls-webpki` 0.103.17 to 0.103.16, `cargo update --precise`). I checked with `diff` that only that crate directory differed.
Both registries were written into deterministic tars (sorted names, mtime 0, uid/gid 0) so that identical files give identical bytes.
Three estimates were made:
(a) `zstd -3 --patch-from=A.tar --long=27` of B;
(b) a gear-hash, FastCDC-style normalised chunker run in pure Python on the uncompressed tar, counting chunks whose BLAKE2 hash is not in A, then zstd-3 of the concatenation of the new chunks (min/avg/max shown);
(c) file-level content hashing.

| Method | Bytes B would need to upload | Notes |
|---|---|---|
| Whole `tar.zst` of B | 44,093,514 | zstd -3, A was 44,077,234 |
| zstd `--patch-from=A` (long window 27) | 146,708 | needs A at both ends; 0.5 s |
| CDC 16/64/256 KiB | 388,537 (933,767 raw) | 9 of 1,655 chunks new |
| CDC 64/256/1024 KiB | 438,439 (1,127,537 raw) | 4 of 419 chunks new |
| File-level hashing | 232,275 raw bytes in 11 files (of 6,052) | before compression |

So for a one-crate change a chunked or delta store would move about 0.3-1% of the bytes (about 100x fewer for CDC, about 300x for patch-from), before the cost of manifests and many small PUTs.
Caveats: the changed crate is small (about 230 KB raw), so a big crate would change the ratio. The chunker was Python at about 2 MB/s, so the 165-180 s timings are not representative of a Rust FastCDC. Chunking compressed tarballs does not work (a one-byte change shifts the whole stream), so a chunked store must chunk the uncompressed tar or the files. This was one pair of versions on one tree.

### R4. mbx-cache on S3 backed by RustFS

- Build: `cargo install --locked --root .../rustfs-exp --path refs/mr-boxington-cache`, with `CARGO_TARGET_DIR` in scratch, rustc 1.99.1: **succeeded**, release profile in 5m40s under load. Binary `mbx-cache` v0.0.1.
- Server: `MBX_CACHE_STORAGE=s3 MBX_CACHE_S3_BUCKET=mbxcache MBX_CACHE_S3_ENDPOINT=http://127.0.0.1:19001 MBX_CACHE_S3_PATH_STYLE=true MBX_CACHE_S3_REGION=us-east-1 MBX_CACHE_ALLOW_ANONYMOUS=true MBX_CACHE_DATABASE_URL=memory:// MBX_CACHE_LISTEN=127.0.0.1:19080`, with RustFS credentials from the standard AWS env vars. It started, logged `mbx-cache listening`, and `GET /v1/status` returned `{"protocol":1,"status":"ok"}`. No errors in its log.
- Client round trip with the real `mbx 1.29.2` (executed only, with isolated HOME, XDG dirs, CARGO_HOME and `MBX_CACHE_DIR`; `MBX_REMOTE_URL=http://127.0.0.1:19080`, namespace `exp/hello`, read-write):
  - `mbx doctor`: remote "http://127.0.0.1:19080 (exp/hello) is compatible", 0 failures, 3 warnings (atime, no cargo shim).
  - Build 1 of a tiny crate, empty local cache: the remote stored it. "0 hits, 0 misses ... 463.7 KiB uploaded", "uploads: 8 published (6 of them in 1 packs)". Wall 0.68 s.
  - The RustFS bucket then held 7 blobs listed under `v1/blobs/blake3/<hash>/<size>` (131 B to 472,440 B; `logs/r4-bucket-listing.txt`).
  - Build 2 of an identical crate in a different checkout with a new empty local cache: "1 hits, 0 misses, 1 prefetched; 463.7 KiB downloaded", "compiler time: 151.1ms estimated avoided". Wall 0.32 s.
- **Result: works.** It is a tiny test: one crate, one namespace, anonymous mode, in-memory metadata (production uses PostgreSQL). It only shows S3 path-style compatibility with the AWS Rust SDK for put/get/head (and whatever else mbx-cache calls). It does not show behaviour under load, lifecycle expiry, or multi-site operation. Note that mbx-cache's metadata is separate from the blobs (PostgreSQL), so blob-store replication alone does not give you a replicated cache.

## 3. Caveats

- One machine, one loopback, no network, no real disks. The "EC 4 drives" are 4 directories on one APFS image, so there is no failure isolation, and RustFS refused to start without the unsafe bypass flag.
- macOS `F_FULLFSYNC` dominates strict-mode numbers. Linux XFS NVMe numbers are unknown.
- Load average 60-117 on 18 cores. Several rows overlapped with other agents' work (and one of mine, noted above). No row was repeated enough for confidence intervals, except the R2 sub-steps (3 runs).
- GET numbers include OS page cache hits (data had just been written).
- The 16 KiB GET and the `none` rows are noisy. Do not rank `relaxed` vs `none`.
- Client matters: `aws` CLI (Python) for large objects and `boto3` with 16 threads for many objects. A native Rust client (velnor's) could be faster, and the aws CLI is probably the limit for the 2 GiB PUT in the faster modes. I did not check.
- The registry used in R2/R3 is only 131 crates (190 MiB), not a full workstation registry, and the `target/` snapshot (8.7 GiB, 357 crates) was not tested here.
- R3's chunker is a toy and its timing is meaningless.
- Site replication, ILM expiry, quota, STS/OIDC, Helm and the operator were read, not run. Statements about them are only as good as the docs and code I read.
- The source clone (about 1.0.5+) and the binary (1.0.6) differ slightly.

## Implications for Velnor

(Opinion drawn from the facts above. These are not measurements.)

1. **Topology: one RustFS cluster per site, with site/bucket replication between sites, not one cluster across DE and FI.**
   Every write needs a quorum of drives inside one erasure set, and the docs describe no WAN-aware placement. A cluster across 35 ms RTT would put that RTT in the path of each PUT/GET.
   Replication in RustFS is asynchronous (seconds to minutes, scanner-bound for missed objects), which fits the L2 role: a warm copy, never a correctness path.
2. **Sizing per site.** With N workers per site, the doc-backed options are: 3 drives (EC:1, K=2) tolerates one node loss for reads and writes; 2 drives gives read-only after one loss; 6 drives (2 per node on 3 nodes, default EC:3) tolerates a node loss with more headroom. Set parity explicitly and keep it identical on all nodes.
   The layout cannot be changed later, so decide drives per node up front and grow only by adding pools. SNSD cannot grow, so it is a dead end for production.
   Issue #9480 (3 nodes x 1 drive, EC:1) describes exactly the small-set case, so test node-restart and drive-replacement drills before relying on it.
3. **What belongs in RustFS (per-site shared tier):**
   - `actions/cache` entries stored as **one zstd tarball per key** (about 45 MB for a 190 MiB registry; GET about 1 s locally). The S3 part of a restore was cheap in R2.
   - Artifacts v4 and promoted release artifacts (large, few objects, long-lived: the case where RustFS did best).
   - mbx-cache blobs (works as S3 backend in R4, and the store's lifecycle bounds storage).
   - The shipped warm `target/` snapshots (8.7 GiB): large-object path, multipart. Plausible, but I did not measure it at that size.
4. **What should stay node-local (L0):** extracted trees and anything restored per job. Extracting a tarball of 6k files took about 17 s here against about 5 s for a reflink clone. The S3 transfer was about 1 s, so the tier cannot fix the untar cost. Use L0 reflink clones for hot trees and make L1 only a refill source.
   Do not store file trees as many small objects: 16 KiB PUTs ran at 120-385 obj/s in the best case here, and 1 MiB PUTs at 27-70 obj/s, versus about 500 files/s for a plain local copy. A cache entry made of thousands of objects will be slower than one tarball.
5. **Durability setting.** `strict` (default) is the safe default; the macOS result shows how much fsync policy changes write cost, but the Linux cost is unknown. A cache can lose entries, so `relaxed` or `file-sync` may be acceptable for cache buckets, but not for promoted release artifacts. The setting can be chosen per bucket (`RUSTFS_NEW_BUCKET_DURABILITY_MODE`, admin API per bucket). Measure on the Hetzner XFS NVMe nodes before deciding.
6. **Delta/CDC.** For a one-crate change a chunked store moved about 1% of the bytes, and `--patch-from` about 0.3%. That is a real saving for cross-site (WireGuard) transfer of registries and snapshots. It needs a chunk manifest layer in Velnor, not something RustFS provides. Worth prototyping in Rust for registry caches, and it needs chunking of uncompressed data.
7. **Expiry and capacity.** GitHub's cache semantics (7-day unused eviction, size caps per repo) are not the same as S3 lifecycle. ILM is day-granular and scanner-driven, quota admission has a warm-up 503, and neither knows "least recently used". Keep the index and LRU policy in Velnor and use ILM only as a backstop, as mbx-cache does.
8. **Credentials.** `AssumeRoleWithWebIdentity` with a session policy and 15 minute minimum lifetime fits "per-job scoped credentials" in principle, if the issuer's JWKS is reachable (a private or loopback issuer needs `RUSTFS_OUTBOUND_ALLOW_ORIGINS`). This was not tested, and the GitHub OIDC claims mapping for policies needs a trial. A fallback is for the runner to hold the credentials and pass only presigned URLs to jobs (presigned GET/PUT are supported).
9. **Key naming.** Keys with `.`/`..`/empty path segments or control characters are rejected. Cache keys from user input (`actions/cache` key strings) must be hashed or encoded before use as object names.
10. **Risks to weigh:**
    - Youth: 1.0.0 is 2 months old, 6 patch releases since, 98 alphas before it, and the repo moves at about 27 commits a day. Pin a version and budget for upgrades every few weeks.
    - Open issues on node return after outage (#9480), replication backfill stalls (#8768), and small-PUT regressions (#9161) are in exactly the places Velnor would lean on (HA, L2, small objects).
    - Operator is 0.0.8 and the chart is a plain StatefulSet. Plan to own the manifests.
    - `mc admin replicate` does not work against it; `rc` is needed for resync.
    - The bypass flag shows RustFS is deliberate about shared disks. On the real cluster each drive must be a separate device, which matters if the L0 NVMe also holds RustFS data (one XFS filesystem shared between L0 and L1 would fail the distinct-disk check unless separate devices or partitions are used).
    - Replication conflict semantics for two writers are undocumented. Keep keys single-writer (content-addressed or per-site prefixes).
11. **Bottom line from the data collected:** RustFS is a plausible L1 for large, immutable, per-key blobs (artifacts, cache tarballs, mbx blobs, target snapshots). It is not a replacement for L0 on the restore path, and it is a poor fit for storing trees as many tiny objects. The numbers that decide the design (Linux fsync cost, real 5-node EC behaviour, WireGuard replication throughput, `target/` snapshot size at 8.7 GiB) were not measured here and need a run on the actual cluster.

## Cleanup performed

- Stopped by PID: RustFS servers on 19000 (ec4), 19001 (ec4r), 19002 (sd1r), 19003 (ec4n), mbx-cache on 19080, and the r1 workers. No other processes were touched.
- Detached the APFS image (`hdiutil detach`, "disk4 ejected") and deleted `rfs.sparseimage` (24 GB allocated, 40 GiB virtual) and its mount dir.
- Deleted: `scratch/` (4.2 GB: test data, both cargo homes, tarballs, mbx workdirs), `dl/` (332 MB), `cargo-target/` (1.1 GB), `venv/` (203 MB), the `bin/rustfs` binary (250 MB), and the generated `creds.env`.
- Kept: this file, `logs/`, `scripts/`, and `bin/mbx-cache` (small). The directory is now about 21 MB.
- Nothing was written to GitHub, no cluster was touched, `rex` was not run, and nothing under `~/.local/share/mise` was modified (mbx 1.29.2 was executed only).
