# Fastest-round conflicts and landscape ideas: rulings

Date: 2026-11-19. This document rules on the conflicts the fastest round left open (C02, C03, C04, C12, C13 and X01 of `decisions.md`) and on every idea in the three landscape analyses (`landscape/opus-5.5.md`, `landscape/gpt-6.2-astra.md`, `landscape/gemini-4-neon.md`), merged by technique as L01–L20. It rules only on those; it does not redesign.

**Method.** Votes were not counted. Each ruling rests on evidence checked for this document: the official runner at tag v2.338.2, the toolkit and the pinned `actions/cache` bundle, kernel, Kubernetes and Git documentation fetched today, read-only measurements on velnor-new at `5ae93781e0`, and one reproduction (`~/.cache/ci-ambition/adjudicate2/`, local to the adjudication machine and described here fully enough to repeat). Confidence is *settled* (documented or reproduced), *likely* (the evidence points one way and one link is unobserved) or *needs a lab*. Section numbers are v3's. The adjudicator runs on the same model as one panel member (Fable 5.2); C12 and C13 do not adopt that member's figures. Earlier rulings (Q5, Q12, Q16, Q17, Q19) stand; where a ruling here touches one, it says how.

## Summary

| Id | Ruling | Confidence | Credited |
|---|---|---|---|
| C02 | Buildkite-style agents: predicted tie on compile and tests, a lead only through reuse and the missing Actions hop. No claim; S3's persistent-agent arm is the stand-in and says what it cannot show. | likely | Fable; Astra, Sol, Opus, Muse Wonder Full |
| C03 | Rented fast runners: predicted behind per thread and ahead on push-to-merge. No claim until owner question D22's arm runs, configured as the vendor recommends. | likely (direction) | Fable, Neon; Muse Wonder Full |
| C04 | Remote execution: expected to win cold and wide builds if the crate graph allows it. No claim on that class; one `cargo build --timings` run decides whether the optional arm is worth building. | needs a lab | Opus, Neon; Astra, Sol |
| C12 | The quoted floors measure three different things. A pool hit costs the same with pods or slots; the choice turns on the miss. Budget stays ≤ 1 s P90; the Docker class gets its own. | needs a lab | Carbon, Argon; Astra, Muse Wonder |
| C13 | The check is bound by file count and cold reads, not by SHA-1 speed. The full hash stays; the estimate is restated per file and measured on a fresh clone. | likely; needs a lab | Astra, Carbon, Neon |
| X01a | No for verdict keys. A trace cannot show that an execution has no clock or randomness input, and the paths read vary between runs on one tree (reproduced). Q5 and "the trace proposes and revokes" stand. | settled | Astra, Sol |
| X01b | Yes for compile units only, as a choice of what to rebuild, never as proof: step 3's root-cold rule may narrow behind a lab, with the cold digest comparison as the proof. | needs a lab | Neon; Sol |
| L01 | Pool sized by a floor plus the plan's own forecast: adopt. Idle members hold small requests and are resized at claim. | likely | Opus; Astra, Neon |
| L02 | Per-job fork of a sealed generation: already the design. Adopt a size-triggered cold reset. "Stale rather than a miss" only for generations. | settled | Opus; Neon, Astra |
| L03 | Placement by the state a node holds: adopt, as a soft preference. | likely | Opus, Astra; Neon |
| L04 | Batched presence query, present-before-commit, output digests, subtree ids: adopt. A client-written action cache: reject. | likely | Astra, Opus; Neon |
| L05 | Generation shipping as a manifest of large chunks: a lab B3 candidate; nothing on the verdict path. | needs a lab | Opus |
| L06 | Whole-sandbox snapshot restore: adopt the contract text only; build nothing. | settled | Astra; Neon, Opus |
| L07 | Shared page cache through a read-only lower layer: lab B2, as ruled in Q16, with two measurements added. It does not license a delta-only hash. | needs a lab | Neon; Opus |
| L08 | Pre-filled action-archive cache for GitHub Official: adopt now, in both roles. | settled | Opus |
| L09 | Cache v2 redirection for GitHub Official: feasible as read from the source; adopt behind ST05. The upload side is the Azure Blob protocol. | likely | Opus, Astra; Neon |
| L10 | Co-located caches: already the design. Adopt parallel ranged L1 reads and batched resolves. | likely | Opus, Astra |
| L11 | Peer-to-peer blob tier and asynchronous artifact upload: reject. | likely | (against Neon) |
| L12 | High-clock nodes: owner question D20 stands, restated: one node per site cannot shorten a green. | needs a lab | Astra; Opus, Neon |
| L13 | Test splits by recorded timings: adopt behind lab C. | needs a lab | Opus, Astra, Neon |
| L14 | Flake scoring from witness pairs: adopt. Quarantine: only with an equivalent blocking check; the owner-exception form waits on D17. | settled | Astra; Opus, Neon |
| L15 | Narrowing only by enrolled contract; prediction only to order the fast-red lane: already the design. A1 must not pick its work by a narrowed key. | settled | Astra; Opus, Neon |
| L16 | Queue: prefix reuse and speculation are the design. Bisection: reject, the first failing prefix is the culprit. Merge train: D18 stands. | settled | Opus; Astra |
| L17 | Optimistic merge: reject. A1 is the only asynchronous half. | settled | all three |
| L18 | Mirror concurrency: one writer, merged fetches, atomic ref updates, no pruning in place: adopt. | likely | Opus; Astra, Neon |
| L19 | Before the label: fetch only. A compile is owner question D19, default no. | settled | Astra; Opus |
| L20 | Persistent image builders with a layer cache per scope: adopt behind a count of image-key changes. | needs a lab | Opus, Neon |

## Evidence gathered for this document

- **W1. Runner source at the tag.** `git -C refs/actions-runner show v2.338.2:<path>` (tag `d1eb12d`). `NodeScriptActionHandler.cs:55-79` and `ActionManager.cs` do not differ between the tag and the panel's clone `7afce9e`; `Constants.cs` differs by 36 lines. `ActionManager.cs:1232` reads `ACTIONS_RUNNER_ACTION_ARCHIVE_CACHE`; the name is defined at `Constants.cs:327` (Opus's `:293` is the unreleased file), and `ACTIONS_RUNNER_SYMLINK_CACHED_ACTIONS` at `:328`.
- **W2. Who reads the Results URL.** `ResultsServiceUrl` in the system connection's data is read in two places only, `NodeScriptActionHandler.cs:71-74` and `ContainerActionHandler.cs:237-239`, both to export `ACTIONS_RESULTS_URL` to an action. The Worker's own log and summary uploads use the job variable `system.github.results_endpoint` (`JobServerQueue.cs:121-138`, `FileCommandManager.cs:278`). `ACTIONS_CACHE_SERVICE_V2` is set from the variable `actions_uses_cache_service_v2` (`:76-79`). The Worker takes its job message as JSON from the listener's pipe (`Worker.cs:54-72`).
- **W3. Toolkit and the pinned action.** `actions/toolkit` `main` at `0c3a9c3` (2026-10-21), `packages/cache/src`: v2 uses `ACTIONS_RESULTS_URL` (`internal/config.ts:42-58`); a save calls `CreateCacheEntry`, then `uploadCacheArchiveSDK`, then `FinalizeCacheEntryUpload` (`cache.ts:702-744`); the upload is `new BlobClient(signedUploadURL)`, `getBlockBlobClient()`, `uploadFile` with a 128 MiB single-shot limit (`internal/uploadUtils.ts:133-159`), from `@azure/storage-blob`; a restore uses the Azure SDK only when the host ends in `.blob.core.windows.net` and otherwise one plain GET (`internal/cacheHttpClient.ts:152-182`). velnor-new pins `actions/cache@55cc8345` (v6.1.0, 23 uses in `.github/`); its bundle has the same code (`dist/save/index.js:92328-92345`, `dist/restore/index.js:93034-93038`).
- **W4. GitHub's own images pre-fill the archive cache.** `actions/runner-images`, `images/ubuntu/scripts/build/install-actions-cache.sh:12-16`: "Prepare directory and env variable for ACTIONS_RUNNER_ACTION_ARCHIVE_CACHE", `/opt/actionarchivecache`.
- **W5. Reproduction for X01** (`adjudicate2/x01/t.rs`, `run.sh`; rustc 1.99.1, no dependencies). A test of the shape "check one representative fixture": four fixture names fixed in code are put in a `HashSet`, the first one the set yields is read and must equal `ok`. It lists no directory and reads no clock, network or `rand`. Tree T has four good fixtures; T2 breaks `a.txt`. On T, 40 of 40 runs pass and read `a`, `b`, `c` or `d` 12, 10, 10 and 8 times. The trace of run 1 is `{d.txt}`; `d.txt` is identical in T2, so a trace-keyed lookup returns the old pass. On T2, 12 of 40 fresh runs fail. std seeds every `HashMap` from the operating system (`library/std/src/hash/random.rs:55-78`, "giving every corresponding HashMap a different iteration order").
- **W6. Repository counts** (`git`, `main` at `5ae93781e0`). 80 of 4,179 `.rs` files name `HashMap` or `HashSet`, 18 of them among 1,115 files under `tests/`; 54 files spawn threads or tasks. Of 213 first-parent commits since 2026-11-01, **137 (64%) add at least one `.rs` file**, 73 under `generator/` or `ci-helper/`.
- **W7. Tree-hash proxy** (`adjudicate2/c13/treehash.py`; this Mac, APFS, warm cache). Today's checkout (`/*` without `/docs/**`) is 5,164 files and 37.3 MiB, median file 4.8 KiB; with `docs/` it is 6,916 files and 93.5 MiB. Opening, reading and SHA-1 hashing every blob and comparing it with the index id: 1.5–2.0 s on one thread, 0.43 s on eight; `git hash-object --stdin-paths` takes 1.5 s. Hashing 37.3 MiB at 1 GB/s is 0.04 s, so over 90% of the time is per-file cost. Absolute figures do not transfer to Linux and XFS.
- **W8. Clones have no resident pages.** `experiments/mbx-cache-cluster.md:77-80` (E1d): "after reading clone1: clone1 14.29 GB resident, clone2 **0**".
- **W9. Kernel and Docker documentation.** fs-verity: "Currently, it is supported by the ext4, f2fs, and btrfs filesystems" (https://docs.kernel.org/filesystems/fsverity.html); the work volumes are XFS. Overlayfs: "creating a hard-link also requires copy_up" (https://docs.kernel.org/filesystems/overlayfs.html). "OverlayFS supports page cache sharing. Multiple containers accessing the same file share a single page cache entry" (https://docs.docker.com/engine/storage/drivers/overlayfs-driver/).
- **W10. Kubernetes.** In-place pod resize: "Stable since Kubernetes v1.35" (https://kubernetes.io/docs/tasks/configure-pod-container/resize-container-resources/). The cluster runs k3s v1.37.2 on kernel 6.12.94 (`evidence.md:455-456`).
- **W11. Git** (2.54). `git fetch` has `--atomic` ("use atomic transaction to update references") and `--no-auto-maintenance`. git-clone(1) on `--reference`: objects made unreferenced in the source "may be removed by normal Git operations ... which automatically call git maintenance run --auto ... the cloned repository will become corrupt".
- **W12. RustFS small objects.** `experiments/rustfs.md:130-135` (one loaded Mac): 1,000 objects of 1 MiB are written at 8 a second under `strict`, 27 under `relaxed`; one 2 GiB object at 43.9 MiB/s under `strict`.

## The open conflicts

### C02. Buildkite-style persistent agents

- **Ruling.** No claim names them, as §11 already says. The prediction the evidence supports is Fable's: a tie on compile and tests, a lead only through reuse and through having no Actions job on the verdict path. S3's "strong conventional baseline" (plan BM07) is the stand-in, and §11 must say what it cannot show: it keeps the Actions job hop, so it bounds a Buildkite-style fleet from above on stages 3–9 only. "Buildkite with the same ledger" (Carbon, Neon) is Velnor's verification system with another agent; S3's "Velnor with Official" arm measures exactly that, and §1.1 already assigns the speed to the verification system. Neon's 45–60 s adds a merge train and high-clock nodes, which are owner questions D18 and D20, not properties of an agent.
- **Evidence.** §3 stages 7–8 are plain cargo over the same warm target on the same CPUs in both systems; §5.3: "Native saves seconds per Actions job ... not on PR or group verdicts". No arm has run.
- **Confidence.** Likely for the direction; the size needs S3.

### C03. Rented fast runners (Namespace, Blacksmith, Depot)

- **Ruling.** No claim until the arm of owner question D22 runs; default no arm, and the claim names only arms run. The supported prediction: behind per thread (D20's ratio, measured by BM15 without renting a vendor), ahead on push-to-merge, because a rented runner still starts from an Actions job offer and repeats the full run for the PR, the group and the push unless the workflow changes. If D22 is yes, the arm runs with the vendor's recommended caches on and phase 0 applied (auto-merge, ALLGREEN, no `edited` trigger, no `-j2` caps); otherwise it is another handicapped baseline (C01).
- **Evidence.** Ruling V5 (job created to started, p50 13 s, p90 27 s on the scale set); §A T7 (three full runs per merge); dossier §2 (Ryzen 7950X, vendor figures only).
- **Confidence.** Likely for the direction; no measurement of either term on this workload.

### C04. Bazel and Buck2 remote execution

- **Ruling.** The critics agree on the direction and nobody has a number: action-granular remote execution should win cold and wide builds, warm local cargo the small edit. So the published sentence excludes the cold and wide class against remote execution unless BM10 has run. BM10 stays optional and a subgraph result, and production stays cargo (G25, and blaze built 0 of 5 workspaces and missed a clippy `deny`, `experiments/blaze.md:73`). Before BM10's 16 agent-hours, run `cargo build --timings` once on the cold runner root: if the longest dependency chain is at least 70% of the 92 s wall, more cores cannot help and BM10 is dropped with that figure published; otherwise BM10 runs.
- **Evidence.** `experiments/mbx-cache-cluster.md`: 92 s cold on 96 threads, 39.9 s from a unit cache, 67.4 s after a lockfile change. Whole-tree paths in 42 of 214 commits and a `Cargo.lock` in 10.5% (C11) size the class.
- **Confidence.** Needs a lab.

### C12. Sandbox-start floor, pods against namespace slots

- **Ruling.**
  1. The figures are not in conflict; they time different events. A pod created per job is 0.4–1.5 s [E, Carbon] and was about 6 s under ARC [M]. A claim of a pre-created, pre-cloned sandbox is about 0.1 s [E, Opus, Neon, Fable]. A namespace slot is created in 0.01–0.03 s [E, Carbon, Argon]. Only the 0.25 s clone, the ARC pod and the 3–4 s dind probe are measured.
  2. On a pool hit, pods and slots do the same work (claim, exec). The choice turns on the miss: how often the pool is empty under the real arrival trace, and what a miss costs (a pod through the API server, scheduler and kubelet against a local `clone3`).
  3. Warm pods stay the default. Idle members hold small requests and are resized in place at claim (W10), so an idle build pod no longer reserves 24 GiB; a deferred resize counts as a miss. Namespace slots stay P08's variant, adopted only if faster at P90 with misses included and with the same ownership proofs.
  4. §3 stage 3 keeps ≤ 1 s P90 and says it is the build class on a pool hit. The Docker class has its own row: a private daemon per request starts in seconds (3–4 s probe today [M]), as Argon says.
- **Evidence.** W10; §3; `evidence.md` §B (436 ARC starts; 57 probe failures an hour).
- **Confidence.** Needs a lab: P08, which must report hit latency, miss latency, the miss rate on the 2026-11-18 trace, resize latency and deferral rate, and idle memory held.

### C13. Cost of the full tree hash

- **Ruling.** Astra, Carbon and Neon are right about what bounds it. §7.4's "0.1–0.3 s for the 110 MiB tree [E: SHA-1 at about 1 GB/s]" prices the wrong term: hashing today's 37.3 MiB is 0.04 s, and the cost is 5,164 opens and reads (W7). They are cold reads: a reflink clone has new inodes and no resident pages (W8), so the base's warm cache does not help. The full hash stays, because §6.4's mirror row rests on it. §7.4 restates the estimate per file and marks it unmeasured on a worker; stage 4's ≤ 1 s stands as the budget. A delta-only hash needs a lower layer the kernel verifies, and fs-verity does not exist on XFS (W9); that means dm-verity over an image, or another filesystem for sealed bases. It stays a candidate, built only if BM13 shows the full hash above 0.3 s at P90.
- **Evidence.** W7, W8, W9.
- **Confidence.** Likely for the shape; the figure needs BM13, on a fresh reflink clone, idle and at 80% CPU, by thread count.

### X01. May a complete trace license a narrower key?

- **X01a, verdict keys: no.** The observed-input argument is a theorem only for a process that is deterministic given the traced inputs, and no audit can establish that premise for a Rust test.
  1. *Not observable.* `clock_gettime`, `gettimeofday`, `time` and `getcpu` are answered in the vDSO without a system call (vdso(7)), and `RDTSC` and `RDRAND` are instructions. A trace cannot show a clock was not read.
  2. *Always present.* Every Rust process that builds a `HashMap` seeds it from the operating system (W5), and 80 files here use one (W6). "No randomness input" is false for nearly every test binary, or true only by ignoring that call.
  3. *Reproduced.* W5: with no clock, network, listing or `rand`, the path read differs between runs on the same tree; a trace-keyed lookup returns a pass on a tree where 12 of 40 fresh runs fail. Thread scheduling (54 files) does the same.
  4. *The audit as specified is not a complete trace.* §6.4 lists the exits of `openat`, `openat2`, `newfstatat`, `statx`, `readlinkat` and `getdents64`. It misses `access` and `faccessat2` (an existence probe with no open), `execve` lookups, the legacy x86-64 calls (`open`, `stat`, `lstat`, `readlink`, `getdents`) and io_uring's `IORING_OP_OPENAT` and `IORING_OP_STATX`, which complete without a syscall exit. This also weakens the *enforcing* audit of enrolled contracts, so it is a required fix whatever X01 says.
  - So Q5 and §6.1 stand unchanged: narrow keys need a reviewed contract, enforced on every execution; the trace proposes and revokes. Astra's sentence is the rule: "On a cache hit no process runs to expose a missing dependency."
  - *Proof and fallback for enrolled keys.* A hit executes nothing, so only an execution chosen independently of the key can catch a contract that missed an input. The 2% sample does that. A1 as written does not: it re-runs "changed obligations", chosen by the same key. Ruling Q12 called A1 "reuse-free"; under whole-tree keys the two coincide, under narrow keys they do not. A1 therefore re-runs a narrow-key obligation at the hit-sampling rate whether or not its key changed, and one reuse-free run of the full inventory covers the tip every night. Fallback as in §6.4: enrolment revoked, whole-tree key.
- **X01b, compile units: yes, behind a lab, as selection and never as proof.** This is G17. Step 3 of §7.3 sends a whole Cargo root cold when a changed path is outside every unit's declared inputs. A new `.rs` file is always outside them, and 64% of landed commits add one (W6), so the rule fires on about two heads in three and §3's 15–25 s compile row does not hold for them.
  - *What may change.* Step 5 already records, for each unit, what `rustc`, its build script and its proc macros opened, listed or failed to find. A path added or removed that meets no unit's recorded set rebuilds nothing by itself; a path that meets one rebuilds that unit and its dependants. Cargo still plans on the real tree, so a new target, manifest or build script is seen by cargo.
  - *Why this does not reverse Q5.* A stale unit is a different binary, and digest equality with a cold build detects it without a failing test. A stale verdict has no such detector. Step 5 already reuses on observed inputs (dep-info) under that proof, and change 18 of the earlier rulings is kept: no time or trace rule alone authorises a hit.
  - *Conditions.* Every unit of the root has a complete trace with no lost events, or the root builds cold. The narrowing is its own reuse class under C6: the second witness builds cold with equal digests until about 3,000 clean comparisons on added- or removed-path deltas per root, then 2% and A1's cold digests.
  - *Lab B5.* 300 replayed diffs that add or remove a path, plus planted cases: `foo/mod.rs` beside `foo.rs`; a new `tests/x.rs`; a new file in a directory a build script globs; a new file a proc macro lists; a new `clippy.toml`, `rustfmt.toml` or `.cargo/config.toml` below the root. Pass: every planted case rebuilds and every binary digest equals the cold build.
  - *Default and fallback.* Step 3 as written. After adoption, the first digest disagreement retires the generation and restores root-cold for that root.
- **Confidence.** X01a settled; X01b needs a lab.

## The landscape ideas

Ids in brackets are each analyst's own: O Opus, A Astra, N Neon.

### L01. Pool sized by demand (O B01, A B01, N B01)
**Adopt.** Per class: a floor by hour, plus the sandboxes `ensure`'s obligation vector asks for, plus one group's worth per auto-merge enqueue (the group follows about 19 s later [M]). Members are single-use and hold no credentials. Opus's worry that an idle build pod reserves a quarter of a worker is answered by W10: resize at claim is stable on this cluster, which Opus left unchecked. Refilling the pool on an unlabelled push verifies nothing and fits decision 12 (Astra). Astra's stage timers (delivery, queue, claim, attach, first process) are adopted; Neon's `idle_time` hysteresis is a tuning value. The mechanism is C12's lab.

### L02. Fork a sealed generation per job (O B02, A B02, N B02)
**Already the design** (reflink from a generation trusted after A1); "commit on exit 0, last writer wins" is rejected by all three. Adopt one detail from Opus: a generation that came from a warm build is replaced by a cold one when its size passes a set multiple of the cold target, which keeps §7.5's budget without a collector. "Stale rather than a miss" holds for generations only, where M_G's diff makes an older base slower and never wrong; for `actions/cache` it would change `cache-matched-key` (Opus, against Neon's wider wording).

### L03. Placement by held state (O B03, A B03, N B01)
**Adopt.** Node agents advertise generation ids, L0 manifests and mirror heads; the dispatcher ranks candidates by predicted completion, queue wait included (Astra), and the preference is soft. The rule of distinct nodes and distinct generations for witnesses is unchanged, so this moves time only. The wait bound is lab B3's crossover, as §7.3 says. No writable disk moves between jobs.

### L04. Presence query, leases, output digests, subtree ids (O B09, A B08, N B08)
**Adopt.** `BlobStore` gains a batched `find_missing`; a backend without it answers from Velnor's index or per-key checks. A manifest or record commits only after every blob it names is confirmed present and its last use renewed, so eviction cannot leave a record whose bytes are gone. Records carry the digests of their log and JUnit output. An enrolled key is computed from Git subtree ids, plus the contract's single files and absent paths. W12 is why the saved PUTs matter: per-object cost dominates, and 415 of 419 chunks were unchanged in the one measured pair. Not adopted: results written by clients (C4). Neon's `salt` is the epoch the design already has.

### L05. Chunked generation shipping (O B04)
**Experiment gate.** It joins lab B3 as a transport: a signed manifest, the receiver fetches what it lacks, chunks of at least 4 MiB (W12). The 5–15% delta is Opus's guess and B3 measures it. Ruling Q16 holds: no transport on the verdict path until one beats a cold build there.

### L06. Whole-sandbox snapshot restore (O B05, A B04, N B04)
**Adopt as contract text only** (decision 21). §5.4 says: a snapshot's digest is in the execution fingerprint; only the builder class takes one, from a pristine environment before any assignment; a job never does; restore supplies fresh identity, entropy and credentials and private writable storage; a backend chooses restore only where its measured cost beats fresh provisioning (WarpBuild's 45–60 s boots are Astra's caution). Nothing is built. The page-cache part is L07.

### L07. Shared page cache between clones (O B05, N B02, A B06)
**Experiment gate, unchanged from Q16:** lab B2 and BM12, reflink against a read-only overlayfs lower layer. The mechanism is documented (W9) and unmeasured here. Two measurements are added: page cache per additional sandbox, and bytes copied up per build, because cargo hard-links artifacts and a hard link copies the file up (W9). Neon's second claim is refused: a read-only lower layer stops a job's writes, not a wrong base, so it does not let the checkout hash only the delta (C13). Neon's "4 to 6 slots" is an estimate until BM12.

### L08. Pre-filled action-archive cache for GitHub Official (O B06)
**Adopt now.** Opus's citation holds at the tag (W1; the constant is `Constants.cs:327`). Two layouts exist: `<dir>/<owner_repo>/<sha>.tar.gz`, copied and unpacked (`ActionManager.cs:1285-1296`), and, with `ACTIONS_RUNNER_SYMLINK_CACHED_ACTIONS=true`, an unpacked `<dir>/<owner_repo>/<sha>/<one directory>` that is symlinked (`:1236-1259`). The runner checks no digest, so the store is read-only, filled by pinned SHA, and part of the tool-store digest already in the key. It is stock behaviour (W4), so the reference role uses it too. Decision-log I019 limited this to "until the executor cutover"; decision 20 makes Official permanent, so the limit goes. It removes 4.4 s of downloads per Official job [M], which matters for S2: without it most of the 5 s threshold for an executor claim would be action downloads.

### L09. Cache v2 redirection for GitHub Official (O B06, A B05, N B05)
**Adopt behind plan ST05 and its conformance cases;** decision 22's open item is answered from the source.
- *It can be done.* Velnor builds the message the Worker reads (W2). Rewriting `ResultsServiceUrl` redirects only what actions send, because the Worker's own logs go to `system.github.results_endpoint`. That is narrower than Opus and Astra feared, but the endpoint must still pass the artifacts v4 Twirp methods through to GitHub with the job's token.
- *The protocol.* Opus and Astra are right and Neon's "Twirp/HTTP endpoint" is incomplete: the save path uploads with the Azure Blob SDK (W3), so the URL Velnor returns must accept Put Blob, Put Block and Put Block List. The restore is one plain GET on any non-Azure host, so Depot's eight streams do not apply to the stock action.
- *The gain.* Only the transfer: 44–61 s [M] for 1.92 GB becomes a loopback GET. The stock action still creates and extracts a tar (Astra), so Opus's "0.25–5 s clone" does not apply to Official and Neon's 2–4 s omits extraction.
- *Rules.* Scope from the job token as in §7.2; the reference role keeps GitHub's endpoints, or the oracle checks Velnor against itself (Opus); the bundle of each pinned action revision is re-read when the pin moves (Astra); until ST05 passes, acquired Official jobs use GitHub's cache. No receipt-minting obligation restores a cache.

### L10. Co-located caches and parallel reads (O B07, A B06, N B06)
**Already the design** (extracted trees in L0, names resolved through the home index before any tier). Adopt two details: L1 reads split into parallel ranges, hash-checked per chunk, for Velnor Native and the node agent; metadata lookups batched across the 35 ms site link. Speed only.

### L11. Peer-to-peer blob tier and asynchronous artifact upload (N B06)
**Reject.** A worker-to-worker tier adds a third transfer path before the second has passed ST06; if ST06 fails, peers come back as a `BlobStore` backend, where a hash check on read keeps them slower and never wrong. Reporting an `upload-artifact` step done before GitHub holds the artifact departs from the official step's outcome, which §5.7 compares.

### L12. High-clock nodes (O B08, A B07, N B07)
**Owner question D20 stands, restated.** Astra's point changes the question: a green waits for the slower of two witnesses on distinct nodes, so one high-clock node per site shortens a red and A1, not a green. D20's "helper class about 25 → 18 s" needs two such nodes in the executing site. One rented node is still right for BM15's ratio. Neon's `serial_fast` capability is not added until such a node exists. BM15 also states whether the fingerprint (§6.1, "the CPU features the toolchain may target") gives the two CPU models one key; if not, mixed witnesses cannot fill one record.

### L13. Test splits by recorded timings (O B10, A B09, N B09)
**Adopt behind lab C,** as §6.2 already gates fan-out. The three agree. Durations come from signed receipts and set the layout only, so even a PR's timings are safe (Opus). The layout is in the execution manifest; the union of shard ids must equal the inventory exactly; an unknown id gets a conservative estimate and runs (Astra); tests in one nextest test group stay in one shard (Astra's interaction caution; `crates/velnor-runner/.config/nextest.toml:19-24` has such a group); partitioning is static (Neon). The floor is the longest test, so this follows §8.5's floor work and does not replace it.

### L14. Flake scoring and quarantine (O B11, A B10, N B10)
**Adopt the scoring:** per-test discordance from witness pairs, A1 and the 2% sample, an issue opened automatically, an expired quarantine restored to blocking automatically, and a quarantined test kept running in a lane that does not block. **Quarantine is where the three differ.** Opus and Neon accept an owner-signed expiring exception; Astra requires an equivalent blocking check shown on the same planted fault. Ruling: Astra's rule is the only quarantine available now. An owner exception is an inventory reduction, which is owner question D17, and D17's default is that none is accepted. This keeps decision-log N14 and makes its second branch wait for D17. Mute, skip and retry-until-green are rejected by all three.

### L15. Sound narrowing; prediction for order only (O B12, A B11–B12, N B11)
**Already the design:** narrowing is the enrolled contract (X01a), and a history model may only order the non-minting fast-red lane; prediction as selection is rejected by all three. Start with changed-package and recently failing tests first, and compare a model only if that leaves waiting time (Astra). Neon's `.github/` and `.velnor/` scopes are §6.4's rule, with X01a's caveat that the audit not finding a reader proves nothing. The one change is X01a's: A1 must not pick its work by a narrowed key (Astra).

### L16. Merge queue (O B13, A B13, N B12)
**Prefix reuse and speculation are §8.3.** On bisection Opus is right against Astra and Neon: under ALLGREEN every entry's prefix gets its own group and status (rulings V3, V4), so the first failing prefix names the culprit, and the single-ejection bypass trees already cover what follows. No bisection lane is built. Depth stays under §8.3's gate. Neon's merge train is owner question D18, unchanged.

### L17. Optimistic merge (O B14, A B14, N R1)
**Reject,** unanimously: a commit would land before its own tree passed (C1, decisions 11 and 12). The asynchronous half that is sound is A1, which the design has.

### L18. Git-mirror concurrency (O B15, A B15, N B13)
**Adopt.** One writer per mirror per node (`flock`); at most one fetch running and one waiting that absorbs later requests, checking after the lock whether the commit already arrived (Astra); explicit refspecs with `--atomic --no-auto-maintenance` (W11), which makes Opus's staging namespace unnecessary; `gc.auto=0` and no pruning in place. A refresh is a new mirror swapped in, like a sealed base, and the old one is deleted only after its readers are proven absent. That replaces Astra's per-reader pins with the tri-state proof the platform has. The tree hash already protects ordinary checkouts; this protects jobs that read `.git` through alternates (W11; `root.rs:22`).

### L19. Preparation before the label (O B16, A reject list, N B01)
**Fetch only.** Decision 24 already fetches PR heads on push. A compile before the label is owner question D19, recommended no; Astra's reading of decision 12 is the default. Cache prefetch starts at the label.

### L20. Persistent image builders (O B17, N B03)
**Experiment gate.** §4 row 16 already builds an image once per image-input key. A layer cache per scope helps only when that key changes, so first count those changes per week; build the cache if image builds exceed 20 runner-min a day. If built: PR scopes read the trusted cache and write their own; trusted images come from the builder class with inputs pinned by digest, because a `RUN` layer is keyed by its command, not by what it fetched (Opus).

## Changes the design must make

Each group is one commit, "research: final design, landscape rulings from <model>". Section numbers are v3's.

**Adjudication (evidence new in this document; no panel member)**
1. §6.4, the read audit: the syscall list is incomplete (`access`, `faccessat2`, `execve`, the legacy calls, io_uring). Hook path resolution in the kernel (LSM), or deny every unaudited path call by seccomp so that it fails the witness (X01a).
2. §6.1, §1.2 C2: add the second reason a trace cannot prove a key, with the reproduction: the paths read vary between runs on one tree, and clock reads are invisible to the audit (X01a).
3. §7.4 step 3: replace "0.1–0.3 s for the 110 MiB tree [E]" with the per-file statement (5,164 files, 37.3 MiB, cold after a clone, unmeasured on a worker); the delta-hash candidate needs dm-verity or a non-XFS store. BM13 runs on a fresh clone, idle and loaded (C13).
4. §7.3, §3 stage 7, S6: state that step 3 fires on a head that adds a source file, 64% of landed commits; S6 reports that cohort separately (X01b).
5. §7.2 "Open" and §5.3: the route is verified at v2.338.2; only action traffic follows `ResultsServiceUrl`; cite `Constants.cs:327` (L09, L08).
6. §5.4, §9: in-place resize is stable on the cluster's Kubernetes; idle pool members hold small requests (C12, L01).
7. §11, S3: a rented arm runs with the vendor's recommended caches and phase 0 applied (C03). Plan BM10 is preceded by one `cargo build --timings` run with a 70% rule (C04).

**GPT-6.2-Astra**
8. §1.2 C6, §6.4 (per-root row), §6.5: A1 re-runs narrow-key obligations at the hit-sampling rate regardless of key, plus a nightly reuse-free full-inventory run of the tip; A1 is no longer listed as the detector of an input a narrow key missed (X01a, L15, ruling Q12).
9. §7.2, plan ST05: the Official cache endpoint implements Put Blob, Put Block and Put Block List on its upload URL, passes artifacts v4 through, and is re-qualified when the pinned action changes (L09).
10. §1.3: quarantine needs an equivalent blocking check shown on the same planted fault; the owner-exception form is a D17 reduction and inactive until D17 is answered (L14). §13 D17 names quarantine.
11. §5.4: the snapshot contract paragraph (L06).
12. §13 D20, §9: one high-clock node per site cannot shorten a green; the stated gain needs two (L12).
13. §6.2: split rules for unknown ids and nextest test groups (L13).

**Claude Opus 5.5**
14. §5.3, §9: `velnor-official` runs with `ACTIONS_RUNNER_ACTION_ARCHIVE_CACHE` on a read-only, digest-pinned store, in both roles; S2 runs with it; decision-log I019 loses "until the cutover" (L08).
15. §7.4: the mirror writer rules of L18.
16. §5.4, §9: the pool sizing rule and stage timers (L01); §5.5, §9: placement by advertised state (L03).
17. §7.1: `find_missing`, present-before-commit and renewed last use; §6.2: log and JUnit digests in a record; §6.1: enrolled keys from subtree ids (L04).
18. §8.3: one sentence, the first failing prefix names the culprit and no bisection runs (L16).
19. §7.5: the size-triggered cold reset (L02). Plan B3: the chunk-manifest transport, chunks ≥ 4 MiB (L05). §7.1: parallel ranged L1 reads (L10).

**Gemini 4 Neon**
20. §7.3 step 3, §10.3: lab B5 and the narrowed rule it gates, as its own reuse class (X01b; G17).
21. §10.3 lab B2, plan BM12: add page cache per additional sandbox and copy-up bytes; state that a read-only lower layer does not replace the full tree hash (L07).
22. §1.3: per-test discordance scoring, automatic issues and automatic expiry (L14; also Opus).
23. Plan: count image-input-key changes per week before any layer cache is built (L20; also Opus).

**Gemini 4 Carbon and Gemini 4 Argon**
24. §3 stage 3: the ≤ 1 s budget is the build class on a pool hit; a separate row for the Docker class's private daemon. Plan P08 reports hit, miss, miss rate, resize deferral and idle memory (C12).

**Claude Fable 5.2**
25. §11: one sentence per comparator. BM07 stands in for persistent agents and keeps the Actions hop; "Velnor with Official" is the "same ledger, other agent" case; the published sentence excludes the cold and wide class against remote execution unless BM10 ran (C02, C04).
