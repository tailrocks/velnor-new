# Reimagining the runner for AI agents: GPT-6.2-Sol

| Field | Value |
|---|---|
| Model | GPT-6.2-Sol |
| Model id | `gpt-6.2-sol` |
| Client | codex-cli 0.169.0 (codex exec) |
| Reasoning effort | high |
| Date / run | 2026-11-19 |
| Brief | agent-native brief v1 (2026-11-19): the final design, the user-experience draft and the owner decisions |

This is the model's own answer to the owner's prompt to reimagine the runner when AI agents do the work, especially running agents' local verification through Velnor so CI can skip what was already verified, unedited except for formatting.

---

# Velnor for coding agents

## The picture

A coding agent runs a check from its worktree and gets a live log, an exit code, and a receipt.
By default, checks intended for CI run on warm Linux nodes. The Mac is the terminal and the editor.
It can also run a sealed Docker sandbox for offline feedback.
The same released planner defines verification during development, on a labelled PR, and in the merge queue.
When two eligible executions have already passed an obligation for the same inputs, CI reads their receipts.
When anything differs, CI runs the missing work. The agent can see exactly what remains before pushing.
GitHub still requires green on the latest PR head and separately on the actual merge group.
A good session ends with the agent pushing, adding `velnor:verify`, and seeing which work CI reused.

## Ideas

**Recommendation.** Move explicit pre-push verification earlier in the delivery plan.
Start with whole-tree keys and one Rust obligation. Use the same ledger and node agent that the PR gate will use.
The estimates below assume those services exist. Each estimate includes implementation and review.
The examples show break-even conditions and illustrative workloads. The architecture's measured results are labelled.
For one comparable 30 s local check followed by two 30 s CI witnesses, the old execution total is 90 s.
If the agent's check becomes one witness, the total is 60 s plus dispatch and proof overhead.
Running both witnesses before the label also moves the second execution off the label clock. Count the
30 s saving once across routing, signing, sharing, and verdict lookup.

A *verification obligation* is one released check with a defined command and expected inventory.
For this proposal, an exact key covers the intact tree, dependency and fixture digests, command graph, helper,
tools and images, executor identity, features, profiles, canonical paths, environment, and execution fingerprint.
Use the certified executor selected by the base policy. Reference runs and results from another executor cannot fill slots.
Receipts also bind the binaries each witness actually executes, including binaries launched by the tests.
The trusted planner reads the policy from the base and checks the expected test IDs and approved reductions.
C5 is the architecture's rule for incomparable inputs such as live network, wall time, unseeded randomness,
event payload, and commit identity that varies by request. These obligations run fresh unless the varying input
is fixed and keyed. Unknown inputs force fresh work. Keep whole-tree keys until a reviewed dependency contract
covers absent files and directory membership and the platform can enforce it on every execution.
The test inventory and a read trace alone cannot authorize a narrower key.

| ID | Idea | Mechanism in Velnor | What it removes | Correctness, tier and fallback | Owner decision fit | Cost, agent-hours |
|---|---|---|---|---|---|---:|
| N01 | Route day-to-day commands through Velnor | Explicit CLI, exact Cargo argv matching, streamed logs, sealed local or managed execution | Example: 3 agents × 2 matched 30 s reruns = 3 slot-min avoided | T1 local, T2 only for a released obligation observed on an eligible node. A custom or dirty run stays advisory, so CI executes the uncovered check | Fits 10c, 11, 13, 20 | 14–22 |
| N02 | Make CI-bound commands remote by default | `velnor verify` sends a commit bundle to a warm cluster node. `--local` selects sealed Docker feedback | Example: a 55 s local run versus 2 s dispatch + 1 s provision + 25 s work saves 27 s before queueing | T2 on certified placement with the same key and plan. Use qualified cold capacity on a warm miss. A local diagnostic leaves CI work open | Fits 9–11, 13, 20–24 | 18–28 |
| N03 | Make trust and witness count visible | T0 shell, T1 sealed local, T2 CI candidate, T3 A1-promoted. Require two independent T2 passes per new key | One eligible pre-push pass of a 6 slot-min obligation removes one 6 slot-min execution from the labelled request | One node, attempt and test order per slot. A cold second build until qualified twins. An incomplete, failed, or revoked record cannot satisfy the gate | Fits 8, 10–11 and architecture §6.2 | 10–16 |
| N04 | Treat CPU architecture as an input | Planner resolves host, execution and target triples. Dispatcher requires eligible Linux `amd64` for that contract | At 20 CI-bound invocations, direct dispatch avoids up to 20 redundant Mac or emulated executions | Arm64 and amd64 keys differ. T1 for a Mac result, T2 only for the exact eligible execution contract. A missing amd64 worker leaves that obligation pending | Fits 9–11, 21 | 8–14 |
| N05 | Verify committed, unpushed snapshots automatically | Opt-in local watcher, debounce, latest-only claims, and a capped background lane. No GitHub event or status | A final snapshot verified during 5 min of review can hide a 35 s chain. At 40 commits and 25% reuse, 30 × 4 = 120 speculative slot-min may be wasted | Same T2 checks and exact keys. On stop or supersession drop claims. Default to the explicit command pending approval | Owner question for 12. Allow a background trigger before the label without changing the gate? | 10–16 |
| N06 | Let agents ask what work is already covered | `velnor explain` returns completed, in-flight, missing, and fresh obligations. `ensure` joins exact-key work | Four agents asking for the same two 90 s witnesses use 3 slot-min instead of 12 | Only valid T2 or T3 records cover an obligation. An advisory lookup cannot grant a pass. A stale or unavailable query goes through fresh resolution | Fits 8, 10–11, 23 | 8–12 |
| N07 | Find flakes during development | Rerun suspect tests in advisory diagnostics with seeds and resource observations. Preserve the first result | With failure probability 0.2, three independent diagnostic runs give a 49% chance of at least one red for 3 slot-min at 1 min each | T1 or non-minting cluster diagnostics. A required red cannot become green through a diagnostic pass. Reproduce through normal proof when needed | Fits 10–11 | 8–14 |
| N08 | Prioritize interactive feedback | Use a bounded interactive queue above unclaimed batch work. Elevate joined work to its highest live claimant | Starting within 5 s instead of behind 40 s of restartable batch saves 35 s of feedback wait | Priority never changes the definition, resources, inventory, or T2 proof. Reserve merge pairs and A1. Queue or offer T1 local feedback when full | Fits 10–11, 13, 15, 21 | 10–18 |
| N09 | Sign what actually ran | External node signer issues lease- and epoch-bound receipts from observed exits, inventory, and binary digests | Each validated 30 s witness hit avoids a 30 s rerun. If receipt bodies are 4 KiB, 40 pairs need about 320 KiB for proof lookup | T1 signature authenticates a local report. T2 requires an eligible signer, slots, and logged execution. Invalid proof is a miss. Revoke suspect signers | Fits 8, 10–11, 20, 23 | 14–22 |
| N10 | Turn the PR verdict into a lookup | Fenced App resolves the latest head and approved test tree, validates the obligation vector, executes misses, and posts one status | Example: 35 s rerun versus 3 s resolution + 8 s of fresh work saves 24 s on that labelled request | App success requires all valid T2 or T3 records and current fresh passes. Repeat for the exact group under ALLGREEN. Unknown proof runs fresh or blocks | Fits 7, 10–12. Owner question for 12 if shared SHA statuses conflict with per-PR label authorization | 14–22 |

### N01. Route the commands agents already run

Start with `velnor verify` and a proposed `--only` selector that names an obligation from the released plan.
Then add one explicit command form, such as `velnor run -- cargo clippy …`, for agents that work in Cargo terms.
Match the executable, working directory, argument vector, and supported environment to a released definition.
Arbitrary Cargo subcommands, filtered tests, and a host build can still answer a development question.
They have distinct results or stay advisory when the released CI command differs. A plain `cargo build`
never covers Clippy or a test obligation. Generated tests already split build and execution and can accept
an empty filter. Receipt eligibility therefore requires the full expected inventory and a result for each ID.
Report the executed command, resolved inputs, coverage, and reason for any advisory result with its exit code.
Prefer explicit CLI adoption in agent task instructions. If a workspace Cargo wrapper earns adoption,
make it a thin client of the same CLI. Use a client daemon for upload, log reconnection, and cancellation.
Only the worker observes and signs. [Current Cargo command generation](../../pr-worktree/generator/crates/velnor-render/src/cargo.rs#L176)

### N02. Default to the cluster for CI-bound work

Send an authenticated bundle of the exact committed snapshot through the existing repository mirror path.
Check the commit and materialized tree server-side, then merge it with the current queue tip using the PR
planner's policy. Keep unpushed objects in access-controlled repository staging and route the request to warm
Kubernetes sandboxes with pinned tools, pre-mounted work volumes, and eligible base generations.
Give each attempt a private writable layer. Use two compatible nodes for a full verification request.
If the agent has uncommitted edits, offer a separately identified dirty snapshot for feedback.
Show which modified files would leave the Mac and upload only files the agent selects for that snapshot.
An explicit request for CI evidence uses the committed snapshot and shows its SHA before running.
`--local` runs Docker with the closest supported environment and shows its actual fingerprint.
For the latency comparison, charge upload, queue and the slower witness. The architecture's measured
0.25 s clone + 0.31 s no-op build versus 92 s cold describes preparation on one idle workspace.
Measure real edited work before using that difference to predict agent feedback. [Architecture §7.3](../../pr-worktree/docs/proposed/velnor-runner-architecture.md#73-compile-reuse)

### N03. Count independent executions

T0 is plain shell output. T1 is a result observed by a local Docker node, useful for feedback.
T2 is an eligible cluster execution, stored as candidate evidence in the repository's ledger.
T3 is evidence in the trusted `main` scope after landing and cold audit A1. T3 still needs the exact key,
valid epoch, and complete witness record. Every new gate key needs two T2 passes on different nodes,
with distinct attempts and order seeds, on different qualified generations or with a cold second build.
Until compile reuse graduates, require a cold second build and agreement on binary digests.
A qualified interactive attempt can fill one assigned slot. A labelled request executes the other
if it is still missing. An explicit `velnor verify` can request both. A failed witness blocks its record,
and cancelling or repeating the command cannot turn it into a pass. Qualify seed ordering and independence
against delayed pairs. If the twins are correlated, run the changed obligations again in the merge group,
including an exact-tree match, as the architecture prescribes. [Architecture §6.2](../../pr-worktree/docs/proposed/velnor-runner-architecture.md#62-verification-records-witnesses-joins-and-scopes)

### N04. Separate host, execution, and target architectures

An Apple Silicon Mac can submit Linux amd64 work without running it itself. A native arm64 Linux
container on the Mac still differs in architecture or platform fingerprint from an amd64 cluster sandbox.
A Mac running an amd64 guest under emulation remains advisory. Put the runtime OS, architecture,
kernel, CPU features, sandbox policy, toolchain and target triple in the execution contract.
A cross-compiled Linux arm64 binary can satisfy a released arm64 build obligation when the declared
builder matches. An arm64 smoke test has its own execution contract, including the emulator if prescribed.
A successful cross-build cannot cover tests that must actually execute on amd64 or native arm64.
If a native Linux arm64 test becomes required, qualify matching arm64 workers and schedule a separate
witness pair there. Darwin stays a local feedback target while macOS release builds are paused.

### N05. Ask before adding continuous unpushed verification

Would you amend decision 12 to let an opted-in agent session verify reusable obligations
automatically after a committed, unpushed snapshot settles? This adds a trigger before the label.
It differs from architecture question D19, which proposes work from a GitHub push. A watcher would
observe a specific worktree's commit after a proposed 30 s debounce, keep at most one live background
claim per worktree, and share work by key. Skip fresh-only obligations until demanded. Run only on idle
capacity with a per-repository budget. Drop superseded claims and evict unpushed bundles on expiry.
There is no GitHub status from this flow. Labelled verification still resolves the latest head and
actual test tree, and the merge queue verifies its own subject. The gain is overlap with editing and
review. A replay must show that completed useful proof beats superseded work at a fixed cluster budget.
The default remains explicit pre-push `velnor verify`. [Architecture owner question D19](../../pr-worktree/docs/proposed/velnor-runner-architecture.md#13-owner-questions)

### N06. Query and join the same work

When an agent starts work, have it ask `velnor explain` about the snapshot and intended obligation.
Return the resolved tree, plan epoch, expected test IDs, eligible witness count, and why each item
is complete, running, missing, or fresh. The server computes keys. Avoid a raw query over another
agent's unpushed refs or logs. Scope the answer to authorized repository work and include provenance
only where the caller has access. A running item returns a handle the agent can follow. Cancelling
one worktree or PR drops its claim, and the execution continues while another live claim remains.
An exact failed record can explain a problem. A pass is determined by `ensure` under the current
epoch and request, including the complete proof. A narrower result from a neighbouring package
or a previously green SHA cannot suppress a check.

### N07. Surface flakes before the PR

Let agents ask for a bounded diagnostic rerun of a failed or historically flaky test in the same
sandbox configuration. Preserve the original outcome and record the test ID, binary digest, seed,
timeout, and CPU and memory pressure. A clean rerun records disagreement for triage. An isolated
failure under resource pressure is still a failure in that execution. Keep required retries at zero.
An authorized re-request of a failed obligation needs a new pair of passing witnesses.
Every required failure stays in the flake-rate denominator. Use the architecture's threshold of
two false failures in seven days to flag the test for repair. Removal from the required inventory
requires equivalent blocking coverage or an expiring owner exception. Sampled reuse disagreements
take the reuse-revocation path, even when an exploratory rerun later passes. [Architecture §1.3](../../pr-worktree/docs/proposed/velnor-runner-architecture.md#13-flake-budget)

### N08. Give interactive work a short queue

Schedule fast diagnostics and reusable obligations through the same placement logic, with separate
admission limits. Try one interactive build admission per site when a matching pair of merge slots
and the A1 reserve are protected. Allow it to displace restartable batch work. Keep other organizations'
quotas and existing PR entitlement. Match requests to warm nodes by verified generation and then
compare the estimated wait with starting on a capable cold node. A shared claim takes the highest
live priority. When a higher-priority claim arrives, move queued work or admit its missing witness
against reserve. Only a proven infrastructure abort can replace an already running attempt.
Charge occupied slots, including cancelled work and the second witness. Compare time to first red,
interactive queue wait, group P90, and A1 lag on the same loaded trace. [Architecture §9](../../pr-worktree/docs/proposed/velnor-runner-architecture.md#9-the-cluster)

### N09. Attest the observation, then validate it

Have `velnor-ledger` journal an assignment before launch. The node verifies the tree, mounts pinned
inputs, runs the released definition, and observes exits and exact inventory through a channel outside
the job's control. The Ed25519 signer outside the sandbox signs a versioned statement that binds
repository, role, key, tree and execution manifest, expected and observed outcomes, binary digests,
generation, slot, attempt, node, and epoch. The ledger validates signer and platform eligibility,
the active lease, and append-only log inclusion. A duplicate delivery is idempotent for that assignment.
A different slot or epoch requires a valid new execution. A local signature can connect a trace to
the local daemon for diagnostics. A developer-controlled node, even with a signed image, cannot
establish eligible observation. Keep node, App, and client credential roles distinct, and keep
unpushed PR candidates out of trusted cache scope until the architecture's A1 promotion.

### N10. Resolve at the label, publish to the exact subject

Draft pushes, ready-for-review alone, and unlabelled pushes start no PR verification. The App listens
for `pull_request.labeled`, `unlabeled`, and `synchronize`. A ready PR's verification label tells
`velnor-control` to resolve its latest head and exact test tree. A new labelled push supersedes the
old request immediately. Label removal stops work and future writes for that request. Apply labels
as a user or with an App installation token. GitHub suppresses Actions runs for labels applied with
`GITHUB_TOKEN`. Test App delivery for all three identities independently. The reconciler reads PR
heads, draft state, label history, and active group refs every 10 s to recover missing events.
The released planner derives the complete vector. On a hit, the App validates it under the current
epoch. On a miss, the ledger starts only the missing witnesses and all C5 work. The fenced writer
rechecks generation, head, and authorization immediately before it posts `Velnor / Verified` on the
PR head SHA. `Velnor / Details` reports reuse and remaining work. Bind the status to the App's
`integration_id` on `main`, with empty bypass actors and GitHub's ALLGREEN queue. Give the App the
documented Merge queues read permission for `merge_group` events. Every `merge_group.checks_requested`
resolves the exact group SHA and issues its own verdict. Whole-vector reuse requires matching keys,
including the tree or enrolled dependency proofs, and no fresh work. Publishing a group verdict
cannot fill the PR head's required status. A1 still cold-audits each landed commit before trusted
promotion and release. [GitHub workflow triggers](https://docs.github.com/en/actions/how-tos/write-workflows/choose-when-workflows-run/trigger-a-workflow#triggering-a-workflow-from-a-workflow) · [GitHub webhook permissions](https://docs.github.com/en/webhooks/webhook-events-and-payloads#merge_group) · [GitHub required status sources](https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/managing-rulesets/available-rules-for-rulesets#require-status-checks-to-pass-before-merging) · [GitHub merge queue](https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/configuring-pull-request-merges/managing-a-merge-queue)

## Risks

| ID | Failure or waste | Detection and containment |
|---|---|---|
| N01 | A shim drops flags, runs the wrong tree, or reports zero tests as a pass | Plant wrong flags, dirty inputs, and empty inventory. Require byte-for-byte definitions and inventory parity in shadow. Decline eligibility and run the released obligation on any mismatch |
| N02 | Bundle resolves to the wrong tree, or stale warm state hides a failure. Upload and queues can exceed local time | Corrupt a staged tree and plant an older-mtime failing edit. Verify full tree and generation inputs. Compare with the cold witness. Retire bad generations and run cold. Measure admitted-to-exit latency on loaded workers |
| N03 | Same host, same generation, duplicate receipt, or correlated twins give fewer independent checks | Validate slot identity and order evidence. Run seeded intermittents on twin and delayed pairs. Reject invalid witnesses. Run changed obligations in the group if independence evidence fails |
| N04 | A matching source tree hides differing cfg, CPU, or platform behavior | Plant a target-specific failing test. Mismatched fingerprints miss. An amd64 obligation waits for eligible execution even if an arm64 or emulated run passed |
| N05 | Background work verifies abandoned commits, retains private source, or races the explicit label flow | Meter superseded slot-min and usable coverage. Make upload opt-in, scope bundle access and retention, and prove the watcher lacks a status path. Keep manual triggering unless the owner approves and a bounded replay pays |
| N06 | A stale lookup or failed leader starves the other claimants. Partial matching tests get treated as complete | Fault-test lease expiry, revocation, and one of several claimants cancelling. Re-resolve authoritatively on gate use. Replace a lost attempt only after a proven infrastructure abort |
| N07 | Repeated diagnostics hide flaky required failures or consume the cluster | Show all required attempts and diagnostic seeds. Keep diagnostics non-minting and capped. Suspend reuse on comparable disagreement and follow the flake budget |
| N08 | Interactive work steals witness capacity or a preemption masks a real timeout | Replay saturation and a hung test. Protect two distinct eligible group placements and A1. Cancel batch under proven policy, charge it, and back off interactive admission if group or audit wait grows |
| N09 | Forged, replayed, revoked, or reference-role receipts enter a slot | Test a duplicate, a wrong repo or lease, a reference signer, and a prior epoch. Reject each invalid receipt. Stop publishing on suspected signer compromise, invalidate affected proofs, and re-execute |
| N10 | A stale write, ruleset drift, or same-SHA label race makes GitHub accept the wrong subject | Read back the App binding and status. Exercise old heads, same-SHA PRs, label removal, destroyed groups, and stale leases in the App lab. Keep the existing gate until parity. Freeze publication and re-verify when proof is suspect |

Promote new reuse classes only after their key contracts hold by construction, every shadow hit is
re-executed, and about 3,000 comparisons per obligation kind agree. Then sample 2% and retain A1 and
planted-failure canaries. Quantify the shadow cost. The comparisons help find defects in the contract.
They provide no authorization to use an incomplete key. A disagreement suspends the class, blocks
dependent proof, and starts cold verification. Audit affected landed commits and revert on failure.

**Two cases the App lab must settle.** GitHub commit statuses are SHA-scoped. Architecture §8.2 keeps
a verdict already posted when the label is removed and shares a verdict across PRs at one SHA.
Decision 12 also says an unlabelled PR cannot merge. If GitHub cannot enforce that literally with
the proposed status, ask the owner whether authorization is per verified SHA or per PR until merge.
Per-SHA authorization preserves an existing pass and avoids another status publication on relabelling.
Per-PR authorization requires a gating mechanism that can distinguish the PRs. Hold App cutover for that
case until the policy and enforcement agree. Also test a revocation racing GitHub's merge acceptance.
Velnor can stop new success writes, correct posted statuses, and audit commits that already landed.
An asynchronous status correction cannot make an already accepted merge contingent on the audit.
[Architecture §§6.5 and 8.2](../../pr-worktree/docs/proposed/velnor-runner-architecture.md#82-the-label-flow-and-the-required-status) · [GitHub commit statuses](https://docs.github.com/en/rest/commits/statuses#create-a-commit-status)

## The first three steps

1. **Run one real obligation from a Mac against the cluster.** Use the released `velnor-ci` planner
   and a `velnor-gen` nextest obligation with a stable inventory and reproducible inputs. Implement `velnor verify` for
   committed snapshots, initially in shadow. Check the bundle tree, isolate the sandbox, sign each of
   two attempts, and return the explanation and logs to the agent. Compare against two fresh cold
   executions. Plant wrong-arch, zero-test, stale-generation, wrong-epoch, and mismatched-feature
   cases. Reuse the ledger's signer and slot model. **36–50 agent-hours** after the planner and node
   APIs exist. The wall-clock constraint is two eligible nodes plus time to run the parity matrix.
2. **Prove that the same receipt covers a later labelled request.** Add read-only explanation and
   exact-key joins. Replay an unchanged tree, a moving queue tip, a fresh-only obligation, and a
   group ejection. Shadow the current gate, measuring the full label-to-verdict and agent-request
   clocks, with source edits and realistic load. Start shadow with the first eligible job type. Switch
   the required context only after the App lab and parity for the full required inventory. Delete each
   superseded route at its cutover, per decisions 16 and 18. **22–34 agent-hours** on top of step 1. The architecture's
   parity cohort of 300 heads and 100 groups and the reuse graduation comparisons drive wall time.
3. **Convert agent instructions for one Cargo root and cap interactive admission.** Use `velnor verify`
   for CI-bound checks, `--local` for diagnostics, and `velnor explain` before repeating work. Replace
   project runbook commands that routinely repeat a CI obligation. Record runner-minutes, queue wait,
   first actionable red, and percent of labelled obligation-seconds already covered by valid proof.
   Compare paired requests with a warm conventional baseline. Include the second witness, fresh-only
   work, audits, upload, and superseded claims. Keep commit watching behind the N05 owner decision.
   **16–24 agent-hours** on top of step 2. Shared capacity and a matched sample of agent sessions set
   the elapsed time. [Comparison, benchmarks K03–K09 and K24](../../pr-worktree/docs/research/velnor-runner-2026-11/fastest-comparison.md#benchmarks-proposed)

Basis for the proposal. [Owner decisions](../../pr-worktree/docs/research/velnor-runner-2026-11/owner-decisions.md) · [Runner architecture](../../pr-worktree/docs/proposed/velnor-runner-architecture.md) · [User experience, including pre-push CLI](../../pr-worktree/docs/proposed/velnor-user-experience.md#5-the-cli) · [Explainer](../../pr-worktree/docs/research/velnor-runner-2026-11/explainer.md) · [Comparison of fastest-runner critiques](../../pr-worktree/docs/research/velnor-runner-2026-11/fastest-comparison.md)
