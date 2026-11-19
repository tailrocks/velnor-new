# Design feedback by GPT-6.2-Sol

| Field | Value |
|---|---|
| Model | GPT-6.2-Sol |
| Model id | `gpt-6.2-sol` |
| Client | codex-cli 0.169.0 (codex exec) |
| Reasoning effort | max |
| Date / run | 2026-11-19 |
| Brief | design feedback brief v1 (2026-11-19): the draft design, decision log, comparison, all proposals and reviews |

This is the model's own critique of the draft final design, unedited except for formatting. The owner asked this round to hunt for correctness holes and for places where the design is too timid.

---

## Verdict

The draft should cut push-to-merge sharply, but it leaves a shorter path at the same confidence unexplored by committing to separate PR and queue executions until late in the rollout.
**Ambition 8/10.** Owning placement, test execution and verification receipts is ambitious, though an independently scheduled verifier should be tested before the custom runner fleet.
**Soundness 5/10.** The ledger specifies signatures and keys before establishing who owns the plan, who proves each execution, and which GitHub candidate can consume the result.

## Correctness and confidence holes

1. **Blocker. A job assertion can become portable evidence.** C4 lets an in-job report back a signed receipt for an exact tree. GitHub treats a conditionally skipped job as success,
   and today's workflow obtains group evidence from a helper built at the candidate SHA. Binding a check to the GitHub Actions integration identifies the publisher but leaves
   every workflow in that integration sharing the identity. The job's trusted envelope must include the released plan and command graph. The verifier must observe the immutable
   inputs, process exits, expected test inventory, and completion of every required step. Missing, skipped, cancelled, masked, or zero-test results fail a selected obligation.
   Give the dedicated verdict App sole write authority for the new required context and shadow its output before switching the ruleset. See draft §1.2 C4, decisions S3,
   `velnor-new@95de9b704b:.github/workflows/ci.yml:755–784`, and `docs/goals.md:40–43,76`.
   [GitHub required-check semantics](https://docs.github.com/en/pull-requests/how-tos/merge-and-close-pull-requests/troubleshooting-required-status-checks)

2. **Blocker. A tree digest leaves observable inputs out.** The same tracked bytes can run with different Git parents, event fields, feature resolution, submodule contents,
   advisory data, or host runtime. The current Ledger CLI job reads `HEAD`, and runner-host release identity reads a committed lock at a particular SHA. Define replay
   eligibility per obligation. Key an executable over the actual toolchain and image digests, command and profile, materialized files with modes and symlink targets, pinned
   dependencies, runtime class, and the values it can observe. Include commit/ref/event values where applicable or execute fresh under C5. Mint a receipt for the bytes the
   verifier mounted. A URL, version string, or job-reported checkout hash is insufficient. See draft §4.1 and `velnor-new@95de9b704b:.github/workflows/ci.yml:358,769`,
   `crates/velnor-runner/crates/velnor-runner-host/build.rs:15–42,54–79`.

3. **Blocker. A successful prefix and a successful run prove different things.** GitHub can form a queue candidate using earlier PRs, and HEADGREEN permits a failing
   predecessor in a batch whose tail passed. S4 measures base agreement. Complete plan equivalence needs its own evidence. With C1's per-landed-commit contract, use ALLGREEN or
   a single-entry merge limit until the App proves every prefix the configured merge method can land. Export a signed receipt set for each verified prefix, including the base,
   ordered candidate heads, tree, policy revision, required obligations and actual check SHA. Require the entire consuming plan, even on an equal tree. In phase 1, let a main
   push skip only obligations covered by that evidence in the current policy. Prove the mapping to actual landed SHAs and trees in lab, including squash, ejection, reorder and
   batched push. Ban ordinary direct writes to main or make a preverified commit the prerequisite. A full run after a bypass is incident detection. See draft §6.4–6.6, §11 item
   7, decisions S3–S4 and evidence §A T7.
   [GitHub merge-queue grouping and failure handling](https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/configuring-pull-request-merges/managing-a-merge-queue)

4. **Blocker. A post-run file-read audit cannot validate selection.** A skipped build script, test or lint emits zero reads. A conditional read can appear only after another
   input changes. A missing fixture in the proposed mount namespace can produce an accepted default or an empty discovery set. Keep full merge-group obligations as the default.
   Require skipped obligations to carry a proof from a verified base or prefix that their enforced inputs, including absence and directory listings, remain equal. Compare the
   same build, test discovery and outcomes in the full and restricted lanes before enrolling a class. Enforce declared build inputs on build scripts and proc macros, and widen
   and execute missing obligations before green on an unsupported access or audit loss. Treat `rerun-if-changed` as a Cargo hint. See draft §4.3–4.4, §6.2, decisions I068–I070
   and `velnor-new@95de9b704b:docs/hard-decisions.md:188–214`.

5. **Blocker. Snapshot isolation protects the original but can carry the candidate's build state forward.** Candidate build scripts and tools write `target/` before harvest. A
   reflink preserves those outputs and fingerprints, and `inputs restore` retimes matching source records. Neither proves that those artifacts came from the required
   invocation. Publish trusted seeds only from a controlled build with an authenticated input and unit manifest. Fence and freeze writers before copying. Key transient
   artifacts by repository, authority class, toolchain/ABI, command, input digests and producer. Keep unconstrained candidate targets out of required reuse, including reuse by
   the same PR after a push. A merge group's landing can authorize independently verified receipts. It cannot make an opaque target directory trustworthy. Compare cold versus
   warm on deletes, symlink changes, feature switches, dirty outputs and interrupted builds before promotion. See draft §5.2–5.3,
   `velnor-new@95de9b704b:ci-helper/crates/velnor-ci/src/io/inputs.rs:102–152` and `velnor-legacy/crates/velnor-runner/src/storage.rs:533–552`.

6. **Blocker. The build, Docker and signing boundaries overlap.** Same-repo PR and merge-group source can run build scripts and tests. A taint controls scheduling of the
   privileged Docker sidecar, and a mounted agent socket still reaches a privileged process. Supply declared Docker jobs with dedicated workers without verifier keys or shared
   trusted state, or with a qualified disposable VM. Retain the hosted class for forks without reducing their required plan. Resolve the sandbox class from released definitions
   and full job requirements, including services and composite actions. Keep signing out of the execution node's general service, authenticate every receipt and cache request
   to a verifier-owned execution identity, and make the socket expose only job-scoped operations. See draft §3.3, §3.6, §7, evidence §F.2.3 and
   `velnor-legacy/crates/velnor-runner/src/trust_scope.rs:1–31`.

7. **High. Acquisition has an irreversible gap.** §3.3 records the acquired job after `acquirejob`. A crash or lost response in between can leave a consumed request and no
   token to renew or complete it. A new leader can also race a hand-off or completion started by the old leader. Persist the request identity and acquisition intent before the
   call, then record the sealed response before broker acknowledgement or hand-off. Fence the node's start and terminal authority with a durable epoch and a single
   compare-and-swap transition. Reconcile every ambiguous call against GitHub. Where the response cannot be recovered, bound the expiry/retry path and keep that execution
   ineligible to sign. Cancel and prove process-tree exit when authority is lost. Use returned lease deadlines and backoff. In lab, inject a crash before and after every remote
   call and journal write, including a response dropped after success. See draft §3.3, §3.7, `actions-runner/src/Sdk/RSWebApi/RunServiceHttpClient.cs:100–109,123–160` and
   `Runner.Listener/JobDispatcher.cs:393–418,539–543,621–629,748–830` in that checkout.

8. **High. Native completion has more obligations than reporting success.** The Worker drains its reporting queue, sends step results, annotations and outputs, and retries
   completion. A reused test receipt proves the test. A job that emits `needs` outputs, carries artifact dependencies, executes post steps or has environment rules has a larger
   contract. Restrict synthetic completion to a versioned generated job with its entire contract proven, or publish the verdict with an App and omit the job from that workflow.
   Lab-test delayed/duplicate observations, a dependent consuming an output, token expiry, a successful completion whose response drops, cancellation and reruns. Prove the
   check attaches to the SHA GitHub evaluates for each event. See draft §3.5 and `actions-runner/src/Runner.Worker/JobRunner.cs:290–354`,
   `src/Sdk/RSWebApi/RunServiceHttpClient.cs:123–150`.
   [GitHub check SHA selection](https://docs.github.com/en/pull-requests/how-tos/merge-and-close-pull-requests/troubleshooting-required-status-checks#conflicts-between-head-commit-and-test-merge-commit)

9. **High. The warm pool cannot fit on the proposed workers.** Two idle plus four running build pods reserve `6 × 32 + 4 × 2 = 200 GiB` and `6 × 16 + 4 = 100 CPU`, before
   Docker, on a worker with about 125.5 GiB and 96 logical CPUs. With 6 GiB system reserve, 2 GiB eviction margin and four gates, three 32 GiB builds fit before other tenants.
   The 45 MiB idle RSS is a different quantity from the scheduler reservation. Hold a fixed resource budget for runnable work. Either count idle and running inside it, or
   qualify a warm substrate with dynamic inner resource grants. Kubernetes namespace quotas cap consumption. A bound warm pod cannot gain capacity by scheduling preemption at
   hand-off. See draft §7, evidence §B.1.2, §B.2.2. [Kubernetes resource reservations](https://kubernetes.io/docs/concepts/resource-management/effective-pod-requests-limits/) ·
   [Kubernetes resource quotas](https://kubernetes.io/docs/concepts/policy/resource-quotas/)

10. **High. Offer priority can hide a queue while allowing assignment without capacity.** A capacity header and sorting one batch of visible offers leave earlier assignments in
    place. ARC acquires offered IDs and sizes from `TotalAssignedJobs`. Legacy computes global free capacity after reconciliation. The server behavior when an advertised value
    changes below outstanding assignments is unproven. Reserve a concrete class slot before committing an acquisition, reconcile acquired, assigned, renewing and cleaning work
    with the physical slot count, and publish demand-to-start including work GitHub has withheld. Reserve merge capacity by route or in the external scheduler. Replay a delayed
    assignment, duplicate poll, full class, and slot loss against both counters. See draft §3.3, §9 M9, evidence §F.1.3 and `arc/cmd/ghalistener/scaler/scaler.go:135–175`,
    `velnor-legacy/crates/velnor-runner/src/scaleset/shared_ledger.rs:1–17`.

11. **High. The flake budget samples the wrong direction.** S1's 10/124 is a green-merge-group to red-push disagreement rate, confounded by event, workload and node. Sampling
    only green runs estimates green-to-red. The proposed SLO defines red-to-green. Capture every first failure, timeout and cancellation, independently adjudicate
    matching-input pairs, sample green jobs too, and stratify by obligation, event, load and failure domain. With zero incidents, 50 independent trials leave a one-sided 95% upper bound of
    `1 − .05^(1/50) ≈ 5.8%`. About 300 pairs with zero incidents are needed for 1%. Quarantine a required test only when a blocking equivalent covers the same property. An
    owner sign-off and a visible non-blocking run leave a hole. See draft §1.3, §10 and decisions S1, N14.

12. **High. The committed failover has one control plane.** A standby in FI still needs the single k3s server described in evidence §B.1.5. A lost DE control plane can stall
    leadership renewal, pod accounting, and cleanup on both sites. Moving labels does not release a job whose lease is already owned. Qualify one-site failure before replacing
    ARC. Use independent site admission with durable fencing or a quorum across three failure domains that tolerates either site's outage, and an ARC route for new runs.
    Drain/reconcile old attempts, ensure keys and policy epochs remain valid during a partition, and test a simultaneous node and control-plane loss. See draft §3.7, §7, §14,
    decision I093.

13. **High. The timing cohort cannot support the advertised median.** The 42 s estimate applies to generator/ci-helper-only changes, 47% of 200 merged PRs. At least three more
    percentage points must finish within a minute even if every one of those light PRs does. The 85/457 min baseline starts at CI creation on a final head, while the new goal
    starts at push. Log an accepted push timestamp, PR head and base, trigger, eligible/enqueued, offer, first work, terminal verdict and merged timestamps. Retain superseded
    and uneventful pushes in cost accounting, and report timeouts/open PRs beside the merged cohort. Treat 19 s enqueue-to-run and 26 s run-to-land as observed reference
    segments. Measure their joint distribution on each route before budgeting the tail. See draft §1.1, §6.6, comparison lines 278–293, and evidence §A T1–T2.

14. **Medium. The day-one optimizations already have narrower controls.** Evidence §B.1.4, line 492 explicitly leaves the kubelet root and `emptyDir` device unverified. Inspect
    the real mount and reflink support before moving a fleet component. Tools are seeded but Rust installs per job (`velnor-runners/state/RESULT.md:115`). The existing engine
    already launches several check lanes in parallel, while the generator emits them in sequence (evidence §C.1, line 657, decisions S9). Compare actual DAGs, shared Cargo
    target locks and exact feature sets before taking credit for concurrency or job count. With a shared target, prebuild immutable test binaries and meter incompatible compile
    lanes with one CPU budget.

15. **High. The release consumer expects a push job.** The current `velnor-ci gate` filters to push runs on the exact source SHA and default branch, reads their jobs, and
    judges required names against them. An App check or a successful queue result alone can leave a tag waiting. Keep a lightweight push job that validates the full merge
    receipt and runs C5 work, and update the release consumer in the same cutover if it must accept a new App identity. Verify tag-to-publish with old, new, unmatched and
    revoked proofs. See draft §11 item 7, `velnor-new@95de9b704b:ci-helper/crates/velnor-ci/src/gate.rs:83–123,127–158` and `docs/goals.md:77`.

16. **Medium. The 66% CPU improvement changed two variables.** The cited Fable experiment used `opt-level=3` without debug assertions. Draft §13.8 says assertions and overflow
    checks stay on. Run a four-cell optimization/assertion comparison with overflow checking fixed, and record which assertions actually execute. Credit improvements measured
    under the required profile. See `proposals/fable-5.2.md:345` and decision I064.

17. **High. Repetition can catch an intermittent regression.** If a defect fails a test independently with probability 10%, one pass misses it 90% of the time and two passes
    miss it 81% of the time. Repeating the check on 2% of cache hits and learning after merge cannot preserve that detection probability. Preserve multiple trials where the
    current gate exercises concurrency or an external dependency repeatedly. Reduce such a check to a proven deterministic schedule corpus before making it reusable. Measure
    whether the full check drops cases that reproduce only under live scheduling. See draft §1.2 C5–C7, §4.4, decisions S1, N14.

## Where the design is too timid

The first production experiment should keep the current required checks as the reference and run an independent verifier in shadow. The timing budgets below are estimates. The
lab experiments decide which ones earn a production target.

### Start the generated workload without an Actions job

- **Mechanism.** In phase 1, have a GitHub App consume authenticated push, PR and merge-group events. A released planner materializes the actual candidate and submits a signed
  execution graph to a reserved worker. Use the official Worker through the local protocol for supported generated jobs, or qualify an executor for the narrower command
  contract. Generic Actions, environment approvals, credentials, Docker and publication keep their official route until equivalent behavior is shown. Remove
  `Plan → needs → Required` from the generated graph once the App owns its verdict. The App can publish a check on a specific commit SHA using the documented Checks API. The
  merge-group webhook specifies the SHA to check.
  [GitHub App CI checks](https://docs.github.com/en/apps/creating-github-apps/writing-code-for-a-github-app/building-ci-checks-with-a-github-app) ·
  [GitHub merge-group webhook](https://docs.github.com/en/webhooks/webhook-events-and-payloads#merge_group)
- **Budget.** For a warm light PR, allow 5 s for delivery and fetch, 4 s for plan, 2 s for sandbox, 10 s for compile, 12 s for parallel verification and 3 s for publication.
  That is 36 s. Keeping the draft's 8 s offer, 5 s plan, 2 s start, 2 s clone, 10 s build, 12 s checks and 3 s verdict gives 42 s. The bigger prize is removing the
  Actions-admission tail, which the proposal never measured as a distribution. Astra observed 497 s before the first job in run 44307369619. See draft §6.6 and
  `proposals/gpt-6.2-astra.md:54`.
- **Risk and confidence.** Event delivery, candidate materialization, or App publication can become the new tail. No job runs with simulated GitHub credentials or approval
  state. Keep full-plan evidence and a GitHub-side fallback that can finish the same obligation after a lost delivery.
- **Two-day test.** On velnor-lab, compare 30 matched light and mixed pushes, including updates arriving out of order. Time push to visible check on both routes and diff
  command, configuration, case and output inventories. Advance if the independent path gives at least 15 s P90 improvement on the same work and every red or incomplete
  reference case stays red or pending.

### Share in-flight execution and move push deduplication to the proof boundary

- **Mechanism.** Schedule one execution per authorized obligation key. A PR, preflight, queue candidate or audit request can join already-running work when its required
  envelope and authority match. The first subscriber's cancellation removes its claim. The worker continues while an eligible subscriber remains. Verification closes only when
  every obligation in the consuming plan has a valid pass. Audits that seek independent repeats get their own executions.
- **Budget.** For an 80 s equal-key obligation that a queue candidate requests 60 s into the PR execution, the second requester waits 20 s and the system saves 80 s of
  execution. For push, a rough ceiling is `120 / 159 × 62 ≈ 47` runner-minutes per green group before release and event-specific work. That is sample-weighted opportunity,
  using T7 and T2's push median as a proxy for per-run cost.
- **Risk and confidence.** Shared execution can couple retries, cancellations and trust boundaries. Join only after a trusted planner resolves the full key. Give each
  subscriber its own policy and check attachment. A producer loss fails or reschedules the shared obligation, and a partial result satisfies nothing.
- **Two-day test.** Replay historical PR/group/push candidates and the actual plan digests. In a two-subscriber lab fixture, cancel the PR after the group joins, then kill the
  producer after it finishes half the expected cases. Require one accepted execution in the first case, zero green receipts in the second, and a per-obligation weighted saving
  on the historical sample.

### Publish a warm generation once and place the missing work

- **Mechanism.** Build each `main` root/profile generation once, then publish its signed manifest and changed content to the other site and make node-local reflink snapshots.
  Send immutable compiler outputs and individually addressed test binaries where they are needed. Eagerly seed expensive hot units, and allow lazy prefetch for the rest. Let an
  already verified candidate contribute only units that the qualified builder produced under the identical input and trust envelope. Keep the prior generation readable until
  the replacement is verified and complete.
- **Budget.** Cut five equivalent warmer builds to two site builds for a 60% reduction in warmer CPU even before qualifying a single producer for both sites. If one warm delta
  costs 600 CPU-s and main advances eight times per hour, the two-site version saves `3 × 600 × 8 / 3,600 = 4` sustained logical CPUs. A 150 MB delta at an effective 500 MB/s
  takes 0.3 s of pure transfer. A whole 1.48 GB archive at 1 Gbit/s takes about 11.8 s before packaging and unpacking. The bytes per changed generation need measuring. See
  draft §5.2–5.3 and decision I045.
- **Risk and confidence.** Relocation or mtime mistakes can make Cargo rebuild or reuse stale units. Preserve fixed roots, provenance and compatible toolchain/runtime identity.
  Validate the manifest and use a clean build when landing a seed or reconstructing a job would otherwise be ambiguous.
- **Two-day test.** Replay ten historical edits across one publisher and two receivers. Record CPU-seconds, bytes, P90 seed readiness and the hit rate of expensive units. Run
  cold controls on each receiver. Require identical discovery and conclusions, no false freshness, and at least a 40% warmer CPU reduction with no P90 regression.

### Attack the cold build's longest dependency chain

- **Mechanism.** Keep the pinned Cargo/rustc behavior and trace the actual unit graph. Isolate stable Turso adapters, protocol types and reusable fixture builders behind narrow
  crates. Start ready metadata/test units concurrently. Cache or distribute complete independent actions only where the measured graph exposes ready work, with links allowed to
  stay near the full inputs. Use the same manifest for each feature configuration and retain required package-alone checks where workspace feature unification differs.
- **Budget.** N17's 696 CPU-s over 24 equivalent threads has a theoretical 29 s width floor. A 170 s chain sets a 170 s floor even on 480 threads. If refactoring or reusing an
  unchanged expensive predecessor cuts the chain to 80 s, the new budget is `max(29, 80) + 5 = 85 s` with 5 s of scheduling and transfer. Removing an 89–109 s image-publication
  fixture build from the serial path can matter more than either change. See decisions N17, I073 and `proposals/fable-5.2.md:111–118`.
- **Risk and confidence.** More crates may increase linking and split nominally identical features. Preserve the configuration grid, selected tests, real toolchain and full
  fixture assertions. Give any reused fixture an input manifest that includes the builder and its sources.
- **Three-day test.** Trace six runner/engine edits and two clean builds on one worker. Move one contiguous heavy dependency or fixture behind a measured interface on a lab
  branch, then test a second worker only for an independent ready unit. Kill a split that adds whole-graph work and saves under 20 s on the observed P90-critical chain.

### Schedule ordered queue candidates before they become required

- **Mechanism.** Keep a small frontier of `base ⊕ ordered heads` prefix graphs, including the likely next ejection only when the input or CPU budget warrants it. Start from
  webhooks and live queue order while the PR verdict is still running. Deduplicate common obligations across those hypotheses. At `checks_requested`, recompute the plan for
  GitHub's actual candidate. A speculative pass can discharge only keys in that exact plan. Release the remaining work on reserved capacity.
- **Budget.** On a full match, immediate eligibility plus about 19 s to create the observed group, 3 s to bind and publish, and 26 s to land yields a reference 48 s from
  enqueue to merge. With a 36 s PR verdict and 3 s to enqueue, the path is 87 s. If one missing obligation takes 60 s after the request, the path is 147 s. Estimate percentiles
  on complete paired traces. See decisions S2–S4 and draft §6.6.
- **Risk and confidence.** A prefix guess can increase contention or become obsolete after ejection. Charge it to a preemptible CPU-seconds budget. Count correct, completed
  predictions before demand, weighted by seconds off the critical path. A wrong prefix supplies no proof except independently identical obligations.
- **Three-day test.** Replay timestamped queue entries and membership changes, using the newest inputs known at each timestamp. Compare `main ⊕ PR`, one ordered prefix, and an
  adaptive beam. Pass only if useful lead time beats the budgeted contention cost at the observed 7–8 entries/hour, and all mismatched candidates take their full required path.
  See `proposals/fable-5.2.md:50–59`.

### Find out whether the forge's landing delay is removable

- **Mechanism.** Keep GitHub's queue as the first implementation. In parallel, specify a lab-only merge service that produces a verified commit and updates the protected ref
  only with an atomic compare-and-swap on the verified base and candidate plus equivalent review and policy enforcement. The public pull-request merge API exposes a head
  precondition. Prove the base, ordered candidate and policy preconditions on the chosen landing route before giving it write authority.
  [GitHub pull-request merge API](https://docs.github.com/en/rest/pulls/pulls#merge-a-pull-request)
- **Budget.** If a preverified candidate can land in 5 s after a 3 s check, its 8 s budget cuts about 40 s from the queue scenario. Re-measure the 19 s and 26 s GitHub segments
  under the new path.
- **Risk and confidence.** A base advance, review dismissal or eligibility change between proof and landing can invalidate the commit. Require atomic rejection of stale state
  and an auditable PR-to-commit record with GitHub protections equivalent to today's. Keep this experiment outside the production path if the forge cannot enforce that
  contract.
- **Two-day test.** Model and replay the actual queue's base/head/review changes against the landing transaction. Inject stale base, concurrent update and dismissed approval.
  In lab, obtain the landed OID and compare its exact tree and parent with the verified candidate. Lack of an atomic route kills the experiment before any protected production
  ref changes.

## The hardest problems

### 1. Make a 270 s test take 15 s and keep what it proves

The CLI loop starts a fresh fixture for each of 40 authority cases. The runner loop starts a fixture and opens a journal for each of six states. My earlier log sample had 274.3
s, 270.8 s and 203.4 s outliers. Equal-cost splits suggest `274.3 / 40 = 6.9 s` and `270.8 / 6 = 45.1 s` per case. They give no bound on the slowest case. Fable's isolated
profile attributed 142.5 CPU-s to an operation with zero block I/O. Moving its work directory cannot remove that parse work. See `proposals/gpt-6.2-sol.md:123–128`,
`proposals/fable-5.2.md:111–116`, `velnor-new@95de9b704b:crates/velnor-actions-cli/tests/image_publication_public.rs:85–148`, and
`crates/velnor-runner/crates/velnor-runner-host/src/launch/bootstrap/unbound_retirement/tests/shape.rs:48–56,97–108`.

Turn each case into a named, individually scheduled test. Build the immutable image fixture once per input digest. Give each invocation fresh registry, process and scratch
state, and keep the real CLI invocation and side-effect assertions. For runner tests that start on the current schema, clone a checkpointed, closed database fixture and
validate its schema/version digest once. Preserve separate tests that exercise every supported migration, concurrent startup and crash recovery from old journals. Use
test-specific isolation to remove artificial serialization in the twelve archive-guard goldens after proving each case still checks its own oracle. Measure setup, assertions,
subprocess work and teardown per case in both runner feature configurations. See `velnor-new@95de9b704b:.config/nextest.toml:17–19`.

A 15 s test budget needs the slowest runner case to fall from roughly 45 s to 15 s, a threefold reduction after splitting. For a 90 CPU-s suite on eight effective cores, the
optimistic bound becomes `max(15, 90/8) = 15 s` plus setup and scheduling. First run the split with the old fixture and the new fixture on one loaded EPYC. Require the same
named cases and failures under controlled faults to remain observable. Then ship the template and schema fixes separately so the winning change is measured. A single case above
budget gets profiled and refactored before the team buys more shards.

### 2. Make one verification answer three different GitHub requests

Build an obligation graph for each consuming event with an explicit evidence decision for every vertex. The inputs include the authoritative candidate and a signed policy
revision. Each vertex ends with a fresh witnessed pass, an independently witnessed pass on equivalent enforced inputs, or failure/pending. Track artifact recipes and test
inventories alongside conclusions. For a moved base, compute keys on the *new* ordered prefix and resolve its graph. Compare against the last verified prefix only when both the
source and proof are available. An ejection invalidates the affected descendants' candidate records and their pending publication, while genuinely identical obligations remain
reusable.

Start with one narrow closed class such as a generated test binary with a pinned fixture directory, no Git/event context and no service. Include the binary, loader/libraries,
directory entries and absence, mode bits, runtime settings, and policy freshness in the proof as applicable. Run the same selection and discovery, and compare success/failure
when a fixture is added, removed, renamed or moved through a symlink. The actual generator goldens choose a samples directory via an environment variable and enumerate it. The
dependency graph must capture both behaviors. See `velnor-new@95de9b704b:generator/crates/velnor-gen/tests/it/golden.rs:19–36`.

Test the gate with preflight that used the wrong base, a PR that skipped a required queue obligation, a reordered two-PR group, a push whose squash OID differs, and a revoked
producer. Require zero accepted stale or incomplete proofs. Measure weighted critical-path time saved. Skipping ninety fast obligations can save zero wall time when one
expensive obligation is still on the path. Keep network, time-sensitive policy and release duties fresh, including the advisory snapshot's freshness check.

### 3. Reserve zero-wait capacity without asking GitHub for node placement

Five nodes expose 480 logical CPUs. The highest per-node readings reach 95%. At 16:42:29 the five workers total 392.3 CPUs, about 82% of fleet capacity, with large placement
imbalance. A 16-CPU build can still wait behind memory and affinity constraints. At 95% fleet load only 24 logical CPUs would be left before reservations. Idle warm pods cannot
create more capacity. See evidence §B.1.2 and §B.2.1–2.

Reserve a 16-CPU build slot and 2 gate CPUs at each site initially. After the draft's 3 CPU per node reserve, that is `36 / 465 ≈ 7.7%` of allocatable CPU, plus enforceable
memory and disk budgets. Let revocable warming use spare CPU. Keep memory headroom for latency-critical jobs. Use a budget for actual starts, borrowed CPU,
cancellation-to-reclamation, and a service floor per organisation. Give Cargo and nextest one shared CPU budget for each concurrent lane. Measure SMT-effective throughput and
pressure, since 16 logical CPUs can have different useful capacity.

Submit generated work directly to the scheduler. While using ARC, route generic work by known class and make compatible workers warm. If precise placement still earns a
listener, make JIT identities independent of nodes and place the acquired work after the first renewal. GitHub picks the identity and Velnor picks the worker. Verify class and
capacity before taking ownership. Start M9's clock at demand eligibility before any offer. Replay peak arrivals plus a site loss, then drive a mixed-tenant load in lab. Require
start-wait P90 ≤ 2 s for the declared burst envelope and other tenants' P90 ≤ 110% of baseline. Add capacity or reduce reuse misses when the measured envelope cannot fit.

### 4. Start before the push and keep cold starts off the critical path

Put a preflight stream on the agent's normal save/test path in phase 2. Send a content manifest and the missing Git objects to a verifier ahead of the public push. Begin static
work as soon as the candidate is readable, and compile after computing the relevant input digests. A later edit invalidates only changed inputs. Remote verification runs in
disposable Linux environments with the required toolchain. The agent has no authority to report a required pass. On push, check the actual head, base and graph, then attach
completed and in-flight proofs to eligible obligations. No waiting to push is required.

Overlap saves `min(work time, useful lead time)`. With 45 s of required work and 35 s of useful lead, 10 s remains. Add a 5 s webhook/path budget and 3 s of publication to
reach 18 s. Record time since the last edit and useful/wasted CPU alongside push time. Count every push, including pushes with no preflight or a changed base. Run a three-day
opt-in test and require at least half of final heads to arrive with useful proof or running work, plus a reduction on both clocks.

For work without a head start, pre-stage Git objects, tools and action archives. Hold a credential-free sandbox ready. Test a snapshot-restored VM for Docker or fork-compatible
work before scaling privileged warm pools. Inject credentials and unique runtime state after restore. The 30 s Worker pipe timeout means a 25 s refresh cadence across 32 idle
Workers could start about `32 × 3,600/25 = 4,608` processes per hour at full inventory. Instrument idle CPU and channel hand-off. Compare cold Worker, pre-created pod with
on-demand Worker, and prestarted or restored Worker under load. Drop the refresh loop if its P90 gain is under 1 s. See draft §3.5, §7 and
`actions-runner/src/Runner.Worker/Worker.cs:23,48–68`.

### 5. Make required decisions insensitive to scheduling noise

Keep the observed failure as evidence. Use a fresh, matched execution to diagnose nondeterminism and keep a product failure blocking until a verifier can separate it from
infrastructure loss. Snapshot the authenticated event before parallel steps. The Worker rewrites `event.json` at step start and the producer parses it. Replace wall-clock
sleeps in state-machine tests with explicit signals, injected clocks and deterministic interleavings. Keep real subprocess and integration tests for the same conditions, using
readiness/progress as the deadline signal where elapsed time is infrastructure behavior. Keep real deadlines in tests that assert real timeouts. See decision I023,
`actions-runner/src/Runner.Worker/ActionRunner.cs:139–141`, `ExecutionContext.cs:1322–1336` and
`velnor-new@95de9b704b:crates/velnor-actions-orchestrator/src/task_producer.rs:87–117`.

A depth-eight queue can rebuild seven descendants after its head fails. Even under independent 2% per-candidate noise, `1 − .98^8 ≈ 15%` of full eight-candidate windows see at
least one false failure. Shared-worker contention can correlate those failures. Use a pressure-controlled replay to find deterministic timeout and workspace races first.
Observe every ejection, classify false-red and false-green separately, and choose depth from surviving throughput and rework CPU. Preserve a small independent sample of passing
and failing executions after deduplication. Aim for zero unexplained disagreements in 300 pairs before budgeting a 1% rate, with larger samples for correlated strata.

## Decisions you would overturn

- **I003, I013, N06, N09.** Move the independent verifier and App verdict experiment to phase 1. Keep warm ARC as the production control for ordinary Actions jobs. A merge-group
  webhook and a check on its SHA can remove the generated job graph without acquiring a runner job. Use a custom listener only where matched warm-ARC evidence shows that
  placement or custody requires it. T4 measures only 0.2 min median cluster queue today. See evidence §A T4 and §F.1.3, and `reviews/fable-5.2.md:251–273`.
- **I053, I055, N01.** Approve only independently witnessed, complete consuming-plan evidence, starting with queue-to-push. Remove C4's exception for in-job reports and the
  phase-1 shortcut based on a run conclusion and SHA. Keep the signed push verification marker required by the release gate. S3 has no current integration pin. T7 counts 120
  SHA matches with no per-obligation proof. See `velnor-new@95de9b704b:ci-helper/crates/velnor-ci/src/gate.rs:127–158`.
- **I054, I068, I069, N02.** Require closed-input proof per skipped obligation. Keep full required coverage for unknowns. A fanotify audit and two weeks of agreement qualify
  observability. By themselves they leave skipped and unexercised paths unobserved. See `velnor-new@95de9b704b:docs/hard-decisions.md:188–214`,
  `reviews/muse-wonder-full.md:598–606`.
- **I030, I044.** Publish each compatible main generation once and replicate changed content. That replaces five builds with one where replay proves the artifact can travel.
  Under CPU pressure, compare one builder per fleet, one per site and five local builds using seed-to-ready P90 and CPU cost. See evidence §B.2.1–2 and draft §5.3.
- **I064.** Test dependency optimization with the required assertions and overflow behavior intact. The cited 142.5 to 47.9 CPU-s result toggled debug assertions too. Give the
  66% figure credit only for the combination actually measured. See `proposals/fable-5.2.md:345`.
- **I067, N04.** Trigger distribution when `predicted local finish − predicted distributed finish` exceeds the uncertainty margin, including packaging, earliest runnable shard,
  queue, locality, exclusive groups and IO. For 640 CPU-s and a 12 s longest case, 16 versus 32 equivalent cores gives 40 s versus `20 + 5 = 25 s` in an idealized case. A cold
  1.48 GB archive can erase the win. See draft M6 and decision I045.
- **I075, I079.** Replace the fixed 4/8 steps with an arrival/rework replay and a capacity limit. Give merge work a guaranteed resource reservation. Enable wider batching only
  after proving each landable prefix or switching to ALLGREEN. Fable's worst day had 7–8 arrivals/hour against 1.9 merges/hour on its calculation. Two observed clock intervals
  and a 2% failure trigger cannot size concurrency. See decisions S2–S4 and `proposals/fable-5.2.md:50–59`.
- **I093, I094.** Qualify loss of a site or control plane before cutting over. Keep the rollback route until it passes regular drills through a runner version change. A label
  switch affects new jobs and leaves previously acquired jobs needing reconciliation. See evidence §B.1.5 and §F.2.4.
- **N03, N07, N08.** Move verification during editing and ordered candidate prediction ahead of runner replacement. Rank by seconds removed from the demand
  path per CPU-second. T7 and S4 already show why single `main ⊕ PR` guesses and exact PR tree matches cannot carry the target.
- **I119, N14.** Count matched-input red-to-green incidents over eligible first attempts. Measure green-to-red separately. Keep an equally protective blocking replacement before
  quarantine. S1 compares distinct event contexts and samples only one direction.
- **N15.** Keep a complete PR verdict and expose faster partial feedback separately. Withholding a required obligation until the queue moves latency out of the measured PR
  verdict and raises the cost of a real failure. A queue attempt rebuilt after another fast signal still spends CPU. See draft §1.1, M2, and decisions S2, N15.
- **N17.** Reject distributing the unchanged serial chain. Reopen distribution of independently schedulable units after measuring the graph and shortening the chain. The 696
  CPU-s and 170 s figures in N17 bound the old graph. Measure the split graph and reusable predecessors separately.

## Targets

These proposed exit budgets depend on the confidence gates above. Phase 3's 6/14 min push-to-merge assumes safe plan reuse. Use 8/18 min while full merge-group execution is
still required. Phase 4's one-minute median covers the whole workload mix. Light cohorts cover 58% in Fable's sample (47% generator/ci-helper, 7.5% local, 3.5% no Rust). Even
if every light head meets the P90 budget, 32 of the remaining 42 percentage points must also meet it. Phase 5 counts pushes with and without preflight.

| Phase | Budget | PR verdict P50/P90 (min) | Push-to-merge P50/P90 (min) | Runner-min/merged PR | Compile hit |
|---|---|---:|---:|---:|---:|
| 0. Gate routing, measurement, merge admission | Commit | 13 / 25 | 25 / 60 | ≤ 350 | 0% |
| 0 | Stretch | 11 / 18 | 18 / 42 | ≤ 280 | 0% |
| 1. Test floors and trusted push proof | Commit | 7 / 14 | 15 / 35 | ≤ 160 | 0% |
| 1 | Stretch | 5.5 / 10 | 12 / 25 | ≤ 125 | 0% |
| 2. Verified local seeds, feature parity, verifier shadow | Commit | 3.5 / 8 | 9 / 20 | ≤ 75 | ≥ 85% |
| 2 | Stretch | 2 / 5 | 6 / 13 | ≤ 55 | ≥ 93% |
| 3. App gate, reserved execution, proven exact reuse | Commit | 2 / 6 | 6 / 14 | ≤ 45 | ≥ 90% |
| 3 | Stretch | 1.2 / 3.5 | 4 / 9 | ≤ 30 | ≥ 96% |
| 4. Enforced inputs, useful queue speculation | Commit | 1 / 3.5 | 3 / 8 | ≤ 25 | ≥ 92% |
| 4 | Stretch | 0.6 / 1.8 | 1.8 / 5 | ≤ 18 | ≥ 97% |
| 5. Continuous preflight, shorter heavy paths | Commit | 0.75 / 2.5 | 2.5 / 6 | ≤ 20 | ≥ 94% |
| 5 | Stretch | 0.33 / 1 | 1.2 / 3.5 | ≤ 13 | ≥ 98% |

- **Clocks.** Start at accepted push and use the first terminal required result for an eligible head. Report superseded heads and stuck checks alongside that distribution.
  Measure push-to-merge for every merged PR. Record review and manual hold separately and keep them in the elapsed result. These budgets assume auto-merge and timely
  eligibility. Report misses without redefining the cohort. Keep cold, fork, preflight-miss, heavy and peak-load groups visible.
- **Queue.** Commit to demand-to-start P90 ≤ 2 s and P99 ≤ 10 s in phase 3 at a declared burst envelope. Stretch to P90 ≤ 1 s. Replay and sustain at least 12 merges/hour at
  phase 4's workload mix with all ejections counted, for 50% headroom over the observed 8/hour. Record eligible-to-enqueue, actual queue wait and GitHub publication separately.
- **Compute.** Divide total active verification sandbox/job-minutes by merged PRs. Charge failed and cancelled attempts, unmerged work, speculation, warmers, and audits in the
  same window. Report reserved idle slot-minutes, CPU-seconds and GiB-minutes separately. Fable measured 4.77 PR events and 1.25 group attempts per merge. For phase 2 the
  budget is about `4.77 × 10 + 1.25 × 16 + 7 = 74.7` runner-min. Phase 4 needs roughly `4.77 × 3 + 1.25 × 5 + 4 = 24.6`. Compute exit gates from actual sums. See
  `proposals/fable-5.2.md:120–124`.
- **Cache.** Count verified cross-job compiler-unit hits over all requested compile units, including cold, lockfile and pin changes. Exclude work never requested after receipt
  reuse. Report hit share weighted by avoided CPU and missed units on the critical path alongside the unit count. Keep the draft's 95% no-lock-change target as a diagnostic for
  phase 2. Report obligation receipts, crate sources and toolchain hits separately.
- **Promotion.** Use at least 50 merges to catch obvious regressions. For P90 promotion, use at least 200 matched heads spread over peak and quiet periods, compare against the
  incumbent, and report uncertainty and censoring. Validate same-confidence invariants and first-failure attribution separately from latency. A fast green check with unknown
  coverage fails the gate.

## Anything missing

- **Revocation of a green check already visible to GitHub.** C6 turns reuse off for future lookups. Define what happens to queue candidates carrying green checks from the
  revoked signer or policy. Carry a revocation epoch in verdict publication, pause or drain affected entries, and prove what invalidation reaches GitHub before landing. A
  seven-day receipt TTL alone leaves outstanding greens live. Preserve the evidence needed to identify the affected landed commits and release artifacts.
- **Who verifies changes to the verifier.** Velnor tests a candidate change to its own planner, runner and ledger. Keep the current released verifier authoritative during the
  upgrade. Run the proposed verifier on the same immutable graph, including controlled missing-test, wrong-configuration and wrong-candidate cases. Roll a new receipt schema or
  policy epoch only after the current authority qualifies the migration. Lock this to the release boundary in `docs/goals.md:76–78`.
- **Infra failure on the day every cache becomes cold.** Toolchain, runner and image upgrades invalidate related keys across the fleet at once. Pre-stage the new lane before
  making it required. Test an empty site after a toolchain bump with a real cold checkout and both feature configurations. Report the time to restore useful capacity and the
  backlog during the switchover.
- **Proof custody for unpublished work.** Scope preflight by immutable repository ID and authorized principal. Upload tracked inputs and declared dependencies only. Define
  denial, retention and deletion for content that never becomes a PR. Keep exported reports and traces bound to the run, attempt and audience. Legacy's run-attempt and artifact
  incidents show why naming a blob by job name cannot supply that binding. See evidence §E.6, incidents #670–#672.
- **A regression corpus for omitted work.** Compare the two gate implementations on controlled faults as well as ordinary green runs. Remove a selected test, change a fixture
  hidden from one lane, and produce a check for an old prefix. Require the same blocking behavior before enabling any reuse mode. Matching green inventories over 100 ordinary
  runs is too weak.
- **Decision order.** Ratify the pre-merge proof and publication contract before approving a new merge route. Fund the test-floor work, isolated gates, warm Cargo control, and
  App shadow in parallel. Commit to custom acquisition only if the matched comparison proves a residual need. Allow a later landing experiment to change the forge protocol only
  after it proves equivalent protection.

The Prove It Works principle shaped the release and lease findings. I checked the pinned gate and runner implementations for the behavior each optimization depends on. The
phase targets and test budgets are proposals derived from the supplied measurements. The lab trials above are future qualification work.
