# Reimagining the runner for AI agents: Muse Wonder Full

| Field | Value |
|---|---|
| Model | Muse Wonder Full |
| Model id | `muse-wonder-full-1.0` |
| Client | Muse Code 1.4.20 (muse exec) |
| Reasoning effort | high |
| Date / run | 2026-11-19 |
| Brief | agent-native brief v1 (2026-11-19): the final design, the user-experience draft and the owner decisions |

This is the model's own answer to the owner's prompt to reimagine the runner when AI agents do the work, especially running agents' local verification through Velnor so CI can skip what was already verified, unedited except for formatting.

---

# Agent-native Velnor: run it once, wherever it first runs

**Status:** proposal against the v3 architecture (`velnor-runner-architecture.md`), the
user-experience draft, owner decisions 1–24, `explainer.md` and `fastest-comparison.md`.
Numbers keep the design's tags: [M] measured, [E] estimated with arithmetic, [T] target.
Nothing here changes a binding decision. Where an idea needs one changed, the fit column
says "owner question" and names the gain at stake.

## The picture

When agents do the work, CI stops being a place code goes after it is written. It becomes
the way every build and test runs from the first edit. An agent types `cargo test`. A shim
sends that command to Velnor, which runs it once — locally for a fast red, or sealed on a
warm cluster node for a result that counts. Velnor records what it ran, on what inputs,
and signs the result. The next agent, the PR check and the merge queue look the record up
instead of running the work again. The PR verdict is mostly a ledger lookup plus the small
set of checks that must always run fresh. The Mac stays in the loop for speed of feedback;
it never becomes evidence, because its architecture and kernel differ from the gate's.
Nothing about the gate weakens: two witnesses, whole-tree keys until they are earned,
a cold audit of every landed commit, and a green only on the latest commit.

## Ideas

Trust tiers used below. **T0 advisory:** any run Velnor did not observe in a sealed sandbox
(a Mac host run, an incremental build). It guides the agent and can produce a fast red. It
never fills a witness slot. **T1 candidate:** a run the node agent observed in a sealed
Linux sandbox, signed by that node's signer daemon (design §6.5). It fills at most one
witness slot of one record and is reusable by any request with the same key. **T2 trusted:**
a T1 record whose tree is on `main` and whose cold audit A1 passed. Only T2 records seed
generations and `main`-scope caches. The gate needs what the design already requires: for
every obligation in the vector, a current record with both witness slots passing (T1 or
better), plus every C5 obligation run fresh for that request. Agent-native execution changes
when and where witnesses run. It never reduces their number by itself.

| id | Idea | Mechanism in Velnor (components, commands, receipts, trust) | What it removes, with numbers | Correctness: why no false green; tier and fallback | Fit with decisions | Cost (agent-hours) |
|---|---|---|---|---|---|---|
| N01 | Route every local build and test through Velnor | `velnor-shim` on `PATH` ahead of `cargo` (opt-out `VELNOR_SHIM=off`); a per-host `velnor-agentd` daemon that classifies the command against the released obligation inventory, computes the key, checks the ledger, and runs or replays; `velnor run -- cargo test -p x` as the explicit form. Sealed runs mint T1 receipts; host runs are T0 | The duplicate execution of the agent's last local run by CI. Design §4 row 6 marks this "unmeasured"; at the goal a labelled head costs 2 witnesses × about 3 runner-min of changed obligations [E, §4]. If the agent already paid for one sealed run, CI pays for one witness, not two: about 3 runner-min per head, about 6 per merged PR at 2 labelled heads [E]. The shim's first job is counting how many local runs per task would have hit | The shim never invents a result. On a ledger hit it replays a signed record's outcome and says so; on a miss it executes. A host execution is T0 and fills no slot. Fallback: daemon down or ledger unreachable means plain local cargo, no receipt | Fits (10c, 11, 13) | 60–90 |
| N02 | Remote by default for runs that should count | `velnor-agentd` ships the new git objects as a bundle plus the uncommitted delta as a content-addressed patch set to `velnor-dispatch`; a warm node materialises the exact tree from its mirror (design §7.4), runs the obligation in a sealed sandbox over a verified generation, streams logs back, and returns the exit code and test inventory. Local sealed Docker runs stay available with `--local` | The architecture and kernel mismatch, plus the cold local target. A bundle of one commit's new objects is typically kilobytes to a few megabytes against a 110 MiB tree the node already mirrors [E]; checkout is a 0.25 s reflink clone plus changed files [M] against 6 s per job today [M]. One sealed run replaces the pair "local run, then CI run" for the final state of each task | The receipt is T1 because a cluster node agent observed it, exactly as a PR run. The tree hash must equal the requested tree before step one (§7.4), so a bad bundle or stale mirror fails closed. Fallback: fetch the commit from GitHub, or run in CI | Fits (10, 13, 21, 24); promotes `velnor verify` from stretch to the main path | 50–80, after the ledger and node agent exist |
| N03 | Name the trust tiers and count witnesses in the open | The ledger stores the tier on every receipt and exposes it in `velnor explain` and `Velnor / Details` ("12 run twice, 29 reused, 4 from pre-push, 1 fresh"). `velnor verify --witnesses=2` asks dispatch for both slots at once, on two nodes with different order seeds, one over a different generation or cold until compile reuse graduates | The second, hidden wait. An agent that ran one witness pre-push leaves one witness (about 3 runner-min [E]) on the label path; an agent that ran both leaves only C5 work and the lookup, so the verdict falls from about 35 s [E] to about 3–5 s plus C5 time [E]. Without the counter, agents cannot choose, and reviewers cannot see what was reused | Both slots still need distinct nodes, attempts and seeds (design §6.2). A pre-push witness is bound to its epoch; an epoch revocation voids it like any other receipt. Fallback: any missing slot runs on the PR | Fits (10, 11); makes the existing two-witness rule legible | 15–25 on top of the ledger |
| N04 | Treat arm64 Mac results as advice, amd64 Linux results as evidence | The execution fingerprint already contains architecture, kernel and CPU features (design §6.1), so an arm64 key never equals an amd64 key. `velnor-agentd` on a Mac runs natively at T0 for feedback and routes counting runs to amd64 Linux (N02). Obligations that could differ by architecture (`cfg(target_arch)`, build scripts, proc macros, SIMD, atomics) are flagged by the planner; only that flagged set would ever need a second architecture lane | A whole class of "passed on my Mac, failed in CI" loops, and the wasted idea of a parallel arm64 gate. Decision 9 pauses macOS; the shipped non-amd64 target is `aarch64-unknown-linux-musl`, already cross-compiled and smoke-run under qemu at release [C, §8.4]. Dual-architecting every obligation would double witness cost (about 12 runner-min per merged PR at the goal [E]); flagging confines any future second lane to the small arch-sensitive set | No arm64 receipt can satisfy an amd64 obligation, by key construction. A Mac run that passes proves nothing to the gate; a Mac run that fails is a useful red. Fallback: the amd64 witnesses run regardless | Fits (9, 11, 20). If the owner later wants a real arm64 gate, that is a new question, not a silent widening | 10–20 for the planner flags; the key already does the work |
| N05 | Verify committed-but-unpushed work continuously, at idle priority | `velnor-agentd` watches local commits in agent worktrees. On each commit it may upload the bundle and ask dispatch to fill candidate records for `merge-tree(queue tip, head)` at batch (`cpu.idle`) priority, joining the same records the label will later claim. Nothing is pushed, no status is written, and no PR is needed | Label-to-verdict wait. If the records are already filled, the label resolves by lookup: about 15–35 s off the verdict [E], at a cost of about 5–10 runner-min per speculated head [E, design D19]. With 45% of PR runs cancelled as superseded today [M], roughly half of speculated heads may never be claimed, so the waste is real and must be charged and capped | Candidate records are T1 like any sealed run; the gate is unchanged, because only a labelled head's vector can produce `success`. Unclaimed records simply age out. Fallback: the label runs whatever is missing | **Owner question.** Decision 12 says an unlabelled push starts nothing, and D19 already recommends no for pushed heads. Unpushed commits were not covered, and the agent's own daemon, not a push, is the trigger. Gain at stake: verdicts that are lookups by default. Proposed default until answered: off, except when the agent runs `velnor verify` itself (N02) | 25–40 |
| N06 | Let agents query the ledger before they run anything | `velnor ensure --dry-run --json` returns the obligation vector for a tree with each record's state (satisfied, one slot, in flight with its claimant, missing, C5-always-runs) and the changed input that explains a miss, as `velnor explain` already sketches. `velnor watch <key>` streams a record's completion so an agent can join in-flight work instead of starting its own | Duplicate work between parallel agents. Twelve builders share most of every tree; today each compiles about 4.4 copies of each crate per run [M] and no agent can see another's result. If an agent finds 38 of 41 obligations satisfied, it runs 3. Across stacked and concurrent PRs the design already shares records (§6.3); the query surface is what lets agents act on that instead of re-running first and learning later | Read-only. A dry run executes nothing and writes nothing, so it cannot mint a green. The view is a snapshot: the gate re-resolves the vector under the active epoch before writing `success`. Fallback: a stale answer costs a re-run, never correctness | Fits (10, 15) | 20–35 |
| N07 | Catch flakes during development, not at the gate | When a sealed run passes, dispatch may schedule a twin with a different order seed at batch priority on another node (the second witness, brought forward). A disagreement marks the test in a flake register the planner and agents can read (`velnor flakes`), with counts and first-seen links. During development only; the gate keeps zero retries | Late flake discovery. Today 8.1% of full runs end in a false failure [M], each costing a rerun or a queue ejection that rebuilds everything behind it. Two runs detect a bug that fails with probability p with probability 1−(1−p)²; running the pair during development moves that detection from the labelled head, where it blocks a merge, to the worktree, where it blocks nothing | The first failure always stands for its record; a later pass never overwrites it, so flake hunting cannot retry a test green. Twin runs are T1 executions or audit-stream runs, never a silent second chance. Fallback: the gate's budget, issue and quarantine rules (design §1.3) apply unchanged | Fits (11); spends batch capacity only | 25–40 |
| N08 | Give interactive agent commands their own scheduler class | A fifth priority class, `interactive`, between gate and PR: `merge > gate > interactive > pr > batch`. It is latency-budgeted (a sealed run should start within about 2 s [T] or the shim starts a local T0 run in parallel for a fast red), capped at a set share of each site (say 20% of build slots [E]), preemptible by merge work, and never allowed into the reserved merge slot or A1's slot | Agent idle time, which is the real cost when the worker is a model waiting on a tool call. Cluster arithmetic: 5 workers × 4 build slots = 20 slots; reserving about 4 for interactive work still leaves the merge lane and A1 untouched. Without a class, a 10 s test queues behind batch speculation and the agent stops routing through Velnor, which kills N01 and N02 | Interactive priority changes order, never evidence. A hedged local copy is T0 and can only produce a red sooner, per the design's hedging rule (§6.2). Fallback: over budget, the command queues as PR class or runs locally with no receipt | Fits (decision 15; extends §9's priorities — confirm the share with the owner when sized) | 20–30 in dispatch |
| N09 | Sign local results, but enrol only cluster signers for the gate | Every node keeps its Ed25519 key in a signer daemon outside all sandboxes (design §6.5). A Mac gets the same daemon shape, but its key is enrolled only in the advisory namespace: its signed results are tamper-evident T0 records with a full attestation (tree, command graph digest, binary digests, generation or "host", epoch). Cluster keys are enrolled per node and sandbox class only for certified image digests. The ledger verifies signature, class and append-only log entry before any receipt fills a slot | Nothing directly; it is what makes N01 and N02 safe to reuse. Without it, a buggy or compromised worktree process could write its own "passed" record. Signing on the Mac still pays: a signed T0 record lets one agent show another exactly what ran, and lets the flake register (N07) trust its inputs | A signature proves who observed the run, not that the run counts. Tier comes from enrolment, so a stolen or self-signed Mac key can only forge advice. A cluster key compromise is an emergency revocation by epoch (design §6.5): writes stop, affected successes are marked `failure`, landed commits in the window are audited cold. Fallback: unsigned or unverifiable means re-execute | Fits (8, 11); extends §6.5 to developer hosts | 30–45 |
| N10 | Make the PR verdict a lookup, and say so on the PR | On the label, `velnor-control` calls `ensure(tree, plan)` (design §5.5). Records filled by N02, N05 or other PRs resolve instantly; only missing slots and C5 obligations execute. `Velnor / Verified` is written when the whole vector is satisfied under the active epoch, and `Velnor / Details` lists reused, pre-push, run-now and fresh counts with their sources | The verdict wait in the common case. About 60–75% of merge groups already match the PR-tested tree with auto-enqueue [E]; agent-native pre-push pushes the same effect one stage earlier, to the PR itself. Lookup plus status write is about 1–3 s [E] against a 35 s [E] goal-path verdict and 26.7 min [M, proxy] today. Runner-minutes do not fall again here — the work was paid earlier — and the report must charge it there, per §1.1's accounting | A lookup is sound only if every record's key equals the obligation's key at that commit (contract C1) and every slot holds a passing, unrevoked receipt. A dropped test or obligation against `main`'s inventory fails the vector (C3). Fallback: ledger unreachable means nothing resolves by lookup and everything runs fresh (design §6.5 degraded mode) | Fits (10, 12). The label still starts the verdict and the latest commit still needs its own green | 10–20 beyond `ensure`; mostly reporting |
| N11 | Share one warm generation across an agent's many worktrees | `velnor-agentd` keeps sealed local generations the way a node does (§7.3): a new worktree gets a reflink or APFS clone of the verified target at a fixed logical path, with inputs proven by content against the generation manifest before cargo runs. Locally this is a T0 speed device; remotely it is the node's existing mechanism | Local disk and compile time in parallel-agent work. The runner workspace target is 10.6 GiB [M]; a reflink clone costs 0.25 s and 2.5 MB, then builds 0 crates in 0.31 s on an unchanged tree [M], against 8.6 s and 10.75 GiB for today's copy [M]. Ten worktrees go from about 100 GiB of targets to one base plus deltas | On a Mac the clone only feeds T0 runs, so a stale local base can waste time but cannot mint evidence. On cluster nodes the design's defences hold: dep-info audit, a cold second witness until graduation, and retirement of the generation on any mismatch. Fallback: build cold | Fits (13, 23, 24) | 25–40 locally; the node side already exists in the design |
| N12 | Keep a fast-red inner loop that never pretends to count | The shim's default for uncommitted edits is the design's non-minting lane (§6.2): incremental local build, `cargo check`, changed and historically failing tests first, streamed at once. Sealed execution (N02) runs on commit, on demand, or in the background. The two lanes print different banners, and only the sealed lane ever shows a tier above T0 | The risk that remote-by-default makes the inner loop slower than today and agents route around Velnor. Incremental rebuilds measured 1.5 s against 9.2 s and 10.9 s against 61 s (design §6.2's sources); paying a sandbox start and a WAN round trip for every keystroke edit would lose that. Time to first actionable red is its own metric (§1.1) and this lane owns it | The lane can only say "failed fast" or "no failure seen yet". It never fills a slot and never writes `success`, so an incremental-compilation bug can delay a red but cannot create a green. Fallback: the sealed lane is the only source of passes | Fits (11; Q17 keeps incremental off for minting witnesses) | 15–25 |

### N01 — Route every local build and test through Velnor

The cheapest duplicate to remove is the one nobody measures: the agent runs the test suite
locally, watches it pass, pushes, labels, and pays for the same execution again. A shim makes
Velnor the default path for `cargo build`, `cargo test` and clippy without changing what the
agent types. It classifies the command against the released inventory, so an ad-hoc command
that maps to no obligation runs locally and mints nothing, while a command that does map can
join or replay a record. Adoption is the risk to design for: the shim must be faster than
plain cargo in the common case (N11 and N12), must fail open to plain cargo when Velnor is
down, and must print one line saying what it did ("replayed record from 12:03, key …" or
"ran sealed on velnor-12"). Its first deliverable is evidence: counts of local runs per task,
their keys, and how many would have hit the ledger, which turns §4 row 6 from "unmeasured"
into a number before any trust is placed in the path.

### N02 — Remote by default for runs that should count

A sealed sandbox on the developer's Mac cannot count, however careful it is, because the
fingerprint differs (N04). So the run that should count has to happen where the gate's runs
happen: a warm amd64 Linux node. The transfer is small because the node mirrors the
repository and holds verified generations; the agent uploads only new git objects and any
uncommitted delta, and the node proves the staged tree equals the requested tree before the
first step. Logs stream back so the agent's experience is a normal test run with a banner.
This is `velnor verify` promoted from a day 16–18 stretch item to the standard way an agent
checks work before pushing. The local Docker backend keeps a role for offline work and
conformance, with its results labelled T0, exactly as the user-experience draft already says
for Macs. Bandwidth and latency from a home connection are the open measurement: if the
round trip hurts, N12 keeps the inner loop local and only commits travel.

### N03 — Name the trust tiers and count witnesses in the open

"Executed locally" is true and useless. The useful questions are who observed the run, in
what sandbox, and how many of the two required witnesses it can supply. Naming three tiers
(T0 advisory, T1 candidate, T2 trusted) lets every surface answer those questions in one
word, and it stops a category error the brief could otherwise invite: treating a signature
or a sandbox on a Mac as if it upgraded a result to gate evidence. Counting witnesses in the
open also gives agents a real choice with a price on it. One pre-push witness halves the
label-path execution. Two reduce the label to C5 work and a lookup. Neither choice weakens
the gate, because the slots, their distinctness rules and the epoch all come from the design
as written. My recommendation: default `velnor verify` to one witness, and let the agent ask
for two when it believes the work is final, since the second witness is pure waste on a head
that is about to be rebased.

### N04 — Treat arm64 Mac results as advice, amd64 Linux results as evidence

Both architectures are little-endian and most tests are architecture-blind, which is exactly
why this needs a rule instead of an argument per failure. The differences that bite are
specific: `cfg(target_arch)` code, build scripts that probe the host, proc macros compiled
for the build machine, atomic widths, SIMD paths, page size, and a different kernel under
Docker Desktop. The fingerprint already makes cross-architecture reuse impossible, so the
work here is presentation and routing: never display a Mac pass in a way that reads as a
verdict, and route counting runs to amd64 automatically. The rejected alternative, running
every obligation on both architectures, would double witness cost to defend a target
decision 9 has paused. If macOS or an arm64 product returns, the planner's arch-sensitive
flags say which obligations need a second lane, and that lane earns trust through the same
enrolment and shadow process as any new reuse class.

### N05 — Verify committed-but-unpushed work continuously, at idle priority

Decision 12 exists because per-push CI wasted most of its work: 45% of PR runs were
cancelled and a merged PR had 4.77 runs. That evidence is about pushes drawing full pipeline
runs at full priority. Local commits in an agent's worktree are a different case: no push, no
pipeline, no status, just candidate records filled at `cpu.idle` that a later label may
claim or ignore. The gain is that verification finishes while the agent is still writing the
PR description, and stacked work verifies against its predicted merge tree instead of
against a `main` that will have moved. The waste is equally clear: heads that are rebased
away burn runner-minutes nobody claims. So the proposal ships with a budget — a per-agent,
per-day cap on unclaimed speculation, charged in the same accounting as everything else —
and it stays off until the owner answers, because D19's recommendation on pushed heads
points the other way and the spirit of decision 12 is "do not verify what nobody asked for".

### N06 — Let agents query the ledger before they run anything

Parallel agents duplicate each other in silence. One fixes `velnor-ci` while another rebases
the same change; both run the same suites. The ledger already joins in-flight work by key,
but an agent cannot act on that until after it has started. A dry-run query reverses the
order: the agent learns which of the 41 obligations are satisfied, which are running under
another claimant, and which changed input makes the rest miss. That turns test selection
from a guess into a lookup, and it gives the agent the one fact that decides what to do
next — "your tree is 3 obligations from green, and here is why those 3 miss". The query must
be cheap and read-only, and its answer must be treated as a hint: the gate always re-resolves
the vector itself. Published as JSON, it also becomes the natural tool surface for agent
harnesses, without inventing a separate agent API.

### N07 — Catch flakes during development, not at the gate

Flakes are cheapest where they are discovered today at the highest price: on a labelled head
or in the merge queue, where one false failure ejects a group and rebuilds everything behind
it. Development-time twins move the same detection earlier at batch prices. The second
witness a sealed run will eventually need can run minutes after the first, with a different
seed, while the agent works on something else. A disagreement writes to a flake register
instead of failing a merge, and two disagreements in seven days feed the design's existing
issue rule with evidence attached. The discipline that keeps this honest is that no run ever
retries a failure into a pass: the failing receipt stands, the record stays failed, and a
later green is a new record for a new key or a human's re-run. What development buys is
samples — hundreds of extra executions of the tests agents actually touch, which is what a
1% flake budget needs to be measured rather than hoped for.

### N08 — Give interactive agent commands their own scheduler class

An agent waiting on `cargo test` is a worker idling at full price, so latency for small
interactive commands matters out of proportion to their CPU cost. But interactive work is
also the most preemptible work in the system: the agent can be told "queued, running locally
meanwhile". A dedicated class with a start-latency budget, a site share cap and no access to
the reserved merge or A1 slots captures both facts. The cap is the safety property, not a
detail: at 92–95% worker CPU on the measured bad day, an uncapped interactive class would
let a dozen agents starve PR verification with speculative test runs. Shared work still runs
at its highest live claimant's priority, so a record an interactive command started rises to
merge priority the moment a merge group claims it. If the budget cannot be met, the shim
says so at once and runs locally at T0 rather than queueing in silence.

### N09 — Sign local results, but enrol only cluster signers for the gate

Signing answers "who says this ran". Enrolment answers "may the gate believe them". Keeping
those separate is what lets local signing be useful without being dangerous. The Mac signer
produces attestations that other agents and tools can verify — the exact tree, command graph,
binary digests and outcome — which makes handoffs and flake reports trustworthy as records.
But the ledger accepts a receipt into a witness slot only from a cluster node key enrolled
for that sandbox class on a certified image digest, with the signer outside every sandbox
and the receipt present in the append-only log. A buggy agent process on a Mac can therefore
forge advice but never evidence, and a compromised cluster key triggers the epoch revocation
the design already specifies. Attestation format should stay boring: reuse the receipt the
node agent already signs, with the tier and host identity added, rather than a second format
for local results.

### N10 — Make the PR verdict a lookup, and say so on the PR

The end state of N01, N02 and N05 is that the label stops meaning "start verifying" and
starts meaning "resolve and report". The mechanics already exist: `ensure` computes the
merged tree and the vector, records resolve or schedule, and the fenced writer posts the
status. What agent-native execution adds is that most slots are filled before the label
exists, so the visible verdict time collapses to the C5 set plus a second or two of
resolution. The reporting matters as much as the speed. A reviewer who sees "29 reused, 4
from pre-push, 12 run now, 1 fresh" can judge a green; a reviewer who sees only a fast green
cannot. One accounting warning: a lookup verdict must not be reported as near-zero cost.
The runner-minutes were spent earlier, by named claimants, and the daily report should charge
them to the PR that claimed them so the trade stays visible.

### N11 — Share one warm generation across an agent's many worktrees

Agents multiply worktrees the way developers multiply branches, and each worktree today
carries its own multi-gigabyte target directory. The node's generation mechanism — a sealed,
content-proven target, cloned by reflink at a fixed path, with changed files stamped after
the newest output — works just as well on the machine the agent runs on, and APFS clones give
the same copy-on-write behaviour on a Mac. The local base never needs to be trusted, because
local runs are T0; it only needs to be fast and to retire itself when its manifest disagrees
with the tree. This is the idea that makes the shim welcome instead of tolerated: routing
through Velnor should feel like a speed-up on the first day, before any receipt is ever
reused in CI. It also caps disk growth, which is a real limit when twelve builders each keep
several worktrees on one machine.

### N12 — Keep a fast-red inner loop that never pretends to count

Remote-by-default fails if it taxes the edit-compile-test cycle that agents run dozens of
times per task. So the inner loop stays local, incremental and unsealed, ordered to surface
a red in seconds: check first, changed tests next, historically failing tests after that.
Its output must be visually and mechanically distinct from a sealed result — a different
banner, no tier above T0, no record an agent could mistake for evidence. The design already
draws this line for the gate's fast-red lane and for incremental compilation (Q17); this idea
only extends the same lane to the worktree and makes it the shim's default for uncommitted
changes. Sealed runs then happen at natural moments: on commit, on `velnor verify`, or in
the background at interactive priority. An agent that wants certainty pays for it once, at
the end, instead of on every keystroke.

## Risks

| Idea | How it could produce a false green, or waste | Containment |
|---|---|---|
| N01 shim | A replayed record is mistaken for a fresh pass, or the shim misclassifies a command and replays the wrong obligation's result | Replay prints the record, key and age; classification uses the released inventory only, and an unmapped command always executes. A wrong replay misleads the agent (T0 consequence) but cannot reach the gate |
| N02 remote by default | The uploaded tree differs from the agent's worktree (a missed uncommitted file), so the agent trusts a pass on the wrong code; WAN cost makes agents bypass the shim | The staged tree is hashed in full and the hash is printed and bound into the receipt; `velnor explain` shows it. The gate later verifies the pushed tree, whose hash will differ and miss, so the error costs a re-run. Bypass is measured, and N12 removes the motive |
| N03 tiers and witnesses | `--witnesses=2` becomes the default habit and doubles pre-push cost on heads that get rebased; or a reader treats T1 as "merged-quality" | Default is one witness; the details view always shows slot counts. T1 is named "candidate" everywhere, and only A1 promotes to trusted |
| N04 arm64 advice | An agent reads a Mac pass as a verdict and stops investigating an arch-specific failure that CI then catches late | Keys make the confusion mechanically harmless at the gate. The shim banner says "advisory (arm64 host)" on every local pass |
| N05 continuous verification | Speculation burns the cluster: at about 5–10 runner-min per head and roughly half of heads superseded, a fleet of agents could spend more on unclaimed work than the merge path saves; it could also crowd out batch audits | Off until the owner answers. Then: `cpu.idle` only, a per-agent daily cap on unclaimed minutes, first to be preempted and requeued, and a weekly claimed-against-wasted report that can switch it off |
| N06 ledger queries | An agent trusts a stale snapshot and skips a run it needed for its own feedback; query load hits the ledger on the hot path | The answer is a hint with a timestamp and epoch; skipping a local run is the agent's choice and never the gate's. Reads go to a replica; the gate re-resolves before any `success` |
| N07 flake twins | Twin sampling becomes retry-until-green by habit, or correlated twins (same image, same load) under-detect, as lab C2 warns | A failure is never overwritten by a later pass for the same record. Twins use different nodes and seeds, and detection claims wait for the C2 result; until then the register is advisory |
| N08 interactive class | Interactive work starves PR or audit work under saturation, repeating the measured 92–95% CPU day with better labels | Hard share cap, no access to reserved merge or A1 slots, preemption by merge and gate classes, and per-class wait metrics with the merge-path P90 kept at or under 10 s |
| N09 signing | A Mac key is presented as gate enrolment, or a cluster node key leaks and forged receipts fill slots | Tier comes from enrolment, not from the signature; Mac keys enrol only in the advisory namespace. Cluster keys sit in a signer outside sandboxes, receipts must appear in the append-only log, and a leak is an epoch revocation with cold re-audit of the window |
| N10 lookup verdict | A key that misses an input (the standing risk of all reuse) resolves instantly and at scale, so one wrong key greens many PRs before A1 notices | Unchanged defences: whole-tree keys until enrolment, C5 always fresh, shadow re-execution to about 3,000 comparisons, 2% sampling after, A1 on every landed commit, epoch revocation on the first disagreement. Agent-native pre-push adds executions earlier; it adds no new reuse class by itself |
| N11 local generations | A stale local base hides a failure from the agent (the Q16 older-mtime shape) and the agent pushes broken code confidently | Local consequences only: the sealed witnesses still run. The local clone uses the same content-proven input and stamping rules as §7.3, and the standing canaries run against the local path too |
| N12 fast-red lane | "No failure seen" from the fast lane is quoted as a pass in a PR description or an agent report | The lane has no pass state to quote: it prints only failures or "no failure found (advisory)". Only sealed runs produce a tier, and only the App produces `Velnor / Verified` |

The largest risk is not in any row. It is that agent-native routing succeeds socially —
agents route everything through the shim — and thereby makes the ledger's keys the single
point on which all confidence rests, earlier and more completely than the design's staged
enrolment assumes. The mitigation is sequencing: the shim ships in count-only and advisory
modes first, sealed pre-push fills one witness slot before it may fill two, and each step up
in reuse waits for the same shadow evidence (about 3,000 clean comparisons per class) that
the design demands of every other reuse class. Speed of adoption must not outrun enrolment.

## The first three steps

**Step 1 — The shim in count-only and advisory mode, plus the ledger query (N01, N06, N11,
N12). About 120–180 agent-hours, no owner decision needed.** Ship `velnor-shim` and
`velnor-agentd` on agent hosts: classify cargo commands against the released inventory, keep
one content-proven local generation per Cargo root, run the fast-red lane locally, and record
every run's key, wall time and would-be ledger state. Add `velnor ensure --dry-run --json`
once the ledger exists, read-only. Acceptance: four weeks of counts replace §4 row 6's
"unmeasured" with local runs per task and their hit rate; local wall time is no worse than
plain cargo at P50 and better on repeated worktree runs; nothing the shim does can write a
record above T0. This step pays for itself in local speed and decides, with numbers, how
much duplication the later steps can remove.

**Step 2 — Sealed remote pre-push as one witness, then two (N02, N03, N09, N10). About
110–160 agent-hours after the ledger, node agent and signer land; fits decision 10c and
promotes `velnor verify` from the stretch row.** `velnor verify` uploads a bundle, runs the
vector for `merge-tree(queue tip, head)` in sealed sandboxes, streams logs, and fills
candidate records: first one slot per new key in shadow (the PR still runs both witnesses),
then, after about 3,000 clean comparisons for the class, one counted slot, then
`--witnesses=2`. `Velnor / Details` gains the tier and source counts. Acceptance: planted
failures (the Q16 edit, an undeclared build-script input, a clock-reading test) are red
through the pre-push path; a labelled head whose vector was pre-filled reaches a verdict in
about 3–5 s plus C5 time [T]; every pre-push minute is charged to its claimant in the daily
report. This is the step that turns the PR verdict into a lookup.

**Step 3 — The interactive scheduler class and development-time flake twins (N07, N08).
About 50–70 agent-hours; the scheduler share is confirmed with the owner when sized.** Add
the `interactive` class to `velnor-dispatch` with its cap and latency budget, and schedule
second-seed twins of passing sealed runs at batch priority into a flake register that agents
and the planner can read. Acceptance: interactive sealed runs start within about 2 s at P90
[T] while the merge-path wait stays at or under 10 s at P90 under the replayed 2026-11-18
load; the register reports a per-test disagreement rate from at least 300 rerun pairs, which
is the evidence the 3% then 1% false-failure budget needs. Only after these three steps
should the owner be asked the N05 question, because by then its price — unclaimed
runner-minutes per lookup gained — is measured instead of estimated, and the answer can be
given against the decision 12 evidence that motivated it.
