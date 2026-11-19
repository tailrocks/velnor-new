# What to borrow from other runners: Gemini 4 Neon

| Field | Value |
|---|---|
| Model | Gemini 4 Neon |
| Model id | `gemini-4-neon` |
| Client | Antigravity CLI 1.5.9 (agy -p) |
| Reasoning effort | high |
| Date / run | 2026-11-19 |
| Brief | landscape brief v1 (2026-11-19): the runner landscape dossier, the final design and the owner decisions |

This is the model's own analysis of what Velnor should borrow from, or avoid in, other well-known CI runners, based on the public landscape dossier, unedited except for formatting.

---

# Velnor: What to Borrow from Other CI Runners

Design analysis of `velnor-runner-architecture.md` (v3) against the public runner landscape,
grounded in owner decisions 1–24 and the nine fastest-runner critiques. Facts not in
`runner-landscape.md` or the v3 docs are marked *(from my knowledge)*.

## Verdict

- **Ahead of GitHub-hosted, ARC, and cloud-VM runners (RunsOn, Depot, Ubicloud, GitLab):** Velnor bypasses Actions job offers/acquisition on PR and merge-group verdicts, avoids cloud-VM boot overhead, and promotes verified release bytes by digest instead of running three full CI passes per merge.
- **Ahead of local-NVMe fast runners (Namespace, Blacksmith, Actuated, WarpBuild, Buildkite hosted) and optimistic monorepo CI (TAP, Sandcastle) on soundness:** Velnor turns best-effort caches into a cryptographically signed, two-witness ledger with `M_G` input proofs, dep-info checks, and cold A1 audits—reusing verdicts across pushes, PRs, and merge groups without accepting TAP/Sandcastle's false-green risk.
- **Behind high-clock fast runners (Blacksmith, Ubicloud Premium) on the serial floor:** Fixed EPYC 9454P throughput chips (~3.8 GHz boost, *from my knowledge*) trail 5.7 GHz Ryzen nodes by an estimated 1.3–1.5× on single-crate `rustc`, linking, and serial tests.
- **Behind BuildBuddy, EngFlow, and REAPI on storage/CAS and page-cache efficiency:** XFS reflinks duplicate 14.29 GiB of page cache per clone (capping workers at 4–6 slots), `velnor-official` still lacks a local Cache-v2 proxy (Depot/Ubicloud), and blob shipping lacks `FindMissingBlobs` and peer-to-peer worker CAS.
- **Behind Buildkite, CircleCI, and third-party merge queues (Trunk, Mergify, Aviator) on tail-shaping:** Velnor leaves timing-based test sharding off by default, lacks mirror file-locking/GC guards, and keeps GitHub's ~45 s merge queue (Decision 7) without speculative bisection trees.

## Ideas to borrow

Every listed topic is evaluated below; **optimistic merge with asynchronous tests** (Google TAP,
Trunk) and **ML test skipping** (Meta PTS, Google Transition Prediction) conflict directly with
Decision 11 (zero false greens) and are rejected in `## Ideas to reject` rather than borrowed.

| id | technique | who does it | what changes in Velnor | expected gain | correctness argument | fit with owner decisions | cost in agent-hours |
|---|---|---|---|---|---|---|---|
| B01 | Adaptive warm-pool hysteresis, webhook pre-warming & trait placement | Depot, RunsOn, GitLab (`idle_count`), Blacksmith, Meta Jupiter | §5.4, §5.5, §9 (`velnor-dispatch`, `velnor-node`) | Eliminates cold-pod start (~6 s ARC → ≤0.3 s); cuts second-witness placement wait 5–25 s under burst load [E] | Placement only selects a node; every obligation still verifies `M_G`, tree hash, and two independent receipts | fits | 16–24 |
| B02 | Per-node generation lineage with stale-over-miss fallback & shared RO lower layers (`overlayfs`/EROFS) | Namespace, Buildkite hosted, BuildBuddy | §5.4, §7.1, §7.3 (`velnor-node`, `velnor-platform`) | Reclaims ~43 GiB duplicate page cache across 4 clones (14.29 GiB/clone [M]), lifting 4→6 slots/node; cuts clone+hash 1.3 s → <0.2 s [E] | Kernel-enforced RO `lowerdir` isolates writes; `M_G` content-diff + dep-info check makes stale bases safe (slower, never wrong) | fits | 20–30 |
| B03 | Content-addressed BuildKit layer cache on scoped persistent NVMe (Docker/image jobs only) | Depot, Blacksmith, Namespace, Buildkite remote builders | §5.8, §7.1 (`velnor-platform`, `docker` class) | Cuts image-candidate (200 s [M]) & Docker-class build steps 30–120 s by avoiding tarball save/load and repeated layer pulls [E] | OCI layers are content-addressed by digest; restricted strictly to BuildKit cache, never shared with build/test workspaces; PR scope never writes `main` | fits | 12–18 |
| B04 | Content-addressed chunked snapshot contract in `Platform` (lazy CoW restore for future microVMs) | BuildBuddy (UFFD + FUSE CoW), GitLab suspend/resume | §5.4 (`velnor-platform` trait & `Capabilities`) | Sub-second ("hundreds of ms") microVM/Apple-VM cold start when future backends land, without core rewrites [vendor] | Base snapshots are immutable, content-addressed, and key-bound in the execution fingerprint; dirty chunks write to private per-job CoW | fits (Decision 21) | 8–12 |
| B05 | On-node Cache-v2 proxy with tuned parallel streams for `velnor-official` | Depot (4 up / 8 down), Ubicloud, RunsOn, Blacksmith | §5.3, §7.2 (`velnor-official`, `velnor-cache`) | Cuts Official cache restore from 44–61 s (GitHub Azure [M]) to ~2–4 s (4–10× LAN throughput, 400–1,000 MiB/s [vendor]) | Authoritative index enforces GitHub's exact ref/base/default scope, immutable keys, and content-hash checks; never used by receipt-minting obligations | fits (resolves D22 open item) | 16–24 |
| B06 | Co-located site-local artifact read-through & worker-to-worker LAN blob cache | Blacksmith, Depot, EngFlow | §5.6, §7.1, §7.2 (`velnor-storage`, `velnor-native-actions`) | Cuts inter-job artifact waits (1.48 GB upload 36–43 s at 13.6–22.4 MB/s WAN [M] → ~2–4 s LAN) and bypasses FI RustFS LVM/fsync bottlenecks [E] | Every blob is addressed and verified by SHA-256 digest on read; missing or corrupt LAN peers fall back to L1/GitHub | fits (Decisions 22–23) | 18–26 |
| B07 | Capability-routed high-clock serial tier (`serial-fast` nodes) | Blacksmith (Ryzen 7950X 5.7 GHz), Ubicloud Premium (Ryzen) | §5.4, §9 (`velnor-dispatch`, `Capabilities`) | 1.3–1.5× faster single-crate compile, link & serial test floor [E, I104]: helper 25→18 s, cold runner-root 92→60–70 s | CPU microarchitecture is already in the execution fingerprint (§6.1); zero cross-arch reuse and unchanged witness/A1 rules | needs an owner question (when to buy/rent 1–2 nodes) | 8–12 (routing) |
| B08 | REAPI-style CAS (`FindMissingBlobs`, batch RPCs, lease extension on probe, `salt` epochs) | REAPI v2 (Bazel/Buck2), BuildBuddy, EngFlow | §6.1, §6.5, §7.1, §7.3 (`velnor-storage`, `velnor-ledger`) | Avoids redundant chunk uploads (e.g. 6.8 GiB mbx lockfile upload [M] → only missing deltas; ~99% dedupe on 1-crate changes [M]) | Content-addressed digests verified on read; `salt` enforces strict epoch and executor-ID namespace separation; probe lease-extension prevents GC races | fits | 16–24 |
| B09 | Historical timing-based LPT test splitting with manifest-bound shards | Buildkite (`bktec`), CircleCI (`--split-by=timings`) | §6.2, §8.5 (`velnor-ledger`, `velnor-ci`) | Pulls long suites toward `max(longest_test, sum/N)`: e.g. 679 s `non_inputs_differential` → ~180–250 s at 4 shards [E]; `velnor-ci` 10/62 s → ~3–5 s | Exact expected test-ID inventory (C3) is partitioned deterministically and bound in the receipt; any missing, extra, or duplicate test ID fails the gate | fits | 20–28 |
| B10 | Automated flake telemetry, shadow re-verification of quarantined tests & auto-expiring exceptions | Buildkite Test Engine (tracking), Trunk anti-flake | §1.3, §6.5 (`velnor-ledger`, `velnor-control`) | Drives paired false failures from 15.5% (8.1% single [M]) toward ≤1–3% budget, cutting queue ejections that rebuild entire depth-2/4 prefixes | Zero retries and zero silent mute/skip; quarantine requires an owner-signed expiring exception or equivalent blocking check and runs in shadow until fixed | fits | 12–16 |
| B11 | Sound path-scope narrowing & failure-affinity test ordering (fail-fast only, never skipping) | Google TAP (dep-graph), Mergify (scopes), Meta PTS (ordering only) | §6.1, §6.4, §8.5 (`velnor-ci`, `velnor-ledger`) | Narrowing `.github/`/`.velnor/` moves ~20% of commits (42/214 [M]) off whole-tree keys; fail-fast ordering cuts time-to-first-red by 50%+ [E] | Path scopes are enforced on every run by the eBPF read audit (C2/C6); failure-affinity only reorders execution, never omits any test from a green verdict | fits | 16–24 |
| B12 | Speculative bisection-tree & disjoint-target pre-verification behind GitHub's merge queue | Trunk (halving bisection, impacted queues), Mergify, Aviator, Uber SubmitQueue, Graphite | §8.3 (`velnor-control`, `velnor-dispatch`) | Turns post-ejection GitHub MQ rebuilds (19.9/84.1 min today, 15–90 s in v3) into <2 s ledger lookups; optional CAS fast-forward train saves ~45 s/merge [M] | Speculation only populates content-keyed candidate records; every landed group still checks its exact tree with two witnesses under ALLGREEN | fits (speculation); needs an owner question for CAS fast-forward (amends D7) | 16–24 (speculation) |
| B13 | Per-mirror file locking and GC-safe fetch/repack discipline | Buildkite (`git-mirrors`) | §7.4 (`velnor-node`) | Eliminates concurrent `git fetch` lock contention and alternates-pruning failures across ~10,000 checkouts/day [E] | Single-writer mirror lock + disabled auto-GC prevents mid-checkout object disappearance; staged tree is still hash-verified in full before step 1 | fits | 4–8 |

**B01 — Adaptive warm-pool hysteresis, webhook pre-warming & trait placement.** Depot's standby
pools, GitLab's `idle_count`/`idle_time` hysteresis, Blacksmith's webhook-to-in-memory-queue push,
and Meta Jupiter's worker-trait matching solve Velnor's unmeasured Stage 3 (`provision + attach`) and
second-witness queueing gaps (Objections O10, O27). In §5.4 and §9, `velnor-node` should maintain
pre-initialized warm slots with configurable `idle_count` and `idle_time` floors (preventing pool
thrash across bursty agent waves), while `velnor-dispatch` matches obligations to nodes by live
traits—held generation ID, free page-cache headroom, and site—before acquiring or starting work. On
`pull_request` / `merge_group` webhooks, nodes pre-fetch missing Git objects and warm L0 cache
manifests immediately, shaving cold-start and placement tails without altering any verification key.

**B02 — Per-node generation lineage, stale-over-miss fallback & shared RO lower layers.** Both
Namespace and Buildkite hosted agents fork local NVMe volumes per job and commit only on exit 0,
with Namespace explicitly preferring a slightly stale local generation over delaying startup for a
remote fetch. Velnor already upgrades this model with `M_G` input manifests and A1 cold-digest
verification (rejecting last-writer-wins for trusted generations), but XFS reflinks do not share the
Linux page cache—consuming 14.29 GiB per read clone (E1d) and capping 125.5 GiB workers at 4–6
slots. Borrowing BuildBuddy's shared-read-only-chunk principle via a read-only `overlayfs` or EROFS
`lowerdir` (*from my knowledge*, Linux `overlayfs` shares lowerdir page-cache pages across mounts)
plus Namespace's stale-over-miss local preference reclaims ~43 GiB across 4 concurrent sandboxes,
lets sealed trees skip full 110 MiB re-hashing by verifying only `upperdir` deltas, and stays 100%
sound because `M_G` diffing and dep-info checks still catch every changed input.

**B03 — Content-addressed BuildKit layer cache on scoped persistent NVMe.** Depot, Blacksmith,
Namespace, and Buildkite all avoid Docker tarball save/load by keeping BuildKit layer caches on
persistent NVMe disks co-located with builders. While raw mutable sticky disks are unsafe for build
and test workspaces (see R4), OCI image layers and BuildKit caches are content-addressed by digest
(*from my knowledge*). In §5.8 and §7.1, Velnor's `docker` class (private daemon per request) and
image-candidate Actions jobs can mount a read-only trusted layer cache plus a per-scope BuildKit
cache volume on node NVMe (with PR jobs able to read `main`'s cache and write only their own
scratch scope, evicted by 7-day LRU like Blacksmith/WarpBuild). This eliminates repeated image
pulls and layer rebuilds without permitting PR writes to pollute `main`'s trusted layers.

**B04 — Content-addressed chunked snapshot contract in `Platform`.** WarpBuild's whole-VM snapshots
boot in 45–60 s on network disks and leak org-wide state, which is unusable here. BuildBuddy's
Firecracker snapshots, by contrast, chunk memory and disk by content address, restore memory lazily
via `userfaultfd`, back disk writes with FUSE copy-on-write, and resume in hundreds of milliseconds
while sharing immutable chunks across VMs. Velnor builds only Kubernetes and Docker backends now,
but Decision 21 requires designing `Platform` (§5.4) so microVM (Firecracker/Cloud Hypervisor) and
Apple Containerization backends drop in cleanly later. Refining `Capabilities::snapshot_restore` to
specify an immutable, content-addressed base snapshot ID bound into the execution fingerprint plus
private CoW upper storage prevents a breaking trait redesign when microVMs arrive.

**B05 — On-node Cache-v2 proxy with tuned parallel streams for `velnor-official`.** Depot's
on-runner Go proxy and Ubicloud's local proxy prove that stock `actions/cache` steps need zero
forking when the runner redirects the Cache API to a fast co-located backend; Depot further boosts
throughput to ~1,000 MiB/s by using 4 upload and 8 download streams (vs GitHub's default 2). This
directly resolves Decision 22's open question (§7.2): `velnor-node` should expose a loopback
Cache-v2-compatible Twirp/HTTP endpoint backed by Velnor's authoritative index and L0/L1 tiers,
injecting its URL into `Runner.Worker`'s job message (handling the shared `ACTIONS_RESULTS_URL`
routing noted in §7.2). `velnor-official` then drops GitHub's 44–61 s Azure restores to ~2–4 s,
enables cross-executor cache round-trip conformance tests, and makes Native-vs-Official benchmarks
(`O04`) measure true executor speed rather than Azure bandwidth.

**B06 — Co-located site-local artifact read-through & worker-to-worker LAN blob cache.** Blacksmith
(~400 MB/s co-located vs ~100 MB/s WAN) and Depot (runners on the same private network as builders)
show that eliminating WAN hops dominates cache latency, while EngFlow eliminates a separate CAS
tier by serving blobs peer-to-peer directly from workers. Velnor's §5.6 still uploads 1.48 GB
artifacts to GitHub US (36–43 s at 13.6–22.4 MB/s), and FI workers have 100% of their 3.84 TB NVMe
allocated to one unshrinkable XFS LV (O29), risking RustFS L1 delays alongside RustFS 1.0.x's open
issues (#9480, #8768, #9161). Serving immutable SHA-256-addressed blobs (test binaries, artifacts,
and cache chunks) peer-to-peer between `velnor-node` agents over LAN as an L0.5 read-through tier
gives immediate ~1 GB/s site-local speed before RustFS passes ST06, while asynchronous parallel
uploads keep `gh run download` compatible.

**B07 — Capability-routed high-clock serial tier (`serial-fast` nodes).** Blacksmith (Ryzen 7950X
at 5.7 GHz, PassMark 4484, ~2× single-thread vs GitHub) and Ubicloud Premium (Ryzen, ~2× CPU-heavy)
target the exact bottleneck Velnor faces once concurrency and reuse are solved: single-crate `rustc`
front-end/codegen, linking, and indivisible serial tests on 96-thread EPYC 9454P chips (~3.8 GHz
boost, *from my knowledge*). Hardware is fixed at 5 × EPYC 9454P for now, so Velnor should add a
`serial_fast` capability/trait in `velnor-dispatch` (§5.4, §9) today—already safe because CPU
features/microarchitecture live in the execution fingerprint (§6.1)—and ask the owner whether to
rent or buy 1–2 high-clock nodes (e.g., Ryzen 9950X / EPYC 4564P, ~$150–250/mo, *from my
knowledge*) earlier than "after floor fixes" to capture the 1.3–1.5× serial speedup immediately.

**B08 — REAPI-style CAS (`FindMissingBlobs`, batch RPCs, lease extension on probe, `salt` epochs).**
Velnor's ledger (§6) already goes beyond REAPI's `ActionCache` by binding two independent Ed25519
witnesses, test-ID inventories, and A1 cold audits to every record, so Velnor should keep its
ledger rather than adopting REAPI's single-result `ActionCache`. However, Bazel/Buck2 REAPI v2
formalizes four CAS primitives that Velnor's §7 storage layer and `mbx-cache` lack:
`FindMissingBlobs` (probing digests before uploading, cutting redundant transfers like the 6.8 GiB
`mbx` lockfile upload down to the ~1% new chunks), `BatchReadBlobs`/`BatchUpdateBlobs` for small
metadata/chunks (mitigating RustFS's small-PUT regression #9161), automatic blob lifetime extension
on presence checks (preventing GC from deleting blobs referenced by an in-flight build), and an
explicit `salt` field in every key digest (unifying epoch, executor ID, and reference-namespace
isolation in §6.1). Adopting these RPC semantics inside `velnor-storage` makes L0/L1/P2P transfers
uniform and race-free.

**B09 — Historical timing-based LPT test splitting with manifest-bound shards.** Buildkite `bktec`
(16 min of tests across 4 partitions: 10 min → 4 min) and CircleCI (`--split-by=timings` from JUnit
XML) cut test tails without altering test semantics. Velnor §6.2 leaves test fan-out off by default
until Lab C, yet `non_inputs_differential` (679 s) and `velnor-ci` (781 tests, 10/62 s) dominate
verdict latency. `velnor-ledger` should ingest per-test timings from `cargo-nextest` JUnit output
(*from my knowledge*) and statically partition each obligation's exact C3 test-ID inventory across
2–4 sandboxes using longest-processing-time-first (LPT) bin packing (*from my knowledge*). Because
the shard layout and exact expected test IDs are bound into the signed receipt—and every shard runs
twice across independent witnesses—no test can silently drop, avoiding dynamic work-stealing's
nondeterminism while cutting P90 test wall time directly.

**B10 — Automated flake telemetry, shadow re-verification & auto-expiring quarantine exceptions.**
Buildkite Test Engine and Trunk anti-flake show the operational value of systematic flake tracking,
but their silent mute/skip and retry-until-green mechanics violate Decision 11. Velnor §1.3 already
sets the right policy (zero retries; 2 false failures in 7 days open an owner issue; quarantine
only with a blocking equivalent or expiring owner exception), yet lacks automated enforcement. By
computing per-test-ID discordance across twin witnesses and A1 runs in `velnor-ledger`,
auto-labelling issues, executing owner-quarantined tests in a non-blocking shadow lane to prove
fixes before re-admission, and automatically flipping expired quarantine exceptions back to
blocking-red, Velnor drives paired false failures from 15.5% toward ≤1% without ever masking a bug.

**B11 — Sound path-scope narrowing & failure-affinity test ordering (fail-fast only).** Google TAP's
dependency graph and Mergify's path scopes limit re-verification to affected targets, while Meta's
Predictive Test Selection (PTS) ranks tests by failure probability. Velnor cannot use ML to *skip*
tests (see R2), but can safely borrow both halves: (1) use the eBPF read audit (§6.4) to enrol
explicit Mergify-style path scopes for `.github/**` and `.velnor/**` rather than forcing every
per-root contract to include them—moving ~20% of commits (42 of 214 on `main`) off whole-tree keys;
and (2) score obligations and test binaries by historical change-to-failure affinity (from ledger
history) strictly to *order* execution so likely reds run first and cancel their twin on first
failure, cutting time-to-first-actionable-red by 50%+ with zero effect on green soundness.

**B12 — Speculative bisection-tree & disjoint-target pre-verification behind GitHub's merge queue.**
Mergify, Trunk, and Aviator batch queued PRs and bisect failed batches by halving (with Trunk also
running parallel queues per impacted target), while GitHub's merge queue (kept by Decision 7)
offers only basic `min/max` merge-group batching and ejects a failed entry without native
bisection, forcing full rebuilds behind it. Without leaving GitHub MQ, Velnor's speculative
scheduler (§8.3) can pre-verify halving bisection sub-batches and disjoint-root bypass trees at
`cpu.idle` priority whenever a queued entry shows a red fast-lane signal or high queue depth—so
GitHub's post-ejection `merge_group` resolves in <2 s from the ledger instead of rebuilding.
(Separately, Graphite/Aviator-style fast-forward merging via a Velnor CAS merge train, N31, would
save GitHub's ~45 s floor [19 s + 26 s M], but **needs an owner question** amending Decision 7.)

**B13 — Per-mirror file locking and GC-safe fetch/repack discipline.** Buildkite's `git-mirrors`
implementation enforces two simple invariants missing from Velnor §7.4: a per-repository file lock
so only one writer fetches into a bare mirror at a time, and explicit branch/SHA fetches that avoid
background `git gc` pruning objects still referenced by live `objects/info/alternates` checkouts
(*from my knowledge*, uncoordinated `git gc --auto` on a bare mirror can delete loose/pack objects
mid-read and break concurrent jobs using alternates). Adding an in-agent mutex plus `flock` per
mirror in `velnor-node`, setting `gc.auto=0` on active mirrors, and running maintenance repacks
only on quiescent or ref-pinned mirror snapshots costs 4–8 agent-hours and eliminates a nasty class
of sporadic checkout flakes at scale.

## Ideas to reject

- **R1. Optimistic merge with asynchronous full tests and auto-rollback (Google TAP, Trunk optimistic merging).**
  TAP ships changes on a fast presubmit subset (~95% pass confidence) and runs full tests post-submit
  with auto-rollback and Build Cops. This directly violates Decision 11 (zero false-green tolerance),
  Decision 12 (latest commit must pass before merge), and C1/ALLGREEN. Velnor's post-merge Audit A1
  is a *third* cold verification of an already-verified commit, never a substitute for pre-merge gating.
- **R2. ML / Predictive Test Selection for skipping tests (Meta Sandcastle PTS, Google Transition Prediction).**
  Meta PTS reports >95% of individual test failures and >99.9% of faulty changes—meaning up to 0.1%
  of broken changes pass silently. Under Decision 11, probabilistic skipping is forbidden; historical
  failure models may only *reorder* tests for faster reds (B11), never omit tests from C3's inventory.
- **R3. Silent flake muting, skipping, or automatic retry-until-green (Buildkite mute/skip, Trunk auto-retry).**
  Muting or retrying a flaky test until it passes converts real intermittent bugs (races, ordering
  faults) into false greens, violating C4 and §1.3's zero-retry rule. Flakes must fail, count against
  the budget, and be fixed or quarantined only under time-boxed owner exceptions (B10).
- **R4. Mutable sticky disks attached directly to build/test workspaces (Blacksmith, RunsOn EBS, Namespace/Buildkite last-writer-wins for build trees).**
  Reattaching mutable disks across jobs permits cross-PR state bleed, undeclared files surviving
  between builds, and stale-mtime false greens (Q16)—violating C2, C4, and Decision 8's PR-to-`main`
  isolation rule. Restrict persistent NVMe disks to content-addressed BuildKit layers (B03) or
  immutable read-only lower layers verified against `M_G` (B02).
- **R5. Mutable org-wide network-disk VM snapshots (WarpBuild snapshot runners).**
  45–60 s snapshot boot latency is ~150× slower than Velnor's ≤0.3 s goal, network-disk I/O
  degrades many-file trees (`target/`), and org-level snapshot sharing leaks state across repos.
- **R6. Cloud-VM scale-from-zero and per-job heavyweight VMs (RunsOn EC2, Depot EC2, Buildkite Elastic ASG, GitLab Fleeting).**
  Built for elastic cloud billing and hostile multi-tenancy; on Velnor's 5 owned bare-metal servers
  with trusted authors (Decision 8), cloud-VM lifecycles add boot/network-disk latency and discard
  local NVMe generations and shared host page cache.
- **R7. Full Bazel/Buck2 build-system migration and third-party merge-queue SaaS (BuildBuddy/EngFlow full RBE; Mergify/Trunk/Aviator/Graphite).**
  Rewriting five Cargo workspaces (~350k+ LOC) into Bazel/Buck2 carries severe maintenance and
  semantic-drift risk (already demonstrated by `blaze` failing 0/5 workspaces and missing a clippy
  `deny`); replacing GitHub's merge queue with third-party SaaS violates Decision 7.

## Where Velnor should lead

No system in the landscape combines **Cargo-native zero-false-green verification reuse**,
**content-proven copy-on-write build generations**, and a **dual-executor conformance oracle** on
stock GitHub workflows. To turn that architectural lead into an unassailable speed lead:

1. **Cryptographic cross-stage verification ledger (`ensure`) over stock Cargo and GitHub.**
   Fast GHA runners (Namespace, Blacksmith, Depot) only accelerate individual jobs; RBE systems
   (BuildBuddy, EngFlow) require abandoning Cargo; TAP/Sandcastle are proprietary monorepo stacks.
   Velnor's signed, two-witness verification records reuse work across pushes, PRs, merge groups,
   and digest-promoted releases. *Strengthen it:* add a production key-bound `CLOCK_REALTIME` shim
   (keeping `CLOCK_MONOTONIC` real and nightly A2 at +8d/+30d) so the 11 clock-reading files stop
   tainting exact-tree merge groups as C5, and fix the 79 root-climbing fixtures so per-root keys
   actually enrol ≥80% of test time.
2. **Soundness-proven warm Cargo generations (`M_G` + dep-info + cold A1 + canaries).**
   Unlike Namespace/Buildkite's best-effort NVMe volume forks, Velnor proves warm build inputs by
   content before trusting outputs. *Strengthen it:* pair B02's read-only `overlayfs`/EROFS lower
   layer (ending 14.29 GiB/clone page-cache duplication and full-tree re-hashing) with fixing §7.3
   step 3 so newly added files invalidate only their owning crate and reverse-dep closure instead
   of triggering a whole-root cold rebuild.
3. **Dual switchable executors (`velnor-native` + `velnor-official`) with a permanent differential oracle behind a capability-driven `Platform` trait.**
   No listed GHA runner pairs a native Rust executor with the official runner behind one verified
   interface and multi-backend trait (K8s, Docker, future microVMs/Apple containers). *Strengthen
   it:* ship B05's local Cache-v2 proxy before running the paired Native-vs-Official shootout
   (`O04`), so Official is tested on equal cache footing and Native's real marginal gain is proven
   with numbers rather than assumed.
4. **Zero-Actions-hop App verification gate (`Velnor / Verified`) with pre-push reuse.**
   Every third-party GHA runner still suffers GitHub Actions job-offer tails (226 s P90), acquire
   waits (13/27 s), and the skipped-job ruleset trap. Velnor's App gate eliminates them while
   keeping GitHub's UI and rulesets. *Strengthen it:* promote pre-push `velnor verify` (N07) ahead
   of stretch goals so agent pushes resolve by instant lookup, and pair B12's speculative bisection
   pre-warming with auto-merge to minimize GitHub's queue tail.
