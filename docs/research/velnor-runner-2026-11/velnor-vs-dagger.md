# Velnor and Dagger: what to learn, what to keep

Written 2026-11-19 to improve Velnor's design. This is not a benchmark. Sources: the
[Dagger dossier](dagger.md) (cited as "dossier §n"), checked against the Dagger source at commit
`fcce0f8` (`internal-docs/egraph.md`, `internal-docs/dynamicinputs.md`, `core/modfunc.go`,
`core/schema/module.go`, `engine/remotecache/`, `CHANGELOG.md`); the
[architecture](../../proposed/velnor-runner-architecture.md) (cited as "§n"); the
[user-experience draft](../../proposed/velnor-user-experience.md) ("UX §n"); the
[owner decisions](owner-decisions.md) ("decision n"); and the agent-native brief and the panel's
answers to it ("N01" and similar). These bind: decisions 1–24, correctness first with zero false
greens, and GitHub as the platform. An idea that conflicts with a decision becomes an owner
question.

## Verdict

- **Dagger** is a programmable build engine: typed functions in eight SDK languages, run by a Go
  engine as a lazy, content-addressed DAG of containers, every call cached by its recipe. It sells
  the same checks on laptop, CI and cloud, with no trust split between them.
- **Velnor** is a GitHub-Actions-compatible runner and merge gate. It runs the existing YAML jobs
  natively in Rust and turns PR and merge-group verification into signed, witnessed records.
- **Overlap:** both skip work by content-addressed keys, keep a warm engine per node, run checks
  with no workflow file, and want agents to verify before pushing. They split on trust: Dagger
  reuses whatever its engine computed; Velnor reuses only what its node agent observed and two
  witnesses confirmed, then audits it. **For Velnor:** reuse no Dagger code; borrow D01–D11, above
  all structured keys, reuse keyed on test-binary digests, and named checks for agents.

## Side by side

| Topic | Dagger | Velnor | What it means for Velnor |
|---|---|---|---|
| Unit of work and its key | A DagQL call: a module function, or a core operation such as `withExec`. Its key is a structured `ResultCall`: field, receiver, module provenance, explicit arguments, implicit inputs, view and extra digests (mainly a content digest). An e-graph unions recipes that produce equal outputs. There are two cache levels: function results and the operations inside them (dossier §3.1; `egraph.md`). | An obligation: format, lint, docs, a per-crate check, one test binary under one configuration, or release bytes. Its key is the definition digest, the whole Git tree (narrowed only by an enrolled contract), the execution fingerprint and the executor identity. A record has two witness slots (§6.1, §6.2). | Velnor's key is coarser and stricter. Dagger shows two refinements: keys you can inspect (D01), and identity by output bytes (D02, D03). |
| Cache scope and trust | One cache per engine, shared by every session that connects to it. Results that depend on a secret are only reused by sessions that loaded that secret. Dagger Cloud shares a cache per organisation; per-branch or per-PR scoping is not public, and engines export to Cloud themselves (dossier §2, §3.4; `engine/remotecache/`). | Receipts are minted only from processes the node agent started and observed, and are signed outside the sandbox. PR records stay in a `candidate` scope; they become `trusted` once the tree is on `main` and audit A1 passes. `actions/cache` follows GitHub's branch scoping on every tier (§1.2 C4, §6.2, §7.2). | Velnor is stronger here (next sections). Do not copy "clients populate the shared cache". |
| Local and CI parity | The same engine version and module code run everywhere, and the CLI must match the engine's version. Architecture follows the engine, so an Apple Silicon engine runs arm64. Engine configuration is per engine and is not in the key (dossier §4). | The kernel, architecture, CPU features, CPU count and sandbox profile are in every key. A Mac result never counts. `velnor verify` runs in the cluster's sealed Linux sandbox (UX §1.2, §5). | Velnor's parity is honest: it says when local is not CI. Dagger's selectable engine target is worth borrowing (D05). |
| Remote engines vs warm nodes | `--engine` URLs: `image://`, `container://`, `tcp://` (no authentication), `tls://`, `ssh://`, `kube-pod://`, `unix://`, `cloud`. On Kubernetes, one engine per node runs as a DaemonSet, with its cache on the node's disk; replacing the node loses the cache. With self-hosting, engines share no cache unless you buy Cloud (dossier §2, §5). | A node agent DaemonSet keeps warm single-use sandboxes, sealed generations, mirrors and the L0 cache. Generations and chunks travel through RustFS (L1), with replication between sites (L2). A new node seeds from the site store (§5.2, §7). | Velnor already has the DaemonSet. Dagger's own advice ("a new engine can make checks faster or slower"; "do not run engines on ephemeral machines") confirms that placement must weigh warmth. |
| GitHub integration | Two routes. Either a job on GitHub Actions runs `dagger check` against an engine, or Cloud Checks runs on every push and PR through the Dagger GitHub App, with no workflow file (dossier §5). Dagger does not handle the merge queue, required checks or rulesets itself. | Velnor executes Actions jobs through the scale-set protocol. A dedicated App writes `Velnor / Verified`, the only required context. Velnor implements the label flow, `merge_group` handling, ALLGREEN, a fenced writer, canary PRs and an empty `bypass_actors` (§8.2, §8.3). | Velnor owns the gate; Dagger only reports. Cloud Checks' trigger on every push conflicts with decision 12. |
| Agent support | `LLM`, `Env` and `Agent` are types inside the engine. Modules can be served as MCP tools. container-use gives each agent a container plus a Git worktree branch, with a command log (dossier §7). | `velnor verify`, `velnor explain`, ledger records reused by the PR, and an agent identity that cannot edit the ruleset (UX §5, §9). The brief's ideas N01–N14 add shims, trust tiers, a ledger query and agent shells. | Borrow the environment-per-task shape (D09) and named checks (D07). Keep LLM loops out of the runner. |
| Correctness guarantees | Results are keyed by their recipe inputs. Network, clock and image-tag inputs are the user's responsibility: TTLs, `@cache(policy: Never)` and opt-in lockfiles are the tools. The DagQL cache is "fundamentally in-memory", checkpointed to SQLite, and may be lost or wiped. A check's verdict is its exit code. Nothing re-executes reused results (dossier §3.3). | C1–C7: whole-tree keys by default; C5 for clock, network and event reads; two independent witnesses; an inventory of expected test IDs; shadow and 2% sampled re-execution; cold audit A1 of every landed commit; revocation by epoch; a durable ledger written by compare-and-swap (§1.2, §6.4, §6.5). | Velnor's contract is far stricter. Borrow Dagger's ways of making a policy explicit (D04, D10), not its defaults. |
| Observability | An OpenTelemetry trace for every operation, a TUI, `dagger trace` replay, cache-span counts, and per-test views with reruns in Cloud (dossier §6). | The `Velnor / Details` check run, `velnor explain`, `velnor logs`, a daily report, and proposed Prometheus metrics (UX §5, §6). No traces yet. | Adopt OTel traces per request (D11): §1.1 and §11 already need every stage timed. |
| Cost and operations | The engine is Apache-2.0 and needs privilege. Its garbage collector defaults to 75% of the disk. Cloud costs $20 or $200 a month plus usage, and repeated work costs a fifth. The shared-cache server is closed source. The CLI and engine versions must match (dossier §3.4, §9). | Everything is self-hosted on the owner's cluster: control, dispatch, node agent, signer, ledger, cache index, RustFS and mirrors. About 1,008 agent-hours to build. Conformance is re-run within 14 days of each official runner release (§5.7, §10). | Velnor runs more services but depends on no SaaS. Dagger's shared cache cannot be self-hosted without Cloud. |

## What Dagger already solves that Velnor plans to build

Decision 1 makes the executor Rust. Decision 20 keeps the official runner as the only alternative
executor. Decision 11 makes a result count only when its key provably covers every input. Decision
17 allows reuse only after review. Against these, no Dagger code is a fit. Several of its
mechanisms are good references.

| Velnor plans | Dagger's counterpart | Verdict | Why |
|---|---|---|---|
| The node agent, platform backends and warm sandboxes (§5.4) | The Go engine: containerd snapshotters, CNI pooling, an OCI runtime | **Do not reuse.** | Decision 21 would allow a "Dagger platform backend", but it gains nothing. Velnor could not trust the engine's cache (C4), so it would only run containers, which the Kubernetes and Docker backends already do. It would add a privileged engine and a second runtime to conform for each backend. Its cache layer replaced BuildKit only in v0.21.0 (May 2026), and fixes for cache persistence followed in v0.21.x. |
| The ledger, keys, `ensure` and in-flight joins (§6) | The DagQL cache: structured call identity, the e-graph, in-flight deduplication | **Reference** (D01–D03). | The identity model is well documented. Velnor's records need durability, witnesses and epochs, which the DagQL cache lacks by design. |
| The obligation inventory and planner (§8.1, C3) | Modules and Checks discovered as typed collections at run time | **Reference** (D07, D08). | The inventory must come from the released helper and the base commit, never from code under test (C3). Writing checks in eight SDK languages would also replace the YAML format that decision 7 keeps. |
| The `velnor` client | The Dagger Rust SDK (`sdk/rust`) | **Do not reuse.** | The SDK is a GraphQL client of the Go engine. Velnor has no engine for it to call. |
| Storage tiers and chunked `actions/cache` (§7) | The Cloud remote-cache protocol: offers, exports, 16 concurrent uploads | **Reference.** | Only the engine side is open; the service is closed. Velnor's single-writer index plus content-addressed blobs is already the right shape. |
| Agent shells and `velnor verify` (N03, UX §5) | container-use and filesync of the host directory | **Reference** (D06, D09). | Keep the shape: one environment per task, on its own worktree. Velnor's thin pack against the node mirror sends less than syncing the host directory, and the 18 s versus 4 s report blames that sync. |
| Per-stage timing (§1.1, §11) | OTel spans for every operation | **Reuse the standard** (D11), not Dagger's code. | Rust has OpenTelemetry crates; the traces go to a self-hosted collector. |

## Where Velnor is stronger

1. **Witnessed receipts instead of a cache that clients write to.** In Dagger, any session that
   reaches an engine creates reusable results, and every engine exports to the organisation's
   Cloud cache. Velnor mints a receipt only from a process its node agent started, in a sandbox it
   created, on inputs it mounted. A signer outside the sandbox signs it, and a per-site
   append-only log records it (§6.5). A Mac, an agent or a PR cannot place a result where `main`
   trusts it (decision 8): PR records stay `candidate` until A1 passes on `main`.
2. **A per-commit gate.** Dagger's checks report results; GitHub's rules decide what they block,
   and a `neutral` or skipped check passes. Velnor makes one App status the only required context.
   Only `success` passes. Every landed commit, not just the group's tip, is covered (ALLGREEN, C1).
   Permanent canary PRs probe every 10 minutes that an unverified head stays blocked (§5.7, §8.2).
3. **Inputs that are not hermetic are handled by the system, not by users.** Dagger keys a
   `withExec` by its inputs, whatever it reads from the network or the clock, and it offers TTLs
   and cache busters. Velnor classes clock, randomness, network and event reads as C5, which runs
   on every request. Tests get loopback-only networking, no `.git` and a scrubbed environment, and
   an injectable clock is fixed in the key (§6.1).
4. **Reuse is audited.** Dagger never re-executes a cache hit. Velnor re-executes every reuse in
   shadow for about 3,000 comparisons per class, then samples 2%. It rebuilds every landed commit
   cold (A1) and revokes a class or epoch on the first disagreement (C6, §6.5).
5. **Evidence is durable.** Dagger's cache is in memory, checkpointed on a graceful shutdown and
   wiped when suspect, which is fine for a cache. Velnor's ledger persists the intent first and
   writes results by compare-and-swap. "Unknown" is never treated as "absent" (§5.9).
6. **Tests are counted.** A Dagger check passes on exit code 0. A Velnor obligation fails on a
   missing, duplicate, extra or zero-test result, and it fails when the head runs fewer tests than
   `main`'s plan without a signed reduction (C3).
7. **The workflows stay unchanged.** Dagger asks you to rewrite pipelines as modules. Velnor runs
   the existing YAML with GitHub's exact `actions/cache` semantics. Conformance against the official
   runner proves it (§5.7, decision 22).

## Ideas to borrow

Every idea fits decisions 1–24 as written. The two Dagger ideas that would conflict with a decision
are under "Ideas to reject", tied to the owner questions they would reopen.

| Id | Idea | Mechanism in Velnor | Gain | Correctness argument | Fit with owner decisions |
|---|---|---|---|---|---|
| D01 | **Call-identity keys**: a structured key, not one opaque digest | Each record stores its key as a typed, canonical struct. The fields are the definition (command, profile, nextest profile, features, helper and toolchain digests), the inputs (tree, or the contract's file digests), the execution fingerprint, the executor identity and the reuse class. The digest is computed over the canonical encoding. `velnor explain` and the ledger query show the difference field by field ("miss: toolchain 1.99.1 → 1.99.2"), as Dagger's `ResultCall` and implicit inputs do. | Every miss can be explained, by humans and agents (N07). It speeds up enrolment reviews (§6.1) and the diagnosis of key bugs found by A1. | The identity is unchanged; this only changes its representation. Adding a field raises the key version, so old records stop matching and the result is a miss, never a hit. | Fits decisions 11 and 17. About 6 agent-hours in `velnor-ledger`. |
| D02 | **Content-preferred identity as telemetry**: measure the reuse that equal outputs would allow, before enabling any of it | For every witness, also compute a "content key": the digest of each executed test binary and every `CARGO_BIN_EXE_*` it spawned (already bound in the receipt by C4), plus the runtime inputs. Report every execution whose content key equals an earlier passing record's while its source key differs. Dagger does exactly this, and its own doc calls it "an identity hint, not a cache-hit decision". | It counts D03's gain on real heads before D03 exists. It also flags keys that are wider than they need to be. | It is telemetry only: no code path reads it in the gate. | Fits. It is D03's shadow phase (C6). About 4 agent-hours. |
| D03 | **Digest equivalence for test execution**: a narrow, byte-only version of Dagger's e-graph | Split a test obligation into a build step and a run step. The run step is keyed by the test binary's digest, every `CARGO_BIN_EXE_*` digest, the enrolled runtime contract's file digests, the definition and the fingerprint. When the build of a new head gives byte-identical binaries to a record that passed (a change to `.github/` or a docs-only path that no `build.rs` reads), the record is reused and no test runs. Equivalence comes only from byte hashes Velnor computed itself, never from "semantic" content labels: in Dagger, the content digest of a host socket is its handle. | Tests are the bulk of a verdict (runner-host 192 / 417 s P50 / P90 [M]). About 20% of heads touch whole-tree paths (42 of 214 [M]). Today all their tests run; with D03, tests whose binaries are unchanged are reused. The share is unmeasured, and D02 measures it. | (1) Everything compiled into the binary, `include_bytes!` data included, is in its bytes, so the compiled part of the key cannot miss a source input. Everything read at run time stays in the enrolled contract. (2) The danger is a stale warm build that reproduces the old binary and so hits the old record (the Q16 hazard). Therefore the digest used for a lookup must come from two builds with equal digests, one of them cold until compile reuse graduates (§7.3). (3) A1, 2% sampling and the planted cases of S1 detect a failure. (4) The fallback disables the class and returns to the per-root or whole-tree key. A nondeterministic build only causes a miss. | Fits decisions 10 and 11. It needs the commit stamped after link (lab R06), because `VELNOR_COMMIT` makes every binary unique; until then it gains nothing. About 16 agent-hours. |
| D04 | **Function cache policies**, mapped onto Velnor's clock and network rules | Each obligation in the released inventory declares `reuse = content \| per-request \| never`. Dagger's `Default` maps to `content`. Its `PerSession` maps to Velnor's C5, which runs fresh on every verification request. Its `Never` maps to `never` (every witness runs, as for publishing). The planner takes the stricter of the declared class and the classifier's finding (clock, network or event reads). Dagger's TTL is not adopted for evidence. TTLs are allowed only for advice: ledger hints to agents (N07) and prefetch. As in Dagger, where `Never` still lets the operations inside hit the cache, a C5 obligation still compiles over a verified generation; only its execution is fresh. | The C5 rules become reviewable data instead of planner logic. Agents see why something ran fresh. | A declaration can only make reuse stricter, never looser. The class is part of the key (D01), so changing it re-verifies. The fallback is the classifier's result. | Fits decision 11 and §6.1 ("nothing is reused on a time-to-live"). About 5 agent-hours. |
| D05 | **Engine selection by URL** | One client takes `--target` or `VELNOR_TARGET`: `local` (the Docker backend on this machine), `k8s://de`, `k8s://fi`, `docker://host`, and later `microvm://` or `apple://`. `velnor verify`, `velnor run` and `velnor check` (D07) use it. The default is the site, so agents work "remote by default" (N02). Every target authenticates with the agent identity over mTLS; there is no equivalent of Dagger's unauthenticated `tcp://`. | One command for quick local feedback and for minting on the cluster. Fewer flags to learn, and one place to choose arm64 or amd64 (N05). | The target only chooses where work runs. The result's trust tier comes from the signer class of the node that executed it (C4), never from the URL, so `local` cannot mint whatever the URL says. | Fits decisions 13 and 21. About 4 agent-hours on top of the CLI. |
| D06 | **One engine per node, reachable over a node-local socket** | Velnor's node agent is already a DaemonSet. Borrow Dagger's `hostPath` socket: agent shells running on a cluster node (N03) reach their own node agent through a request-only socket. `velnor verify` there starts a fresh minting sandbox on the same node, over the generation already there. The node agent reads the committed tree's objects and hashes them itself. Also adopt Dagger's lesson as a placement rule: count warmth explicitly and never scale out to cold nodes for verdict work. | No upload, and the generation is already local. A verdict from a cluster shell costs only compile and tests (the goal is ≤ 25 s compile and ≤ 60 s tests [T]). | The socket accepts requests, not results. The receipt comes from a fresh sandbox, never from the shell, and the shell's sticky state never mints (N03). | Fits. It depends on N03's experimental shells; their capacity priority is N09's owner question. About 6 agent-hours. |
| D07 | **Checks that need no workflow file, run by name** | After the cutover, PRs and groups already have no Actions run (§8.1). Borrow `dagger check -l` and `dagger check <name>`: `velnor check -l` lists the obligations of the released inventory by name (`clippy:generator`, `test:velnor-ci`). `velnor check <name> [--target …]` runs exactly that obligation's released command graph. Agents and the `hk` hooks (N14) call names, not raw `cargo` arguments. | It removes the shim's riskiest step: mapping argv to an obligation (N02's risk row). It also gives agents and CI one shared vocabulary. | The command graph comes from the inventory at the base commit (C3), never from the agent. A name the inventory lacks is refused. | Fits decisions 7 and 12: the trigger stays the label flow. About 8 agent-hours. |
| D08 | **Every generator gets a staleness check automatically** | Dagger v0.21 adds a `check` for each `generate` function. In Velnor, the inventory registers each generator (velnor-gen's workflows and goldens, schema dumps), derives a freshness obligation for each (regenerate in the sandbox; any diff fails), and refuses a generator registered without one. | It closes a class of silent drift (a stale generated workflow or golden). It replaces hand-maintained freshness jobs (cohort 1). | The check only adds work: a missing check is an inventory error that blocks, never a pass. | Fits decision 11. About 4 agent-hours. |
| D09 | **container-use-style agent environments with worktrees** | `velnor env new <task>` creates a Git worktree from the node mirror (on a Mac, from the developer's clone) and a sticky sandbox on the chosen target (D05). It keeps a command log; `velnor env push` publishes the branch. Each environment has its own target directory, temporary directories and ports, so parallel agents do not collide. On the cluster the environment is amd64, like CI. `velnor mcp` exposes `env`, `check`, `verify` and `explain` as MCP tools, beside the `--json` CLI. | Fewer cross-agent collisions and local flakes caused by shared ports and target directories (§8.5's port fixes), and dev feedback on the CI architecture. The gain is unmeasured. | Environments never mint: they are the brief's non-minting tier (T1c, N04). Minting always happens in a fresh sandbox over the committed tree. | Fits for Docker. Cluster environments compete for capacity, which is N09's owner question. About 16 agent-hours. |
| D10 | **Lockfile modes** (`live`, `pinned`, `frozen`) | Obligations and receipt-minting jobs run `frozen`: every input resolved over the network (container images, toolchains, mise tools, git dependencies, action SHAs) resolves only from a committed lock (`Cargo.lock`, `mise.lock`, `.velnor/*.lock`), through the digest-addressed mirrors (decision 24). An unlocked input makes the obligation fail with the input named, or makes it C5. Never a hit. `velnor run --target local` may use `pinned`. `live` exists only in `velnor lock update`, which opens a PR. | Obligations whose only "network" is a digest-pinned mirror leave C5, so they are reused on exact-tree rebases and identical groups. The C5 share is unmeasured (census S5). | A lock is part of the tree, so it is in the key. Resolution that fails closed cannot fetch a moving tag. The mirror verifies every digest before use (§7.4). | Fits decisions 11 and 24. About 8 agent-hours. |
| D11 | **OTel traces per request** | Each `ensure` is one trace. Its spans are the plan, record resolution (hit, join or run, with D01's reason), provision, checkout, compile, each test binary, signing and the status write. They go over OTLP to a self-hosted collector. `velnor trace <sha>` renders them in the terminal, and `Velnor / Details` links to them. Per-test outcomes and durations come from the expected-test-ID inventory. | Section 1.1's stage clocks and section 11's "every stage timed" from one source, so P90 outliers can be attributed. | Read-only: nothing in the gate reads traces. | Fits. About 10 agent-hours. |

### Where each idea lands in the design

| Id | Section it changes | Change |
|---|---|---|
| D01 | §6.1 Keys; UX §5 `velnor explain` | The key is defined as a canonical struct with named fields. `explain` prints the field that differs. |
| D02 | §6.4, §11 (S5 census) | Add a "content-key would-hit" counter to the shadow ledger and the daily report. |
| D03 | §6.1, §6.4 (a new soundness row), §7.3 | A test obligation becomes a build step plus a run step. The run key uses binary digests from twin builds. It depends on lab R06 (stamping the commit after link). |
| D04 | §6.1 "Time and randomness"; §8.1 inventory | A `reuse` field per obligation. The planner takes the stricter of the field and the classifier's result. |
| D05 | UX §5 CLI; §5.4 | `--target`, authenticated per site and backend. No backend is named in the core: a URL scheme maps to a declared capability set. |
| D06 | §5.2 node agent; §9 placement | A request-only socket for cluster agent shells. Placement ranks warmth explicitly. |
| D07 | §8.1 inventory; UX §5, §9 | `velnor check -l` and `velnor check <name>`. The `hk` steps call names. |
| D08 | §8.1 inventory; cohort 1 (§10.1) | Generators are registered and get derived freshness obligations. The hand-written freshness jobs are deleted (decision 18). |
| D09 | UX §1.2, §9; N03 | `velnor env` on the Docker backend first; cluster environments wait on N09. |
| D10 | §7.4 mirrors; §6.1 C5 | Obligations resolve inputs `frozen`. `velnor lock update` is the only route to `live`. |
| D11 | §1.1, §11; UX §6 | OTLP export from control, dispatch and the node agent. `velnor trace`. |

### Sequencing

1. **D01, D04 and D07 first.** They are cheap, they change no reuse decision, and D03 and D10 need
   their data shapes. D07 also removes the riskiest step of the brief's shim idea (N02) before
   agents start relying on it.
2. **D02 and D11 together.** Both are telemetry. D02 needs D01's fields and D11's spans to explain
   its counts. They run in shadow from the day obligations verify PRs (§10.2, day 5.2).
3. **D08 and D10 with the inventory work.** D08 deletes hand-written freshness jobs. D10 moves
   pinned-mirror obligations out of C5, after census S5 measures the C5 share.
4. **D03 last.** It needs lab R06 (the commit stamped after link), twin builds with equal digests,
   and about 3,000 clean D02 comparisons per obligation kind (C6). If R06 fails, D03 is dropped,
   and D02's counts show what that costs.
5. **D05, D06 and D09 follow the agent-native work** (N02, N03, N09). D05 is the CLI surface those
   ideas share. D06 and D09 on the cluster wait for N09's owner answer on interactive capacity.

### D03's soundness case in full (decision 11)

- **Why the key is complete.** The run step's inputs are the bytes of every binary it executes
  (C4 already binds them), the files of the enrolled runtime contract, the definition (the nextest
  profile, filters, the scrubbed environment, the fixed CPU count) and the execution fingerprint.
  Source files, features, build scripts, proc macros and `include_bytes!` data can affect the
  result only through the binary's bytes or through files read at run time. The bytes cover the
  first; the contract covers the second.
- **What fails if it is not complete.** A test reads a file outside its contract at run time.
  Today's per-root keys have the same exposure, and the enforced read audit on every execution,
  the sibling-file canary and A1 contain it (§6.4). New to D03: a stale build that reproduces the
  old binary. Equal digests from twin builds, one of them cold until compile reuse graduates,
  close that hole.
- **How it is detected.** A1 rebuilds cold and re-runs changed obligations. 2% of hits are
  re-executed. S1's planted failures include an edit that changes a test's behaviour without
  changing its binary (a data file outside the contract), and it must turn red.
- **The automatic fallback.** The first disagreement suspends the class. The record keys fall back
  to per-root or whole-tree keys, and the epoch is revoked if five re-runs show a wrong key (C6).

## Ideas to reject

| Dagger idea | Why not |
|---|---|
| Adopting the Dagger engine as the executor or the sandbox runtime | Decision 1 and decision 20 (amended) make the executor native Rust or the official runner. As a platform backend it would add a privileged second runtime, and Velnor could not trust its cache (C4). See the reuse table above. |
| Rewriting pipelines as Dagger modules in SDK languages | Decision 7 keeps the workflow YAML format, and `velnor-gen` emits it. Checks defined in code under test would also break C3 (the plan comes from the base commit). |
| Dagger Cloud, Cloud Engines, the `magicache` shared cache | Decision 7 keeps GitHub as the platform. The trust model (scoping per branch or PR, defence against poisoning) is not public. Self-hosting gets no shared cache. Velnor's RustFS tiers already cover this. |
| Cloud Checks run automatically on every push and PR | This conflicts with decision 12 (a push starts nothing; only the label starts PR CI). Velnor keeps the "no workflow file" half (D07). Starting work early is already owner question D19 (speculate before the label: about 15–25 s off label-to-verdict for about 5–10 runner-minutes per head). Dagger adds no new evidence, so the recommendation stays "no". |
| Reuse by TTL (`@cache(ttl: …)`) for anything that becomes evidence | Decision 11 says "when in doubt, re-execute", and §6.1 says nothing is reused on a time-to-live. A TTL bounds staleness by time, not by inputs. TTLs stay for advice only (D04). |
| Clients writing to the shared cache, as engines exporting their sessions' results do | Only node-agent observation mints (C4). A Mac, an agent or a PR never writes `main`'s trusted scope (decision 8). |
| Equivalence learned from asserted or "semantic" content digests, or from candidate-scope builds | It could merge different bytes into one class, which is a false green. D03 uses only byte hashes Velnor computed, confirmed by two builds. |
| Syncing the working directory, untracked and ignored files included, to the engine | Velnor keys committed trees only. A snapshot of a dirty tree risks reusing a record for a tree that differs from the one committed (N01's risk). The thin pack against the node mirror is cheaper too. |
| Rerunning one failed test until the check turns green (the per-test rerun of Dagger v1.0) | Retries stay at 0, and a pass that mixes attempts hides flakes (§1.3). Per-test reruns are allowed for diagnosis only, never filling a slot (D11 shows per-test results). |
| A best-effort, in-memory store for evidence | Acceptable for a cache, and Velnor's L0 behaves this way. The ledger and the success index must be durable (§5.9, §6.5). |
| LLM, `Env` and `Agent` types inside the runner | Agents are external clients. Running model loops on the runner adds secrets and nondeterminism to the trusted path and does nothing for verification. Velnor serves agents through the CLI, `--json` and MCP (D09). |
| Credential-conditional reuse (Dagger's handle-form session resources) | Obligations carry no secrets (§5.8), and jobs that hold secrets are promoted, not looked up. Nothing in Velnor needs this. |
