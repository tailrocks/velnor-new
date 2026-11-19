# Reimagining the runner for AI agents: GPT-6.2-Astra

| Field | Value |
|---|---|
| Model | GPT-6.2-Astra |
| Model id | `gpt-6.2-astra` |
| Client | codex-cli 0.169.0 (codex exec) |
| Reasoning effort | high |
| Date / run | 2026-11-19 |
| Brief | agent-native brief v1 (2026-11-19): the final design, the user-experience draft and the owner decisions |

This is the model's own answer to the owner's prompt to reimagine the runner when AI agents do the work, especially running agents' local verification through Velnor so CI can skip what was already verified, unedited except for formatting.

---

# Velnor from the agent’s terminal

## The picture

An agent on a Mac types `velnor cargo test -p velnor-ci`. A test name and its failing assertion come back to the terminal.
If the command matches a required check, the agent also sees which commit and tree were tested, where they ran, and how much of the check is complete.
The usual destination for that work is a warm Linux worker. The agent can choose a sealed local sandbox for a quick edit or an offline session.
Another agent working on the same inputs can follow the running check. Neither agent needs to keep a terminal open for the other to get the result.
When someone labels the PR, Velnor plans the latest head against the queue tip and asks which checks still need work.
Two qualified observations made during development can answer that question just as well as two made after labelling.
GitHub still decides what enters and leaves its merge queue. Only Velnor’s status writer can tell GitHub that a particular head passed.

## Ideas

| ID | Idea | Mechanism in Velnor | Work or wait removed | Why a green is sound, trust tier and fallback | Owner decision fit | Estimated agent-hours |
|---|---|---|---|---|---|---:|
| N01 | Give Cargo commands a receipt | An explicit CLI maps an invocation to a released obligation or runs it for feedback | One matching development invocation replaces one later execution of that witness slot | Exact command, inputs and inventory required for a candidate slot. Partial and unknown commands stay diagnostic | Fits 10c, 11, 18 | 14–22 |
| N02 | Run eligible commands on the warm cluster by default | Upload missing Git objects, build the subject on the worker, stream output. Offer sealed Docker locally | With a 35 s eligible path and a 25 s head start, up to 25 s comes off the label path | The node verifies the staged tree. Only cluster production work can fill candidate slots. Offline yields local feedback | Fits 9, 10c, 13, 21, 24 | 20–32 |
| N03 | Count qualified witnesses across requests | Ledger reserves two independent slots per new key. Local observations, candidates and audited main records have different uses | With 0, 1 or 2 valid slots, a request needs 2, 1 or 0 additional witnesses for a comparable key | Two distinct qualified passes required. Failure blocks. Missing evidence runs on eligible nodes | Fits 8, 10, 11, 20 | 10–18 |
| N04 | Route by actual execution platform | Planner matches host, target, toolchain, kernel, CPU and sandbox capabilities | Routing one required amd64 test from the Mac to amd64 at the outset saves one redundant arm64 run when only CI feedback was wanted | Exact platform and executor key. Cross-compiles and emulated executions have their own identity. Mismatch runs on the required platform | Fits 9, 11, 13, 20, 21 | 8–14 |
| N05 | Sign observations at their source | Isolated node signer attests the observed execution. Ledger verifies signer class and site-log inclusion | A valid signed pair can serve ten matching requests with two executions instead of twenty | Enrolled cluster signers can attest candidate slots. A local signature authenticates a diagnostic only. Invalid evidence is refused | Fits 8, 10, 11 | 12–20 |
| N06 | Let agents query and join the ledger | A read-only plan shows misses, partial records and live claims. An explicit run attaches through the ledger’s ensure operation | Four concurrent agents asking for one new comparable pair use two executions instead of eight | Server recomputes keys, enforces scope and epoch, and fills slots atomically. Unknown state requires fresh proof | Fits 8, 10, 11, 12 | 10–16 |
| N07 | Verify committed but unpushed edits continuously | Optional worktree observer submits the latest immutable commit at idle priority | Could hide 15–25 s of compile on a hit at an estimated 5–10 runner-min per fully speculated head | Candidate-only, same two witnesses, no status. Default is explicit invocation with the watcher off | **Owner question** on 12 versus 10c | 8–14 if approved |
| N08 | Find flakes while the agent can still reproduce them | Stream the first failure and preserve test ID, seed, load and binary digest. Compare discordant observations | Surfacing a failure at 5 s in a 40 s run gives the agent 35 s earlier feedback | Diagnostics cannot make a green. A failed required witness blocks. Reruns for investigation stay separate | Fits 10, 11 | 8–14 |
| N09 | Schedule for the agent waiting at a terminal | Dispatch interactive requests fairly within spare capacity. A joined record inherits its highest live claimant’s priority | Starting a 10 s check ahead of a 30 s batch backlog can return its result 30 s earlier | Scheduling changes start time only. Protected merge and audit capacity, independent slots and abort proofs still apply | Fits 10, 11, 15 | 8–14 |
| N10 | Make the labelled PR mostly a lookup | On label and merge-group events, the App resolves an authoritative obligation vector and runs fresh work and misses | Of 40 comparable obligations with complete records for 37, only six witness executions remain, down from 80 on a full miss | Complete, unrevoked vector for the exact subject. A missing status blocks. On lookup failure, verify fresh | Fits 7, 10, 11, 12 | 14–24 |

Effort is incremental design, implementation and review assuming the architecture’s planner, ledger, node agent and App interfaces exist.
Examples are arithmetic, not measured gains. Rows often describe the same saved execution. Count it once.
The 35 s figure is the architecture’s estimate for the helper/generator class. The 15–25 s and 5–10 runner-min estimates come from its D19 scenario.

**N01. Preserve the meaning of the command.** Start with `velnor cargo build`, `velnor cargo test` and `velnor cargo clippy`.
Resolve the working directory, workspace selection, arguments, features, profile, toolchain and environment against the base-controlled inventory.
The proposed CLI prints the command and subject it will run, streams stdout and stderr, and returns the observed exit status.
A filtered test stays filtered. A successful build proves only an eligible build obligation. Cargo test and the CI nextest profile may select different tests.
For a mismatch, offer the exact required invocation as a separate choice. An unsupported invocation gets ordinary diagnostic execution or an explicit unsupported error.
An agent that needs host build products chooses local Cargo. Remote execution returns platform-labelled artifact digests and an explicit download path.
Start with the explicit CLI so tools that spawn Cargo and scripts that expect a populated local target directory keep their existing semantics.

**N02. Move the checkout to the worker while the edit is still in context.** On a clean worktree, bind the request to an immutable commit.
For the literal Cargo command, run the requested tree. For explicit `velnor verify`, resolve a proposed merge tree using the queue tip and the head,
display both SHAs, and submit the released plan for that tree. Never silently run a different tree for an ordinary command.
Use the node mirror and a bundle of missing committed objects. The worker verifies the commit, reconstructs the full tree and checks staged contents before execution.
Upload no host environment, target directory or developer credentials. A dirty worktree needs an explicit snapshot of the selected edits for remote diagnostics,
or local execution on the edits. A head-only upload of a dirty tree would answer a different question.
The terminal can reconnect by request ID. Compare upload, placement, output transfer and witness time with the agent’s local time for the same edit.
The measured 0.25 s clone and 0.31 s no-op build describe an unchanged warm worker. A real edit also pays compilation, tests and possibly a cold second witness.

**N03. Keep three levels of evidence.** Host Cargo output and sealed Mac Docker receipts are development diagnostics with zero gate weight.
An enrolled cluster node in production mode can contribute one candidate witness. Two passing slots form a record reusable by other PRs with the same key.
The trusted main scope opens only after the tree lands and its A1 audit passes. Receipt-minting work reads trusted inputs and its own outputs.
A new key needs distinct physical nodes, attempts and test orders, with separate verified generations or a cold build. Before compile reuse graduates,
the second witness builds cold and the executed binary digests must agree. An explicit full verify asks for both slots.
One ordinary command may satisfy just one. If it fails, its record blocks. A later claimant cannot fill the record by selecting two other passing attempts.
A1 is separate audit evidence. Reference and shadow executions issue no production receipts. Native is the default.
A deliberately configured, certified Official production executor uses its own keys. Changing executor requires re-verification.

**N04. Treat the laptop as a place to ask for Linux work.** Distinguish Darwin/arm64 host Cargo, Linux/arm64 under Docker on Apple Silicon,
Linux/amd64 under emulation, and the required native Linux/amd64 cluster class. Include host and target triples, actual execution architecture,
translation mode, kernel, OS, CPU features/count, image, sandbox profile, tool versions and executor identity in the definition.
A matching target triple alone says little about where build scripts, proc macros and tests ran.
An arm64 cross-build can supply only the specific cross-build obligation when its entire definition and trust class match.
For instance, the design’s Linux arm64 musl release cross-build and QEMU smoke test have distinct requirements from a native amd64 test.
Keep Docker and Kubernetes behind the capability API. Docker without an enforced file-access audit uses whole-tree keys.
If native arm64 feedback matters to the author, run it explicitly as a separate observation. Mac release jobs stay paused.

**N05. Give a signature a narrow meaning.** A workstation signer, if enabled, signs a local diagnostic in its own namespace for the agent’s history.
For CI credit the authority is the per-node, per-sandbox-class Ed25519 signer outside both the job and the node agent’s general process.
It signs a versioned envelope for a journalled attempt only after Velnor observed the released process graph, exits and expected test outcomes.
Bind repository, key, tree, definition, executor role and digest, fingerprint, epoch, slot, node, attempt, generation or cold marker,
order seed, observed test IDs and digests of test binaries and every invoked helper including `CARGO_BIN_EXE_*`.
The ledger checks the signature, signer enrolment, authorised class, active epoch and inclusion in the site’s append-only log.
A replay of the same envelope is idempotent. It cannot satisfy a different slot. The key that signs local diagnostics has no candidate authority.
The receipt describes verification only. Release promotion still checks the exact attested release bytes and waits for A1.

**N06. Expose what another agent has already paid for.** Proposed `velnor plan --json` returns the requested subject, resolved tree,
required checks, key-match or miss reason, current witness count and any shareable in-flight request. A query does no heavy work.
An agent can see that another worktree is finishing the same clippy check and attach to its log with `velnor follow <request>`.
An agent that needs completion acquires its own claim through `velnor verify` or the matching Cargo command. The service derives the key itself.
If one agent rebases or disconnects, release only that claim according to its request lifetime. Other claimants keep the execution.
Make the query advisory. Between reading it and acting, another agent may finish, cancel or revoke the record.
Atomic claim and slot updates settle the race. Fresh-only obligations have request-specific execution identities and cannot join across requests.

**N07. Owner question on decision 12.** May Velnor start background verification when a local commit appears, before anyone requests CI?
This extends the architecture’s D19 question back one step, before push. A commit hook or daemon that routinely starts full checks would change
the owner’s on-demand policy even if it never writes a GitHub status. Keep the shipped default at explicit `velnor verify` under decision 10c.
If approved, coalesce to the newest commit per worktree, cap runner-minutes per agent, use batch capacity and cancel obsolete unshared claims.
Track commits whose work was later consumed and the time those records took off the critical path.
If 4 of 10 speculated commits are consumed and the other 6 each cost 5 runner-min, 30 runner-min bought no later reuse.
Historical cancellation of 45% of PR runs motivates the experiment’s budget. It does not measure the reuse rate of local commits.

**N08. Catch disagreement before the queue.** The agent can investigate the first failing ID while the second witness is still running.
Stream the seed, command, timeout, resource pressure, subject and fixture digests so another agent can reproduce the failure.
A pass and a fail on matched inputs is a suspected flake. Differing OSes, features or test inventories are different experiments.
Keep a non-minting fast-red lane for focused tests, and preserve both observations when they exist.
A later diagnostic pass never clears the required failure. The explicit rerequest path preserves prior outcomes and runs the failed checks under the normal witness policy.
Track required false failures separately from developer suspicions. Two false failures of one test in seven days trigger owner triage.
The design targets at most 3% false failures per paired record initially and 1% at goal, with zero automatic retries.

**N09. Protect time to first feedback without delaying a merge.** Place explicit foreground work below merge and gate,
share available PR-class service fairly with existing CI, and put optional warm-up and speculation in batch. Bound outstanding builds per agent.
Preempt only restartable batch work. Keep the reserved 16-CPU merge build slot, two gate CPUs and the separate A1 build slot per site.
Admit a missing merge pair on distinct nodes. When a merge or labelled PR joins an interactive request, raise its priority to that of the highest live claimant.
On a busy day the gain for one terminal can be an equal loss for somebody else. Replay the busy arrival trace with other organisations’ CI present.
Measure interactive wait P50/P90, time until the slower witness finishes, merge wait P90 and A1 lag before adopting this ordering.

**N10. Replan at each GitHub boundary.** A label applied to a ready PR arms it. Draft pushes, becoming ready and unarmed pushes start no PR CI.
A push while armed immediately supersedes the old request. Unlabelling stops new work and cancels the claim. It writes no new green.
An earlier verified SHA retains its history. A new unverified head has a missing or non-success required status and is blocked.
The released planner and base inventory derive obligations for the current head merged with the queue tip (or main on queue-tip conflict).
Resolve all keys against the active epoch. Run every fresh-only check on this request, even for an identical tree. Run or finish misses with both witnesses.
The fenced App rechecks the request generation, subject and epoch, writes `Velnor / Verified` on the latest PR head SHA and reads it back.
The same App-bound status is required on each exact merge-group SHA, with ALLGREEN and an empty bypass list.
Resolve each group’s own plan. Whole-plan reuse needs the same tested tree and comparable inputs. Different trees can still share equal enrolled keys.
The design estimates a 60–75% whole-tree group match under auto-enqueue, against 16% observed under today’s flow.
Neither figure predicts a lookup-only verdict until the share of fresh-only work is measured. The UX target of about 10 s after label applies to a qualified hit.

## Risks

| ID | Where a pass could be wrong, or work wasted | How Velnor detects and contains it |
|---|---|---|
| N01 | An alias changes features, a test filter drops cases, or a PR edits the planner | Use the released plan and base inventory. Compare expected ID set and multiplicities to observed outcomes. Missing, skipped, duplicate, extra or zero expected-test execution blocks the test obligation. Unmatched CLI requests remain diagnostic |
| N02 | A changing checkout or stale mirror supplies another tree. An upload or a cold worker costs more than local feedback | Recompute commit and tree after materialisation, including modes and symlinks. Refetch missing objects or fail. Report queue and upload costs separately. Local fallback is explicit and diagnostic |
| N03 | The same machine, generation or test order supplies both passes. A failing attempt is discarded | Check pair independence and every attempt disposition. A lost, timed-out or cancelled execution cannot be replaced as though it passed. Only proven infrastructure absence or policy preemption permits replacement. If the independence lab finds correlation, rerun changed obligations on the merge group even on an identical tree |
| N04 | A cross-compiled binary passes under emulation while native execution fails | Plant an architecture-dependent failing test. Changing host, target, emulator or CPU features must miss. If identity is unknown, schedule the required qualified platform |
| N05 | A copied signature or compromised node upgrades local results or reuses revoked evidence | Verify class, slot, epoch and log entry on admission and eligibility at consumption. Revoke affected evidence, freeze success writes, fail dependent open heads and queued groups, reverify and audit affected landings cold |
| N06 | Two agents claim one slot, cancel each other, or narrow a key across a sibling-file change | Persist intent and compare-and-swap each slot. Test disconnect, duplicate completion and revocation during a join. Fall back to whole-tree keys or fresh execution. Content-addressed cache hits alone cannot satisfy a check |
| N07 | A commit storm burns cluster time or a watcher labels a PR | Keep the feature off pending the owner decision. Watcher credentials cannot write labels or statuses. Under any trial, cut off speculation at its quota and report stranded runner-minutes |
| N08 | A dashboard chooses the best of several runs or drops a repeatedly failing test | Keep immutable attempt history and separate diagnostics. Enforce the exact inventory. Require equivalent blocking coverage or an expiring owner exception for quarantine |
| N09 | Agents flood the foreground lane or preemption conceals a failing witness | Use quotas, age waiting requests, and keep merge/A1 reservations. Journal proven preemptions. Unknown execution state blocks. Revert the scheduling preference if it worsens merge wait or audit lag under load |
| N10 | A stale label event, queue-tip change, missing dependency or revoked epoch produces a status for the wrong subject | Reconcile GitHub state every 10 s, deduplicate events, fence writes and read back statuses. Inject a superseding push, destroyed group and partitioned writer. If evidence or publication authority is unavailable, keep the gate blocking |

Across all ten ideas, prove key completeness before admitting reuse. Start with the full tree, exact package and feature selection,
dependency lockfiles, commands, flags, profiles, fixtures, allowed environment, tool and execution fingerprints.
Run on an intact tree. An unknown changed path selects the full inventory. Narrow a key only with a reviewed, enforced dependency contract
covering directory membership, absent paths and ancestor configuration. A read trace can refute that contract. It cannot establish completeness.
Unfixed wall time, randomness, network inputs, event data, commit identity and publishing require fresh work under C5.
A verified key can include a genuinely fixed input, such as an injected fixture clock. Nightly A2 still exercises shifted dates.

Shadow each new reuse class with fresh executions until about 3,000 clean comparisons per kind, then sample 2%.
A disagreement immediately suspends reuse for that class and its dependants. Diagnose flake versus wrong key while blocked.
Every landed first-parent commit gets A1, a cold rebuild and rerun of changed obligations with digest comparison.
Sampling tests the implementation. Eligibility still requires the complete key, full inventory and two valid witnesses on every hit.

## The first three steps

1. **Make one real terminal command explain its eligibility.** Start with the helper workspace and one clippy obligation.
   Implement the explicit CLI, a read-only plan and local diagnostic output. Record command-to-plan matches, missed matches and reasons on opt-in sessions.
   Seed a changed feature, test filter, untracked fixture and arm64/amd64 mismatch. Each must either change the required key or remain diagnostic.
   Ship when the agent sees the requested subject, command, exit status and eligibility reason from its worktree.
   Estimate 12–18 agent-hours for this slice and one working day for a reviewed prototype once the planner interface is available.

2. **Let two worktrees finish one pair on the cluster.** Connect the same command to bundle import, sealed Linux execution, isolated signers and ledger claims.
   Start agent A, attach agent B, disconnect A, then request the same key from a labelled PR in shadow.
   Verify that two distinct nodes filled exactly two slots, the cold digest matched, and B received the logs and exit.
   Repeat with a failing witness, forged local receipt and revoked signer. Count accepted passes for each invalid case. The required count is zero.
   Estimate 30–46 agent-hours for the slice, about 2–3 elapsed days after two qualified workers, signers and a reviewer are available.

3. **Use the evidence at label time, then measure the whole path.** Start with explicit pre-push verification. Compare paired runs with and without early submission
   on the same workload and executor. Include dirty iterations, root or lockfile edits, a moved queue tip, C5 work, superseded commits and loaded sites.
   Measure from the agent’s first command to actionable output and to merge, as well as label-to-verdict. Charge local CPU, both witnesses,
   abandoned work, sampling and A1. Report compute actually removed separately from work moved before the label.
   Require exact-subject parity, planted failures blocked, and no bypass on the latest head or group. Keep this path in shadow until
   the design’s parity and reuse gates pass. Cut over per qualified job type and delete its superseded route in the same change.
   Estimate 18–30 agent-hours for instrumentation and integration. Evidence takes real traffic. At the design’s volume, 100 merge groups alone take about 44 hours.
   A useful first release ends here. Continuous verification is a separate owner decision informed by observed demand, waste and queue delay.

The design contract and timing assumptions come from the [runner architecture](../../pr-worktree/docs/proposed/velnor-runner-architecture.md),
[agent-facing UX](../../pr-worktree/docs/proposed/velnor-user-experience.md), [owner decisions](../../pr-worktree/docs/research/velnor-runner-2026-11/owner-decisions.md),
[explainer](../../pr-worktree/docs/research/velnor-runner-2026-11/explainer.md) and [comparison of speed proposals](../../pr-worktree/docs/research/velnor-runner-2026-11/fastest-comparison.md).
