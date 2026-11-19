# Reimagining the runner for AI agents: Claude Fable 5.2

| Field | Value |
|---|---|
| Model | Claude Fable 5.2 |
| Model id | `claude-fable-5-2` |
| Client | Claude Code 2.1.325 (Agent subagent) |
| Reasoning effort | high |
| Date / run | 2026-11-19 |
| Brief | agent-native brief v1 (2026-11-19): the final design, the user-experience draft and the owner decisions |

This is the model's own answer to the owner's prompt to reimagine the runner when AI agents do the work, especially running agents' local verification through Velnor so CI can skip what was already verified, unedited except for formatting.

---

# Velnor for agents: verify in the loop, look up at the gate (Claude Fable 5.2)

## The picture

1. Today a runner waits for a push, then repeats on other hardware what an agent just did on a Mac. With agents, verification should happen once, inside the agent's loop, and the PR check should only look the result up.
2. The unit of CI stops being "a job started by an event". It is **an obligation on a tree**, requested by whoever needs it first: an agent's command, a label, the merge queue. All three call the same `ensure`.
3. Agents keep typing `cargo`. The cargo wrapper slot in `mise.toml` (today `mbx`) becomes Velnor's front end, and a small daemon on the Mac keeps each worktree's Git tree id and sends changed blobs to a cluster node. Nothing touches GitHub.
4. "Verified locally" is redefined as "verified **from** the local loop, on the gate's hardware". The command runs in the same sealed amd64 Linux sandbox as the gate, on a warm node; the agent gets the output, the ledger gets signed candidate receipts.
5. A result computed on the Mac never becomes evidence: the architecture, the kernel and the key holder are all wrong. It is still recorded, as **advice**: test order, flake history, a predicted verdict, and skipping repeated local runs.
6. Trust is set by where a result ran, never by who signed it. The gate still needs two cluster witnesses per new key, and audit A1 still rebuilds every landed commit cold. No owner decision is weakened.
7. Dev runs target what the gate will judge: the worktree merged with the queue tip. `velnor ship` verifies, pushes, labels and enables auto-merge; the label finds every obligation resolved and `Velnor / Verified` turns green in about 3 s.
8. Before running anything an agent asks the ledger: is this tree verified, is this failure already on `main`, is someone else running it now? Diagnosis is shared, not repeated by every agent.
9. Agents stop polling GitHub. Velnor pushes the verdict to the agent as a short failure list with the ledger's context, not as a log.
10. The honest size of the gain: about 30 s per labelled head at the design's goal, about 6 min per head on the way there, and most of all fewer failed labelled heads (27 of 77 completed PR runs failed).
11. The same traffic pays twice: 462 test runs a day from agents are the comparisons that graduate the design's reuse classes, and the place where flakes are caught before they reach the queue.
12. Five questions go to the owner. Only one conflicts with a decision: verifying commits before anyone asks (decision 12). The default for each keeps decisions 1–24 as they are.

## Ideas

**Measured for this page** [M, transcripts]: Claude Code transcripts of the velnor projects on the
owner's Mac (757 files, 2026-11-01 to 11-19, 10 active days; other agent tools are not counted, so
these are lower bounds). Of 61,337 shell calls, 7,271 contain a cargo build, test or lint command:
nextest 4,621, fmt 2,336, clippy 1,764, build 899, check 288, `cargo test` 193. Wall time from call
to result (it includes chained commands): nextest P50 7.2 s, P90 71.0 s; clippy 6.4 / 43.4 s; build
5.8 / 50.8 s; about 29 h in test, build and clippy calls. Also 2,089 `git commit`, 1,698 `git push`,
570 `git worktree add` and 3,168 CI polling calls (`gh pr checks`, `gh run view|watch|list`). 319 of
4,396 `nextest run` calls use `--workspace` (7%); at least 92% are scoped to a package, a binary or
a test name [heuristic]. `main` gained 214 first-parent commits in the period [M, `git log`], so per
landed commit: 34 cargo calls, 8 min of cargo wall, 9.8 commits, 7.9 pushes, 14.8 polls. The Mac is
arm64 with 18 cores and 128 GiB; it holds 132 worktrees [M]. The cluster has 5 workers with 96
threads each (evidence pack), so 20 build slots of 16 CPU: 480 slot-hours a day.

| id | Idea | Mechanism in Velnor | What it removes | Correctness: why no false green; tier; fallback | Fit | Agent-hours |
|---|---|---|---|---|---|---|
| N01 | Route every local command through Velnor | `velnor cargo …` takes the cargo wrapper slot (`mise.toml:43-45`, today `mbx` [C]); `velnord`, one daemon per Mac, tracks worktrees; the released planner maps each command to obligations or to "scratch"; every run writes a record | 7,271 cargo calls routed with no change in agent habits [M]; 193 `cargo test` calls that break the AGENTS.md rule are refused | Routing decides where a command runs, never what counts. The record's tier comes from the node that executed it (N04). A mapping error makes the run scratch: no receipt. Fallback: plain local cargo | Fits 10c, 13, 18 | 14 |
| N02 | Address work in progress by tree, not by commit or push | A private index per worktree and `git write-tree` give the tree id; missing objects go as a thin pack to the node mirror under `refs/velnor/wip/…` (7-day TTL); the node re-hashes the materialised tree | Uncommitted, committed and pushed work share one key. Pushes made only to get CI: 7.9 pushes per landed commit [M] | Content addressing: a client cannot name a tree whose bytes differ, and the node checks the whole tree before the first step (decision 24's rule). Tier T2. Fallback: push and label | Fits 7, 24 | 10 |
| N03 | Remote by default, in two lanes | Lane A, interactive: a sticky session sandbox per worktree, cloned from `main`'s generation, incremental, streams output, mints nothing. Lane B, minting: the released obligation command over a verified generation, two witnesses. Routing per command class follows a measurement | A new worktree starts warm: 0.25 s clone and 0.31 s no-op build against 92 s cold [M, cluster]; 570 worktrees made in 10 days [M]. 18 Mac cores shared by all agents against 480 cluster threads | Lane A writes only the `advisory` type, which the resolver cannot read. Lane B is the PR's own path (section 6.4, the `velnor verify` row). Fallback: the local lane | Fits 10c, 13 | 24 |
| N04 | Four trust tiers; the gate needs two cluster witnesses | T0 native Mac. T1 sealed Linux sandbox on the Mac, device-signed. T2 cluster sandbox, node-signed, `candidate`. T3 `trusted`: on `main`, A1 passed. The gate counts two T2 receipts per new key, as now; a minted dev run can be the first | The gate's second execution of anything minted from the loop; repeated local runs of one tree (advisory skip) | T0 and T1 are a separate Rust type in a separate stream: no code path leads to a witness slot. Independence stays by node, seed and generation | Fits 8, 11, D10. A T1 receipt as second witness is OQ2 | 6 |
| N05 | arm64 against amd64: architecture stays in the key | A Mac result can never equal a gate key. So the loop executes on amd64 nodes (N03) and Mac runs are advice. Optional: a non-gating `linux-arm64` lane fed by T1 runs | Nothing at the gate. It adds coverage: aarch64 binaries get only a qemu smoke test today | Additive only: an arm64 result can add a red or a note, never satisfy an amd64 obligation. Cross-compiling or Rosetta changes the fingerprint, so it never hits | Fits 11, 13. arm64 as gate evidence is OQ3 | 4 (+16) |
| N06 | Dev runs target the gate's subject; `velnor ship`; the verdict is a lookup | The node computes S = merge-tree(queue tip, worktree tree) and runs obligations on S. `velnor ship` = `ensure` S, push, label with the agent's App identity, enable auto-merge | Label to green: about 35 s [E] at the goal and about 6 min [E] at whole-tree keys become about 3 s on a hit. Hit rate 83% with a tip ≤ 1 min old, 37% at 20 min [M, replay] | The control plane derives the PR's subject tree itself, never from the client; equal key or no hit. A moved tip is a miss, which runs. C5 obligations run anyway. Tier T2. Fallback: the label runs it | Fits 10c, 12 | 8 (+10, V11) |
| N07 | Continuous verification of committed, unpushed work | `velnord` requests a mint when a commit has been HEAD for 90 s; batch priority; cancelled by the next commit; misses re-run when the tip moves | First red at commit + 1–2 min, not at label; the label is a lookup without the agent asking. Cost: 209 commits a day × 6 runner-min = 21 runner-h a day at the goal (4.4% of capacity); 265 runner-h (55%) at whole-tree keys | The receipts are N06's (tier T2); only the trigger changes. Preemptible, so it cannot delay the merge path. Fallback: explicit `verify` | **OQ1** (decision 12) | 12 |
| N08 | Agents ask the ledger before they run | `velnor resolve <tree>`, `velnor baseline`, `velnor why-red`, in-flight join for dev requests; the same calls as an MCP server | A baseline run on clean `main`: 0 s. Two agents on one tree: one execution. Repeated diagnosis of a failure that is already on `main` | Read-only. Answers follow the gate's key rules; advisory answers are marked advisory | Fits 10b | 12 |
| N09 | Flake detection during development | (a) two receipt-grade runs of one key that disagree are counted at once; (b) `verify` soaks new or changed test IDs 20 times under different seeds and CPU pressure; (c) Mac results give per-test priors | 8.1% false failures per full run today [M]. A soak finds a 5% flake with 64% probability and a 10% flake with 88%, before it can eject a queue group | A soak failure is a failing receipt on that key: it blocks. Nothing here can add a pass | Fits 11, section 1.3 | 14 |
| N10 | An `interactive` scheduling class | merge > gate > interactive > PR > batch. A budget per agent identity (30 CPU-min, refilled at 4 CPU-min a minute [proposed]); over budget, a request runs at PR priority. A warm session pool | The bar is the Mac: nextest P50 7.2 s [M]. A 13 s acquire would triple it; attach must stay ≤ 1 s | Priority changes order, never results. A preempted minting run follows section 6.2: it fails or restarts fresh | **OQ4** (other organisations' CI) | 8 |
| N11 | Signing and attestation | Requests: an envelope (identity, tree, base, intent) signed with a short-lived key from `velnor-control`. T2: the node signer of section 6.5. T0, T1: a Secure Enclave device key signs a DSSE statement `velnor.dev/receipt/v1` | Nothing alone. It makes N04's tiers checkable and every result attributable | A device signature proves which device signed, not that the result is true. A signature never raises a tier | Fits 8, 11 | 8 |
| N12 | Dev traffic graduates reuse classes | Each receipt-grade dev run is paired, at batch priority, with the shadow re-execution C6 asks for | 462 nextest calls a day [M] against about 11 landed commits a day: the 3,000 comparisons that set days 7.5–10 arrive sooner | Shadow runs only compare; one disagreement suspends the class | Fits 11, 15 | 6 |
| N13 | Push the verdict to the agent | `velnor wait <tree or PR>` long-polls the control plane and returns failures only: test ID, signature, 20 log lines, the ledger's context | 3,168 polling calls [M], 14.8 per landed commit, and the log tokens they carry | Read-only | Fits 7 | 6 |
| N14 | Per-test-case receipts (experimental) | A receipt per (test binary digest, test ID, definition); an obligation passes when every expected ID has two passing witnesses | Filtered runs would count: at least 92% of nextest calls are scoped below the workspace [M, heuristic] | Sound only if tests do not interfere. Shadow: composed result against whole-binary run, 3,000 clean, then 2% sampled. Until then no slot is filled | Fits 11 after graduation | 30 |
| N15 | Agent workspaces as cluster sandboxes (experimental) | The agent's shell runs in a Velnor sandbox beside the mirror and the generations | The sync, the architecture gap and the Mac's limit: 132 worktrees on one machine [M] | A workspace is class `dev` and never a witness; receipts still come only from lane B | Fits 13, 21; scope is OQ5 | 40 |

**N01. Route every local command through Velnor.** The routing point already exists: `mise.toml`
sends every `cargo` through a wrapper. `velnor cargo` takes that slot and calls `mbx` itself where
the local lane needs it, so there is one wrapper, replaced in one change (decision 18). The front
end asks the released planner at the base commit what the command is. `cargo nextest run -p
velnor-ci --test it` is one obligation; `cargo clippy` in a root is one; a run with a test-name
filter or extra flags is "scratch". It then picks a lane (N03) and prints one machine-readable last
line: the tree, each obligation's state, and what was minted. A CLI alone would need every agent
prompt changed; a daemon alone cannot see commands. The three parts do different jobs: the wrapper
catches the command, the CLI names the new verbs (`verify`, `ship`, `resolve`, `wait`), and the
daemon keeps the tree id and the session warm.

**N02. Address work in progress by tree.** The design's keys are Git trees, so a commit is not
needed to verify. `velnord` keeps a private index per worktree (the agent's own index is never
touched), hashes changed files as they are saved, and uploads the missing objects to the node
mirror, as the user-experience draft proposes ("an uploaded bundle, not a pushed branch"). The
repository's secret scanners (kingfisher, betterleaks) run on the delta first; a finding refuses the
upload. An untracked fixture that the commit later omits gives a different tree, so the PR misses and
runs: the test fails where it should. An ignored file is absent from the sandbox, as in CI.

**N03. Remote by default, in two lanes.** A minting run must build over a verified generation with
incremental compilation off (section 7.3), which costs 8–20 s for a helper crate [E]. The Mac's
median test call takes 7.2 s [M]. So minting every command would slow the loop. Lane A answers the
agent: a session sandbox that keeps its target directory between commands, on gate hardware, sealed
like the gate. It is the design's fast-red lane with greens shown too, and it mints nothing. Lane B
mints when asked (`verify`, `ship`) or, if OQ1 is granted, in the background. A command that is a
whole obligation and is already cheap goes straight to lane B and streams its first witness.
**Routing is measured, not assumed:** replay 200 recorded dev commands on both paths; a command
class runs remotely by default only when its remote P50 is within 1 s of the Mac's. Otherwise the
local lane answers and the cluster is used for minting only. Running on both sides is allowed by
decision 10 only with these numbers shown.

**N04. Trust tiers and the witness count.** The answer to "how many trusted witnesses": **two T2
receipts for a new key, zero from T0 or T1**, plus A1 as the third execution. Considered and
rejected: one T2 alone (D10 rules two witnesses, and D21's default keeps the second compile as the
cross-check); one T2 plus one T1 (OQ2). The new part is the lower tiers. T0 and T1 results answer the owner's sentence at their own
level: when a second agent or a reviewing subagent runs the same command on the same tree on the
same Mac, Velnor returns the recorded advisory result. That skip cannot cause a false green,
because the gate never reads the advisory stream. Its key (tree, command, toolchain, lockfile, an
environment allow-list) may be weaker than the gate's, and that is acceptable for advice only.

**N05. arm64 against amd64.** Three options were weighed. (1) Run the loop on amd64 nodes: adopted
(N03). (2) Build for amd64 on the Mac, by cross-compiling and Rosetta in a Linux VM: rejected as
evidence, since the kernel and CPU features differ, so the key differs; it has no use that T0 does
not give more cheaply. (3) Make arm64 gate evidence. With microVM backends (decision 21) a Mac and
an arm64 cluster node could run the same guest kernel image and vCPU count and so share a
fingerprint. But the cluster is amd64, the host key is still held by the author, and a cluster
witness is still needed, so the gain is about 3 runner-min per head and no wall time: not worth it
(OQ3). What is worth having is the reverse: T1 runs on Macs are real Linux arm64 executions, and
the project ships aarch64 binaries that only a qemu smoke test covers. A non-gating `linux-arm64`
lane reports their failures on `Velnor / Details`.

**N06. The verdict as a lookup.** The PR is judged on its head merged with the queue tip, so loop
receipts hit only if the loop ran on that same tree. The node therefore merges before it runs (a
conflict is reported at once and the raw tree runs as scratch). The remaining enemy is tip
movement: 83% of trees still match 1 min later, 37% after 20 min [M, the design's replay]. `velnor
ship` closes the gap to seconds by doing verify, push, label and auto-merge as one command. Then:
the label webhook arrives, the control plane derives the vector for the subject it computed itself,
every key resolves, the C5 obligations run, and the status is written. The merge group repeats the
lookup when its tree is the same. Per-root keys later make a moved tip harmless for most heads.
Push-to-merge then approaches GitHub's own share (19 s to form a group, 26 s to merge).

**N07. Continuous verification (OQ1).** Decision 12 says CI runs on demand and names the cause:
45% of PR runs were cancelled. A local commit is not a push, but verifying it unasked breaks the
decision's intent, so this is an owner question. The cost depends on the stage. At the goal (6
runner-min per head) 209 commits a day cost 21 runner-hours, 4.4% of capacity, and about half that
with the 90 s settle rule. At whole-tree keys (76 runner-min per head) the same traffic would take
55%. The workers were sampled at 92–95% CPU on 2026-11-18, so "idle capacity" must be shown first.

**N08. Agents ask the ledger.** Three lookups remove work that no cache removes today. `velnor
baseline` answers "is `main` green?" from trusted records, instead of a test run on a clean
checkout. `velnor resolve` shows which obligations of a tree are passing, failing, running or
unknown, and a dev request joins a running record instead of starting a copy (section 6.2's join).
`velnor why-red` stores a failure signature per failing test and answers: the same failure is on
`main` at this commit; or this key has passed and failed (a flake); or it first fails on your tree
and the base passes; and which open PR holds a passing candidate for it. Two of `main`'s last
five commits are P0 fixes for a red `main` (#1400, #1381); every agent that met it had to find
that out alone.

**N09. Flake detection.** Development produces far more executions than CI (4,621 nextest calls
against 124 push runs in the evidence). Same-key disagreement is the exact definition of a false
failure, so each one is counted without waiting for a "Re-run". An agent cannot retry its way to a
pass: every failing receipt-grade run stays on the record and counts against the test. New tests
are where flakes start, so `verify` runs each new or changed test ID 20 times (a few seconds for
most tests), and a soak failure blocks that record until the test is fixed.

**N10. Scheduling.** An interactive request is short and has an agent waiting on it; a PR request
is the same work a few minutes later. So interactive sits above PR and below merge and gate, with a
per-identity budget so a loop of full suites cannot starve verdicts. This also ranks velnor-new's
agents above ChainArgos's and jackin's CI on shared nodes, which the owner must decide (OQ4).

**N11. Signing.** A Mac cannot prove to the cluster what software produced a result: there is no
remote attestation for a command-line process. A device signature therefore gives accountability
(which device, which identity, when) and nothing more. Request envelopes give quotas and an audit
trail for N10 and N02. The statement format (DSSE, in-toto style) is the one the release path
already uses for provenance, so one verifier serves both.

**N12 and N13.** The design's dates after day 5 are set by evidence counts, not coding. Dev runs
are that evidence if each is paired with its shadow run. And agents today learn a verdict by
polling GitHub 14.8 times per landed commit; a long-poll that returns only failures saves the calls
and, more important for agents, the context spent reading logs.

**N14 and N15 (experimental).** Per-test receipts would let the many filtered runs count, but they
change what a receipt means; they stay in shadow until composition matches whole-binary runs.
Workspaces on the cluster remove the Mac from the loop entirely; this is a product decision, not a
runner feature, and comes last.

**Owner questions.**

| # | Question | Gain at stake | Recommendation and default |
|---|---|---|---|
| OQ1 | May Velnor verify a commit that has settled for 90 s, before any push or label (N07)? Decision 12 says on demand | First red about 1–2 min after the commit; label to green about 3 s without the agent asking; 21 runner-h a day at the goal | Yes, but only after per-root keys and with measured slack; opt-in per worktree. Default: no, explicit `verify` and `ship` only |
| OQ2 | May one device-signed T1 receipt be the second witness? Decisions 8 and 11, ruling D10 | About 3 runner-min per head; no wall time | No. Default: no |
| OQ3 | Should arm64 results gate (a required `linux-arm64` lane, or arm64 gate hardware)? | Coverage of aarch64 binaries before release | Advisory lane only. Default: nothing built |
| OQ4 | May velnor-new's interactive commands outrank PR-class work, including other organisations' CI? | Loop latency near the Mac's 7.2 s under load | Yes, with the per-identity budget. Default: interactive equals PR |
| OQ5 | Should agents work inside cluster sandboxes instead of on Macs (N15)? | The whole local/remote gap | Trial with one agent after N03. Default: no |

## Risks

| Idea | How it could produce a false green, or waste | Containment |
|---|---|---|
| N01 | The planner maps a command to the wrong obligation, and a narrower run fills its slot | The node runs the inventory's command and expected test IDs, never the agent's command line; a mismatch is scratch |
| N02 | A tree id is accepted without its bytes; a secret in an untracked file is uploaded | Full tree hash on the node before the first step; scanners on the delta; WIP refs expire and never leave the site |
| N03 | An advisory green is read as a verdict; lane A and lane B compile the same change twice | Separate type and stream; the status writer reads only signed T2 and T3 records. The duplicate compile is charged in runner-minutes and removed by routing whole-obligation commands to lane B |
| N04 | A T0 or T1 record reaches a witness slot through a bug | No conversion exists in the type system; a canary writes a forged advisory pass for a failing key every 10 min and the gate must stay red |
| N05 | An arm64 pass is taken for an amd64 obligation | Architecture is in the definition digest; the arm64 lane has its own obligation names, listed in no ruleset |
| N06 | The client names a subject tree that is not the PR's; a stale tip is assumed | The control plane computes the subject from GitHub's refs; a different tree is a different key |
| N07 | Waste: superseded commits; starving the merge path | 90 s settle, batch priority, preemption, a daily runner-minute cap; off by default |
| N08 | An agent trusts an advisory "passed" and skips a real run | Advisory answers are labelled; nothing an agent skips removes an obligation from the gate |
| N09 | A soak or dev failure is discarded to get a pass; the soak is waste on stable tests | Failing receipts are permanent for their key; the soak covers only new or changed test IDs |
| N10 | A preempted witness is replaced by a retry that hides a failure | Only a platform-proven abort allows a fresh attempt (section 6.2); budgets cap the class |
| N11 | A signature is treated as trust | Tier is a property of the executing node's key class, checked by the ledger |
| N12 | Biased shadow samples graduate a class early | Shadow pairs are chosen by the ledger, per obligation kind, with a held-out set; A1 and 2% sampling stay |
| N13 | None for correctness; a stale long-poll answer | Each answer carries the subject SHA, tree and epoch |
| N14 | Tests pass alone and fail together | Shadow comparison against whole-binary runs; the class is off until 3,000 are clean |
| N15 | A workspace's state leaks into a minting sandbox | Minting sandboxes read only the trusted scope and their own outputs (section 6.2) |

A general risk is the claim itself. The cluster may not beat an M-series Mac on a hot incremental
edit. This page claims only that the loop's results can count; the routing rule in N03 decides
where each command class runs, from measurement.

## The first three steps

1. **The front end with the advisory ledger, on the Mac only (about 30 agent-hours; N01, N02, N04,
   N11's device key).** `velnor cargo` in the wrapper slot, `velnord`, tree ids for work in
   progress, command-to-obligation mapping, and a local advisory stream with local-to-local skips.
   It needs no cluster component, so it can start today, beside phase 0. It delivers the first
   skips, enforces "nextest, never `cargo test`", and produces the dataset every later number
   needs: tree, command, outcome and duration for every agent command. Done when 95% of cargo calls
   in a week of transcripts carry a Velnor record.
2. **`velnor verify` and `velnor ship` straight after exact-tree records (about 28 agent-hours;
   N06, N08's `resolve`, N13).** This is plan item V11 moved from days 16–18 to the day after
   whole-tree `ensure` works (about day 5.2), as the decision log's G04 row already adopts. The tree arrives by
   upload; both witnesses run on merge-tree(queue tip, tree); `ship` pushes, labels and waits. At
   that stage the verdict is about 6 min, so a hit saves the most it ever will. Done when 50
   shipped PRs show label-to-green under 10 s at P50, with zero verdicts the ledger did not hold.
3. **The interactive lane, the `interactive` class and the development feeds (about 52
   agent-hours; N03, N10, N09, N12).** First the replay of 200 recorded commands on both paths,
   which sets the routing default per command class. Then session sandboxes, same-key disagreement
   counting, the 20-run soak in `verify`, and shadow pairing of dev runs. Done when the routing
   table is published with its numbers and the graduation counters of section 1.2 (C6) advance
   from dev traffic.

After step 3 the owner has the measurements to answer OQ1: the real commit rate, the cost per head
and the cluster's slack.
