# What to borrow from other runners: Claude Opus 5.5

| Field | Value |
|---|---|
| Model | Claude Opus 5.5 |
| Model id | `claude-opus-5-5` |
| Client | Claude Code 2.1.325 (Agent subagent) |
| Reasoning effort | high |
| Date / run | 2026-11-19 |
| Brief | landscape brief v1 (2026-11-19): the runner landscape dossier, the final design and the owner decisions |

This is the model's own analysis of what Velnor should borrow from, or avoid in, other well-known CI runners, based on the public landscape dossier, unedited except for formatting.

---

# Velnor against the CI landscape: what to borrow, what to refuse, where to lead

Panel member: Claude Opus 5.5 (`claude-opus-5-5`), 2026-11-19, brief `landscape-brief-v1.md`.
Read: the landscape dossier, the v3 architecture, owner decisions 1–24, the nine fastest-runner
critiques and their comparison. Tags: **[M]** measured, **[E]** estimated (arithmetic shown), **[T]**
target, all as the design uses them; "dossier §N" cites the landscape dossier; **[C]** read from
code at the cited path (the official runner is at `refs/actions-runner`, commit `7afce9e`);
"(from my knowledge)" marks a fact that is not in the dossier and that I have not verified here.
Design section numbers (§) refer to `docs/proposed/velnor-runner-architecture.md`.

## Verdict

- **Hosted fast-runner vendors** (Blacksmith, Depot, Namespace, WarpBuild, RunsOn, Ubicloud): Velnor is ahead on what it trusts. Generations come only from audited builds, not from "commit on exit 0, last writer wins". Checkouts are verified against the tree, and verdicts are reused by content. Velnor is behind on clock speed per thread (a 9454P against a 5.7 GHz Ryzen), on page cache shared between clones, on snapshot restore, and on measured numbers: Velnor has none end to end.
- **Agent fleets** (Buildkite, GitLab): equal on warm local state and ahead on keys and receipts. Behind on mature pool sizing, on git-mirror concurrency rules, and on a test engine (timing splits, flake detection).
- **REAPI systems** (Bazel, BuildBuddy, EngFlow): ahead on trust. Velnor's receipts are observed by the node agent and signed, need two witnesses and a cold audit, while the REAPI action cache accepts results that clients upload. Behind on granularity: REAPI keys each action, while Velnor keys mostly the whole tree until roots are enrolled. Also behind on fan-out.
- **Big-tech CI** (TAP, Sandcastle, SubmitQueue): ahead on the per-commit guarantee, because Velnor does no optimistic submission. Behind on how precisely it selects tests, which is acceptable at five nodes.
- **Merge-queue tools** (Trunk, Mergify, Aviator, Graphite): decision 7 ties Velnor to GitHub's queue, which has no parallel queues and no native bisection. But the ledger makes verifying each prefix nearly free, so Velnor needs neither bisection nor batches that verify only the tip.

## Ideas to borrow

| id | Technique | Who does it | What changes in Velnor (§) | Expected gain | Correctness argument | Fit | Agent-h |
|---|---|---|---|---|---|---|---|
| B01 | Pool sized by demand: a scheduled floor plus demand the plan predicts; webhook start | Depot standby pool, RunsOn warm pools, GitLab `idle_count`/`idle_time`, Buildkite floor + queue-depth scaler, Actuated | §5.4 `provision`, §9: per-class floor by time of day, topped up from `ensure`'s obligation vector and from auto-merge enqueues; pool members never hold build-class memory | ARC pod about 6 s plus dind probe 3–4 s [M] → claim ≤ 0.3 s [T]; about 5.7 s off each sandbox stage on the critical path | Single-use; the pod-spec digest is in the execution fingerprint and is checked at claim; a stale member is drained, never reused | fits | 12 |
| B02 | Per-node generation lineage, background spread, reset when a lineage outgrows its size | Namespace and Buildkite cache volumes (only the parts that are not "commit on exit 0") | §7.3, §7.5: keep reflink from sealed generations; add Namespace's lineage size reset and background spread of trusted generations | Keeps the 0.25 s reflink [M] against 8.6 s ext4 copy [M] inside the 40% budget; no new speed | Generations still come only from A1-qualified trusted builds; the background spread carries warmth only | fits | 4 |
| B03 | Job follows the disk: placement aware of warm state, in place of sticky disks | Blacksmith sticky disks, RunsOn EBS sticky disks, Jupiter worker traits (Sandcastle) | §5.5 step 3, §9: nodes advertise the generations, L0 manifests and mirror freshness they hold; the dispatcher scores placement | Warm ≤ 25 s [T] against 92 s cold [M]; if a generation sits on 2 of 5 nodes, random placement hits 40% of the time and scored placement nearly always [E] | Affects only speed; the rules for distinct nodes and distinct generations are unchanged; no writable disk moves between jobs | fits | 8 |
| B04 | Generation shipping in content-addressed chunks, fetching only missing chunks | BuildBuddy chunked CoW snapshots, REAPI `FindMissingBlobs` | §7.1, §7.3: a generation becomes a signed chunk manifest; the receiver fetches only the chunks its node store lacks, from L1 | 10.6 GiB target [M]; if consecutive tips differ by 5–15% [E, unmeasured], that is 0.5–1.6 GiB, a few seconds on the LAN, against 92 s cold | Chunks are hash-checked; the rebuilt generation's manifest digest must match; the trust scope is carried over, never upgraded | fits (decision 23; after ST06) | 16 |
| B05 | Snapshot and restore of the whole sandbox | BuildBuddy Firecracker + UFFD, WarpBuild snapshot runners | §5.4 already has `snapshot_restore`; build it only with a microVM backend; meanwhile get the page-cache part through a read-only overlay lower layer (lab B2) | The real prize is page cache: 14.29 GB re-read per clone [M]; nothing extra now | If built: the snapshot digest goes into the fingerprint, and snapshots are taken only by the builder class, never by a job | fits as deferred (decision 21) | 0 now; about 40 later |
| B06 | Cache proxy for stock actions on the GitHub Official executor; action-archive cache | Depot (proxy), Ubicloud (runner fork), RunsOn Magic Cache, Blacksmith | §7.2 "Open": rewrite `CacheServerUrl`/`ResultsServiceUrl` in the job message given to `Runner.Worker`; the node agent serves cache v2 and passes other Results calls through to GitHub; set `ACTIONS_RUNNER_ACTION_ARCHIVE_CACHE` | 1.92 GB restore 44–61 s [M] → L0 clone of 0.25–5 s [M]; action downloads 4.4 s per job [M] → about 0 | Same index, scope and order as Native; production role only; the reference role keeps GitHub's cache; a cache failure is a miss | fits (decision 22's open item) | 24 |
| B07 | Co-located caches with parallel range reads | Blacksmith, Depot (8 down / 4 up streams), Ubicloud, Namespace | §7.1: L0 already sits next to the job; L1 reads split into parallel ranges; "stale rather than a miss" only for generations | Depot claims 1 GB/s against 100–150 MB/s (dossier §3); L1 miss of 1.92 GB: about 17 s on one stream at 110 MB/s → about 3–5 s [E, NIC unverified] | Ranges are hash-checked per chunk; `actions/cache` resolution stays exact (no stale prefix masks an exact entry) | fits | 4 |
| B08 | Higher clock speed for the serial floor | Blacksmith (7950X, 5.7 GHz), Ubicloud Premium (Ryzen) | §9: now, tune and pin the merge slot's serial stages and measure (K11); later, one high-clock node per site for witness 1 on the merge path, links and the longest tests | 1.3–1.5× serial [E, I104]: 15 s warm compile → 10–11.5 s; 200 s slow class → 135–155 s | CPU features are in the fingerprint; a different CPU makes the two witnesses more independent, not less | needs an owner question (purchase) | 6 measure + 8 onboard |
| B09 | REAPI ideas: presence checks that extend blob lifetimes, output manifests, input roots from Git subtree ids | REAPI, BuildBuddy, EngFlow | §6.1, §6.2, §7.1: every blob a record names is checked present and its lease extended before the record commits; records carry digests of outputs and logs; enrolled keys use subtree ids | 1.48 GB archive upload 36–43 s [M] → only missing blobs; eviction cannot leave a record whose bytes are gone | Content addressing; a record never names a blob it has not confirmed present; only receipts the node agent observed fill a record (no client writes) | fits | 12 |
| B10 | Test splitting by measured timings, longest first | CircleCI `--split-by=timings`, Buildkite bktec | §6.2, §8.5: the layout comes from per-test durations in past receipts and is recorded in the manifest; run lab C first | 679 s suite at 4 shards → about 170–250 s [E]; bktec example 10 → 4 min (dossier §1) | Timings only set the layout; the union of shard IDs must equal the expected inventory exactly (C3) | fits | 10 |
| B11 | Flake detection from paired observations Velnor already makes; quarantine only through a signed reduction | bktec mute/skip, Trunk anti-flake, TAP | §1.3, §6.5: score each test ID from witness pairs, A1 and 2% samples; a quarantined test keeps running in a lane that does not block | Detection needs no extra runs (3 or more observations per new key); turns the 8.1% [M] into named tests | No automatic mute; removal needs an owner-signed entry (D17); the test is still observed | needs an owner question (D17) | 8 |
| B12 | Test impact analysis that stays sound; ML used only for ordering | TAP (dependency graph), Meta Predictive Test Selection, Google Transition Prediction | §6.1: impact analysis is the enrolled per-root and per-package keys; a non-minting fast-red lane runs the tests a history model ranks as likely to fail first | Meta PTS catches over 99.9% of faulty changes with half the test cost (dossier §18), so as an order it should bring most first reds into the first half of the test stage [E] | The order changes no verdict; green still needs every expected ID from two witnesses | fits | 10 |
| B13 | Merge-queue batching, culprit finding by prefix, landing the exact tested commit | GitHub MQ, Mergify, Trunk, Aviator, Graphite, TAP | §8.3: raise depth and merge limit by the existing gate; each prefix reuses records for unchanged keys; the failing prefix names the culprit, so no bisection runs | A batch of k costs about C5 + changed keys per entry, not k full runs [E]; exact-tree reuse 16% → 60–75% [E, replay] | Every prefix is verified under ALLGREEN; GitHub lands the group SHA that was tested | fits (decision 7) | 6 |
| B14 | Optimistic merge with asynchronous full tests | TAP, Trunk "optimistic merging" | none: rejected | TAP waits about 11 min to submit (dossier §17); removing verification from push-to-merge would save the whole verdict time | **Cannot hold zero false greens**: a commit lands before its own tree passes | **conflicts with decisions 11 and 12** | 0 |
| B15 | Git mirror locking and safe pruning | Buildkite agent git mirrors (file lock, fetch by ref) | §7.4: one writer per mirror; fetches merged (one running, one waiting); explicit refs fetched into a staging namespace and moved in one `update-ref` transaction; no pruning; refresh means a new mirror swapped in | Removes "cannot lock ref" failures and the fallback fetches from GitHub they cause; GitHub traffic per push stays once per node | The full tree hash already blocks a wrong tree; this protects obligations that read `.git` from missing objects | fits | 6 |
| B16 | Preparing on unlabelled pushes at idle priority | Depot/RunsOn standby behaviour, Uber SubmitQueue (speculation) | §6.2: fetch and prefetch on every push (decision 24 already fetches); compile the closure under `cpu.idle` without minting anything | 15–25 s compile [E] taken out of label-to-verdict (G03); 45% of pushes superseded [M] wastes idle CPU only | Mints no receipt; a compile output must match by digest before any witness uses it | needs an owner question (decision 12) | 8 |
| B17 | Persistent BuildKit builders with local layer cache, separated by scope | Depot, Buildkite remote builders, WarpBuild, Namespace, CircleCI DLC | §5.8 builder class, §7.4 registry mirror: one builder per site and scope; PR builds never write the trusted layer cache | Image candidate 200 s per push [M] → once per image-input key, then incremental [E] | The image digest is in every key that uses it; trusted images are built in the builder class only | fits | 12 |

**B01, pool sized by demand.** GitLab keeps `idle_count` instances for `idle_time`. Buildkite keeps a
scheduled floor and scales on queue depth. Velnor knows more than either: when the label arrives, `ensure`
derives the obligation vector, so it knows how many sandboxes of which class the head needs before one is
claimed. With auto-merge, an enqueue predicts a `merge_group` about 19 s [M] before GitHub sends it. Size
the pool as a scheduled floor per class plus this forecast. The one constraint the vendors do not have: a
build sandbox reserves 24 GiB and a worker fits four (§9), so an idle reserved build pod costs 25% of a
worker's build capacity. Pool members must hold little memory, and the sandbox takes its build size only at
claim. That needs in-place pod resize (beta in Kubernetes 1.33, from my knowledge; k3s version unchecked) or
slots the node agent owns (I082). Lab P04/K17 picks between them, and O27 (no new host mounts into a running
pod) means the work volume must be bound before claim. The gain arithmetic uses the 436 measured ARC starts.

**B02, forked volumes against reflink.** Namespace and Buildkite fork a per-run volume from the last
committed generation and commit only on exit 0, last writer wins (dossier §1, §4). Velnor's reflink from a
sealed generation is the same mechanism with a stricter source of trust, and it should stay. Two details are
worth taking. First, Namespace resets the lineage when a generation outgrows its configured size, which
keeps §7.5's 40% budget without a separate garbage-collection pass. Second, Namespace commits on one node
and spreads the result in the background, which is B04 under ST06's gate. Nothing in a job's exit decides
what becomes a base.

**B03, the job follows the disk, not the other way.** Sticky disks move state to wherever a job lands.
Jupiter instead matches jobs to workers that advertise traits such as "preloaded repositories" (dossier
§18). On five fixed nodes, matching is better than moving: nodes advertise the generation ids, L0 cache
manifests and mirror heads they hold, and the dispatcher scores placement by (state held, free slot, site).
Witness 2 must still run on a different node and over a different generation, so placement only picks
*which* holder. A writable disk that survives between jobs is rejected (see the rejections below).

**B04, chunked content-addressed generation shipping.** BuildBuddy stores snapshot disks as chunks addressed
by content, cached on the host and shared across VMs (dossier §14). REAPI clients call `FindMissingBlobs`
before uploading (dossier §16). The same pattern fits generation shipping, which decision 23 already lists
as a data class. Generations of consecutive `main` tips share almost every third-party `rlib`, so a receiver
lacking G_{n+1} but holding G_n fetches only the delta. Use large chunks (4–16 MiB): RustFS managed only
27–70 one-MiB PUTs a second on the Mac test [M], and small objects are its weak point. The 5–15% delta is my
guess; lab B3 measures it. Until ST06 passes, routes stay `local`, as the design says.

**B05, whole-sandbox snapshot restore.** WarpBuild snapshots a VM that a job saved; BuildBuddy resumes
Firecracker snapshots in hundreds of milliseconds, with memory restored lazily (dossier §5, §14). Most of
what such a snapshot holds (toolchain, `target/`, mirror) Velnor already gets from reflink and read-only
stores. The part it lacks is the warm page cache: 14.29 GB per fully read clone [M], which caps a node at
four to six build sandboxes. A shared read-only overlay lower layer (lab B2, G10) gets that on Kubernetes
and Docker without a VM. Keep `snapshot_restore` in the capability set (decision 21) and build it with the
first microVM backend.

**B06, cache proxy for the GitHub Official executor.** Depot runs a proxy on the runner that speaks GitHub's
cache API; Ubicloud forked the runner (dossier §3, §8). Velnor must not fork: decisions 19 and 20 require
the latest official runner. It does not need to, because the Worker reads the cache and Results URLs from
the job message's system connection (`CacheServerUrl`, `ResultsServiceUrl`,
`NodeScriptActionHandler.cs:58-74` [C]), and Velnor builds that message. Three cautions:
- `ResultsServiceUrl` also serves artifacts v4, so the endpoint must pass every non-cache Twirp method
  through to GitHub unchanged.
- With cache v2, `@actions/cache` uploads to the signed URL with Azure Blob block calls (from my knowledge),
  so the node agent must speak that subset of the Azure Blob protocol, not S3.
- The reference role must keep GitHub's real cache, or the oracle would be checking Velnor against itself.

Separately, the Worker uses unpacked actions from `ACTIONS_RUNNER_ACTION_ARCHIVE_CACHE`, keyed by owner/name
and resolved SHA (`ActionManager.cs:1232`, `Constants.cs:293` [C]). A read-only store of the pinned action
SHAs removes the 4.4 s of downloads per job [M]. This is the cheapest large win for the Official executor,
and decision 20 asks for numbers that compare the two executors on equal terms.

**B07, co-located caches.** The design already does what Blacksmith, Depot and Namespace sell: the cache
sits on the node's NVMe, unpacked once and cloned per job. Two details are worth taking. Depot downloads on
8 parallel streams. Namespace prefers a stale cache to a miss so that a job never waits at start (dossier
§4). The first applies to every L1 read. The second applies **only to generations**: for `actions/cache`,
GitHub's semantics fix which entry `restore-keys` returns and what `cache-matched-key` reports, so a stale
entry would be a conformance divergence. §7.2 is right to resolve the manifest through the authoritative
index before reading any tier.

**B08, high clock speed.** Blacksmith's whole pitch is a 5.7 GHz Ryzen 7950X with local NVMe, about 2×
single-thread over hosted runners (dossier §2). The EPYC 9454P boosts to about 3.8 GHz (from my knowledge).
Hardware is fixed for now, so the free step is to measure (K11): cold build of the runner root and the 20
slowest tests, with the `performance` governor, against the default governor. When buying is allowed, one
high-clock node per site should carry the merge path's witness 1, links and the longest tests. Keys stay
sound because the fingerprint includes CPU features. A side benefit: witnesses on two CPU types are more
independent than two identical nodes, which bears on lab C2.

**B09, REAPI against the ledger.** Velnor's ledger is already an action cache with a stricter writer: the
definition digest maps to the REAPI command, the Git tree to the input root, the execution fingerprint to
platform properties, C5 to `do_not_cache`, and the epoch to `salt`. Three things are missing:
- **Lifetimes.** REAPI servers extend the lifetime of blobs a client finds present. Velnor should check and
  extend every blob a record names (test binaries, spawned executables, artifacts) before the record
  commits, so eviction never leaves A1 or 2% sampling unable to compare digests.
- **Outputs.** A record should carry digests of its logs and JUnit output, so a reused verdict can show its
  evidence.
- **Input roots.** An enrolled contract's key should be the Git subtree ids it covers, which costs no hashing.

What not to copy: `UpdateActionResult` from clients (see the rejections).

**B10, splitting by timing.** CircleCI and bktec split by the timings of past runs; bktec's example cuts 16
minutes of tests to about 4 across 4 partitions (dossier §1, §13). Velnor has better inputs: exact test IDs
per obligation (C3) and per-test durations in signed receipts. Lay tests out longest first, record the
layout in the execution manifest, and require the union of shard IDs to equal the inventory with no
duplicates. Timing data cannot affect a verdict, so even timings from a PR are safe to use. Two witnesses
with different layouts also run in different orders, which is the order difference §6.2 cannot yet claim
with nextest 0.9.151. The floor is the longest indivisible test (924.8 s and 679 s today), so this
complements the test-floor programme and does not replace it.

**B11, flakes.** Buildkite mutes (runs a test but ignores the result) or skips; Trunk has anti-flake
protection; TAP removes flaky tests from presubmit (dossier §1, §17, §20.4). Velnor gets something none of
them has for free: every new key runs on two witnesses, then A1, then a 2% sample, so the ledger holds
paired outcomes for identical keys with no extra runs. Score each test ID from those pairs and open the
issue the design already requires. A mute is a false-green channel: a muted test that would catch a real
regression passes. So a quarantine is an owner-signed inventory reduction with an expiry (D17), and the test
keeps running in a lane that does not block, so its rate and its fix stay visible.

**B12, test impact analysis and ML.** TAP's dependency-graph selection is what Velnor's enrolled per-root
and per-package keys do, with a stronger guard: a read audit enforced on every execution. That is the sound
form, and it is in the design. Meta's Predictive Test Selection reports over 95% of test failures and over
99.9% of faulty changes (dossier §18). Applied as selection, the remaining 0.1% of faulty changes would go
green, so it is rejected for gating. As an *order* it is free: the fast-red lane (G05) runs the tests a
simple co-failure model from the ledger ranks highest first, which shortens time to the first red, and green
still requires the full inventory.

**B13, merge queue: batching, bisection, fast-forward.** Mergify, Trunk and Aviator bisect a failing batch
because they tested only its tip (dossier §20). Under ALLGREEN every entry is verified on its own prefix
tree. Velnor resolves each prefix's vector against shared records, so a prefix costs its C5 set and its
changed keys, and the first failing prefix *is* the culprit, with no bisection runs. "Fast-forward" in
Aviator's and Graphite's sense (skip CI when the tree was already tested) is the design's exact-tree reuse.
GitHub lands the group SHA it tested, so the landed commit is the verified one. Keep §8.3's gate for depth:
4 only once group P90 ≤ 5 min and paired false failures ≤ 3%. SubmitQueue-style ranking of speculative trees
by pass probability is worth considering only well above today's 2.44 entries an hour.

**B14, optimistic merge.** TAP submits on a fast presubmit subset and runs the rest afterwards; Trunk's
optimistic merging lands entries ahead of a later passing one (dossier §17, §20.4). Both land commits whose
own tree has not passed every obligation, which breaks C1 and decision 12's "merging still requires a
passing verdict on the exact merged tree". The gain is real (TAP's average wait is about 11 min to submit)
but not available at zero false greens. It is listed here because the brief asks; it goes in the rejections.

**B15, git mirror locking.** Buildkite uses one bare mirror per repository per host, a file lock so only one
agent updates it, and `git fetch origin <branch>` so that maintenance never prunes objects that reference
clones need (dossier §1). Velnor's push webhooks will fire fetches on five nodes for every push, in bursts.
So:
- Allow one writer per mirror (`flock`), with at most one fetch running and one queued that takes in later
  requests.
- Fetch explicit refs into `refs/velnor/staging/*`, then move them in one `git update-ref --stdin`
  transaction.
- Set `gc.pruneExpire=never` on mirrors, and rebuild a mirror as a new directory that is swapped in, like
  the sealed bases.

The full tree hash already protects ordinary obligations; this protects the `.git`-needing ones
(`root.rs:22`) from a pruned alternate object, where an error a test tolerates could otherwise pass.

**B16, preparing before the label.** Standby fleets prepare before demand arrives. Fetching and cache
prefetch on every push fit decision 12 as written: nothing is verified, and decision 24 already fetches PR
head refs. A speculative compile at `cpu.idle` mints nothing and takes 15–25 s [E] out of label-to-verdict.
It still spends CPU on pushes the owner said should start nothing, and 45% of PR runs were cancelled [M], so
it is the owner's call.

**B17, persistent image builders.** Depot, Buildkite, WarpBuild and Namespace keep remote BuildKit builders
with local layer caches, and Depot gives untrusted PRs no cache (dossier §3). For Velnor, that is one
builder per site *per scope*: a PR builder never writes the trusted layer cache. BuildKit keys `RUN` layers
by command string, not by what the command fetched (from my knowledge), so a trusted image comes only from
the builder class, with its inputs pinned by digest, and its digest enters every key that uses it. The image
candidate (200 s per push [M]) then runs once per image-input key, as §4 row 19 intends.

## Ideas to reject

- **Optimistic merge and post-merge full testing** (TAP, Trunk). A commit lands before its tree passes,
  which breaks C1 and decisions 11 and 12. Automatic rollback (TAP) does not repair a false green that
  already reached `main`.
- **Batches verified only at the tip, or HEADGREEN** (GitHub's "only merge non-failing" off, Trunk/Mergify
  tip-only batches). A later PR can mask an earlier failure; three such commits already sit on `main` [M].
  ALLGREEN stays.
- **ML or predictive test selection for gating** (Meta PTS, Transition Prediction). Their own figures leave
  about 5% of individual test failures and up to 0.1% of faulty changes uncaught (dossier §18). Use it to
  order tests only (B12).
- **Automatic mute or skip of flaky tests** (bktec) and **automatic reruns until a pass** (CircleCI "Rerun
  Failed Tests" used as a gate). Both turn an intermittent real regression green. Retries stay at 0; a
  quarantine is an owner-signed reduction (B11).
- **Writable shared volumes that commit on exit 0, last writer wins, as trusted state** (Namespace and
  Buildkite cache volumes, Blacksmith and RunsOn sticky disks). A PR's state would reach the next `main`
  job, against decision 8's rule. Velnor's generations come only from the builder after A1.
- **Snapshots saved by jobs as bases** (WarpBuild `snapshot-save`; its docs warn that snapshot data is
  exposed across the org). Their content is whatever the job left behind.
- **Results uploaded by clients into the action cache** (REAPI `UpdateActionResult` from clients; Bazel
  uploading local results, from my knowledge). This breaks C4: only receipts the node agent observed count.
  A developer Mac's run never fills a record.
- **"Stale rather than a miss" for `actions/cache`** (Namespace). It changes `cache-matched-key` and breaks
  conformance. Allowed only for generations (B07).
- **Forking the official runner to point it at a proxy** (Ubicloud). This breaks decisions 19 and 20 (the
  latest official runner, the oracle role). Rewriting the job message gets the same result (B06).
- **A merge train owned by Velnor in place of GitHub's queue** (N31; Trunk/Aviator-style engines). It saves
  about 45 s (19 s + 26 s [M]) but conflicts with decision 7. Report push-to-merge net of GitHub's share
  instead.
- **Incremental compilation for minting witnesses** (G14). Ruling Q17 stands; incremental builds belong only
  in the non-minting fast-red lane.
- **Spot capacity, elastic cloud and hosted offload** (RunsOn spot, Buildkite ASG with a Lambda scaler,
  vendor runners). Decision 9 runs every job on Velnor, and the hardware is fixed. Elastic scaling has
  nothing to scale on five owned nodes.
- **Firecracker microVM per job now** (Blacksmith, Actuated, BuildBuddy). Decision 8 needs no hostile-tenant
  isolation, and decision 21 says design for it, build two backends. Defer it; do not reject it.

## Where Velnor should lead

No system in the dossier combines these. Each item says how to make it stronger.

1. **Evidence the node agent observed, two witnesses and a cold audit of every landed commit.** REAPI trusts
   any authorised writer, the vendors' volumes trust an exit code, and TAP and Trunk trust probability.
   Velnor alone makes "verified" mean "seen executing, twice, then rebuilt cold". *Stronger:*
   - Run the planted-failure campaign (K07) continuously as canaries, not once.
   - Publish A1's disagreement count per reuse class as a standing metric.
   - Bind the sandbox profile digest and the read-audit summary into every receipt.
2. **Verdicts reused by content, from PR to queue to `main` to release.** Graphite and Aviator skip CI only
   when a tree is identical; Velnor reuses per obligation and promotes release bytes by digest. *Stronger:*
   make the release attestation cite the ledger records that verified those bytes (provenance of the
   verification, not just the build), and replace the 60–75% model with lab C1's measured rate.
3. **A differential oracle built into the product.** Every vendor runs the official runner as is; Velnor
   reimplements it and runs the official runner beside it as a reference, in conformance, shadow and
   on-demand modes. *Stronger:*
   - Report the per-job-type speed difference that decision 20 requires, from paired runs with B06 applied,
     so the comparison is fair.
   - Keep the 95% mutant-kill bar.
4. **Fallback by capability: "slower, never wrong".** Vendors pick one isolation and one store. Velnor lets
   each backend declare what it can do and falls back to a slower correct path when it cannot. *Stronger:*
   make every declared capability a conformance case. A backend that claims `reflink` or `read_audit` must
   pass a test that would fail if the claim were false, so a wrong declaration cannot enable an unsafe path.
5. **CI that runs only on request, with a status only a fenced writer can make green.** No vendor removes
   the push-triggered run; Velnor's label flow, together with permanent canary PRs that must stay
   unmergeable, makes "not requested" provably blocking. *Stronger:* model the label and status state
   machine (section 8.2's table) in TLA+ and check the stale-write and draft cases before lab A, not after.
6. **Flake knowledge from the witnesses** (B11). Two witnesses per key produce paired outcomes that systems
   running a test once cannot have. *Stronger:* publish per-test false-failure rates from that data; it is
   the only honest input to the 3% / 1% budget.
7. **Checkouts verified against the tree.** Buildkite and Namespace trust their mirror or cache volume;
   Velnor hashes the staged tree before the first step. *Stronger:* seal bases with fs-verity and hash only
   the delta (G09), so the check costs milliseconds and can never be skipped for speed.
