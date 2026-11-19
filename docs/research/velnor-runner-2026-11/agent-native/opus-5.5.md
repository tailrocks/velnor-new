# Reimagining the runner for AI agents: Claude Opus 5.5

| Field | Value |
|---|---|
| Model | Claude Opus 5.5 |
| Model id | `claude-opus-5-5` |
| Client | Claude Code 2.1.325 (Agent subagent) |
| Reasoning effort | high |
| Date / run | 2026-11-19 |
| Brief | agent-native brief v1 (2026-11-19): the final design, the user-experience draft and the owner decisions |

This is the model's own answer to the owner's prompt to reimagine the runner when AI agents do the work, especially running agents' local verification through Velnor so CI can skip what was already verified, unedited except for formatting.

---

# Velnor for agents: local work that counts (Claude Opus 5.5)

Panel answer to `agent-native-brief-v1.md`. It builds on the v3 design (`velnor-runner-architecture.md`,
cited as §N), the UX draft, owner decisions 1–24 (cited as "decision N") and the open owner questions
D1–D22. Tags: **[M]** measured, **[E]** estimate with arithmetic, **[T]** target, **[C]** read from code.
New measurements for this answer come from this Mac's Claude Code transcripts for velnor-new and its
worktrees, 14 days to 2026-11-19 (scripts in `scripts/`). They cover one machine and one agent harness, so
they are a lower bound on fleet activity.

| Local fact [M, transcripts, 14 days] | Value |
|---|---|
| Sessions that ran cargo; cargo invocations | 108; 2,259 (nextest 1,369, fmt 859, clippy 805, build 485, check 40) |
| Invocations per session | median 9, P90 55, max 204 |
| Wall time per call (1,433 timed calls) | median 2–3 s, P90 7–31 s by kind; 3.1 h in total; none longer than 300 s |
| nextest runs that failed (compile errors included) | 427 of 1,337 (32%) |
| Reruns of the same nextest command with no edit in between; fail→pass flips | 36; 0 |
| Sessions that ran cargo on a tree before editing it in that session | 44 of 107 (122 of 2,882 calls) |
| `git commit` calls, each running `hk.pkl`'s fmt + check + clippy + deny hooks | 749 (23 with `--no-verify`) |
| First-parent commits on `main` per day, 2026-11-16 to 11-18 | 56, 46, 52 [M, `git log`] |

These numbers change the brief's premise. Agents' local runs are many, narrow, incremental and cheap. They
find reds: a third of them fail. They are almost never repeated on an unchanged tree. So most of the loss is
not Mac seconds spent twice. The loss is that none of this work can count, so CI repeats everything after
the push: 4.77 PR runs × 52.9 runner-min per merged PR today [M], and two cluster witnesses plus the A1 cold
audit in the design. The aim is not to make local results count. It is to make the agent's *last* local run
happen inside Velnor, where it is evidence.

## The picture

When agents write the code, the runner stops being where finished work is sent and becomes where work happens.
Every cargo command an agent runs goes through `velnor`. The Mac (arm64, incremental, fast single core) finds reds.
The cluster (amd64, sealed, signed) mints greens. A verification record exists once per key, whoever asks first:
an agent at minute 3 of its task, another agent, the PR or the merge group. Every later request is a lookup.
A Mac result is never evidence: arm64 macOS is another key, and a process cannot witness itself.
"Verified" keeps exactly the design's meaning: two signed Velnor executions on distinct cluster nodes, seeds and
generations, then the cold audit A1. Agent-native Velnor moves *when* the witnesses run, not *what* counts.
The agent's "I think I'm done" becomes `velnor verify`. Its two witnesses run while the agent writes the PR text,
against the head merged with the queue tip. The label then publishes what is already verified: the verdict is a
ledger lookup plus the clock- and network-reading (C5) obligations, in seconds instead of minutes, and the merge
group gets the same lookup when its tree matches. Agents ask the ledger before they run anything, so two agents
never verify the same tree twice. Interactive agent commands get their own scheduling class, between gate and PR.
The gate and the label flow do not change. The work moves earlier, into the loop where the agent waits anyway.

## Ideas

| id | Idea | Mechanism in Velnor | What it removes | Correctness: why no false green; trust tier; fallback | Fit | Agent-h |
|---|---|---|---|---|---|---|
| N01 | Route every agent cargo command through `velnor` | `velnor` binary, pinned in `mise.toml`, provides `cargo` and `cargo-nextest` shims. A per-user `velnord` daemon (launchd on a Mac). Each call records argv, the Git **tree** of the worktree (a dirty tree is written through a temporary index), the toolchain and the outcome in a local journal, mirrored to the ledger as T0 observations | Nothing by itself. It is the substrate for N02, N07, N08 and N12. Overhead ≤ 50 ms per call [T], against a 2–3 s median call [M] | T0 has zero weight in the gate. The shim never changes what cargo runs locally, so local behaviour stays the same. If `velnord` is down, the shim execs real cargo | fits | 14 |
| N02 | Remote by default for obligation-shaped commands | The shim matches argv against the released obligation inventory (G03). On a match, or on `velnor verify`, it sends the tree as a thin pack to the site's node mirror. The node agent materialises the tree, checks its hash, runs the **released command graph** of the obligation in a warm amd64 sandbox and mints a T2 receipt into a candidate record. A local run races it for a fast red | The last local full run before a push becomes witness 1. Executions per changed obligation fall from 3 (Mac + two CI twins) to 2. Wait at the label falls from one twin build to a lookup | The receipt comes from the node agent running the inventory's command (C4), never from the agent's argv. The key is the tree Velnor checked, so if the agent later commits a different tree the result is a miss, not a false green. Tier T2. Fallback: run at the label as today | fits (decision 10c) | 18 + V11's 10 |
| N03 | Agent shells on the cluster (experimental) | An agent's whole Bash session runs in a sticky `velnor shell` sandbox on an amd64 node: worktree on XFS over a reflink of the generation, incremental target, mirrors and tool store mounted. Every command is observed. The Mac is only the editor and the harness | The arm64 problem disappears. Every final run is already a T2 witness. No tree upload is needed. Wide rebuilds use 16–64 CPUs instead of a laptop's cores | Same minting rules as N02: only obligation-shaped executions in a fresh minting sandbox fill slots. The sticky shell is incremental and non-minting (T1c). Fallback: the local shell | owner question (capacity on a cluster shared with 2 orgs; design §5 left it out of scope) | 40 |
| N04 | Trust tiers and the witness count | T0 agent-claimed; T1 Velnor-observed on a Mac (signed, N10); T1c observed on the cluster in a non-minting lane; T2 cluster minting receipt (C4); T3 trusted (on `main`, A1 passed). A record slot accepts T2 only. The gate needs **2 T2 witnesses per new key** (distinct node, attempt, seed, generation or cold), then A1 as the third execution | Nothing is removed from the gate. It makes the agent's own T2 runs fill slots, and it gives T0/T1 data a defined use: hints only | No tier below T2 can reach a slot, so no new path to green exists. Bonus: witnesses separated in time (agent at t0, label at t1) are less correlated than simultaneous twins, which answers objection O05. Fallback: refuse the slot and run | fits (C4, D10) | 6 |
| N05 | arm64 vs amd64 | (a) Arch, kernel and CPU features stay in every key (§6.1), so a Mac result never matches a cluster key. Use N02 or N03. (b) Add an `aarch64-linux` obligation class on 2 Hetzner Ampere nodes, because the release ships aarch64 musl bytes that are only smoke-tested under qemu today (R03). (c) A Mac Docker sandbox as one aarch64 witness | (a) stops Macs from duplicating cluster work on a key that can never match. (b) adds native aarch64 coverage, and does not cut amd64 work. (c) saves one arm-node execution per aarch64 key | (a) is sound by key. (b) adds obligations, so it can only add reds. (c) uses another kernel (LinuxKit) and a key that the agent's machine holds. It needs one cluster aarch64 witness plus A1 on aarch64. Never cross-arch equivalence: `cfg(target_arch)`, memory ordering and 16K pages differ | (a) fits; (b) owner question (hardware, new inventory class); (c) owner question (a developer machine producing gate evidence; design ruling N07 rejects it) | (a) 0; (b) 20; (c) 12 |
| N06 | Continuous verification of committed, unpushed work | (a) `velnor verify --async` from a `post-commit` hook, run only when the agent asks (an `hk` step or a flag). (b) `velnord` verifies **every** commit in every worktree at `cpu.idle` against `main`'s tip, minting candidate records | (a) The label becomes a lookup for the commits the agent chose. (b) Every label is a lookup: 35 s → ~3–5 s at the goal, 6/18 min → C5 only at day 5.2 [E]. Cost: 749 commits / 14 d ≈ 53/day from this Mac against ~15–52 landings/day, so ≥ 70% of the work is superseded [E] | Candidate records only, the same rules as PR work, never `trusted` before A1. Nothing writes a status. Fallback: none needed | (a) fits (10c); **(b) owner question: conflicts with decision 12** ("a push starts no heavy verification") and with D19's recommended no | (a) 6; (b) 8 |
| N07 | Agents ask the ledger before running | `velnor ledger ask` (and the shim, transparently): for a tree and obligation, reply with `trusted` / `candidate` / `in-flight (join)` / `failed on main at <sha>` / `none`. A matching record answers the agent's command with the stored result and its log | 41% of sessions run cargo on a tree before editing it [M]: baseline and review runs on trees that are often already verified. Two agents verifying the same PR head join one execution | Read-only for the gate: it answers the agent and never writes a status. A `failed on main` answer could lead an agent to ignore its own regression, but the gate still runs, so the cost is agent time, not a false green. Ledger down → the command runs | fits | 8 |
| N08 | Flake detection during development | Per test ID and key, the ledger keeps outcome history from T1c and T2 runs. A pass and a fail on the same key make a **proven** flake. A test that failed on a Mac (T0, no compile error) while the cluster passes the same tree becomes a candidate, and `velnord` queues 20 repeats at `cpu.idle`. Results feed §1.3's issue rule | Flakes found before a PR pays for them. Today 8.1% of full runs had a false failure, and each costs a rerun or an ejection that rebuilds the entries behind it [M] | Advisory only: no retry, no quarantine without §1.3's owner rule, and the gate ignores the flake DB. Organic local evidence is thin (36 unchanged reruns, 0 flips [M]), so stress runs, not local volume, supply it | fits | 8 |
| N09 | Scheduler class for interactive agent commands | Priorities become merge > gate > **interactive** > PR = other orgs > batch. Interactive means an agent is blocked on the result. A token bucket per agent identity (e.g. 30 CPU-min/h) applies, with demotion to PR priority past 120 s. One 16-CPU slot per site may be borrowed by restartable work | An agent's queue wait at high load: queueing grows ~19× service time at 95% utilisation (§9). For 2,259 calls in 14 d, 30 s saved per call is ~19 agent-h per 14 d on this Mac alone [E] | Pure scheduling: no key, slot or status changes. The demotion rule stops interactive work from starving PR verdicts. Merge and gate stay above it | fits (changes §9's order, not a decision) | 6 |
| N10 | Signing and attestation of local results | The Mac `velnord` holds a P-256 key in the Secure Enclave, enrolled per machine by the owner. T1 receipts are in-toto statements: tree, command, execution fingerprint, binary digests, sandbox profile digest. Appended to a per-machine log in the ledger | Nothing in the gate. It gives T1 data provenance for N08, conformance runs on the Docker backend (decision 13) and N05c if the owner allows it | Decision 8: authors are buggy, not hostile, so the signature proves "Velnor observed this on that machine". It does not prove independence. T1 never fills an amd64 slot. Fallback: unsigned means T0 | fits | 10 |
| N11 | The PR verdict becomes a lookup | `velnor verify` targets two bases: the queue tip and `main`. At the label, `velnor-control` resolves the head's vector against records and runs only C5 obligations and misses. With auto-merge (D2) and an empty queue, the group's tree equals the verified one, so the group verdict is a lookup too | Label → verdict ≈ C5 + 3 s. P(tip unchanged) with 46–56 landings/day: e^(−t/27 min), so 93% if the agent labels 2 min after verify and 69% after 10 min [E]. Push-to-merge → label + seconds + GitHub's ~45 s | The status is written for a head only after its own vector resolves to passing records with full slots (decision 12: never green for an unverified commit). A moved tip changes the key, so it is a miss. C5 always runs | fits | 6 on V11 |
| N12 | Agent-readable verdicts | `--json` on `verify`, `explain` and `logs`. A failure card lists the failing test IDs, the obligation, the exact reproduce command for the shim, the log tail and the reasons a check was reused or re-run. `Velnor / Details` embeds the same JSON | Agents stop scraping GitHub logs (minutes of tool calls and tokens per red), and fewer runs are repeated just to see the error | Output only | fits | 6 |
| N13 | Predict merge-queue ejections across parallel agents | When two or more PRs are labelled, `velnord` verifies their pairwise merges at batch priority (via `merge-tree`). A failing pair is posted to both PRs' `Velnor / Details`, never to the status | Ejections rebuild every entry behind them. Many agents in parallel make semantic conflicts likely. Gain unmeasured: count ejections first | Never writes `success`. The queue still verifies the real group | fits decision 10's speculation clause **only with numbers**; otherwise drop | 12 |
| N14 | Route `hk` hooks through Velnor | The `hk.pkl` steps `cargo_fmt`, `cargo_clippy` and `cargo_deny` call the shim. Their keys equal the fmt, clippy and deny obligations, so they become ledger lookups or N02 witnesses | ~726 hooked commits / 14 d [M] × 3–4 cargo jobs each, invisible in the census above, repeated again in CI | The same as N02. A local hook result is T0 | fits | 4 |

**N01, the shim.** Agents keep typing `cargo nextest run -p velnor-ci`. `mise` puts `velnor`'s `cargo` first
on `PATH` inside the repository, so the harness needs no change and nothing has to learn new habits. The one
piece of work that matters is the tree, not the commit: keys are already the whole Git tree (§6.1).
`velnord` writes the worktree to a tree object through a temporary index (`GIT_INDEX_FILE`, tracked files
plus untracked files that are not ignored), so uncommitted work has a key. When the agent later commits that
exact tree, every record it earned still matches. The local journal also makes the census in this document a
standing metric: share of calls that are obligation-shaped, share answered by lookup, and seconds spent by
agents waiting.

**N02, remote by default.** "Remote by default" applies only where the result can count: commands whose argv
equals an inventory obligation (`cargo fmt --all --check`, the per-root clippy, `cargo nextest run --profile
ci -p X`) and `velnor verify`. Narrow loops (`-E 'test(foo)'`, `cargo check`) stay local, because the Mac's
single core beats an EPYC 9454P for incremental edits and network latency would cost more than it saves. The
upload follows the UX proposal: a thin pack of the objects the node mirror lacks (KB for a typical change),
never a pushed branch, so decision 12's "a push starts nothing" is untouched. A local run of the same
command starts in parallel, and the first red from either side is reported. Racing is allowed by decision 10
because it is measurably faster to red, and a local green is labelled provisional. The minting rule is the
main correctness point: the agent's argv only *selects* an obligation; the node agent executes the
obligation's released command graph on the materialised tree. So a narrowed filter or an extra feature flag
cannot be passed off as the obligation.

**N03, agent shells on the cluster.** This is the bold version. Agents are processes, not people, and they
have no reason to compile on a laptop. If the harness runs its tools in a Velnor sandbox on an amd64 node,
then local *is* CI's platform. The arm64 question goes away, every command is observed, and the loop gets
16–64 CPUs and the node's warm generation. The sticky shell is incremental and therefore non-minting (ruling
Q17). Its value is that `velnor verify` from inside it costs only a reflink clone, not an upload. The open
question is capacity. Five workers are shared with ChainArgos and jackin. 108 sessions in 14 days with
median calls of 2–3 s suggest a small steady load, but a session holds memory while idle: 24 GiB per build
sandbox (§9). The owner should decide whether agents may hold cluster sandboxes, and how many.

**N04, trust tiers.** The question "how many trusted witnesses" has the design's answer, and I would not
lower it. Two T2 witnesses per new key keep detection at 1−(1−p)², which is parity with today. A1 stays the
third execution. Agent-native changes *who requests* witness 1, not how many witnesses there are. T0 and T1
exist so that the large volume of local data has a defined, harmless use: ordering (run locally-failing
tests first in the fast-red lane), flake candidates (N08) and duplicate hints (N07). One real gain is
independence. Today's design runs the twins together; lab C2 worries that they are correlated (same load,
same moment). A witness minted at t0 by the agent and another at t1 by the label are separated in time and
load, so they are closer to the independent-trials model the 1−(1−p)² arithmetic assumes.

**N05, the arm64 vs amd64 problem.** There are three honest answers, and one dishonest one that must stay
rejected. The dishonest one is "arm64 passed, so amd64 passes". Rust code differs by `cfg(target_arch)`,
weaker ARM memory ordering exposes races x86 hides (and the reverse), Apple's 16K pages differ from Linux's
4K, and SIMD paths differ. Answer (a): run where the key is (N02, N03). Answer (b): treat aarch64 as its own
inventory class. Today the shipped aarch64 musl bytes are smoke-tested only under qemu. Two Ampere nodes
(Hetzner CAX class) would test them natively. Decision 21 already makes arch a backend capability, so the
core needs no change. Answer (c), owner question: let a Mac's Docker sandbox (linux/arm64 on the LinuxKit
kernel, signed per N10) fill *one* aarch64 slot, with the second always on a cluster arm node and A1 also
run on aarch64. The gain is one arm execution per aarch64 key. The cost is that a developer machine
contributes gate evidence for the first time, which design ruling N07 rejects. I recommend (a) now, (b) when
aarch64 releases matter, and (c) not before (b) has run a month.

**N06, continuous verification of unpushed commits.** Variant (a) fits decision 10(c) as written: the agent
asks, through a hook it enables for commits it considers candidates, and Velnor verifies. Variant (b) is the
ambitious one and **conflicts with decision 12**. Decision 12 says a push starts no heavy verification, and
the reason is waste: 45% of PR runs were cancelled. Verifying every commit is worse on waste. From this Mac,
749 commits in 14 days against roughly 15–52 landings per day means at least 70% of per-commit verifications
would be superseded [E]. At the goal's ~10 runner-min per head [E], that is ~530 runner-min/day, ~370 of it
wasted, all at `cpu.idle`. The gain is that every label becomes a lookup without the agent asking: about 30
s saved per labelled head at the goal, but 6/18 min per head at day 5.2. **Owner question:** allow (b) at
`cpu.idle` only, with a per-site cap and an automatic switch-off when superseded idle work pushes PR P90
wait above 10 s? My recommendation: yes during days 4–10, when verdicts are minutes and idle capacity
exists, then re-decide on measured waste. Default: (a) only.

**N07, ledger queries between agents.** With whole-tree keys, cross-agent reuse happens on identical trees.
Those are more common for agents than for humans. Several reviewers verify the same PR head. Every new task
starts from the same `main` tip. Stacked PRs share bases. 44 of 107 sessions ran cargo on a tree they had
not yet edited [M]. A trusted record for `main@tip` answers the baseline instantly. An in-flight execution
for the same key is joined (§6.2 joins), so two agents never verify the same head twice. When per-root keys
land (V08), the hit rate grows, because an agent editing `generator/` gets every `ci-helper/` obligation
answered.

**N08, flakes during development.** The data refutes the easy version. Agents almost never rerun an
unchanged tree (36 times in 14 days, 0 flips), so local volume will not reveal flakes. Three sources do
work. Same-key pass/fail pairs come from N02's raced runs and from witness pairs. A Mac failure that the
cluster does not reproduce on the same tree is a flake or a platform difference; both are worth an issue.
And `velnord` stress-runs, at idle, the tests touched by an agent's change (20 repeats, seeded orders),
which also feeds the design's 300-rerun-pair evidence (V09) much sooner than organic PR traffic would. A
flake record is advisory, and no failure is ever forgiven because a test "is known flaky".

**N09, interactive priority.** The design's order (merge > gate > PR = other orgs > batch) has no class for
"an agent is blocked on this right now". At 95% utilisation, waiting is ~19× service time, so a 20 s remote
witness can wait minutes. An agent waiting is the most expensive idle in this system: it burns wall time,
and it pushes the agent toward running locally, which counts for nothing. The interactive class sits below
merge and gate, so landing is never delayed. Its token bucket and the 120 s demotion keep a runaway agent
from taking a site.

**N10, signing local results.** Decision 8's threat model is buggy, not hostile, authors, so the signature's
job is integrity and provenance: "this observation came from Velnor on the enrolled machine `<machine>`,
in a sealed container, with these binary digests". It is not proof of independence. The Secure Enclave keeps
the key away from the agent's own process; `velnord`'s code-signing identity is the only principal allowed
to sign. T1 records go to a per-machine append-only log, as cluster receipts do (§6.5). Their uses are flake
provenance, Docker-backend conformance runs on Macs, and N05c if allowed. They never fill an amd64 slot, so
a stolen Mac key costs nothing except hint quality. Revocation means removing the machine's enrolment.

**N11, the verdict as a lookup.** Section 6.3 already has the lookup for rebase-only pushes and identical
group trees. Agent-native work makes it the *common* case: the agent runs `velnor verify` on its candidate
merged with the queue tip, writes the PR description while the witnesses run, pushes, labels and turns on
auto-merge. The label resolves the vector and finds every non-C5 slot full. With landings at 46–56 per day,
the tip survives a 2-minute gap 93% of the time [E]. Verifying against two bases (queue tip and `main`)
covers the case where the queue empties in between. The C5 obligations still run fresh on every request
(§1.2 C5), so the floor of a lookup verdict is the C5 critical path. That makes the repository work in §6.1
(an injectable clock and declared fixtures) the main lever for agent-native speed, more than any runner
change.

**N12, verdicts agents can read.** Agents read failures through `gh run view --log` and grep, which costs
minutes, tokens and often a rerun just to see the error. A JSON failure card with the failing test IDs, the
obligation, a reproduce command the shim understands and a bounded log tail lets an agent move from red to
fix in one step. `velnor explain --json` also tells an agent *why* something re-ran ("changed input
`ci-helper/.../plan.rs`"), which stops agents from guessing at cache behaviour.

**N13, ejection prediction.** Parallel agents produce PRs that pass alone and fail together. GitHub's queue
finds this late, and every ejection rebuilds the entries behind it. Pairwise `merge-tree` verification of
labelled heads at batch priority finds it while the agents are still active. Decision 10 permits speculation
only with the numbers shown. So the first step is counting ejections caused by semantic conflicts (not
flakes) over 100 groups. If fewer than 2% of groups are ejected for this reason, delete the idea.

**N14, hooks.** `hk.pkl` already runs fmt, check, clippy and deny on every commit, with two jobs. That is
749 commit calls in 14 days on this Mac. Every one of those checks is repeated by CI's fmt, clippy and deny
obligations. Routed through the shim, a hook on a tree whose obligation record exists returns in
milliseconds. One whose key is new can run remotely and become witness 1. It is the cheapest entry point: no
agent behaviour changes at all.

## Risks

| Idea | How it could produce a false green | Containment |
|---|---|---|
| N01 | The dirty-tree snapshot misses a file (ignored, untracked or generated) that the agent's commit later includes, and the old record is reused | The key is the tree Velnor checked. A different committed tree is a different key, so it is a miss, never a hit. Generated files that are not in Git are not inputs to any obligation, because sandboxes see only the materialised tree |
| N02 | The shim maps a narrowed command (filter, `--features`, `-p` subset) to a wider obligation and mints it | The receipt comes only from the node agent executing the inventory's command graph (C4). argv is a selector, never a description of what ran. The expected test-ID inventory (C3) fails a narrow run |
| N02 | An agent uploads objects whose content differs from what it believes | Velnor computes every hash. The record binds the tree Velnor executed, which is the truth whatever the agent believed |
| N03 | Incremental state in a sticky shell leaks into a receipt | Sticky shells are T1c and never mint. Minting always happens in a fresh sandbox over a verified generation or cold (§7.3) |
| N04 | T1 or T0 data is promoted into a slot by a bug | The slot type accepts only T2 receipts signed by a cluster signer class (V02 test: "a forged report mints 0 receipts"). Add the canary "a T1 receipt for an amd64 key never fills a slot" to the hourly set |
| N05c | A Mac witness for aarch64 is stale or wrong (LinuxKit kernel, a host-mounted cache) | One cluster arm witness is always required, and A1 runs on aarch64. The Mac witness is refused if its sandbox profile digest is not the released one. First disagreement → Mac witnesses off |
| N06b | Waste: idle work grows to fill the cluster, and memory pressure evicts warm generations | `cpu.idle`, killed at the memory watermark (§9), a per-site cap, and auto-off when PR wait P90 > 10 s. Report the superseded share daily |
| N07 | "Already verified" or "red on main" answers mislead an agent | Answers never touch statuses. Wrong advice costs agent time only. Answers carry the record's epoch, and a revoked epoch answers "none" |
| N08 | The flake DB is used to excuse a real failure | No code path reads the flake DB in the gate. Retries stay 0. Quarantine needs §1.3's owner rule |
| N09 | Interactive load starves PR verdicts | Below merge and gate; token bucket; demotion past 120 s; PR wait P90 is an alarmed metric |
| N10 | A Mac key is stolen or the signer is buggy | T1 can fill no amd64 slot. Enrolment is revocable, and the per-machine log shows the window |
| N11 | A verdict from a lookup on a head whose tree differs from the verified one, or a stale tip | The status is written per head SHA after resolving that head's own vector. A different tree is a different key. Epoch revocation (§6.5) re-verifies open heads |
| N13 | A speculative pair result is mistaken for a gate result | It writes only to `Velnor / Details`. The queue verifies the real group |
| N14 | `--no-verify` commits skip hooks | Hooks are T0 hints. Nothing depends on them having run |

The shared waste risk is that agents trust the lookup and stop running anything locally, losing the 32% of
reds the Mac finds in seconds. N02's race keeps the local run, and the fast-red value stays on the laptop.

## The first three steps

1. **The shim and the census (N01 + N14), about 18 agent-h, no dependency on the ledger.** Ship `velnor` as
   a `cargo` shim through `mise`, with the tree snapshot, the local journal and the `hk` routing. In week
   one it only measures: the share of calls that are obligation-shaped, the share whose tree matches a later
   PR head, and the minutes agents wait. Pass criterion: overhead ≤ 50 ms per call at P90, and a daily
   report that reproduces this document's census. This gives the numbers the owner questions N06b and N03
   need.
2. **Move `velnor verify` from the stretch (day 16–18) to right after V04 exact-tree records, and make it
   the shim's remote mode (N02 + N11 + N12), about 30 agent-h on top of V11's 10.** decisions.md already
   rules G04 "after exact-tree records". Add the tree upload as a thin pack, the two-base verification
   (queue tip and `main`), the minting-by-inventory rule, the local race and the JSON failure card. Pass
   criterion: on 50 agent PRs, label-to-verdict ≤ C5 path + 5 s for every preflighted head whose tip did not
   move, 0 slot fills from T0/T1 (a canary), and 0 disagreements against A1.
3. **Ledger queries and the interactive class (N07 + N09 + N04's tier enforcement), about 20 agent-h.** The
   shim asks before it runs and joins in-flight work. `velnor-dispatch` gets the interactive class with the
   token bucket. The slot type refuses everything below T2. Pass criterion: baseline runs on `main` tips
   answered from records in ≥ 90% of sessions that run them, and interactive wait P90 ≤ 5 s at 80% cluster
   load in the K04 replay.

**Owner questions raised (each runs on its default until answered):**

- **N06b**, every commit verified at idle (conflicts with decision 12; gain about 30 s per head at the goal
  and 6/18 min at day 5.2; waste ≥ 70% superseded). Default: no.
- **N03**, agent shells on the cluster (capacity shared with two orgs). Default: no, with a 2-agent pilot
  proposed.
- **N05b**, an aarch64 inventory class on Ampere nodes. Default: no.
- **N05c**, a Mac witness for aarch64. Default: no.
- **N13**, only if the ejection count justifies it. Default: no.
