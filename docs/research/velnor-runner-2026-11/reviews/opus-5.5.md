# Review by Claude Opus 5.5

| Field | Value |
|---|---|
| Model | Claude Opus 5.5 |
| Model id | `claude-opus-5-5` |
| Client | Claude Code 2.1.325 (Agent subagent) |
| Reasoning effort | high |
| Date / run | 2026-11-19 |
| Brief | v1 review brief (2026-11-19): critique 3 anonymised proposals; same evidence pack |

This is the model's own cross-critique of anonymised proposals, unedited except for formatting. The key at the end maps the letters to models; the reviewer did not see it.

---

# Cross-critique of proposals A, B and D

Checked against `evidence.md`, velnor-new @ 95de9b704b, `~/.cache/ci-ambition/refs/{actions-runner,arc}`, the owner's `velnor-runners` docs, and read-only `gh api` GETs (issue comments 6856187529 and 6874232403, PR timelines for #1271 and #1276, and `orgs/tailrocks .plan.name`). I made no writes, pushes, or cluster calls.

Four facts I verified matter for all three proposals:

- **F1. The sources cache miss comes from the cache version, not the key.** `ci.yml:337` restores `${{ runner.temp }}/velnor/cargo/...`. That expands to `/home/runner/work/_temp` on hosted runners and `/home/runner/_work/_temp` on ARC pods. The GitHub cache lookup filters on `entry.version == version` before it compares keys (`local/crates/velnor-cache-rules/src/lookup.rs:28-31`), and that version hashes the path strings. The state compat hash has the same flaw: `store=` and `checkout=` and `ImageOS=` are emitted double-quoted, so the shell expands them (`crates/velnor-actions-contract/src/workflow/cache_family/state.rs:224-226`, `crates/velnor-actions-workflow-renderer/src/state_fragments.rs:15-21`).
- **F2. The org runs on the GitHub Free plan.** `gh api orgs/tailrocks` returns `plan.name = "free"`. Issue comment 6856187529 says the plan shares 20 concurrent hosted jobs. On 11-18, Job image candidate (`push: {}`, every branch) created 306 runs and Source qualification (every PR, 90-minute jobs) created 267, all on hosted runners (evidence T11). Both compete with the CI gates for those 20 slots.
- **F3. Velnor cannot choose which runner gets a job.** A JobAvailable message carries only repo, workflow ref, job display name and labels (evidence F.1.3, `S/types.go:21-57`). GitHub's broker delivers the RunnerJobRequest to a registered runner of its own choosing (`Runner.cs:699-780`). Velnor-side "assign job X to warm node Y" therefore does not exist with a pool of idle runners. Only D states this.
- **F4. Two single tests set a floor near 4 minutes.** `public_authority_rejections_preserve_registry_inputs_and_foreign_resources` loops over 40 cases serially (`crates/velnor-actions-cli/tests/image_publication_public.rs:86-92`). `an_initializer_that_was_not_proven_absent_is_not_claimed` loops over 6 states (`.../unbound_retirement/tests/shape.rs:97-106`). D's logs time them at about 261 s and 234 s. `nextest --partition` cannot split a single test.

## Proposal A

### 1. Weakest assumptions

1. **The listener can place each job on a warm pod (§3.2 Layer A, §4.3).** F3 rules this out. With 12–16 idle registered runners per org (§6.1), GitHub decides placement, and preferred node affinity applies only when a pod is created, not when a job is assigned. Every hit-rate number in §4.5 depends on placement. To settle it, register 2 JIT runners on different nodes in velnor-lab, enqueue 20 labelled jobs, and record which runner gets each job. Workable alternatives are per-cache-family scale sets or labels emitted by the generator (for example one per Cargo root), or registering a JIT runner only after acquisition on the chosen node, which pays the pod-start cost.
2. **Reusing a pod for the next job keeps confidence (§6.1).** A writes that "isolation comes from one job per worker process and a fresh `_work` dir, not from pod death". The DinD image graph, networks, `$HOME` (`~/.cargo/config.toml`), `/tmp` and orphaned background processes all survive into the next job. Legacy shows the failure class: #275 "persistent mise store poisoned by interrupted cargo compile" (evidence E.6). This makes confidence lower than today's ephemeral pod. To settle it, run a hostile job that leaves a daemon, a tagged Docker image and a cargo config behind, then check whether the next job observes any of them.
3. **mbx gives 85–95% cross-job hits and "a stale hit cannot produce a wrong build" (§2 H4, §4.4, §7.2).** The #585 results (comment 6856187529) cut against this:
   - On velnor-new (draft #673), mbx with save-on-PR compiled 46 crates warm. The warm run took 345 s against 291 s cold, because restore took 44–61 s.
   - On the lab, every mbx variant recompiled 10–68 crates on a warm run because `OUT_DIR` was not carried (`StaleItem(MissingFile)`). Plain cargo on a persistent target compiled 0.
   - mbx's own compilation counts matched cargo's log in only 1 of 163 runs.

   A's "Cache check" proof (§4.4) reads exactly those counts. The correctness claim is asserted, not shown, while `MBX_SHARE_OUT_DIR=1` and cached build-script outputs are in play. To settle it, A/B test a node-persistent target directory against an mbx store on velnor-new itself, with a cold-vs-warm differential build check.
4. **A typical PR finishes in 4–5 minutes (§5.5).** The table assumes "one or two crates in one root" and a 1.0-minute shard tail. F4 sets a test floor of roughly 4 minutes for any PR that selects the CLI or runner-host. D found that 33 of the last 100 first-parent changes touched only the nested runner. The maintenance-helpers nextest step alone takes 212–246 s (D, job 134191252252), and A leaves that job out of the critical path.

### 2. Factual errors

- **Sources-miss diagnosis is wrong (§1.4, §4.2 Tier 2).** A says the "compat hash mixes platform identity, so hosted producers never match". D found that the missing key matched Plan's successful key exactly in 48 of 48 jobs. The real cause is F1, the path-derived version. A's fix, splitting the key into content and platform, would leave the miss in place.
- **The mold premise is wrong (§8a rank 3).** A assumes Rust uses the "host default linker / `cc`". Rust 1.90+ links `x86_64-unknown-linux-gnu` with bundled rust-lld by default, and the pin is 1.99.1. B and D both say so. The evidence pack's "default cc" is an inference, and A copied it. The expected gain is smaller than A claims.
- **Maintenance-helpers estimate breaks A's own rule (§2 H4, §4.5).** A predicts "486 s to 60–120 s" while also stating that caching cannot remove test time. That job's nextest step took 245.9 s for 54 tests (D).
- **Toolchain cost is overstated.** A says toolchain install costs "~30–60 s of per-job install", but `Prepare pinned tools`, which includes `mise install rust`, has a median of 10 s (evidence T9b). A cites that 10 s figure in the same paragraph.
- **Disk headroom is measured on the wrong filesystem (§8a "What more hardware buys", §8a rank 2).** The "44–110 GB of 3.78 TB" figure is nodefs. nodefs is the OS disk with an ext4 root (`velnor-runners/PLAN.md:42-45`, `/:ext4:all`), not the XFS data volume. All emptyDir scratch lives on that one OS NVMe. A treats the XFS volume as the place where jobs already run.
- **Minor:** the setup tax uses ×39 jobs, but the median PR executes about 25 (D).

### 3. Missed bottlenecks and risks

- **F2.** A never mentions the 20-job Free-plan limit or the two non-CI workflows that saturate it. After Phase 0, release jobs, state-seal and Source qualification still queue behind Job image candidate.
- **Merge-queue throughput.** The ruleset sets `max_entries_to_build 2`. PR #1271 entered the queue at 07:49Z on 11-18, and its merge_group run was created at 14:56Z (`gh api .../issues/1271/timeline`). 35 of 150 PRs entered the queue more than once (T7). A optimises time-to-verdict and says nothing about push-to-merge, which is the brief's metric.
- **Cold-path slowdown.** Today one pod can burst to 84 cores. A caps build pods at 8 Guaranteed CPUs (§6.2), so lock bumps, new nodes and evictions compile 481 crates at 8-way parallelism. Cold tails get worse while warm medians improve, and A gives no cold-path number.
- **Cancelled PR runs.** 45% of PR runs are cancelled (T1). Their minutes are excluded from the samples, and they still take cluster slots during fan-out.

### 4. Ideas worth taking

- Phase 0 as a measured, no-code step with explicit numeric exit criteria and per-phase rollback (§8).
- A short push workflow (seal, freshness, publish) in place of a full re-run on the same SHA (§5.2). Name the always-full events in config.
- Invariants: no caching of test outcomes (§4.1), and executed-test count per root equal to the unsharded count (§5.3, Phase 3).
- The limiter sanity check: "a claim of 10 s would mean the tests did not run" (§4.5).
- A three-tier cache (node, same site, GitHub only as fallback), with no synchronous cross-site reads (§4.2).
- The blaze analysis (§7.4–7.5), which is complete and correct.
- A native fast lane restricted to the `velnor-gen` profile, admitted by shadow parity, which can never block a merge (§3.4).
- PriorityClasses where only producers are preemptible, plus per-org cluster caps (§6.5).

### 5. Verdict

A is the most balanced design document, but it would not reach the minimum at the same confidence as written. Its warm-placement mechanism relies on a job-to-runner binding that GitHub does not provide. Its pod-reuse pool weakens isolation below today's level. Its headline cache numbers rest on mbx behaviour that the repo's own #585 data contradicts. **Biggest flaw:** the whole H4 payoff runs through "Layer A assigns the job to the warm pod", and the scale-set protocol does not let Layer A do that.

## Proposal B

### 1. Weakest assumptions

1. **Plain cargo over reflinked `target/` clones compiles 0 unchanged crates on velnor-new (§4.4, §7).** The 0-crate result is from the 5-crate lab. The same comment says "Plain cargo is unverified at velnor-new's size." Cargo fingerprints depend on mtimes, absolute paths, `OUT_DIR` and env-sensitive build scripts. Five workspaces, local-mode and lint/doc lanes all have to keep stable fingerprints across checkouts. To settle it, run the lab's warm/leaf/hub matrix on velnor-new with node-local clones and count `Compiling`/`Checking` lines.
2. **The coordinator can place jobs on warm nodes and CCD slots (§3, §4 score function).** F3 applies here too. A native worker does not fix it either, because the broker hands the RunnerJobRequest to whichever registered runner GitHub picks. The `score(n)` formula has nothing to act on unless each registered runner is unique to one node at assignment time.
3. **Sub-minute verdicts (Summary, §5 table, Phase 2–4 targets of ≤1.5 min, ≤50 s and ≤38 s).** F4 contradicts the 25–38 s slowest shard for runner-host PRs. The suite fixes are assumed to land: #1333 and #1349 are open, and "50–60 s per test drops to 3–6 s" is an estimate. "~70% of PRs are leaf" has no source, and D's count (33 of 100 changes runner-only) contradicts it.
4. **Rootless namespaces in under 50 ms and KVM snapshot restore in about 100 ms with guest `dockerd` already running and dm-snapshot cache seeds (§3, §6).** None of this is measured. Legacy's Firecracker lane never ran on Linux in the evidence, and it could not move cache data at all (evidence E.3, `execution.mdx:148-168`). B schedules it for weeks 4–6, together with retiring 185k lines of runner code and ARC.
5. **A local Twirp CacheService v2 and ArtifactService v4 reached through `ACTIONS_RESULTS_URL` in Phase 2.** The official runner overwrites `ACTIONS_RESULTS_URL` and `ACTIONS_CACHE_URL` from SystemVssConnection on every action step (`Runner.Worker/Handlers/NodeScriptActionHandler.cs:55-74`). The plan therefore needs the Phase 4 native worker or TLS interception. Keeping artifacts local also breaks GitHub UI downloads and attestations (D §3.5).

### 2. Factual errors

- **CPU (Summary).** "Workers sit near 0% CPU" generalises one 17:27Z snapshot. The evidence shows 92–95% in three samples (B.2.1), and A measured 84–95% on 11-19.
- **Merge-queue wait (§1, §5.5, Phase table).** B says "`min_entries_to_merge_wait_minutes: 5` adds 5.0 dead minutes". That timeout applies only while the queue waits to reach the minimum group size, and `min_entries_to_merge` is 1 (evidence C.1). D is right. B's 33.8-minute and 141.2-minute baselines each include an invented 5 minutes.
- **Misattributed #585 result (§2 H4).** B cites "`#585` warm result: `0` crates in `0.3 s`" as if it came from velnor-new. It came from the lab. The only velnor-new measurement, mbx with save-on-PR, compiled 46 crates warm and took longer warm than cold. B reports that comparison correctly elsewhere.
- **Maintenance-helpers "508 s drops to ~20 s" (§5 item 4).** nextest alone takes 212–246 s for 54 tests. B also proposes deleting `threads-required = "num-test-threads"` (`.config/nextest.toml:19,46`). That setting deliberately serialises timeout-sensitive archive-guard tests, for example `bounded::times_out_after_guard_mutation_and_cleans_only_its_owned_group`. Removing it trades confidence for speed.
- **Journal size.** B says "64,000+ lines across 382 files". I count 62,666 lines in 375 files under `src/journal`, which is close enough.
- **Checked and correct:** Rust is not baked into the image (`state/RESULT.md:115`). The LVM volume is linear, with no `-i` (`PLAN.md:72-76`). `local-mode` has only positive cfg gates (`velnor-runner-host/Cargo.toml:12-14`). rust-lld is the default linker since 1.90. The ARC deferral code is real (`ephemeralrunnerset_controller.go:323-357`). The #652 blaze figures are real: 248 s and 319 s against 38 s, and 8 of 8 restored-state runs failed.

### 3. Missed bottlenecks and risks

- **Delivery risk.** B's Phase 1 ("Week 1") includes the velnor-gen cutover (#1290, open), deleting state-seal, and moving every gate. Phases 3–4 replace ARC, the runner host and the official worker within 9 weeks. Legacy reached 804k lines with "no performance improvement accepted yet" (E.7) and incidents #389 and #408. Nothing in B's plan prices those failure modes.
- **B's own numbers do not justify Phase 4.** B measures a native worker's gain at about 2.5–4.5 s per job (§2 H2), yet it plans a 10k-line native worker to chase ~10 s. The Phase 4 targets are not supported by that arithmetic.
- **Missing default-feature check.** Running runner-host tests only with `--features local-mode` removes the default-feature build, which is the build that ships. Keep a default `cargo check`/clippy pass.
- **Merge-queue re-entry (23% of PRs, T7)** and the 45% cancellation rate are not costed.

### 4. Ideas worth taking

- **The F1 root cause, which is the most valuable single finding in the three proposals.** It costs very little to fix. Use runner-independent absolute paths, for example a fixed `/opt/velnor/cargo` on both runner classes, or produce the cache on the runner class that consumes it, and make state compat relative.
- **F2:** identify the Free-plan 20-job cap and narrow Job image candidate's `push: {}` trigger.
- **Move the kubelet `root-dir` and scratch off the OS ext4 disk onto the XFS data volume** (§8a defect 1, rank 2). Verify reflink support first.
- **Split pools so jobs without Docker get no DinD sidecar or `init-dind-externals`.** That removes the startup-probe wait, which caused 57 `Unhealthy` events in an hour (B.6).
- **Run the local-mode suite once instead of twice** (5.5–9.2 runner-min), keeping a default-feature check.
- **Tree-verdict receipts** keyed by `(tree, workflow, toolchain)` for identical merge_group and push trees.
- **An inline-or-shard threshold** based on measured sharding overhead (19–25 s on the lab), with LPT scheduling over recorded timings.
- **A CI profile with `debug = "line-tables-only"` and no debug info for dependencies**, after a measurement on velnor-new.
- **The ARC/runner startup audit** (`renewjob` barrier, serial action downloads, 5 s probe period) as cheap Phase 1 items.

### 5. Verdict

B has the sharpest diagnosis and the least credible plan. Its targets of 40–75 s push-to-merge and 38 s per PR contradict test floors that exist in the code today. Its sandbox and microVM numbers are unmeasured. Its local cache service cannot work with the official runner it keeps until Phase 4. It also schedules a rewrite larger than the one that killed legacy for weeks 4–9. **Biggest flaw:** it replaces every layer at once (coordinator, node agent, sandbox runtime, microVMs, native worker) to win seconds, when its own measurements show the minutes are in tests, queue placement and duplicate runs. Adopt its findings and drop its architecture.

## Proposal D

### 1. Weakest assumptions

1. **75% of jobs land on a warm node (§4 hit table).** The arithmetic is 75% × 90% + 25% × 20% = 72.5%. D keeps ARC for the first rollout and itself notes that GitHub assigns jobs to any idle runner (§3, protocol item 2). No mechanism delivers the 75%. Phase 2's "≥70% object hits" target inherits that gap. To settle it, run the same velnor-lab assignment experiment as for A, then test per-root label pools.
2. **mbx is the right compiler cache (§7).** D does not engage with #585, where plain cargo over a persistent target beat every mbx variant on the lab and mbx was slower warm than cold on velnor-new. D's own mbx counts (98.6% of hit-or-miss lookups, but 40.7% of all categories) show how unreliable mbx denominators are as evidence.
3. **The replay counterfactual holds once hosted waits collapse (27.7 → 11.4 min, §1).** The replay keeps the cluster durations that were recorded. On 11-18, contention stretched runner-host from 3.7 to 8.8 minutes (T6). D sizes six heavy slots per worker, 30 in total, shared by three orgs whose scale sets allow up to 160 runners. Concentrating all gate and lint jobs plus 12-CPU slots can move the queue onto the cluster. D says the replay is "before contention", but the Phase 1 target of ≤12 minutes depends on it.
4. **The site tier covers 65% of local misses and test-case splitting reaches about 65 s (§4, §5).** Both are invented parameters. Splitting the 40-case loop needs per-case fixture isolation, and D sets no budget for that refactor.

### 2. Factual errors

I found none that contradict the code or the pack. D correctly counts 16 workflow files where the pack says 15, gets the merge-queue timeout semantics right, and gets the rust-lld default right. **One omission matters.** D leaves the 48/48 source miss undetermined ("check the cache service URL, ref visibility and version"), although `ci.yml:337` and `lookup.rs:28-31` identify the cause (F1).

### 3. Missed bottlenecks and risks

- **F2.** D does not mention the 20-job Free-plan cap, the 306 Job image candidate runs per day, or Source qualification. Its "route and reserve" phase therefore misses the cheapest hosted-queue fix.
- **Scratch on the OS disk.** DinD and emptyDir scratch run on the OS ext4 disk, yet D's §8a item 3 says to "keep the existing XFS/noatime base". D does not notice that jobs do not run on that volume.
- **DinD runs in every pod, including jobs that never use Docker.** D mentions prewarming DinD only for the pool that needs it, but no phase removes it.
- **Few concrete deliverables.** Most §8a items end in "measure first, credit 0 s". That is honest, but it gives the owner no concrete runner or cache design to build in Phase 2. The design space is left open where the evidence already supports a choice: node-persistent stores, a dedicated volume, and per-root pools.

### 4. Ideas worth taking

- **The replay method and the three clocks** (runner-minutes, verdict critical path, push-to-merge). The final plan should use the replay as its acceptance harness.
- **Concrete test refactors** that expose the serial loops at `image_publication_public.rs:86` and `shape.rs:97` as separate tests. This is the only route below the ~4-minute floor.
- **Exact expected-case inventory per shard**, rejecting missing, duplicated or zero-when-nonempty shards.
- **Signed group-to-main evidence** in place of the push re-run, with independent trials kept for flaky or concurrency suites.
- **Route known-unsupported jobs before acquisition.** Never transfer a lease.
- **Native worker adoption gate:** at least 30 s or 10% at p50, no worse p90, and zero semantic diffs.
- **Correct merge-queue semantics** and a throughput model (24 against 10 group builds per hour).
- **Gate capacity reserved in both sites;** trust treated as a scheduling constraint.
- **Hit rates always reported with explicit denominators.**

### 5. Verdict

D is the best calibrated of the three and the most likely to keep confidence equal. Its 3–5-minute PR target and 4–9-minute push-to-merge target are honest, and they acknowledge the single-test floors. It would still stop short of the minimum time, because it defers the decisions that create the cache win: how jobs reach warm nodes, which cache layer to use, and the dedicated volume. It also misses the free-plan cap and the root cause of the cache miss. **Biggest flaw:** the warm-placement hit rate is assumed under an ARC model that D itself says cannot steer assignment. D reads more as an experiment protocol than a design.

## Cross-cutting

### Where all three agree, and the evidence supports them

- Move the gates off hosted runners first.
- H1 fails as a whole-pipeline claim. B measured Node and .NET at about 2.0–2.5 s per job. A and D report about 6 s of action steps.
- Blaze stays out of the gate. Its toolchain ignores the 1.99.1 pin, and #652 measured it 6.5–8.4× slower, with 8 of 8 restored runs failing.
- No test outcome is cached across trees.
- Remove the duplicate push run when the SHA is identical (120 of 159 cases, T7).
- Use node-local caches, kept same-site.
- Use Guaranteed CPU slots, with build and test parallelism derived from the slot size.
- Keep the official runner as at least the fallback and the oracle.

### Where they conflict, and which side the evidence supports

| Issue | A | B | D | Evidence favours |
|---|---|---|---|---|
| Native worker | Optional restricted lane, shadowed | Phase 4 production | Only if it saves ≥30 s or 10% | A/D. B's own measurement puts the gain at 2.5–4.5 s per job. |
| Compiler cache | mbx node store plus site remote | Reflinked `target/` plus plain cargo | mbx | B on the lab (#585), unproven on velnor-new. Run that A/B test first. |
| Replace ARC | Phase 4 | Phase 3 | Only if measured | D. Cluster queue is 0.2 min at median (T4), and F3 caps the placement payoff. |
| Pod reuse | Yes, warm pool reused | Fresh sandbox per job | Fresh per job | B/D. Reuse lowers confidence. |
| Sources cache-miss cause | Key mixes platform | Path-derived version | Undetermined | B. Verified as F1. |
| 5-minute merge-queue wait | Not addressed | A fixed 5-minute floor | Not a floor | D (min group size 1). |
| Typical PR target | 4–5 min | 24–38 s | 3–5 min | A/D, given the F4 floors until the loops are split. |
| Default linker | cc, so mold helps | rust-lld already | rust-lld already | B/D. |

### Missed by all three

- **No proposal solves runner selection under scale sets (F3).** The practical fix is generator-emitted labels per Cargo root or cache family, each served by its own small scale set pinned to a node set. Every cache-locality number in all three proposals is conditional on this.
- **Merge-queue throughput and re-entry.** `max_entries_to_build 2`, 7-hour enqueue-to-group delays on 11-18, and 23% re-entry dominate push-to-merge on bad days. B raises the limit to 5 and D models the rate, but no plan measures it.
- **Cold-path tails after CPU capping**, and capacity for three tenants under Guaranteed slots.

### Hypotheses I now judge differently

- **H4:** I now judge it "holds for locality, unproven for the built-in placement half". Placement-aware caching works only if the job-to-node binding is solved through labels or pools. The layer that matters may be a persistent target directory rather than an object store.
- **H5:** it holds more strongly than I expected. Its cheapest levers are ruleset and trigger configuration: the Free-plan cap together with `push: {}` and Source qualification on every PR, the merge-queue build limit, and the push re-run. None of them needs runner code.
- **H2:** it stays "fails as a general claim". B's measurement settles the size of the prize at seconds per job, not minutes.
- **H1 and H3:** my judgement is unchanged.

---

## Key (added by the manager after the review)

- Proposal D: GPT-6.2-Astra
- Proposal A: Muse Wonder
- Proposal B: Gemini 4 Carbon
