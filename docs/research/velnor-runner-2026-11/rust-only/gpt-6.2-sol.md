# Rust-only redesign by GPT-6.2-Sol

| Field | Value |
|---|---|
| Model | GPT-6.2-Sol |
| Model id | `gpt-6.2-sol` |
| Client | codex-cli 0.169.0 (codex exec) |
| Reasoning effort | max |
| Date / run | 2026-11-19 |
| Brief | Rust-only redesign brief v1 (2026-11-19): design v2, all earlier rounds, the mbx and blaze experiments, and twelve binding owner decisions |

This is the model's own redesign after the owner's Rust-only decision, unedited except for formatting. It answers the Rust-only brief: a native Rust executor, the official runner only as a conformance oracle, latest APIs only, no Node, build once and reuse everywhere, correctness first, and CI started by a PR label.

---

# Velnor runner architecture in Rust

## 1. Summary

Make CI fast enough to fit inside the owner's review and merge flow while preserving the evidence required to land every tree.
Run GitHub Actions jobs on pre-warmed Linux slots with a native Rust listener and worker. Run the latest official runner in a lab to check protocol, execution, and action parity.
A label authorizes PR work. A GitHub App publishes `Required` for the verified PR head and independently for the exact merge-group candidate. GitHub still admits and merges the group.
A verification plan enumerates every required obligation. Sealed inputs and signed receipts let PRs, the queue, and `main` share verified work. Each queue candidate must satisfy its entire plan.
Use fixed-path Cargo workspaces on local XFS reflinks. Share immutable executables and receipts across the cluster. Shorten the longest tests before adding shards.
Keep debug verification and build the optimized Linux release payload once for each landed input that can be released. Publish the exact verified payload by digest.
For a labeled final push and a merge-ready PR, **committed targets [E]** are **90 s / 250 s** to verdict and **3 min / 8 min** to merge, P50 / P90.
The **stretch targets [E]** are **35 s / 110 s** to verdict and **75 s / 4 min** to merge, P50 / P90.
Report actual push-to-merge for every merged PR, including label, review, and hold time. Those waits and GitHub's scheduling tails have no fixed bound.
The measured comparators are **26.7 / 92.3 min** for scale-set PR runs and **85.1 / 456.6 min** for final-head CI start to merge. The old clocks start later than the proposed push clock. [Evidence, A T2](../evidence.md#t2-run-level-sample-via-jobs-api-successfailure-runs-split-by-era) · [Earlier push-to-merge sample](../draft/proposals/opus-5.5.md#11-push-to-merge)

## 2. The latency budget

Start the all-PR clock at GitHub's accepted final push and stop at `merged_at`.
Measure service time from the later of push or label authorization. Publish label/review and other hold time separately.
PR verdict ends when the exact-head `Required` concludes. Track first actionable failure, open PRs, superseded runs and timeouts.

[M] means a supplied or cited measurement, [E] an estimate with assumptions, and [S] source behavior.
`repo/` points to `~/Projects/tailrocks/velnor-new` at `d6be276a6ce9`. Live evidence used earlier revisions including `95de9b704b`.
`refs/` and `evidence/` resolve under `~/.cache/ci-ambition`. Runner source is v2.338.0 at `7afce9e`. §5 identifies the newer oracle.
[Evidence](../evidence.md) · [Design v2](../v2/velnor-runner-architecture.md) · [Decisions](../v2/decisions.md)

### One final labeled push

Three hosted gate queues consumed a median 5.8 min on November 17 and 57.0 min on November 18. PR success wall was 16.0 and 71.8 min respectively [M].
The design columns are central and pressured whole-path scenarios in seconds. Adding marginal P90s would not validate an end-to-end percentile.

| Stage | Today [M], P50 / P90 where available | Committed [E], s | Stretch [E], s | Change |
|---|---|---:|---:|---|
| GitHub event, webhook, offer | PR created to first job 3.7 / 26.3 min. Plan queue 5.5 / 23.1 min. Webhook timing unsplit. | 8 / 24 | 4 / 12 | Use App ingress and one coordinator on miss. Start sealed work while GitHub offers it. |
| Acquire | Included in 12 / 18 s scale-set created-to-started. | 0.7 / 1.5 | 0.5 / 1 | Keep sessions/connections warm. Budget RunService acquire and initial renew. |
| Sandbox | Included in queue/setup. Ephemeral runner and DinD today. | 0.6 / 1.5, overlapped | 0.3 / 1, overlapped | Prepare hot sandbox during acquire. Critical path pays the slower of the two. |
| Checkout | 6 / 8 s maintenance helpers. | 1 / 4 | 0.6 / 2 | Local exact-tree mirror. Fetch only a missing pack. |
| Tools | 10 / 18 s pinned tools, 2 / 5 s mise. | 0.6 / 2 | 0.2 / 1 | Verified baked tools and local activation. |
| Dependencies | Restore lookup 1 / 1 s, 48/48 misses. 435–522 downloads in sampled large jobs, fetch unsplit. | 1 / 5 | 0.3 / 2 | Offline verified source bundle. Fetch lockfile deltas once. |
| Compile and checks | Test build 14 / 23 s ci-helper, 81 / 116 s runner-host. CLI fixture 89 / 115 s. | 25 / 65 | 8 / 30 | Build missing units only. Hoist fixtures. Pressure case includes a large warm edit and link. |
| Tests | ci-helper 10 / 62 s, orchestration 78 / 104 s, host local 246 / 497 s. | 40 / 130 | 16 / 50 | Remove setup and split cases. Committed `30+10` / `100+30` s execution plus materialize/join. |
| Report | Results inside job wall, unsplit. | 2 / 4 | 1 / 3 | Upload logs during work, flush terminal evidence. |
| Required check | Verdict 0.7 min after 1.6 / 24.3 min queue. Required 0.1 min after 1.9 / 21.4 min queue. | 4 / 8 | 2 / 5 | Single App write and readback. |
| Enqueue and group | Green to entry 7.5 / 87.9 min. Entry to group run median 19 s. Group run 19.9 / 84.1 min. | 52 / 170 | 24 / 85 | Armed enqueue, grouping, proof. Committed `4+19+29` / `10+45+115`. Stretch `2+12+10` / `5+30+50`. |
| GitHub merges | Group finish to merged median 26 s. Entry to merge 26.5 / 368.2 min. | 20 / 60 | 12 / 45 | Minimum group one. Stretch needs faster observed platform propagation. |

Sources are evidence A T2, T4–T5, T9a–b and T10 and the 98-PR timeline join.
Instrument broker, sandbox, Results and push-to-webhook separately before measuring gains from Rust.
[Evidence](../evidence.md) · [Timeline join](../draft/proposals/opus-5.5.md#11-push-to-merge)

Committed sums **82.3 / 243.5 s** to verdict and **154.3 / 473.5 s** to merge [E].
Stretch sums **32.6 / 106 s** and **68.6 / 236 s** [E]. Round to §1.
The center is a small warm edit. The pressure case includes broad changes and a group miss.
Validate over ≥300 merges or four weeks with intervals and separate cold, failover, lockfile, release and preflight cohorts.
Charge actual outliers to the all-PR metric.

## 2b. Duplicate-work audit

`P` is a labeled PR, `Q` a merge group, `M` a main push, `R` a release, `N` scheduled/audit, `W` a warmer, and `C` the oracle.
"Same" means all operation inputs match, including authority for effects.
Costs are occupied runner-minutes and decimal GB, unless labeled GiB. Unsplit costs are already inside a measured aggregate.

The checkout has 15 workflows. Velnor Actions 0.1.81 generated the live 39-job CI and product flows.
The new five-root `velnor-gen` / `velnor-ci` workflow and release-plz are in samples. The repo has no root release-plz configuration.
Reconcile the named jobs, fixtures, and feature modes with the new inventory before regenerating required workflows.
`repo/.github/AGENTS.md:4–28`, `repo/.github/workflows/ci.yml:1–59,147–215,367–625,626–2898`,
`repo/generator/samples/velnor-new/expected/.github/workflows/rust-ci.yml:1–1713`, `repo/ci-helper/crates/velnor-ci/src/io/plan.rs:35–92`.

| Operation | Stages | Same inputs? | Today, min and GB | Decision and mechanism |
|---|---|---|---|---|
| Plan, expected jobs, ledger CLI, two gates | P/Q/M/W | Pure plan yes. Publication no. | Plan 0.9, ledger 0.8, verdict 0.7, Required 0.1 min [M]. | Reuse signed plan by tree/policy. Eliminate four helper jobs. |
| Event, JIT, lease, report, check | P/Q/M/R/N/C | No. | Cluster admission 0.2 / 0.3 min, hosted 4.1 / 23.6 [M]. | Keep real effects. Dedup by delivery, attempt and fence. Receipt hits can use an App check alone. |
| Checkout, submodules, LFS, actions | P/Q/M/R/N/W/C | Yes for immutable objects. | Checkout 0.10 / 0.13 min on maintenance helpers [M]. Bytes unsplit. | Reuse by Git object and LFS digest, with action manifests baked in. |
| Prelude, mise, tools and rustup | All | Yes by pin/platform. | 0.17 / 0.30 min pinned tools, mise 0.03 / 0.08 min. Tool entries 0.060–0.401 GB [M]. | Verify one image/tool manifest. Activate per sandbox. |
| Cargo registry/index, git sources and fetch | P/Q/M/R/N/W | Yes except changed identities. | Plan restores 0.086 GB. 48/48 sampled Rust restores miss [M]. | Fetch each lock-resolved checksum or git tree once. Mount offline. |
| Cargo units, native deps, build scripts, proc macros | P/Q/M/R/N/W | Yes by sealed inputs/profile. | Sampled test-build medians 0.23–1.47 min. One cold runner build wrote 10.14 GiB [M]. | Fixed-path XFS plus verified unit CAS. Treat release codegen separately. |
| Fmt, Clippy, docs, doctests, nextest, direct tests | P/Q/M/N | Yes for deterministic matching modes. | Runner-host Clippy 1.17, docs 1.0, tests 3.2 min. CLI tests 3.97 min [M]. | Preserve full plan, local-mode and feature coverage. Key inputs, binaries and inventory. |
| Example binaries and Docker/image fixtures | P/Q/M/N/W | Yes by build closure. | CLI fixture 1.48 / 1.92 min [M]. | Build once per digest. Give tests private mutable instances. |
| Alint, Jactionlint, Machete, Deny, config policy | P/Q/M/N | Partial. Advisories can change. | Four lint medians sum to 1.2 min [E on M]. | Key lint and advisory, tool and config snapshots. Preserve security-data refresh. |
| Samples and maintenance helpers | P/Q/M/N | Often. | Maintenance task 8.1 / 9.5 min [M]. | Keep coverage. Hoist identical fixture and CLI builds. |
| Source qualification ordinary/keep, generation, gate | P/N/manual | Partial. Modes distinct. | 31.8 and 43.15 min in two selected runs [M]. | Fold affected obligations into P/Q. Share plan and fixture inputs. |
| Job image candidate, attest, manual publish | Every push/M | Image often. Publish no. | 5.57 and 5.85 min in two candidates [M]. | Build and qualify once per Docker context/base/tool digest. |
| Push N and N+1, same-tree rebase | P | Yes by unchanged closure. | 45% cancelled. Replacements wait 60 / 626 s [M]. | Cancel old epoch. Reuse matching proof and in-flight work independent of cleanup. |
| Metadata edit or identical rerun | P | Yes if policy and actual inputs match. | 469/2,558 starts reused a head SHA [M]. | No new edit-triggered work. Replay an admissible proof. |
| Concurrent and stacked PRs, identical trees, preflight | P/agent | Yes by closure. | Cross-PR cost unsplit inside 52.9 min PR median [M]. | Share signed project receipts. Require sealed Velnor preflight. |
| Full merge proof and main validation | Q/M | 120/159 Q successes have same SHA on M. P/Q tree equality 7/45 [M]. | Q 64.6 + M 62.0 min [M]. | Full group plan every time. Promote verified landed closure. |
| Queue speculation | P/Q/W | Prediction may match. | Unmeasured. | Use only positive replayed value and spare capacity. Bind actual group SHA before publication. |
| State seal, tools/source producers, cache prune | M/W | Yes if objects are already verified. | Seal 11.93/12.95/21.20 min [M]. Entries 0.097–1.457 GB [M]. | Eliminate producer rebuilds. Warm existing CAS objects and garbage-collect by reachability. |
| Freshness, advisory updates, nightly/full policy, tag-rule drift | N/M/R | Partial. Live state differs. | Weekly freshness and daily tools drift [S]. | Keep fresh probes. Reuse deterministic proofs for a frozen policy epoch. |
| Qualification and official comparison | C/N | No. Executor and cold environment differ. | Manual scale-set/hosted matrix [S], duration unsplit. | Run paired official/native in lab. No production authority. |
| Release eligibility, version/changelog and release PR | M/R | Partial. Current authority differs. | Product eligibility 1.55–2.08 min [M]. | Verify landed proof and current metadata. Release PR uses ordinary label flow. |
| Optimized generator and SDK preview build/qualification | M/R | Yes where product and preview closures match. | Linux generator 6.95–7.27 min, Linux preview 7.73 [M]. | Build once on main. Promote the qualified optimized binary. |
| Linux runner images and resource probe | W/M/R | Partial. OCI layer IDs can match. | Product builds 7.67–8.78 min. Published images 1.116 GB [M]. | Preserve OCI/probe qualification by digest. Export required format once. |
| macOS host, Homebrew, Darwin preview | M/R/C | No, different targets. | Darwin branches 46.53 or 77.48 min per full product release. Darwin preview 17.72 min [M]. | Pause in phase 0, retaining target identity for later. |
| Release-plz tag, drafts and crate publishing | R | Tag/registry effects distinct. | Generated sample [S], no live baseline. | Use verified exact commit and pre-built crate bytes. Record effect idempotently. |
| `<bin>-<target>.tar.gz`, licences, COMMIT, SBOM, SHA256SUMS | M/R | Yes by exact payload/format. | Tools sums 0.17 min [M]. | Package once using canonical archive metadata and signed asset manifest. |
| Sign and attest builds, images and publish | M/R/W | Same subject and claim can match. | Linux generator 0.40–0.45 min, image attestation 1.07–1.33 [M]. | Keep OIDC/Sigstore statement by subject, predicate and signer. New claims need new effects. |
| Artifact transfer, release publish and readback | P/Q/M/R/N/C | Same bytes, distinct external IDs. | Product publishers 5.72–5.78 min including macOS. Tools retained 0.01255 GB of artifacts [M]. | Local CAS reads. Export once for each required GitHub effect. Verify exact asset set. |
| Rust CI tools and SDK preview channels | M/R/N | Partial. Target/format/version can differ. | Tools tag 7.23 min including 2.90 macOS. Preview Linux artifact 0.0160 GB [M]. | Use main payloads. Keep policy and channel receipts distinct. |

Live sources are `repo/.github/workflows/{ci,source-qualification,job-image-candidate,state seal,cache-prune,qualification,freshness}.yml`,
`product-release.yml:12–252`, `product-release-{images,binary,generator}.yml`, `product-release-generator.yml:979–1346` and `release-rust-ci-tools.yml:15–240`.
Generated release sources are `repo/generator/samples/release-bin/view/{release-plz.toml,.velnor/release.toml}`,
`repo/generator/crates/velnor-render/src/{release.rs:178–251,release_binaries.rs:42–119,164–183}` and `repo/ci-helper/crates/velnor-ci/src/{gate.rs:44,publish.rs:59,release_preflight.rs:30}`.

### Choose when to build optimized artifacts

Keep debug assertions, test binaries, Clippy and docs in the merge gate.
The measured Linux generator build and qualification take about 7 min, the image build another 7.7–8.8 min [M].
Adding both to two labeled pushes with distinct inputs would cost `2 × (7.0+7.7) = 29.4` extra runner-min before queue work [E].
Testing optimized artifacts alone would lose debug coverage. Build the optimized closure once on the landed main commit for a release intent, version change or applicable preview policy.
If the gate already needed the identical optimized fixture, verify its main provenance and promote it. Otherwise the publisher waits for the main builder to compile and qualify the exact payload.
The publisher reads the signed main manifest. Preserve existing crate/package verification before credentials.
If release-plz would repackage a verified crate, move its registry write to a narrow adapter that uploads the exact verified `.crate` bytes. Keep release-plz version, changelog, tag and draft orchestration.
`publish_no_verify` skips Cargo's extra verification build only after exact bytes pass the original verification. `repo/generator/crates/velnor-release/src/release_plz.rs:99`.

### Totals

- **Today per merged PR.** `3×52.9+64.6+62=285.3` CI runner-min [E on M], or 174.8 for one successful attempt at each stage using success-only medians. A selected example adds `12.95 seal + 25.45 preview + 31.8 qualification + 3×5.57 image` for **372.2 min** [E on M]. Optional jobs vary. Cancellation occupancy and pruning are unmeasured in this total. [CI evidence](../evidence.md) · [Seal](https://github.com/tailrocks/velnor-new/actions/runs/44339697416) · [Preview](https://github.com/tailrocks/velnor-new/actions/runs/44392894918) · [Qualification](https://github.com/tailrocks/velnor-new/actions/runs/44410050097) · [Image](https://github.com/tailrocks/velnor-new/actions/runs/44402443309)
- **Design per merged PR.** Budget **32 runner-min [E]** as `14 P/preflight + 6 Q + 1 M + 2 superseded + 2 warmers + 4 audits/oracle + 3 optimized/preview`. Stretch `6+2+0.5+0.5+1+2+2 = 14`. Allocate shared work once and report core-minutes separately.
- **Cache per merged PR.** Witnessed Plan transfer gives `5×0.086=0.43` GB [E on M]. Sampled Rust remote restores/mbx transfer contributed 0 GB. A successful state publication adds 0.097–1.457 GB [M entry sizes]. Repo cache stock was 3.495 GB [M]. Design **0 GitHub compile-cache GB, 0.60 GB cross-node per warm edit** [E, two 0.25 GB shards and 0.10 GB deltas], **1 GB new retained blobs** [E, 0.4 compile, 0.5 fixtures/tests, 0.1 input/proof]. Measure other old transfers and charge cold outliers.
- **Today per product release.** Two full runs used **71.37/103.97 runner-min** [M]. Their Linux portions were **24.83/26.48** [E], subtracting measured Darwin branches. One November 16 release published **1.321 GB** [M] across runner, host and generator assets, about **1.164 GB** Linux. Those runs list 0 retained Actions artifacts. Transient transfers and dependency-cache GB are unknown. The narrower tools tag used 7.23 min and 0.01255 GB retained artifacts [M]. [Product A](https://github.com/tailrocks/velnor-new/actions/runs/42339974423) · [Product B](https://github.com/tailrocks/velnor-new/actions/runs/43864723181) · [Generator assets](https://github.com/tailrocks/velnor-new/releases/tag/v0.1.80) · [Image assets](https://github.com/tailrocks/velnor-new/releases/tag/runner-b71a0045e42f6a8b620aa9283d0106bed622fe3b) · [Tools](https://github.com/tailrocks/velnor-new/actions/runs/44403976365)
- **Design per Linux product release.** **8–18 runner-min [E]**, 6–14 on main and 2–4 for publisher effects. **0 GitHub compile-cache GB, 0–1.2 GB cache transfer** [E, co-located read or one replica]. About **1.16 GB outward** on the historical asset mix [E on M]. Keep one logical payload and a durability replica. Measure retained blobs, transport and releases separately.

## 3. The native Rust runner

GitHub renders the workflow graph, matrix, dependencies, environments and permissions.
Velnor runs the worker-facing job and owns the verification graph inside the generated coordinator.
A trusted controller signs repository, exact head/tree, purpose, policy digest and generation in a ticket.
Start sealed pure work on label/group admission while GitHub offers the coordinator. The acquired job joins the work.
GitHub-context effects wait for that job. A receipt-only request can publish directly through the App.

| Component | Ownership |
|---|---|
| `velnor-runner-protocol` | Scale set, JIT, broker, RunService, Results and runtime token DTOs. No processes. |
| `velnor-runner-exec` | Job/step/context validation, expression templates, lifecycle and outcome reduction. |
| `velnor-runner-actions` | Closed native registry, manifests, hooks, output and effect contracts. |
| `velnor-runner-host` | Node agent, supervisor, warm workspaces, isolation, log capture and resource limits. |
| `velnor-ci` and generator crates | Workflow, proposed job plan and test inventories. Trusted policy sets the minimum plan. |
| `velnor-receipts` and controller | CAS, input manifests, receipts, lease/fence outbox, App checks and promotion. |
| `velnor-conformance` | Latest official oracle and fixture/replay comparison in a separate lab runner group. |

Run the listener in the node daemon and each worker in a fresh process with a private channel and limited authority.
Bound job decoding, template nodes, strings and steps. Separate incompatible input, timeout, cancellation and lost lease from success.

### Current protocol

1. Resolve the configured runner group with the App credential and current registration-token/admin exchange.
   Open a fenced scale-set session and poll with a cursor and truthful `X-ScaleSetMaxCapacity`.
2. Reserve actual ready resource capacity before `acquirejobs`. Honor returned IDs, reconcile assignment and refresh a 401 queue token.
   Persist routing before acknowledging the scale-set message. This reserves demand.
3. Generate scale-set JIT config for one ephemeral identity. Decode its typed settings and credentials in protected memory.
   Validate V2 broker URL/auth and expiry. Use current OAuth proof if required. Open the separate broker session and long-poll.
4. Handle `RunnerJobRequestRef` and required broker acknowledgement. Validate supplied RunService origin and billing/request IDs.
   POST `acquirejob` with message ID, runner OS and billing owner. Handle 404/409/422 as absent, already acquired or incompatible.
   The full job arrives here. Renew with its job credential before starting any steps.
5. Continue broker polling for `JobCancel` and runner control. Drain and retire the JIT identity after a single job.

Scale-set runtime routes carry a preview version in the current official client.
Exclude AES legacy sessions, inline requests, distributed-task cleanup and legacy pipelines, cache v1 and artifacts v3.
Current DTOs still use wire names `AgentJobRequestMessage`, `Plan` and `SystemVssConnection`. Missing RunService or Results fails admission.
Keep broker/admin credentials outside jobs. Validate signed upload destinations and drop auth on redirects across origins.
Use acquired URLs and scoped tokens for cache service v2, artifact v4, Results and OIDC. Honor job permissions.
`refs/arc/cmd/ghalistener/scaler/scaler.go:149`, `refs/arc/controllers/actions.github.com/ephemeralrunner_controller.go:853`,
`evidence/proto-raw/scaleset/{client.go:765–796,session_client.go:45–303}`.
`refs/actions-runner/src/Runner.Listener/{Runner.cs:234–257,699–800,RunnerJobRequestRef.cs:1–35,BrokerMessageListener.cs:270–314,433–452}`,
`Runner.Common/BrokerServer.cs:67–84`, `Sdk/RSWebApi/RunServiceHttpClient.cs:70–200`, `Runner.Worker/Handlers/NodeScriptActionHandler.cs:55–83`.
[Runner protocol evidence](../../evidence/protocol.md) · [GitHub self-hosted runner reference](https://docs.github.com/en/actions/reference/runners/self-hosted-runners)

### Job model and templates

Define distinct `ScaleOffer`, `BrokerRef`, `JobLease`, `Job` and `ValidatedJob` types.
Keep plan/timeline/request/job IDs separate from PR, group and attempt IDs.
A job has ordered templated steps, resource endpoints, container/services, workspace/defaults, context data, variables, masks, output and environment declarations.
Preserve missing versus explicit null. Accept additive telemetry fields. Reject unknown executable forms before a repository step.
Validate every remote pin, local composite, nested reference and post hook against the authorized tree before starting. Reject unsupported declarations even in a skipped branch.

Write a fresh Rust expression evaluator using the current oracle semantics.
Preserve phases, spans, context lookup and case, null/coercion, indexing/projection, quoting, comparisons and short-circuiting.
Implement string predicates, `format`, `join`, `toJSON`, `fromJSON` and workspace-sensitive `hashFiles` for admitted jobs.
Evaluate implicit success and `success()`, `failure()`, `cancelled()`, `always()` with correct step/composite scope.
Evaluate `if`, `with`, `env`, defaults, timeout, continue-on-error and outputs at their proper phases. Gate new generator features on oracle cases.
`refs/actions-runner/src/Sdk/DTPipelines/Pipelines/AgentJobRequestMessage.cs:27–47,92–250,486–508`,
`ObjectTemplating/PipelineTemplateEvaluator.cs:56–165`, `refs/actions-runner/src/Runner.Worker/StepsRunner.cs:85–112,170–236`.

### Step execution and Results

Initialize GitHub-provided `github`, `job`, `strategy`, `matrix`, `needs`, `inputs` and `vars`. Add `runner` platform facts.
Build scoped `env`, `steps` and `secrets` contexts. Keep GitHub event SHA/ref distinct from the verified checkout identity.
Execute pinned shell, defaults, flags, working dir and container path mapping. Apply env and PATH in step order.
Create per-step or embedded-scope `GITHUB_ENV`, `GITHUB_PATH`, `GITHUB_OUTPUT`, `GITHUB_STATE` and `GITHUB_STEP_SUMMARY`.
Parse multiline files and reserved names. Process annotations, groups, stop/resume commands and secret-looking outputs.
Mask secrets, encoded forms, regex hints and dynamic masks before logs, spooling, summaries and check output.
Give composite children their nested scopes and outputs. Register eligible pre/post work and run post in reverse order with its stored state.
Keep raw outcome and final conclusion after continue-on-error. The signed verification plan prevents skipped or tolerated failures from counting as passes.
`refs/actions-runner/src/Runner.Worker/{ActionRunner.cs:136–184,282,StepsRunner.cs:71–112,301–381,Handlers/CompositeActionHandler.cs:46–80,133–142,210–269,358–400,FileCommandManager.cs:42–75,221–275,ActionCommandManager.cs:420–447}`.

Stream redacted numbered log batches. Publish monotonic Results timeline steps via `WorkflowStepUpdateService/WorkflowStepsUpdate`.
Obtain signed storage URLs for logs and summaries, upload, then commit metadata. Drain required evidence before `completejob` with conclusions, annotations and outputs.
Live viewer delivery can be best effort. Missing required evidence or uncertain completion blocks the gate.
`refs/actions-runner/src/Runner.Worker/{JobRunner.cs:85–105,290–353,ExecutionContext.cs:1147}`,
`Runner.Common/{JobServerQueue.cs:275,414,698–705,940,ResultsServer.cs:88–93,124,223,Logging.cs:164}`,
`Sdk/WebApi/WebApi/ResultsHttpClient.cs:610–632`.

### Start, cancellation and recovery

Keep 2–4 ready JIT broker sessions per site initially [E], independently of the scale-set poll.
Prewarm clean namespaces, OCI images and tool manifests. Restore clean service sandboxes from snapshots where useful, then inject identity and reset runtime state.
Reserve a slot and start the mount/clone on offer while acquire and first renewal complete. XFS clone measured 0.25 s [M].
Budget 0.3–0.6 s hot sandbox start and 0.7 s acquire+renew [E]. Track cold image/JIT misses separately.
GitHub controls assignment. Use actual class/site capacity and route pure work on the local scheduler.

Renew normally every 60 s and sooner if expiry margin requires it. Use a separate renewal loop.
Fence expired ownership, observe job/step deadlines and re-evaluate cancellation conditions.
Use the official signal contract where relevant, including SIGINT 7.5 s then SIGTERM 2.5 s. Kill remaining cgroup processes after grace. A new head starts independently.
`refs/actions-runner/src/Runner.Listener/JobDispatcher.cs:140–155,380–410,734–822`, `Runner.Sdk/ProcessInvoker.cs:32–33,443–465`.

Persist lease and attempt, external intent, redacted report sequence and outcome in an outbox.
On crash, fence and reap abandoned generations. Reconcile uncertain remote results before retrying with the same identity and payload.
Never resume arbitrary shell midway. Reuse only sealed finished obligations. A missing or expired lease cannot mint a pass.
Publish object bytes before the signed index. Reconcile uploads, tags and completions by readback. Unknown effects remain blocking.

### Reuse boundaries

Copy small reviewed Apache-2.0 Rust modules and tests into velnor-new with notices and origin SHA.
Candidates include scale-set cursor/session, current broker/RunService/Results DTOs, command files and checkout/cache/artifact/mise/attestation primitives.
Strip legacy branches and requalify against the latest oracle. Use archived expression tests as evidence and write the production evaluator fresh.
`refs/velnor-legacy/Cargo.toml:19`, `crates/velnor-runner/src/{scaleset/session.rs:96,308,protocol.rs:2643,9318,action.rs:378–416,admission.rs:744,1010,script_step.rs:771–796,command_files.rs:12,checkout.rs,execution/artifacts.rs,gha_cache.rs,attestation.rs}`.
Reuse velnor-new's probe, image/mount and authority models, generator types and helper plan inputs.
`repo/crates/velnor-runner/crates/{velnor-resource-probe/src/mountinfo.rs:22–107,velnor-runner-host/src/config/job_authority.rs:42–132}`,
`repo/ci-helper/crates/velnor-ci/src/{io/plan.rs:35–92,gate.rs:44}`.
Write closed admission, execution/scheduler state, proof issuance and check/effect outboxes for the new boundaries.

## 4. Native actions

The generator and executor share a registry keyed by repository, path, pinned commit and manifest digest.
Local composites bind to the authorized tree. Validate all nested actions and allowed inputs before the first step.
Replace production-relevant `actions/github-script` with typed Rust operations. The Node/Docker action fixtures become lab refusal cases.
Qualify Docker services and job containers as distinct runtime features.

| Action | Native behavior and locality | Today [M] | Warm target [E] |
|---|---|---|---|
| `actions/checkout` | Local verified Git objects. Match ref/depth, sparse checkout, submodules/LFS, worktree and credential cleanup. | 4 / 6 s in 6,313 steps. | 0.6–2 s local. Exact fetch on miss, budget 4–15 s ordinary pack. |
| `actions/cache/restore` | Cache v2 key/version/scope and ordered prefixes. Preserve exact/fallback/miss outputs, lookup-only and fail-on-miss. Validate identity before local reuse. | 1 / 1 s sources lookup, 3 / 10 s hosted mise. Rust sources missed 48/48. | 0.2–1 s for metadata/local view. Remote bytes and extraction on misses. |
| `actions/cache/save`, `actions/cache` | Cache v2 create/upload/finalize. Preserve immutable keys, conflicts and combined post hook. Internal compiled units use CAS. | No isolated save median. | 0.2–1 s local reference. External save adds ~1 s control plus transfer. |
| `actions/upload-artifact` | Artifact v4 create, upload and finalize archive with ID, digest, URL, retention and selected files. Match empty/hidden input behavior. | Crate reports 3 / 4 s in 4,904 steps. | 0.1–0.5 s internal reference, 1–4 s actual small GitHub upload. |
| `actions/download-artifact` | Resolve authorized run/name/ID or pattern, verify digest, keep merge/extract semantics. Read authorized local bytes if present. | 20 / 33 s matrix download. 2 s bundle, 18–49 s images in one release. | 0.2–1 s small local report. 1–3 s for 1.116 GB at 1–2 GB/s plus metadata. |
| `jdx/mise-action` | Use checksum-pinned local tools, honor config/dir, PATH, env and install/cache modes. | Setup 2 / 4 s in 6,113 steps. Pinned tools 10 / 18 s in cluster sample. | 0.2–1 s activation. Baked image pays install once per input. |
| `jdx/mr-boxington-action` | Pinned mbx from local store, exact setup and post, private target. Remove step from default Cargo lane. | Setup 1 / 2 s. Post 0 / 1 s. | 0.1–0.5 s when admitted. |
| `actions/attest` | Native Sigstore statement over approved subjects and predicate. Scoped job OIDC, signing, GitHub bundle and required registry upload. | Custom state seal predicate unsplit. | ~0.1 s metadata plus 2–10 s new signing/confirmation. Reuse identical verified claim. |
| `actions/attest-build-provenance` | Same adapter with provenance schema. Preserve builder, workflow/ref, subject, bundle outputs and readback. | 2 s Linux generator, 9 s images. Bundle readback 11 s in sample. | 2–10 s new claim, off ordinary merge path. |
| `asamarts/alint` | Verified local binary with specified config, format, path and warning policy. Keep line-cap probe. | 1 / 2 s in 191 steps. | 0.3–2 s fresh. Reuse exact lint proof. |
| `actions/create-github-app-token` | Release credential broker exchanges allowed repo/install scope. Mask output and revoke in post as required. | Generated sample only. | 0.3–2 s each for mint/revoke. |
| `tailrocks/velnor-new/actions/setup-velnor` and local tool composites | Verify helper/platform digest, bind seed paths, honor prelude and restore/scrub. Match composite env and state. | Helper isolated timing unavailable. Pinned tools 10 / 18 s. | 0.2–1 s local. Verified fetch on miss. |
| Declared-task and Linux release build/qualify composites | Run validated child steps, preserve raw outcomes and per-task reports. Local fixture and tool inputs. | Maintenance task 486 / 570 s, release timings §2b. | <0.1 s composite overhead. Required work is charged separately. |

Step medians/tails use timestamped successes in `evidence/ci-raw/jobs_compact.json` and the release sample.
Pins and inputs appear in `repo/.github/workflows/ci.yml:61–127,181–195,308–337,576–589,684–746`,
`repo/.github/actions/*/action.yml`, `repo/actions/setup-velnor/action.yml:20–63` and `repo/generator/crates/velnor-render/src/steps.rs:24–35`.
Byte reuse still needs the original service authorization. Prefix cache hits cannot pass an obligation.
[CI evidence](../evidence.md) · [Release sample](https://github.com/tailrocks/velnor-new/actions/runs/43864723181) · [Cache](https://github.com/actions/cache) · [Upload](https://github.com/actions/upload-artifact) · [Download](https://github.com/actions/download-artifact) · [OIDC](https://docs.github.com/en/actions/reference/security/oidc) · [Attestations](https://docs.github.com/en/rest/repos/attestations#create-an-attestation)

## 5. The conformance oracle

Pin the latest official runner binary and digest in a separate lab repo and runner group.
As checked November 19, the release API returned **v2.338.2** at `d1eb12d4b1`. Source anchors use local v2.338.0 until the live recheck.
The patch adds broker session ownership on deletion and a WebSocket connectivity probe. Include both in the contract diff.
Share immutable Linux image, source, tools, action pins and fixtures between paired jobs.
The lab can run upstream Node-backed actions to compare their outputs. It cannot write production receipts, checks or artifacts. [Latest official release](https://github.com/actions/runner/releases/tag/v2.338.2)

### Corpus

- Actual generated CI and Linux release/preview jobs, job containers/services and every admitted action call shape.
- Expression phases and coercion, scopes, command files, masks, outcome/conclusion, nested composites, post order, cancellation and timeouts.
- Scrubbed wire replay for JIT expiry, stale/duplicate offers, broker ack/cancel, lost renewals, 404/409/422 and ambiguous completion.
- Real cache v2, artifact v4, checkout and OIDC/attestation effects, including missing/immutable/empty cases, wrong bytes and scoped auth.
- Repo cases for no-op, stacked PRs, new locks, same tree/different SHA, stale base, revoked receipt, hidden test failure and failed queue prefix.
- Unsupported inputs must fail preflight in Rust. Record them as explicit refusal fixtures even if the official runner can execute them.

Normalize fixture IDs, paths, timestamps relative to order, ports and opaque service IDs. Preserve raw traces for review.
Compare checkout tree, env/context values, shell args, status and raw outcome, step ordering, annotations, summaries, filenames and digests.
Verify signatures and trust roots, then compare the claims with mapped lab IDs. Test absence of secrets before normalization.
Only reviewed cosmetic differences get a normalizer. A changed verdict, missing post, wrong byte or extra effect is a hard mismatch.

### Cutover and drift

1. Start with 240 semantic, 60 transport/fault and 24 actual job shapes [E coverage floor].
   Run the latest official release and Rust with reuse off. Require **100%** of declared production cells, zero unexplained diffs and zero bypasses.
2. Compare at least 200 real candidate trees with injected red cases across both sites and two representative weeks [E].
   Exercise faults at each effect/lease boundary. Treat any missing expected cell as failure.
3. Authorize only qualified Rust on first production traffic. Cold-qualify broad exact proofs before reuse, then qualify finer cache and receipt classes independently.
4. Poll releases every 30 min and start full pairing on each new release. Probe live broker-to-completion every 15 min per site, plus Results/cache/artifact/OIDC effects.
5. Alert within 30 min and isolate the first changed stage/repro within 2 h [E]. Server drift may arrive without a release.
   Query registration and runtime deprecation dates. GitHub documents 63 days after version availability, with an immediate exception for critical updates.
6. Drain affected builds. Roll to a previously qualified Rust image only while compatible, otherwise keep the gate blocking.
   Advertise the validated compatibility version and require target-pool evidence during progressive rollouts.
   [Runner update policy](https://docs.github.com/en/actions/reference/runners/self-hosted-runners#runner-software-updates-on-self-hosted-runners)

Oracle parity detects observed disagreements. §6 supplies the input and coverage conditions that authorize reuse.

## 6. Compile, cache and test

### Cache choice

Use pinned Rust/Cargo/Clippy 1.99.1 and nextest 0.9.151 at fixed workspace, Cargo home and target paths in private namespaces. `repo/rust-toolchain.toml`, `repo/mise.toml`.
Mount verified dependency/toolchain sources offline. Reflink certified target files into private writable views.
Retain only units whose complete inputs match. Discard affected paths, fingerprints and outputs before Cargo runs.
Use stable Cargo metadata, invocation records and emitted artefacts. Keep a full-root build when the unit or build-script closure cannot be established.
A successful Cargo invocation or equal mtime alone proves no cache lineage.
Keep incremental state private unless its portability and completeness pass a separate audit.

| Choice | Relevant observations [M] | Decision |
|---|---|---|
| XFS clone plus Cargo | 0.25 s and 2.5 MB to clone. Exact warm build 0.31 s, zero compiles. Cold 92 s, 10.14 GiB written. ext4 full copy 8.6 s and 10.75 GiB. | Default on node-local XFS. Clone writable views only. |
| Local mbx | Fixed-path managed target 0.32 s. Store-only hit 7.2 s and 1.94 GiB extra. Cold 93.2 s plus 8.7 GiB upload. Mac APFS 473 vs 457 MiB/checkout, both rebuild six path crates. HFS+ 835 vs 1,411 MiB. | Keep pinned action compatibility. Remove from default builds and prohibit cross-job mutable targets. |
| mbx-cache | Cold second worker 39.9 s and 8.7 GiB down at ~215 MB/s. Lock edit plain local 67.4 s, mbx producer 88.4 s and 6.8 GiB up, remote warm 43.4 s and 6.8 GiB down. | Experiment for cold bootstrap only. Compare against immutable CAS delta, warm placement and cold Cargo. |
| Immutable CAS | A fully read runner snapshot clone used 14.29 GB of its own page cache. Cross-node unit deltas remain unmeasured. | Distribute verified inputs, binaries, image layers and proofs. Read common immutable inodes locally. Extend to compile units after proof. |
| blaze `0.20261105.4` | Every workspace failed some targets. Cold S-gen 215.5 s vs 17.9 s Cargo or 13.1 s nonincremental. Warm edits 5–15× slower. Missed required Clippy denial, used different nightly and 18–44 GB RSS. | Ineligible for the merge gate or its producers. Re-evaluate future versions against the pinned project toolchain. |

Cluster experiments were one fixed-path workload, a lock change and idle nodes at 32 build jobs.
APFS and blaze results came from different Mac conditions. Treat them as evidence about those tests.
Evaluate mbx-cache if >10% of requests still miss locality [E]. Qualify TLS, scoped writes, durable index, restart and collision behavior.
Retain it only after a two-week paired P90 win against CAS and warm placement with the same proof.
At 215 MB/s, 8.7 GiB is about 43 s of payload time [E on M]. A 0.25 GB sealed shard delta is about 1.2 s plus verification.
Choose local copy, transfer or build by measured completion cost. Allow duplicate work only with a demonstrated time win. Avoid default racing.
[Cluster P1–P3, M1–M4, L1, E1](../v2/experiments/mbx-cache-cluster.md) · [Mac](../v2/experiments/mbx-cache-mac.md) · [Blaze B1–B6](../v2/experiments/blaze.md)

### Complete keys and trusted promotion

The trusted planner lists the baseline obligations and new coverage. Compare inventories independently and require reviewed policy for removal.
Use a canonical domain-separated input manifest with schema, operation, policy, runner/image/tool identities and platform.
Include transitive sources, dependency resolution, lockfiles, features, profiles/flags, environment, mode bits, paths, symlinks, linker and generated/fixture data.
A test key includes the actual binary and every executable/library it can load, including `CARGO_BIN_EXE_*`, as well as expected test IDs, command, data and service state.
Key negative lookups and directory membership. Include Git metadata, event fields and hostname or CPU facts whenever the job can observe relevant differences.
Define explicit clock, seed and network fixtures where a case supports them. A live service or observable unsealed clock, entropy or scheduling channel makes the obligation nonreusable.

Enforce the manifest with a host-prepared view, controlled writable paths, pinned libraries and network rules.
A narrow closure must match the full-workspace result before enrollment. A missing file can change discovery or select a benign default.
Observation traces can find undeclared reads. They do not prove completeness. Namespaces alone cannot freeze time, entropy or scheduling.
Require controlled input interfaces or evidence that a varying channel cannot affect the outcome. Use the broader plan or fresh execution when unknown.
A new path, build script, cross-workspace dep, policy generation or toolchain invalidates relevant assumptions.

| Reuse or skip | Completeness | Incomplete result | Detection | Automatic fallback |
|---|---|---|---|---|
| Git, tools, sources, artifact bytes | Exact bytes, path type/mode, trusted provider and origin. Resolve mutable refs first. | Wrong tree/tool/blob. | Digest/provenance check and wrong-object fixtures. | Quarantine and refetch pinned identity. |
| Cargo unit or snapshot, including mbx | Complete compile/build-script environment and verified artifact closure. | Stale link or skipped compiler error. | Cold artifact/result comparison, same-mtime edit, changed linker/feature/script faults. | Disable lineage. Rebuild affected root from empty. |
| Affected/package selection | Full signed minimum plan, reverse deps and enforced filesystem closure. Include new/unowned paths. | Required test or job omitted. | Compare full inventory and seeded failing changes. | Require full root or repo. Revoke selector receipts. |
| Test/lint/fixture pass | Executables, data, expected tests, configuration and stochastic inputs match. | Hidden failure, wrong binary or empty discovery. | Cold audits and mutations of binary, files, env, filters and fixtures. | Revoke proof and dependent joins. Execute fresh. |
| PR rebase, stack, concurrency and preflight | All required keys and producer scopes valid. Same tree can bind a new SHA only when SHA-sensitive inputs match. | Partial or forged proof passes new head. | Independently rebuild plan from GitHub head. Test missing receipts, wrong signer and expiration. | Hold check or run missing work without reuse. |
| Speculation, merge group and main | Exact offered/landed Git identity, full keys and current policy. | Wrong queue prefix or landed proof. | Read actual Git objects and replay ejection/changed prefix. | Discard prediction. Execute full required misses and hold release. |
| Release and public effects | Exact main proof, bytes, predicate, tag, destination and signer policy. | Wrong payload or false provenance. | Verify subject, ref and asset digest at every boundary. | Freeze draft and revoke affected manifest. |
| Warming, schedules and audit | Copies preserve signed bytes. Policy snapshots are frozen. Cold audit uses an empty target and no reused verdict. | Stale policy data, circular audit or expired blob. | Check retention age, I/O lineage and cold setup. | Refresh pinned inputs and disable reuse in scope. |

A host signer binds input manifest, observed exits/inventory, policy, lease and output digests.
A single running key can serve multiple requests. Cancellation drops its subscriber and fences publication for that request.
Share verified preliminary receipts across PRs. The trusted promoter separately re-derives keys at the landed SHA and verifies signatures, scope, revocation, inventory and bytes.
Give `main` a new signed reference only after that check. Missing proof requires trusted execution. PR code cannot insert trusted entries.
No caller may authorize its own output. Preserve proof lineage to revoke every derived check or payload after a fault.

### Bring down the longest test and distribute work

After removing the named loops, observed runner-host maximum cases still took **124.8 / 137.6 s**. One sampled CLI case took 924.8 s.
Measure elapsed and CPU time alone and under contention. Separate migration, fixture build, waits and actual test execution.
[Design feedback F-H33](../v2/design-feedback-comparison.md#correctness-and-confidence-holes) · [Decision log](../v2/decisions.md#f-the-critical-path-floor)

1. Create one closed, checked journal template per schema and reflink private copies. Gate current-schema open at ≤1 s [E]. Keep real historical migration tests.
2. Split 40 image-publication cases and six-state retirement loop. Do the same for blob faults, family publication, receipt integrity and archive-guard goldens.
   Preserve actual CLI and side-effect assertions. Build immutable fixtures by digest and keep per-case registries, ports and transaction state.
3. Replace incidental sleeps with readiness. Isolate temp paths and ports. Keep real timeout/stress checks.
   Select nextest CI profile explicitly. `ci-helper` serializes tests there. Its default-profile 2 s timeout is a different setting.
4. Hoist repeated CLI/example builds and remove eight `build.jobs=2` caps after qualification.
   Test line-table debuginfo and optimized deps with debug assertions/overflow checks held fixed. Preserve original tests.
5. Gate ≤60 s maximum single-case wall after phase 2, ≤15 s stretch and 10 CPU-s objective [E]. Keep an explicit lane for known expensive cases until fixed.

`repo/crates/velnor-actions-cli/tests/image_publication_public.rs:85–92`,
`repo/crates/velnor-runner/crates/velnor-runner-host/src/{launch/bootstrap/unbound_retirement/tests/shape.rs:97–106,journal/file_backed.rs:63–102}`,
`repo/mise.toml:90–193`, `repo/{.config,ci-helper/.config,crates/velnor-runner/.config}/nextest.toml`.

Build each nextest mode once and export inventory, binaries, dynamic libraries, data and fixture helpers separately.
Keep embedded workspace and executable paths valid across slots. Use supported nextest archive/run or partition, plus a distinct rustdoc/doctest lane.
Assign longest predicted tests first, batch tiny cases, allow same-site stealing and enforce resource groups globally.
Check all expected IDs exactly once. Missing, duplicate or skipped required tests and unexpected red results block.
Send a test elsewhere only if saved tail exceeds transfer/startup. A 10 s tail gain beats a 1.2 s transfer and 1 s startup [E].
[Nextest archives](https://nexte.st/docs/ci-features/archiving/) · [Partitioning](https://nexte.st/docs/ci-features/partitioning/)

### Preserve stochastic coverage

Three independent runs catch a 20%-probability error with `1−0.8³=48.8%` likelihood [E model]. One catches 20%.
Remove duplicate executions after deterministic or controlled-equivalent coverage is proven.
If useful diversity remains, use distinct required seeds, schedules or service epochs in parallel. Run unsealed volatile obligations fresh.
A retry may diagnose a failure. It cannot erase one. Audit with 100% cold shadow of new reuse classes, then every twentieth comparable landing plus policy/lineage changes.
Track both disagreement directions under representative load. The old 8.1% was one direction on 124 pairs.
Target <1% infra false reds and <0.5% false ejections [E]. About 300 independent incident-free observations bound a 1% rate at one-sided 95% confidence.
Any confirmed false green revokes the affected lineage. Sampling checks the proof and cannot substitute for it.
[Feedback F-H34–35](../v2/design-feedback-comparison.md#correctness-and-confidence-holes)

## 7. Scheduling and the cluster

Use k3s for fleet deployment and accounting. A Rust node agent owns warm slots, leased resources and COW workspaces.
Use a small number of site/class scale sets selected by generated labels. GitHub assigns broker jobs. Velnor places its own pure obligations.
Score slots by expected finish given memory, queue, cache, network and site. A warm local slot often beats an idle node that needs 8.7 GiB.
Require live heartbeats and expiring inventory. Reconcile advertised idle capacity with actual ready slots.

The evidence records five EPYC workers, 96 SMT threads and roughly 125 GiB RAM each. Topology is approximately two DE and three FI workers, with 35 ms inter-site ping.
Observed CPU reached 92–95% despite little scale-set backlog. ARC requests of 2 CPU/4 GiB hid much larger usage bursts. [Cluster evidence, B](../evidence.md)

| Priority | Admission |
|---|---|
| Gate and actual merge group | Reserve two warm gate slots and a link-capable pool per site to start [E]. Reserve an independent start for PR replacements. |
| Labeled PR and preflight | Fair share per repo/PR, age boost and keyed join. Preflight borrows spare resources. |
| Release, audit, speculation and warmers | Budget by deadline and borrow slack. Yield at task boundaries. Charge wasted work, bytes and retained state. |
| ChainArgos and jackin | Keep separate Apps, runner groups, stores and quotas. Guarantee measured memory/CPU floors and let peers borrow idle capacity. |

Aim for zero local waiting and zero cold setup within the qualified envelope. Target total eligible-to-start p50 ≤0.5 s, p90 ≤2 s and p99 ≤10 s [E].
Measure GitHub offer and actual cold restart separately. Bake images/tools and pre-open sessions to keep JIT and image pull off the warm path.
Schedule all 480 hardware threads work-conservingly, including host work. Reserve four per worker for control, leaving about 460 job threads [E].
Reserve 12 GiB host, 20 GiB shared page cache and 8 GiB emergency per worker. This leaves about 85 GiB admitted job memory [E].
Two 24-GiB/24-thread build slots, two 10-GiB/12-thread test slots and four 4-GiB/4-thread short slots use 84 GiB and 88 threads [E].
A job needing 32–48 GiB consumes multiple shares. Cap nextest threads and coordinate Cargo jobserver slots. Give timing tests sibling-aware cores where needed.
Size from actual peaks, PSI and throttling and clamp work before eviction. [Kubernetes reservations](https://kubernetes.io/docs/tasks/administer-cluster/reserve-compute-resources/) · [CPU Manager](https://kubernetes.io/docs/tasks/administer-cluster/cpu-management-policies/)

### Isolation and both sites

Authors are trusted. Repo processes still need bounded effects and resources.
Use per-job user/mount/PID/network namespaces, private writes, minimal egress, cgroups and supervised cleanup.
Use rootless services where qualified, or warm isolated VMs/dedicated workers for cases requiring stronger container isolation.
Separate release/attestation, preflight and ordinary identities. Keep host Docker, credentials, CAS metadata and signing authority outside job reach.
There is no fork lane. PR-to-main promotion remains an independent correctness check.

Confirm site labels. Keep Git/crates mirrors, warm slots and verified objects at both sites.
Replicate proofs, active manifests and likely-used objects. Keep multi-GiB target views local unless transfer is faster than waiting or building.
Put independent work on both sites. Ship test binaries only when the verified transfer recovers critical path. One-site capacity is about 184 or 276 job threads [E].
A failed node loses its leases. An isolated site may finish provisional work. Only a fenced leader with quorum publishes.
The current single k3s control plane cannot survive arbitrary site loss. Qualify a redundant control plane and a third failure-domain ledger witness for automatic write failover from either site.
With no authoritative quorum, stop check and release writes. Drill partition, loss and reconciliation before promising the recovery target.

### Host and kernel

1. Use local XFS reflink data volumes with per-org project quotas. Keep immutable common inodes and private writable targets. A fully read clone can add 14.29 GB resident [M].
2. Tune `memory.high`, `memory.max`, `pids.max`, I/O weights and eviction reserves from peaks. Keep CPU weights and truthful requests for shared capacity.
3. Keep SMT and the measured kernel baseline. Test exclusive CPU policy only for demonstrated interference.
4. Verify linker choice, debug info, incremental, governor, NUMA, THP and writeback in paired runs before adoption. Measure CPU, I/O, RAM and throttling.
5. Pull images and warm sources off path. Warmers copy sealed objects and yield to eligible work.
   [Linux cgroup v2](https://docs.kernel.org/admin-guide/cgroup-v2.html)

## 8. The merge queue and GitHub integration

### Label-driven requests

Consume signed App webhooks for `pull_request.labeled`, `unlabeled`, `synchronize` and invalidation events.
Use delivery IDs and PR request generations. Reconcile trusted PR state, including current head, base, draft and label ID.

| Event | Work | Required check |
|---|---|---|
| Draft push | None. | Missing or pending. |
| Mark ready | None. | Still blocking. |
| Add `velnor:verify` to ready PR | Arm new epoch and verify the exact current head. Join any available proof. | Pending until full proof passes. |
| Synchronize while armed | Fence and cancel old request immediately. Start new head in a distinct slot. | New SHA blocks. Same-tree proof can bind it in seconds. |
| Synchronize while unarmed | None. | New head blocks. |
| Remove label | Fence epoch, cancel work and remove pending enqueue. | Revoke current head/group before managed removal. |
| Edit, rerequest, auto-merge, duplicate delivery | No new PR compute. Invalidate incompatible base/policy. | Preserve applicable proof or return to blocking. |
| Close or become draft | Revoke admission and withdraw label through the same fence. | No renewed success without a new authorized request. |

The labeled transition requires a ready PR. Readiness never arms a request.
No heavy `pull_request` workflow is dispatched without admission. Preflight is a separate explicit request with no check or label authority.
If Actions work is needed, App-dispatch a protected coordinator using a ticket bound to PR and SHA. Its job name differs from `Required`.
Subscribe to `merge_group.checks_requested` for the independent group request. A generated `on: merge_group` job can join this work for Actions effects.
Retire standalone PR/source and all-push image candidate triggers. Keep their affected obligations under the authorized plan.

GitHub documents that `GITHUB_TOKEN` label writes suppress new Actions runs. Its exceptions include dispatch and some approval-required PR events.
The docs do not settle delivery of these label writes to an App. Compare interactive, App-token and `GITHUB_TOKEN` writes with App delivery IDs and the label timeline in the lab.
Use the App credential for managed labels. For missing deliveries, reconcile PR head and label transitions from the API.
Start only for a proven label add or synchronize of an armed request. Poll armed PRs at up to 10 s when webhook health drops [E].
Keep ambiguous history pending. If an Actions trigger is missing, App `workflow_dispatch` with a validated ticket can run work. Its job check cannot satisfy the PR requirement.
[Webhook events](https://docs.github.com/en/webhooks/webhook-events-and-payloads#pull_request) · [Token behavior](https://docs.github.com/en/actions/how-tos/write-workflows/choose-when-workflows-run/trigger-a-workflow) · [Check eligibility](https://docs.github.com/en/pull-requests/how-tos/merge-and-close-pull-requests/troubleshooting-required-status-checks)

### Required check on each SHA

Use a stable App-owned `Required` commit status with `pending`, `failure`, or `success`. Additional App check runs carry detail under a different name.
Using commit statuses removes neutral/skipped conclusions. Only the proof issuer can request success. Verify the source and read back the latest state.
Use active `main` rules with no bypass, current review requirements, protected PR-only updates, no force push, required `Required` and Velnor `integration_id`.
Require GitHub's merge queue with `ALLGREEN`, `SQUASH`, `min_entries_to_merge: 1` and `strict_required_status_checks_policy: false`. The queue supplies base freshness.
Start with `max_entries_to_build: 2`, `max_entries_to_merge: 1` and `check_response_timeout_minutes: 90`. The min-group wait setting is inert with one required entry. Raise build concurrency and reduce the timeout to 20 min only after tail/fairness trials.
GitHub must require the latest PR head before entry and then the actual group SHA. Fail the lab gate if a skipped, wrong-source or stale status can pass.
GitHub can prefer checks on a PR's synthetic test merge commit. Only publish `Required` for exact role SHAs. Remove workflows that publish an ambiguous synthetic-merge status and test the effect of any remaining integration.
The observed HEADGREEN case landed PR #1276 beneath #1037 even though #1276's group failed. ALLGREEN requires every candidate prefix.
[Ruleset API](https://docs.github.com/en/rest/repos/rules) · [Merge queue](https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/configuring-pull-request-merges/managing-a-merge-queue) · [Latest SHA/precedence](https://docs.github.com/en/pull-requests/how-tos/merge-and-close-pull-requests/troubleshooting-required-status-checks) · [Decision S3 and feedback](../v2/design-feedback-comparison.md#correctness-and-confidence-holes)

### Close same-SHA and removal races

GitHub statuses are per repository, SHA and context. Two PRs with the same head can inherit the same green.
Use managed topic refs and exclusive head-SHA claims. Restrict contributor ref creation and updates to the ref-writer App, excluding `main` and GitHub queue refs from its authority.
Agents still use `git push` through a ref gateway that verifies a current old-ref and reserves the submitted SHA before forwarding the exact update to GitHub.
Include the release PR writer and retire unmanaged refs/credentials that can alias an eligible head.
Reject an already claimed SHA, or have the agent amend metadata to keep the tree and change the SHA. On reusing an earlier head of the same PR, make it pending before exposure.
GitHub documents a different SHA on a queue branch. Test topic/group/test-merge separation in the configured squash mode, including empty or already-applied changes.
Require enrolled contributor-signed topic commits if GitHub-generated commits can overlap. Exact identity separation is a cutover gate.

For removal after green, use the managed App operation. It fences the generation and publishes blocking status before removing the label via GitHub.
Dequeue any affected group first and confirm the state. Serialize status effects while a success write is in flight. Freeze ambiguous same-SHA effects until readback.
GitHub has no documented atomic CAS across label, status and queue entry. Unrestricted ref writers or direct label removal after green leave an asynchronous gap.
Enforce the managed path for strict mode. Keep required gate cutover blocked if exact-head resolution or the permission model fails the lab cases.
[Commit statuses](https://docs.github.com/en/rest/commits/statuses) · [Creation/update rules](https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/managing-rulesets/available-rules-for-rulesets)

### Queue operation and cancellation

Keep a separate merge-when-ready intent. Agents can arm it on opening a PR, without applying its label.
After rereading the head proof and reviews, the App calls `enqueuePullRequest(expectedHeadOid)`. GitHub owns landing. Skip routine catch-up merges of `main`.
For stacked PRs, reuse overlapping proofs and verify actual targets/order before enqueueing.
Remove the per-PR GitHub concurrency lock across generations. Cancellation sends local signals and the GitHub run cancellation asynchronously.
New work waits neither for cleanup nor for the old `always()` gate. Existing replacements wait 60 / 626 s on the sampled median/tail, with a 2,203 s maximum [M].
`repo/.github/workflows/ci.yml:57–59,614–625`. [Queue entry](https://docs.github.com/en/pull-requests/how-tos/merge-and-close-pull-requests/merging-a-pull-request-with-a-merge-queue) · [Expected head OID](https://docs.github.com/en/graphql/reference/pulls#enqueuepullrequestinput)

For each group event, load GitHub's exact SHA/tree and enumerate the full plan. Its check never substitutes for the PR's gate.
An equal tree can reuse the PR's full proof only where the plan and context keys match. Otherwise join valid individual PR/queue/main keys and run the misses.
A diff against the predecessor cannot define the group plan.
Predict current main plus the admitted PR and up to four ordered prefixes with `git merge-tree` [E]. Evaluate a first-ejection alternative only when replay and slack justify it.
Publish nothing for predictions. Bind to the real group. Discard mismatches and prioritize offered work.
Only 7/45 observed heads equaled a group tree [M]. V2's replay rates for integration trees do not transfer to exact heads.
Target ≥70% prediction match and ≤2 runner-min wasted per merge [E]. Report whole-group and individual-key hit rates.

### Main proof and release consumers

On main push, re-derive the plan at the exact landed SHA. Verify the expected group mapping, receipt signatures and payload digests.
Reuse matching keys across a changed commit ID. Missing landed proof triggers uncached verification, a release hold and an incident.
A separate minimal main-evidence workflow can support existing exact-push consumers during migration.
Today `velnor-ci release-gate` requires exact-SHA push and GitHub Actions provenance. Product release expects one push job named `Required`. An App status alone cannot satisfy that API.
Migrate the generator, helper, product/preview and publisher contracts together. If a bridging main witness is needed, a real Rust job validates the signed main proof before reporting.
Require exact landed SHA, allowed workflow/trigger/attempt, protected tag, freshness/ancestry, payload digest and signer before release privileges.
Main builds and qualifies optimized Linux bytes. Release reads and promotes them with verified evidence. Put any new publication claim above the build claim.
Pause Darwin and any pair-only proof requirement as an explicit Linux-only policy. Keep target in manifests for later macOS support.
`repo/ci-helper/crates/velnor-ci/src/gate.rs:15,44–115,127–159,277–288`,
`repo/.github/workflows/product-release.yml:78–86,118,170`, `repo/generator/crates/velnor-render/src/release.rs:148–251`.

## 9. What would make CI not a bottleneck at all

The absolute floor for a merge-ready, labeled push is the longest causal path through required proof and GitHub's merge queue.
A proof computed before the push can remove all build and test time from that path. It cannot remove GitHub's observation of the new SHA, admission into its queue or final ref update.
For a wholly novel input, the remaining verification time is bounded below by the longest dependency path, total uncached work divided by feasible capacity, and unavoidable input transfer. The largest applies even with infinite scheduler efficiency.

For an already verified head and anticipated group, the path is as follows.

1. GitHub accepts the push and Velnor learns the exact head. Bind the preflight's tree and complete inputs to that SHA and publish the head status.
2. GitHub observes the head check and the App requests enqueue for that SHA. GitHub computes and offers the actual merge-group commit.
3. Join the group plan to the sealed receipts, even for a perfect prediction. Publish only on the offered SHA.
4. GitHub lands it. The main verifier records the landed mapping and emits the release-safe pointer off the merge clock.

A center calculation using measured historic queue terms is `4 + 0.2 + 2 + 2 + 19 + 3 + 26 = 56.2 s` [E on M].
The estimated terms are push observation, proof binding, head publication, enqueue and group proof/publication. The two measured medians are 19 s from enqueue to the group run and 26 s from group finish to merge.
Those medians are a reference for the service, without an SLA or a physical lower bound. The design removes redundant waiting around them. A protocol-admissible request can obtain a local result in about 0.2 s [E] and show its GitHub check in seconds.
A queue candidate with genuinely changed inputs pays the missing compile/test path. A novel small warm edit's stretch proof adds about `8 + 16 = 24 s` [E] before capacity savings or overlap.
The 75 s stretch median therefore needs substantial receipt coverage, preflight, or a measured improvement over the old platform spans. State those conditions when reporting the target.
On a cold or unknown root, run what the policy requires, even if that takes minutes. Under today's slow-test implementation a new 125–138 s case can dominate alone.
The irreducible terms are proof of changed inputs, exact-SHA and queue validation, platform propagation and GitHub's protected main update. Label and review time stay in the reported end-to-end push clock. [Measured queue components](../draft/proposals/opus-5.5.md#11-push-to-merge)

## 10. Critique of v2

Keep N57–N64 where they express the owner's Rust, protocol, oracle and archive choices. Keep the coordinator, required group plan and XFS.
The changes below follow v2's section order. [Architecture](../v2/velnor-runner-architecture.md) · [Decision log](../v2/decisions.md) · [Review](../v2/design-feedback-comparison.md)

| V2 section | Gap and decision |
|---|---|
| §1 Goal/confidence | The old clock starts at CI creation. Count accepted push through land. Replace HEADGREEN, unconditional triple runs and full landed reruns with ALLGREEN, complete proofs, preserved stochastic coverage and cold audits. |
| §2 Where time goes | Price standalone qualification/image workflows, release overlap and batch work alongside hosted waits and cancellation. |
| §3 Architecture | §3.4 starts on push, tests a synthetic PR merge and assumes placement control. Start on label, prove exact head, and route using actual ready capacity. No production official executor or hosted forks/Macs. |
| §3.5–3.6 Actions/oracle | Add the generated App-token adapter and versioned effect contracts. Check live drift every 15 min and diagnose within 2 h. Use the current oracle. |
| §4 Ledger | Executable digest appears too late. Key actual binaries/data/env from first admission and verify promotions separately. Share sealed evidence across PRs. |
| §5 Cache | N54 makes mbx-cache automatic and builds per site. S12 only supports some cold cases. Prefer local plain Cargo and distribute sealed objects. N55's mtime rule alone cannot authorize a hit. |
| §6 Workflow | Replace the early push trigger, SHA aliasing and Actions-only release assumptions together. Main records proof and builds unique release bytes. |
| §7 Kubernetes | Model peak RAM/page cache and avoid assuming two sites and one control plane are HA. Fence writes during ambiguous ownership. |
| §8 Hosts | XFS and honest limits come first. Require paired data for governor, NUMA, linker or CPU policy. |
| §9 Moonshots | v2's tree-window replay used integration trees, and its fastest merge path replaces GitHub's queue. Plan preflight and exact proof while keeping GitHub in charge. |
| §10–11 Phases | Remove production official-runner phases. Pause Darwin and prove label/queue behavior, native parity and exact release promotion first. |
| §12 Risks | Cover SHA alias, label removal, wrong asset and stale provenance with live fixtures and revocation. |
| §13 Owner questions | Resolve HEADGREEN, fork/Mac exceptions and a separate merge train within the binding decisions. No optional pass-on-retry. |
| §14 Hardware | Fix test setup and transfer costs before considering v2's estimated 1.3–1.5× serial gain or more nodes. |

Overturn N54 as default, N55 as sufficient cache safety, unconditional triple execution and the §13 rollout exceptions.
Keep N50's coordinator and N52's single writer. Move the writer to the App and migrate release evidence together.
Use archived expression fixtures to guide the fresh evaluator, replacing the copied runtime part of N60.

## 11. Phases

Clock targets apply to the controlled cohort in §2. Publish all-PR, affected-root and site-loss distributions alongside them.
Compute cache hit rate as valid compiler-unit hits divided by eligible unit requests in actual builds, including misses, and report the denominator by workspace.
Report receipt hits, zero-execution requests, provenance/policy misses, object bytes and compile cache in separate series. Release and batch allocations use §2b.
Use the widest qualified execution while a narrower reuse proof is missing. Cutover requires 100% of mandatory conformance cases and correct live protection behavior.

| Phase and order | PR verdict P50 / P90 [E] | Push-to-merge P50 / P90 [E] | Runner-min/merge [E] | Compile cache hit target [E] | Conformance target |
|---|---:|---:|---:|---:|---|
| 0. Remove Darwin shipping paths from the plan, capture baselines and qualify GitHub and native lab scenarios. | ≤12 / 20 min in replay | ≤25 / 60 min in replay | ≤180 in controlled replay | 0% while cold | 100% of first 30 essential fixtures. Lab only. |
| 1. Cut over the Linux jobs to conformed Rust and the strict head/group gate. Reuse only lab-qualified exact proofs. | ≤8 / 16 min | ≤18 / 45 min | ≤130 | 0% compile-output reuse | 100% of mandatory cells and 200 real-tree comparisons with zero unexplained differences. |
| 2. Land test-floor changes, fixed-path XFS, exact/root proof and the Linux main payload promoter. | ≤4 / 8 min | ≤10 / 24 min | ≤60 | ≥70% in qualified build lineages | 100%, plus full cold shadow of each new reuse cohort. |
| 3. Admit per-package and executable proof, preflight, fair cross-node tests and measured queue prediction. | ≤90 / 250 s | ≤3 / 8 min | ≤32 | ≥90% | 100% and zero confirmed false greens. Cold audit and wire probes meet their windows. |
| 4. Qualify the stretch pipeline, smaller binary deltas, faster bottleneck tests and two-site recovery. | ≤35 / 110 s | ≤75 s / 4 min | ≤14 | ≥95% | 100%, including newly admitted runtime/cache cases and tested recovery. |

Phase 0 targets compare instrumented replay positions. They confer no production authority.
Phase 1 requires native implementations of admitted production jobs. Keep the official runner in the lab. Hold Linux publication until the main payload promotion path qualifies.
For phase 2 require a current-schema journal open ≤1 s, no remaining single-case wall above 60 s on qualified loads, and Linux-only release dry runs whose published candidate bytes equal the main manifests.
For phase 3 require p90 local admission ≤2 s, representative saturation and both-site parity, 300 qualifying merged PRs or four weeks of telemetry, and a flake/ejection report.
Require a measurable gain before bumping queue concurrency or enabling a speculative lane. Phase 4 adds ≤15 s maximum isolated-case target and platform-latency evidence for the stretch path.
Measure p50/p90 confidence intervals and charge all red, cancelled, superseded, cold, fixture, batch release and oracle work. Hold capability and policy changes if the needed invariant is unproved.

### First five work items

1. **Pause all macOS release paths in phase 0.** Remove the product binary call and its prepare edge, Darwin generator/SDK preview branches, Homebrew adoption and paired-Darwin publication requirements, and Darwin tools matrix entries. Regenerate workflow and environment requirements. Keep Linux gates and record a Linux-only manifest.
2. **Instrument the baseline spans and costs.** Join accepted push, PR timeline, labeled request, broker/worker lease, steps, checks, merge-group SHA and landed ref. Measure selected source qualification, image candidates, GitHub cache bytes, release payload and current cancellation. Preserve provenance across sites.
3. **Prove the required-head/queue rules in the lab.** Record App webhook and timeline fallback for each token type. Replay same SHA across PRs, old-green rebase, synthetic test-merge precedence, removal during green and failed queue prefix. Qualify managed topic refs and the App status source before writing production rules.
4. **Boot one fully native Linux conformance lane.** Build the scale-set/JIT/broker path, acquired job model, Results stream and sealed receipts with reuse off. Add lease loss, orphan cleanup and latest official runner pairing. Cover every admitted action and Linux release effect before cutover.
5. **Remove the longest build/test setup.** Profile the remaining long cases on a worker. Qualify the closed journal template, split loop cases, move repeated fixture builds behind digests, and check fixed-path warm/cold behavior with a lockfile and same-mtime edit. Move source qualification onto the same plan.

Sequence the rollout around independently verifiable boundaries. First prove the gate with real GitHub behavior, then the executor with the oracle, and then each reuse class against cold execution.
The first-principles redesign changes the unit of CI from a workflow run to a verified obligation. Separating shared state gives each job a private writable target and lets it share proven data safely.

## 12. Risks

| Risk | Detect | Contain |
|---|---|---|
| Protocol or GitHub assignment drift | Fifteen-minute probes by site, 30-min oracle release polling, queue/cancel/renew error rates and changes in the redacted wire shape. Recheck runner deprecation dates. | Stop the affected generation. Route only to an accepted, previously qualified Rust generation. Keep checks blocking if none works. |
| False green from an incomplete key | Cold audits by lineage, changed dependency/executable/fixture mutations and signed plan inventory checks. Watch red-main, unexplained test failures and reverts after a green. | Revoke the key schema and derived receipts, stop publishing, run uncached full proof, hold affected releases and treat already landed changes as incidents. |
| Candidate weakens its own tests or report | Compare trusted expected obligations, approved planned args, test inventory, raw process exits and test/feature policy. Differential cases forge passing output and empty discovery. | Controller rejects candidate-provided authority. Require separate trusted plan approval for baseline reductions and rerun the conservative plan. |
| PR output contaminates trusted main state | Monitor signer identities, verified promotion events, per-scope ACL tests and append-only object digests. Test a gift of stale bytes and an index entry without provenance. | Freeze promotion and affected artifact set. Remove visibility and restart from clean lineage and trusted source. |
| Old status or label state enters the queue | Lab regressions for shared SHA heads, reused heads, synthetic merge ref, status-source drift, draft and remove/finalize races. Track unmatched delivery sequence and queue/head mappings. | Gate deployment on managed topics and serialized removal. Revoke known checks and groups, hold admission on unknown effect, and re-fetch authoritative state. |
| A red group ancestor lands | Exercise the historic red-prefix/green-tip case and independently compare required group receipts with actual landed trees. | ALLGREEN with required App source. On mismatch stop queue publication and releases, and verify or revert exact affected landed changes through protected policy. |
| Release staleness, unauthorized signer or asset drift | Check the actual tag object, ancestor/freshness rule, selected attempt, source/workflow signer and SHA256SUMS. Re-read asset digests and verify provenance after upload. | Keep draft unpublished, revoke bad manifests, freeze the release key, reconcile idempotently from verified main payload. |
| Cancellation leaks a worker or stalls replacements | Track p50/p90 replacement admission, job lease expiry, old cgroup lifetime, and pending outbox operations by epoch. Inject crash after remote success. | Fence and reap the old worker independently. Give the new head another ready slot. Preserve prior terminal evidence only for exact usable obligations. |
| Flake budget hides a real regression | Count both live disagreement directions by load, compare repeated seeds, and preserve the first red attempt. Keep statistical bounds and separate infrastructure failures. | Fix the test or infrastructure before widening the queue. Preserve blocking equivalent coverage. No automatic pass-on-retry for code failures. |
| Build/test placement overcommits RAM or starves other orgs | Track per-org tail, cgroup memory, IRQ/scheduler delay, PSI, duplicate page cache and CPU quota throttling. Replay the busiest hour with a node missing. | Limit admitted bytes and CPU, reclaim background borrowing, keep peer reservations, place heavy links in dedicated slots. |
| Site, ledger or object replica loss | Drill node drain, network partition, lost lease, corrupted blob and a failed control-plane site. Check expected fencing and durability rather than pod health alone. | Healthy site executes qualified work. Publication waits for durable ownership. Restore missing objects by digest or execute the missing obligations. |
| Cross-node cache or speculation spends more than it saves | Compare warm/cold and candidate-hit cohorts by bytes and runner/core-minutes. Verify foreground latency at tail under paired traffic. | Roll back transfer strategy, shrink speculation and use warm placement. Reopen a strategy only after it beats the current one on the same traffic. |
| Unknown action or schema expansion | CI generator contract and registry check, live admission metrics, versioned unsupported-action fixtures. | Refuse the job before steps start. Add a native adapter and the complete oracle cases before widening the registry. |
| Unattainable latency claim | Track all push times, label/review holds, platform delays, cold and source-policy changes. Compute intervals from complete job/PR coverage. | Report actual tails and revise the forecast. Preserve evidence and blocking gates when a performance target fails. |

This is a design based on read-only source inspection, earlier measurements and the cited public job data.
The latency and cache allocations are targets. The exact-label and SHA-alias controls, admission limits, package-closure completeness, and new runner parity all require the lab gates above before production.
