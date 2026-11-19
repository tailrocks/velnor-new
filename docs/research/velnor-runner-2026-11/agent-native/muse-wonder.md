# Reimagining the runner for AI agents: Muse Wonder

| Field | Value |
|---|---|
| Model | Muse Wonder |
| Model id | `muse-wonder-1.0` |
| Client | Muse Code 1.4.20 (muse exec) |
| Reasoning effort | high |
| Date / run | 2026-11-19 |
| Brief | agent-native brief v1 (2026-11-19): the final design, the user-experience draft and the owner decisions |

This is the model's own answer to the owner's prompt to reimagine the runner when AI agents do the work, especially running agents' local verification through Velnor so CI can skip what was already verified, unedited except for formatting.

---

# Agent-native CI: when the author is a fleet of agents

## The picture

When agents do the work, CI stops being a gate at the end and becomes a shared memory of verified work.
Every `cargo` command an agent runs, locally or on the cluster, goes through Velnor once, in a sealed
sandbox the node agent observed, and leaves a signed receipt keyed by everything that could change its
result. Agents ask the ledger before they run anything: a hit means do not run it again, in this worktree,
in another PR, or in the merge queue. A miss runs twice, on two warm cluster nodes, and becomes reusable
by everyone. The PR verdict is then mostly a lookup over receipts that already exist, plus the few
obligations whose inputs actually changed and the ones that must always run fresh. Local Mac runs stay
advisory only: they guide the agent in seconds but never count as evidence, because their architecture
and kernel are different inputs. Merging still needs a green `Velnor / Verified` on the latest commit and
GitHub's merge queue still lands it; Velnor just makes that green cheap to earn and hard to fake.

## Ideas

| id | idea | mechanism in Velnor (components, commands, receipts, trust) | what it removes (duplicate runs, wait time) | correctness: why no false green; trust tier and fallback | fit with owner decisions | cost (agent-hours) |
|---|---|---|---|---|---|---|
| N01 | Route every agent `cargo` command through Velnor: CLI, `cargo` shim and local daemon that mint receipts | `velnor` binary plus a `cargo` shim early on `PATH` and a per-Mac/per-host daemon (`velnor local up`, `velnor-node`) intercept `cargo test/check/clippy/doc/fmt`; the daemon forwards the exact command graph to `velnor-dispatch`, the node agent runs it in a warm sealed sandbox via `velnor-ci obligation exec`, observes exits and the expected test-ID inventory over a channel the sandbox cannot write, and the signer daemon outside the sandbox signs one receipt per witness (key, tree, binary digests incl. `CARGO_BIN_EXE_*`, generation, node, epoch). Bare `cargo` still works for humans but produces no receipt and is labelled as such in agent transcripts. | Today row 6 of the duplicate audit (agent pre-push test, then CI) is unmeasured and fully duplicated; with ~4.77 PR runs per merged PR at 52.9 runner-min each and 45% cancelled (1,156/2,558), even capturing the one pre-push run per head saves roughly one full obligation set per labelled head: at goal sizing ~3 runner-min per witness set, ~6 runner-min per head, ~12 runner-min per merged PR at ~2 heads/PR [E]. It also removes the agent's blind re-runs of unchanged crates (today 4.4x crate builds per run, 2,106 builds of 475 crates). | Receipts are minted only from processes the node agent started on inputs it mounted (contract C4); a shim that merely wraps a host process mints nothing. Tier: candidate (see N03). Fallback: no receipt, no reuse; CI runs the obligation fresh under the label flow. | Fits decisions 10(c) and 11; extends the design's `velnor verify` from a stretch item to the default agent path. | 24–40 |
| N02 | Remote by default: run agent commands on warm cluster nodes; keep local sealed sandboxes for latency-critical feedback only | Default path: daemon sends tree hash plus dirty-file bundle to the home site; `velnor-control`/`velnor-dispatch` place the obligation on a warm build sandbox (warm pool 8 gate / 2 build per site, proposed), checkout is a reflink clone from the node mirror (0.25 s clone, 0.31 s no-op build [M]) with the full staged-tree hash checked before the first step. Local path: Docker backend on the Mac (`velnor run`) with whole-tree keys and no read audit, explicitly non-evidence. One command, `velnor run --where auto`, picks remote when the queue wait forecast beats local cold start, else local advisory. | Removes the cold local build plus the identical CI build: one 92 s cold runner-workspace build [M] and one full obligation set per command instead of two. At fleet scale, if 20 agents each avoid one duplicated 15–25 s compile+test cycle per task-hour, that is ~2–3 cluster-hours/day returned [E: 20 x 20 s x ~20 cycles]. Remote warm start (≤1 s provision target) also beats a cold Mac Docker start plus dependency fetch (3.5–6.1 s/job crates, 10 s tools [M] today). | Same sandbox, key and observation rules wherever it runs; the execution fingerprint (kernel, arch, CPU features, CPU count) is in the key, so a local arm64 run can never fill an amd64 slot. Fallback: if the cluster is unreachable, run locally as advisory and re-run remotely before any verdict; never promote a local result. | Fits decisions 13, 21, 24 (platform trait, capabilities, mirrors); no new backend, only a routing policy in the daemon. | 16–28 |
| N03 | Explicit trust tiers for receipts, with a fixed witness count at the gate | Tier 0 advisory: any local/unobserved run, never stored as evidence. Tier 1 candidate: one passing witness in a cluster sealed sandbox, signed, reusable for planning and for joining in-flight work but not sufficient alone. Tier 2 verified: two passing witnesses on two nodes, two attempts, different test orders, over two verified generations or one cold — this is what `Velnor / Verified` requires for every new key, in PR and merge group alike. Tier 3 trusted: Tier 2 plus the landed commit's cold audit A1 passed; only Tier 3 outputs feed trusted caches, generations and release promotion. PR-produced receipts enter as candidate/verified, never directly as trusted (decision 8). | Removes the third execution the naive design would need: today the same SHA is fully run three times (PR, group, push in 120/159 merges [M], ~25 min and ~53–65 runner-min per run). Two witnesses plus A1 as the third execution gives the same 1-(1-p)2 pre-merge detection as today at ~12 runner-min per merged PR at goal sizing instead of 285–347 [E]. Knowing the tier also lets agents stop early: a Tier 2 hit needs zero runs; a Tier 1 hit needs one. | A receipt counts once, for one slot of one record; any witness failure blocks with no retry (C4, section 6.2). Wrong-key risk is contained by shadow graduation (~3,000 clean comparisons per kind), 2% re-execution, A1 on every landed commit, and epoch revocation that stops `success` writes and re-verifies open heads. Fallback on any disagreement: the reuse class is suspended and obligations run fresh. | Fits decisions 10, 11 and the design's sections 6.2/6.5 unchanged; it only names the tiers the design already implies. | 12–20 (mostly ledger schema and `velnor explain` surfacing) |
| N04 | Solve arm64 vs amd64 by making architecture a key input and giving agents an amd64 twin on demand | Every key carries arch, OS, kernel release and CPU features the toolchain may target (section 6.1); Apple Silicon Macs therefore produce Tier 0 receipts only. The daemon offers `velnor run --arch amd64` (the default for anything receipt-bearing) which executes on the EPYC cluster, and `--arch arm64` only as advisory feedback. Optional later step: one arm64 Linux node (or Apple Containerization backend declaring `kernel: Separate`, `dir_entry: [VirtioFs]`) whose receipts are valid for arm64 obligations only. Cross-compiled release targets (`aarch64-unknown-linux-musl` under qemu smoke tests) stay obligations keyed by target, as today. | Removes the silent duplicate where an agent tests natively on arm64 and CI re-tests everything on amd64 with zero transferable evidence, and removes false confidence from the reverse. It also removes per-agent Docker Desktop Linux VM variance. Quantitatively it converts the Mac from a second, incompatible CI into a pure editor: 100% of receipt-bearing runs happen once, on the arch the gate uses. | Cross-arch reuse is impossible by construction: keys differ, so the ledger cannot resolve an amd64 obligation from an arm64 receipt. The conformance corpus runs per backend and per arch; a new arm64 backend is accepted only at 100% corpus pass like any platform (decision 21). Fallback: whole-tree keys and Tier 0 labelling on any backend without a read audit (Docker/macOS today). | Fits decisions 9 (macOS paused, Linux-only gate), 13, 20, 21; spending on an arm64 evidence node would be an owner question, and this idea does not require it. | 8–14 (policy and routing; no new hardware) |
| N05 | Continuous verification of committed-but-unpushed work: the daemon verifies each local commit as the agent makes it | The local daemon watches the worktree; on each commit (not each save) it computes `merge-tree(queue tip, HEAD)`, uploads the tree by bundle to the site, and runs `ensure` at batch/idle priority, filling candidate records. On push and label, the PR verdict joins or reuses those records. Uncommitted dirty state is never verified as a tree; the agent is nudged to commit early and often, which suits agent workflows. Results surface as `velnor status --local`: green/red per obligation before any push exists. | Hides the compile+test latency behind the agent's own thinking time: design estimate 15–25 s off label-to-verdict per head [E, owner question D19], and at the goal the verdict falls from ~35 s to ~3–5 s on a full hit [E]. It also spreads load: batch-priority verification at idle absorbs work that would otherwise burst at label time, when the reserved merge path matters most. | Records are candidate scope only; the gate still requires Tier 2 on the exact head tree at label time and the merge group still verifies its own tree (C1). A speculated head that is superseded simply never fills a gate slot. Stale-speculation risk is nil for correctness because keys, not commit ancestry, decide reuse. Fallback: speculation off returns exactly to the label flow. | **Owner question — conflicts with decision 12** (an unlabelled/unpushed state starts nothing) and with the current default on D19. Gain at stake: ~15–25 s per labelled head plus burst smoothing, for ~5–10 runner-min per speculated head of which ~45% are superseded [E]. Ask: may Velnor verify local commits at idle priority if it never writes any GitHub status from them? | 20–32 |
| N06 | Ledger-first agents: query before running, so parallel agents never duplicate each other | New read APIs on `velnor-ledger`: `velnor plan <tree>` prints the obligation vector with hit/miss/tier per key; `velnor ledger query --obligation test:velnor-ci --key <digest>` and `velnor explain <pr> <check>` (already sketched in the UX draft) become the agent's first step; a small MCP/JSON mode (`velnor plan --json`) lets the orchestrating agent assign only missing obligations to workers. In-flight joins are first-class: a second agent needing the same key attaches to the running record instead of launching a twin. | Removes cross-agent duplication, which today has no defence at all (audit row 5: no cross-PR reuse). If 5 agents on stacked or sibling PRs each would run the same unchanged `velnor-ci` test obligation (~10–62 s [M]), one runs it and four join: ~40–250 s saved per collision [E]. Across a fleet doing 4.77 runs per merged PR, moving even the unchanged-obligation share (29 of 41 checks reused in the UX example) from run to lookup is the difference between the ~110 and ~37 runner-min rows of the design's own table. | Querying never mints evidence; it only reads records whose soundness case is N03's. A wrong hit is the known wrong-key failure mode, detected by the enforced read audit, 2% sampling and A1, and contained by epoch revocation. The planner and inventory come from the base commit via the released helper, never from the change under test (C3), so an agent cannot plan obligations away. Fallback: ledger unreachable means everything runs fresh — slower, never wrong. | Fits decisions 10(a)/(b) and 11; it is the read side of the ledger the design already builds. | 14–24 |
| N07 | Flake detection during development, not after merge | The dev path gets what the gate forbids: repetition. `velnor run --repeat 5 --shuffle` (non-minting lane) runs a changed test binary repeatedly with different order seeds on two nodes while the agent is still editing; outcomes feed a per-test flake ledger. The fast-red lane (incremental build, `cargo check`, changed and historically flaky tests first, failure posted to `Velnor / Details` at once) gives the agent a red in seconds. A test that flakes in dev is flagged in `velnor plan` output for every later agent, with its measured paired-failure rate. | Attacks the 8.1% false-failure rate [M, 10/124 push runs] where it is cheapest. Two witnesses turn 8.1% into ~15.5% chance that at least one fails falsely, against a 3%-then-1% budget, so every flake found in dev directly buys gate reliability: halving the per-run rate roughly quarters paired false failures. It also removes queue ejections, each of which rebuilds every entry behind it, and the human/agent rerun loop (`Re-run` on Details) from the critical path. | Dev-lane results never fill a witness slot and never write `success` (section 6.2 fast-red rule); the gate keeps zero retries, so a flaky required test still fails and is counted. Shuffle is via seeded filter-expression batches or a digest-pinned patched nextest, whichever measures ≤1.5 s per binary, with the seed recorded. Fallback: a test whose flake cannot be reproduced in dev still hits the existing budget, issue and quarantine rules unchanged. | Fits decision 11's flake budget and section 1.3/8.5; no gate semantics change. | 16–26 |
| N08 | Scheduler priority for interactive agent commands, below merge work but above batch | Add an `interactive` class to `velnor-dispatch` placement: priorities stay merge > gate > interactive > PR > other orgs > batch, with the reserved merge slot (one 16-CPU build slot plus 2 gate CPUs per site) untouchable. Interactive work (an agent waiting on a command result) may borrow idle batch capacity and preempt only batch (`cpu.idle`); it is itself preemptible by gate and merge arrivals. Admission is a pair-aware unit for two-witness obligations so the second witness never queues behind the first. Per-agent fairness tokens stop one agent's fan-out starving another's single command. | Removes the wait that makes agents poll or context-switch: job start target ≤1 s P50 / ≤2 s P90 versus 13 s / 27 s acquire today [M] and 5.5 min median hosted gate queue on the bad day. For an agent blocked on a result, latency is serial wall time, so cutting queue wait from minutes to ~1 s across dozens of commands per task is the largest agent-visible saving in this document: ~5 min per task at only 20 commands [E]. | Priority changes only placement, never keys, witnesses or the gate; starved PR work still completes because interactive requests are short and preemptible. The known hazard — interactive load hiding the second-witness wait — is measured directly (eligibility-to-second-witness-start is a reported stage). Fallback: if merge-path wait P90 exceeds 10 s for two weeks, interactive borrowing is tightened by config, not by code change to the gate. | Fits decisions 13/15 and section 9 priorities; it adds one class to an existing list. | 10–18 |
| N09 | Signing and attestation of local results so a receipt is portable evidence, not a claim | Each node and sandbox class holds one Ed25519 key in a signer daemon outside every sandbox; receipts are appended to a per-site append-only log and the ledger verifies signature, class and log entry before a slot fills (section 6.5). A receipt binds the digest of every binary executed, the generation or `cold`, the node, witness slot and epoch. Mac-local advisory runs may be signed by a developer key for provenance in `velnor explain` but are namespaced Tier 0 and cannot fill slots. Release bytes keep the existing Sigstore path (`actions/attest-build-provenance` natively: OIDC, ephemeral key, Fulcio, Rekor) and are promoted by digest, never rebuilt. Optional: receipts exported as in-toto/SLSA-style attestations so external tooling can verify a PR's evidence chain. | Removes re-verification demanded by distrust: without portable signed receipts, every consumer (another agent, the merge group, a release, an auditor) must re-run to believe, which is today's three-full-runs pattern (~53–65 runner-min each). Signing costs milliseconds per receipt against minutes per re-run. It also removes the audit argument: A1 and epoch revocation can enumerate exactly which `success` writes relied on which receipts via the writer's durable index. | A compromised or buggy signer is the sharp edge: contained by keys living outside sandboxes, per-class keys, the append-only log, and emergency revocation that stops `success`, writes `failure` on every still-open indexed success, and cold-audits everything landed in the window. A Mac key can at worst forge Tier 0 paper, which no gate reads. Fallback: revoke the key/epoch; all dependent records resolve to miss and run fresh. | Fits decisions 8, 10, 11 and section 6.5 as written; the in-toto export is additive and optional. | 12–20 (core already designed; export format is the new part) |
| N10 | The PR verdict becomes mostly a lookup | Assemble the pieces: the agent's pre-push verification (N01/N05) fills candidate records for `merge-tree(queue tip, head)`; at label time `velnor-control` runs `ensure(tree, plan)`, resolves each obligation against current unrevoked records, joins in-flight ones, and schedules only misses plus the C5 set (network, clock, randomness, event, unpinned Docker, commit identity) which always runs fresh. The fenced writer posts `Velnor / Verified` when the vector is satisfied: target ≤1 s after the last receipt, with `Velnor / Details` reporting reused / ran-twice / fresh counts. Merge groups repeat the same lookup against the group tree (60–75% exact-tree hits with auto-enqueue [E]; 16% today [M]), and `main` promotes the group's records while A1 becomes the third execution. | This is the headline saving: PR verdict from 26.7/92.3 min [M, proxy] toward the ≤90 s / ≤4 min goal [T] (~35 s estimate), push-to-merge from 85/457 min toward ≤3/≤8 min (~85 s, of which ~45 s is GitHub's own queue and merge). Runner-minutes per merged PR fall from 285–347 [E] toward ≤50 [T] because the 29-of-41 reused checks in the typical PR cost a ledger read instead of two executions. | A lookup is only as sound as its keys: whole-tree keys until per-root enrolment is proven by the enforced read audit and ~3,000 clean shadow comparisons per kind; C5 always fresh; two witnesses per new key; A1 cold-rebuilds every landed commit and compares digests. If any layer disagrees, the epoch is revoked and the verdict degrades to running everything — the pre-Velnor behaviour — never to a weaker green. The gate itself is unchanged: latest commit green or no merge (decision 12). | Fits decisions 7, 10, 11, 12 exactly; it is the design's section 6.3 with the agent-side producers switched on. | 18–30 (integration and `Details` reporting; the ledger itself is core scope) |

**N01 — one path for agent commands.** The insight is that agents already run the right commands; they
just run them where the result evaporates. A `cargo` shim plus daemon does not ask agents to learn new
habits: the orchestrator sets `PATH` (or `CARGO=/path/to/velnor-cargo`) once per worktree, and every
`cargo test -p velnor-ci` the agent issues becomes an observed obligation on a cluster node, returning
streamed output indistinguishable from local cargo, plus a receipt reference the agent can quote.
Because the node agent — not the shim — starts the process, mounts the tree and watches the test-ID
inventory appear, the shim cannot fabricate a pass: the worst a broken shim can do is fail to forward,
which costs a local advisory run, not a false green. Over a day of parallel agent work this single
change converts the largest unmeasured duplicate in the audit (row 6) into the fleet's primary source
of candidate records, and it gives N06 its data: you cannot query a ledger nobody writes to.

**N02 — remote by default.** Local-first feels fast and is usually the opposite once the gate's arch
and cold caches are counted: the agent pays a cold Mac build, then CI pays the same build again on
amd64. Sending the command to a warm cluster node pays one build, on the evidence arch, starting from
a reflink clone in ~0.25 s with the toolchain and crates already resident. The local sealed sandbox
keeps its role — tight edit-test loops under a second of startup where cluster round-trips would hurt —
but its output is labelled advisory in the transcript so neither the agent nor a reviewer mistakes it
for verification. The routing rule is a forecast, not a dogma: if the interactive queue (N08) predicts
a wait longer than a local cold start, run locally for feedback and remotely for the receipt, in that
order, and let the two race only as preparation, never as competing witnesses.

**N03 — trust tiers.** Naming the tiers matters because agents make decisions from them: a Tier 2 hit
means skip the command entirely; a Tier 1 hit means run one witness, or better, let the second witness
be the next agent who needs the same key; Tier 0 means run it properly. The gate's arithmetic is fixed
and conservative — two witnesses for anything new, A1 as the third execution after landing — matching
today's detection probability 1-(1-p)2 while deleting the third full run. Tiers also discipline cache
trust: PR sandboxes may write candidate records freely, but a generation or trusted cache entry is only
ever built from Tier 3 inputs, so the decision-8 rule (PR results never enter `main`'s trusted stores
unverified) becomes a type-level property of the ledger rather than a convention reviewers must check.

**N04 — arm64 vs amd64.** This is the problem that silently invalidates the naive version of the
owner's idea: an Apple Silicon Mac and an EPYC cluster node do not run the same inputs, and Rust test
binaries are arch-specific artefacts. The design already puts arch, kernel and CPU features in every
key, so the honest answer is not to make Mac results count but to make Mac results unnecessary: the
receipt-bearing run happens on amd64 by default (N02), and the Mac contributes editing speed, not
evidence. That is a better deal than it sounds — the agent keeps sub-second local feedback for syntax
and fast unit tests (Tier 0), while anything it would otherwise re-run in CI is already verified on
the target arch. If arm64 ever becomes a release target rather than a dev machine, the platform trait
admits an arm64 backend as a contained piece of work with its own key space and conformance corpus;
until then, spending cluster money on arm64 evidence would buy receipts the x86_64 gate cannot use.

**N05 — continuous verification of unpushed commits.** Agents think in commits long before humans
think in PRs; verifying each local commit at batch priority means the expensive work happens while
the agent writes the next change, and the labelled PR inherits a warm ledger. The conflict with
decision 12 is real and should not be laundered: decision 12 exists because 45% of pushed runs were
cancelled waste, and speculating on unpushed commits spends runner-minutes on heads that may never
be pushed at all. The containment is priority and scope — idle-only capacity that batch work already
uses, candidate records that expire, and a hard rule that no GitHub-visible state changes until the
label flow runs — so the cost is bounded by genuinely idle cycles while the gain (label-to-verdict as
a near-pure lookup, plus burst smoothing at exactly the moments the merge path is contended) accrues
on every head that does ship. Framed as D19's sibling for the pre-push case, it deserves an owner
decision with the speculation hit-rate measured from the daemon's first month, not a default.

**N06 — ledger-first agents.** The fleet failure mode today is five agents on five worktrees running
the same unchanged test binary because none can see the others' work. A queryable ledger turns the
orchestrator into a scheduler of missing work only: `velnor plan --json` partitions a PR's obligation
vector into hits (skip), in-flight (join or assign elsewhere) and misses (assign once, to one agent's
session or directly to the dispatcher). This is also where stacked PRs pay off mechanically — the
upper PR's plan resolves most keys from the lower PR's records — and where `velnor explain` stops
being a debugging tool and becomes the agent's cost model: changed-input paths per re-run teach the
agent to make smaller, better-scoped changes. The read API must be as fast as the runs it replaces:
a plan lookup is a ledger read over ~41 keys and should answer in well under a second, or agents will
rationally stop asking.

**N07 — flake detection in the dev loop.** Flakes are currently discovered at the worst possible
time: inside the gate, where zero retries convert them into queue ejections and full rebuilds for
every entry behind. Moving repetition into the dev lane exploits an asymmetry — repetition is
forbidden as evidence but priceless as diagnosis. An agent that changed a test can run it 5–20 times
across two nodes and order seeds before pushing, catch the timing assumption while the code is still
on screen, and either fix it or hand the gate a test with a known-clean recent history. The flake
ledger compounds this across the fleet: once one agent measures a test's paired-failure rate, every
later agent's plan shows it, and the owner-labelled issue threshold (two false failures in seven days)
gets evidence from development instead of only from blocked merges.

**N08 — interactive priority.** An agent waiting on a command is a blocked worker whose cost is wall
time, not runner-minutes; the scheduler should treat its commands the way an OS treats interactive
processes: brief, latency-sensitive, and preemptible. Placing `interactive` below merge and gate
protects the one latency the business metric (push-to-merge) is made of, while borrowing from batch
— speculation, audits within their SLO, mirror fills — which is defined by having slack. The pair-
admission rule matters as much as the class itself: a two-witness obligation whose second witness
starts minutes after the first has wasted the first's warmth and doubled the flake-exposure window,
so the dispatcher admits witness pairs against a forecast of two eligible nodes or queues the pair
whole. Fairness tokens keep this honest when twenty agents share five workers: no agent may hold
more than its share of interactive slots, however many worktrees it runs.

**N09 — signing and attestation.** Receipts are the currency of every other idea, so their forgery
resistance decides whether the currency holds. Keeping keys in a signer daemon outside the sandbox,
appending every receipt to a log the ledger checks, and binding binary digests into the receipt means
a receipt is verifiable by anyone later — another agent, the merge group, A1, a release job, or an
external auditor via an in-toto export — without trusting the machine that produced it or re-running
the work. This is what lets promotion replace rebuilding for releases (verify the attestation, fetch
by digest, publish in ~15 s instead of 190–6,238 s of recompilation [M]) and what makes epoch
revocation surgical: the writer's index enumerates every `success` that spent a bad receipt, so the
response to a signing incident is a list, not a forensic investigation.

**N10 — the verdict as a lookup.** The end state inverts today's proportions: instead of a PR run that
executes 41 checks to discover that 29 were already known, the labelled head executes the handful
whose inputs changed, re-runs the C5 set that can never be known in advance, and resolves the rest
from records other agents produced before the push. The merge queue then usually finds its tree
already verified — because the PR was verified against the queue tip, not against a stale `main` —
and `main` inherits verified bytes instead of triggering a third run. Nothing about the gate weakens:
the same inventory, the same two witnesses for anything new, the same latest-commit rule. What
changes is who pays and when: the fleet pays incrementally, at idle and interactive priority, during
authorship, instead of paying in full, at merge priority, after authorship. That is the whole
proposal in one sentence, and it is why the first three steps below are ordered as they are.

## Risks

| id | false-green or waste risk | containment |
|---|---|---|
| N01 | A shim bug forwards a weakened command (fewer tests, wrong features) and the receipt blesses it. | The planner's inventory and exact expected test IDs come from the base commit's released helper (C3); a missing, duplicate or extra ID fails the witness. The shim forwards a command graph, but `velnor-ci obligation exec` expands it against the inventory inside the sandbox. |
| N01 | Agents bypass the shim (`command cargo`, absolute paths) and later assume coverage that never happened. | Transcripts and `velnor plan` show uncovered obligations as misses, never as assumed hits; the gate is indifferent to how the agent worked — uncovered keys simply run at label time. Bypass costs time, not correctness. |
| N02 | Remote-by-default herds every agent onto five workers; loaded P90 lands 2–3x above idle estimates (workers already at 92–95% CPU on the measured bad day). | Interactive fairness tokens, batch preemption, the reserved merge path, and routing forecasts that fall back to local advisory runs. Loaded replay (2026-11-18 trace) is the acceptance test before defaults flip. |
| N02 | Tree upload leaks unpushed IP or the bundle and the verified tree diverge. | Same trust domain as the cluster that already hosts the repos; bundle is built from the commit object the agent names, and the staged tree is fully hashed against the requested tree before the first step — divergence is a failed run, not a wrong one. |
| N03 | Tier inflation: a candidate record quietly treated as verified under deadline pressure. | Tiers are ledger types, not labels: the fenced writer resolves only records with both witness slots filled under the active epoch. Any shortcut requires changing the writer, which is gated, reviewed code. |
| N03 | Correlated witnesses (same image, mirror code, generation family) detect less than (1-p)2. | Witnesses differ by node, attempt, order seed and generation-or-cold; lab C2 measures joint pass rates against time-separated runs, and if correlation appears the merge group re-runs changed obligations even on identical trees. |
| N04 | An agent trusts a green arm64 local run and is surprised by an amd64-only failure (endianness, SIMD, timing). | Tier 0 results are visually and structurally non-evidence; `velnor plan` always shows amd64 obligations as open until an amd64 witness exists. The surprise costs one CI cycle, not a merge. |
| N05 | Speculation burns the fleet's idle budget on the ~45% of heads that are superseded, starving A1 and mirror fills. | Batch priority only, per-agent speculation budgets, candidate-record expiry, and a measured hit-rate report; if speculation's cost per landed lookup exceeds the D19 arithmetic (~5–10 runner-min for 15–25 s), the owner question answers itself and it stays off. |
| N05 | Continuous verification creates pressure to treat an unlabelled green as mergeable. | The daemon has no GitHub credentials and writes no statuses; only the label flow arms verification of record (decision 12), and `Velnor / Verified` exists only on pushed head SHAs. |
| N06 | Stale or wrong-key hits propagate one agent's bad receipt to the whole fleet instantly. | This is the general wrong-key mode: enforced read audit on every execution, 2% re-execution, A1 cold audits, epoch revocation that invalidates dependents and flips affected `success` writes to `failure`. Blast radius is bounded by revocation speed, which the writer's index makes a lookup. |
| N06 | Agents over-trust `plan` and stop running exploratory tests the inventory does not know. | The inventory is the gate's contract, not the agent's curiosity budget; dev-lane runs (N07) remain available and non-minting. A test absent from the inventory is a C3 parity failure, caught by shadow parity against today's CI. |
| N07 | Dev-lane repetition teaches agents to re-roll instead of fixing, importing flake tolerance into the gate. | Dev repeats mint nothing; the gate keeps zero retries and the 3%/1% false-failure budget with owner-labelled issues. The flake ledger records, per test, whether repetition ended in a fix or a shrug — reviewer-visible. |
| N08 | Interactive priority starves ordinary PR verification, moving latency from agents to the merge path. | Merge and gate outrank interactive; interactive preempts only batch; merge-path wait P90 > 10 s sustained auto-tightens borrowing by config. Admission metrics are per class, so starvation is a graph, not an anecdote. |
| N09 | Signer key compromise forges verified receipts at fleet scale. | Keys outside sandboxes, per node and class; append-only log cross-check; emergency revocation stops `success`, fails open heads that spent the key's receipts, and cold-audits the landed window. Break-glass recovery never includes an admin merge (D15). |
| N09 | Attestation theatre: signed receipts over unsound keys look stronger than they are. | Signing authenticates origin, not key completeness; soundness still comes from whole-tree keys, enrolment audits, sampling and A1. `velnor explain` prints the key's scope beside the signature so the two are never conflated. |
| N10 | The lookup path hides a C5 obligation misclassified as reusable (clock, network, randomness read on an untaken branch). | C5 classification is by the trusted planner over code paths, with repository work shrinking it via an injectable fixed clock recorded in the key; A2 nightly audits shift the clock +8/+30 days to catch date bombs. Misclassification is the known residual risk of the whole design and is why A1 and sampling never switch off. |
| N10 | Lookup complacency: verdicts get so fast that nobody notices the fleet re-running less and understanding less. | `Velnor / Details` always reports reused / ran-twice / fresh counts; the daily report tracks reuse rate per kind next to false-failure rate, so a reuse class that climbs while flakes climb is visible before it is revoked. |
| All | The fleet optimises for the lookup and stops doing the underlying work (fewer cold runs, thinner exploration). | A1, 2% sampling, conformance and canary PRs are mandatory cold work that no hit rate can retire; their runner-minutes are charged in the ≤50 budget explicitly (~10 of ~37 at goal sizing). |

## The first three steps

**Step 1 — Make the ledger queryable and `velnor verify` real (days, ~30–45 agent-hours).** Ship the
read side first: `velnor plan --json`, `velnor ledger query` and `velnor explain` over the existing
record store, plus `velnor verify` as specified in the UX draft (bundle upload, `merge-tree` against
the queue tip, candidate records, ~10 s verdict on a later labelled push). This needs no shim, no
daemon and no owner decision — decision 10(c) already authorises it — and it immediately lets
orchestrating agents partition work by hit/miss instead of duplicating it (N06, N10). Acceptance: on
replayed historical heads, plan lookups answer in <1 s and the reused/ran/fresh split matches a cold
control run exactly; the first live agents quote receipt IDs in PR descriptions. This step also
produces the speculation hit-rate data that turns N05 from an argument into a measurement.

**Step 2 — Route agent commands through Velnor: shim, daemon and signer (1–2 weeks, ~50–75
agent-hours).** Ship the `cargo` shim and local daemon (N01) with remote-by-default execution on warm
cluster nodes (N02), Ed25519 signing with Tier 0/1/2 labelling (N03, N09), and the arm64 policy:
Mac runs advisory, amd64 runs evidentiary (N04). Agents need no workflow change — the worktree's
environment routes their existing commands — which is why this outranks any new agent behaviour.
Acceptance: a full agent task's command log shows ≥90% of `cargo` invocations receipt-bearing;
duplicated pre-push/CI obligation sets on labelled heads fall to the C5 set plus misses; no Tier 0
result appears in any `velnor explain` chain for a merged PR. Loaded-cluster replay must pass before
remote-by-default becomes the fleet default rather than an opt-in.

**Step 3 — Close the loop at the gate: interactive scheduling and verdict-as-lookup reporting (1–2
weeks, ~35–55 agent-hours).** Add the `interactive` scheduler class with witness-pair admission
(N08), wire the dev-lane flake loop into agent tooling (`--repeat/--shuffle`, flake flags in plan
output, N07), and upgrade `Velnor / Details` to report reused / ran-twice / fresh per PR with the
would-have-taken estimate, so the lookup verdict (N10) is visible to the owner on every merge. In
parallel, put the N05 owner question with Step 1's measured numbers: speculation hit rate, runner-min
per speculated head, and seconds saved per labelled head. Acceptance: PR verdict P50 on the
helper/generator class moves measurably toward the ≤90 s goal with zero change in gate semantics;
time-to-first-red falls via the fast-red lane; and the owner can decide continuous verification from
data. After these three steps, the remaining distance to the design's goal is the design's own
programme — per-root key enrolment and the test-floor fixes — now with a fleet that produces and
consumes receipts by default instead of rediscovering verification on every push.
