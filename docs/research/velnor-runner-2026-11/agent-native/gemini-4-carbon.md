# Reimagining the runner for AI agents: Gemini 4 Carbon

| Field | Value |
|---|---|
| Model | Gemini 4 Carbon |
| Model id | `gemini-4-carbon` |
| Client | Antigravity CLI 1.5.9 (agy -p) |
| Reasoning effort | high |
| Date / run | 2026-11-19 |
| Brief | agent-native brief v1 (2026-11-19): the final design, the user-experience draft and the owner decisions |

This is the model's own answer to the owner's prompt to reimagine the runner when AI agents do the work, especially running agents' local verification through Velnor so CI can skip what was already verified, unedited except for formatting.

---

# Velnor for AI agents

Evidence tags follow the v3 architecture. **[M]** marks a measurement on `velnor-new` or its
cluster. **[M, proxy]** marks a stand-in clock. **[E]** marks an estimate with arithmetic. **[C]**
marks a fact read from repository code. **[T]** marks a target gated on load tests.

## The picture

When parallel AI agents write the code, CI stops being a separate remote pipeline that repeats
what agents already ran. Every `cargo` and `hk` command passes through a local Velnor shim and
daemon. The daemon syncs changed blobs to warm `linux/x86_64` cluster sandboxes in under a second
and builds over a 0.25 s XFS reflink clone [M]. It streams diagnostics back to the agent and signs
a single-witness candidate receipt `T1` into `velnor-ledger`. Agents query and join that shared
ledger across worktrees, so untouched roots, lockfile builds, and known failures never run twice.
Local Mac `arm64` runs stay sub-3 s edit-check hints at `T0` and never mint `amd64` gate evidence.
Before pushing, `velnor verify` runs the second cluster witness `T2` on the merge tree of
`queue_tip` and `HEAD`. After push, auto-merge, and `velnor:verify`, the PR verdict and 67 to 83%
of merge-queue runs [E] finish as 1 to 3 s ledger lookups [E], backed by a cold `main` audit `T3`
with zero false greens.

## Ideas

| id | idea | mechanism in Velnor | what it removes | correctness, trust tier, and fallback | fit with owner decisions | cost in agent-hours |
|---|---|---|---|---|---|---|
| N01 | Layered CLI, Cargo shim, and host daemon | Replace `mise.toml` `[wrappers.cargo]` `mbx` and `hk.pkl` wrappers with `velnor shim`, Unix-socket daemon `velnor-agentd`, and `velnor` CLI. Hash worktrees via `GIT_INDEX_FILE` and match `velnor-ci` obligations | Removes `mbx` workspace misses across Mac worktrees, where 0 of 10 workspace units hit, leaving `229.7 MiB` per checkout and a `485 GiB` cache [M]. Routes 100% of agent Cargo runs into N02 and N06 | Exact base-inventory matches with `CARGO_INCREMENTAL=0` and scrubbed env inside Velnor sandboxes mint `T1`. Custom flags or mid-run edits stay `T0` and fall back to full CI runs | Fits Decisions 1, 10c, 11, 17, 18 | 12 h |
| N02 | Remote-by-default cluster runs against local sandboxes | `velnor-agentd` seals local tree blobs, uploads missing zstd chunks to `velnor-dispatch`, runs inside a warm `linux/x86_64` XFS clone at `/w/velnor-new`, streams output, and signs `T1` via `velnor-node` | Moves tests off the M5 Max at load 40 to 110. Cuts `velnor-runner` cold build from `265.4 s` to `92.0 s`, or `2.9x`, and tests from `1,353.7 s` to `192 s`, or `7.1x` [M]. Fills Slot 1, saving `2*3 = 6` Goal or `2*16 = 32` warm runner-min per PR [E] | Sandbox runs network-off without `.git`, scrubs shell env per C5, and checks the full staged-tree SHA before step 1 per C2 and C4. Tier `T1`. WAN loss falls back to local Docker `velnor-local` at `T0` | Fits Decisions 8, 10c, 11, 13, 21, 24 | 14 h |
| N03 | Four receipt tiers and 2-witness gate | `velnor-ledger` enforces `T0` local hints, `T1` 1-witness cluster runs, `T2` 2-witness node-disjoint seeded cluster records, and `T3` post-A1 cold-audited `main` records. Gate requires `T2` | Agents iterate at `T1` with 1 witness and run Witness 2 only on surviving trees. Ten edit-test loops at 3 runner-min cost `10*3 + 3 = 33` runner-min instead of `10*6 = 60`, which saves 27 runner-min or 45% [E] | Zero `T0` or `T1` receipts satisfy `Velnor / Verified`. Gate requires `T2` for `1-(1-p)^2` detection plus cold A1 as the 3rd run and 2% random cold sampling per C6. Retired base falls back to 2 fresh cluster runs | Fits Decisions 8, 10, 11, 12 | 6 h |
| N04 | Split `arm64` and `amd64` execution policy | Bind `os`, `kernel`, `arch`, and `cpu_features` into every key. Keep sub-3 s `INCR=1` `S-gen` edits on native Mac `arm64` at `T0` and run obligations on native `linux/x86_64` EPYC nodes via N02 | Avoids 3 to 8x QEMU or Rosetta `amd64` emulation on Mac [E] and catches Linux-only checks like `check-native-image-inventory` in about 25 s instead of after a 26.7 min PR run [M]. Q-AN1 saves 4 s of `R03` QEMU smoke per build [E] | `arm64` `T0` keys never match `amd64` `T2` keys, which prevents `#[cfg]`, APFS case, and libc false greens. Tier `T0` on Mac and `T1` to `T3` on the `amd64` cluster. Cluster outage falls back to local `T0` | Fits. Owner question Q-AN1 on adding a `linux/arm64` cluster node when Decision 9 unpauses | 4 h |
| N05 | Continuous `cpu.idle` checks on unpushed commits | `velnor-agentd` watches `.git/logs/HEAD`, debounces 3 s, computes `merge-tree` against `queue_tip`, and runs missing `T1` or `T2` obligations on the cluster at `batch` `cpu.idle` before push or label | Hides the 25 to 35 s compile-test chain [E] inside the 11 s median commit-to-push, where 38% wait at least 30 s, and 42 s P50 commit-to-CI gaps [M]. Cuts verdict wait from about 35 s to 1 to 3 s at `T2` for `3*6 = 18` or `3*3 = 9` idle runner-min per PR [E] | Writes content-keyed `candidate` `T1` or `T2` records only, never GitHub statuses, so unlabelled PRs stay blocked. Above 80% CPU or RAM watermark, `velnor-node` kills batch jobs and falls back to `velnor verify` or post-label CI | Owner question Q-AN2. Conflicts with Decision 12 and D19. The default keeps N05 off | 6 h |
| N06 | Fleet-wide ledger queries and in-flight joins | `velnor shim` and `velnor ledger query` check obligation keys against `T1` to `T3` records, same-arch `T0` failure hints, and `in_flight` leases before spawning local or remote jobs | Saves up to `11*38 = 418` whole-tree runner-min when 12 agents verify a new `main` tip [E], coalesces shared `Cargo.lock` builds to save 43.4 to 67.4 s per agent [M], and skips untouched enrolled roots at 3 min per witness [E] | Uses full-tree or eBPF-enrolled read-contract keys per C2. `T1` satisfies interactive queries while PR and group gates enforce `T2` and fresh C5 runs. Ledger outage falls back to fresh cluster or local runs | Fits Decisions 8, 10b, 10c, 11 | 6 h |
| N07 | Inner-loop flake trap and 5x5 bisector | Add `--order-seed` to pinned `cargo-nextest` with `retries = 0`. On `MixedOutcome` across witnesses or reruns, dispatch 5 full-binary seeded runs on `HEAD` and 5 on `main` at `batch` priority | Saves 1 to 2 wasted agent fix cycles of 26.7 min, or `27` to `53 min` [M, E], per false red at 8.1% per run [M] and 15.5% per pair, about 9 per day at 58 merges per day [E]. Prevents depth-4 queue ejections that disturb 29% of entries [E] | Failed attempts stay permanently red under `retries = 0`. A manual rerun starts a new 2-witness attempt counted against the flake budget. Bisector writes `T0` `audit` logs only and falls back to deferred batch queue under load | Fits Decisions 11, 15 | 8 h |
| N08 | Interactive agent priority band with fairness | Extend `velnor-dispatch` to `merge > gate > interactive > pr > batch`. Grant each worktree 1 active 16-CPU `interactive` slot and 15 slot-min, or `240 CPU-min`, per hour, preempting `batch` | Removes the `0.95/0.05 = 19x` queueing spike on 92 to 95% busy workers [M, E]. Cuts foreground wait from minutes to at most 2 s P90 [T] while capping 12 agents to 12 slots and `2,880` of `28,800` hourly CPU-min, or 10% | Priority changes start order only, never inputs or witness counts, keeping `T1` for interactive runs and `T2` for gates. Reserved `merge` and `A1` slots stay fenced. Over-budget calls fall back to `pr` priority or local `T0` | Fits Decisions 11, 13, 15 | 6 h |
| N09 | Isolated local signing against cluster signing | Dedicated service user `velnor-local-signer` signs local Docker `velnor-local` runs as `T0`. Out-of-sandbox `velnor-node` Ed25519 daemon plus append-only site log signs cluster `T1` to `T3` | Authenticates shared `T0` failure hints and fast-red timings. Under Q-AN3, TPM-attested `linux/x86_64` workstations filling Slot 1 save 50% of witness CPU, `2*3 = 6` Goal or `2*16 = 32` warm runner-min per PR [E] | The default keeps `T0` at zero gate trust. Under Q-AN3, cluster Slot 2 digest equality plus cold A1 block deterministic false greens, while cluster-only flake capture drops from `1-(1-p)^2` to `p`. A 500 ms signer timeout falls back to unsigned `T0` | Fits default. Owner question Q-AN3 on TPM `amd64` Slot 1 credit, Decision 11 | 8 h |
| N10 | Pre-flight `velnor verify` for lookup verdicts | Pull `velnor verify` V11 into Phase 1 with `velnor-ci` and `velnor-gen` injectable clocks G18 plus Audit A2. Pre-verify `merge-tree` of `queue_tip` and `HEAD` to `T2`, then push with auto-merge and `velnor:verify` | Cuts PR verdict from 26.7 min P50 [M] and about 35 s at Goal [E] to a 1 to 3 s lookup [E] for the 47% helper and generator class. Cuts `merge_group` runs from 19.9 min P50 and 64.6 min [M] to 1 to 3 s on 67 to 83% of groups [E] | Fenced App writer posts `Velnor / Verified` only when active-epoch `T2` or `T3` records cover all base-inventory obligations on the exact SHA tree, plus fresh `T2` runs for uncleaned C5 crates and nightly A2. Misses fall back to 2 cluster witnesses | Fits Decisions 7, 10, 11, 12 | 18 h |
| N11 | Cluster-resident XFS agent worktrees | Host headless agents in unprivileged pods on a dedicated XFS worker. `velnor worktree create` clones source and `M_G` at `/w/velnor-new` in 0.25 s, and `velnor-node` clones into a sealed `build` sandbox for `T1` | Cuts new-worktree warm readiness from `1.2 s` clone plus `57` to `119 s` path rebuild on Mac, or `58` to `120 s` [M], to `0.25 + 0.31 = 0.56 s`, which is `100x` to `210x` faster [M]. Removes WAN sync and retires the `485 GiB` Mac cache [M] | Agent pod has `T0` authority and zero signing keys. `velnor-node` clones into a network-off `build` sandbox, checks the tree hash at `T1`, and runs Witness 2 on a second node for `T2`. Node loss falls back to Mac N02 | Owner question Q-AN4. Reopens decision-log N08 and adds 1 worker at about `$200` per month [E] | 16 h |
| N12 | Fleet merge-matrix oracle and CAS train option | `velnor-ledger` runs in-memory `git merge-tree` across at most 6 active `velnor verify` and armed trees, warns colliding agents in under 1 s, and pre-verifies ordered pairs at `batch`. Optional CAS train N31 | Cuts merge-group tree misses from 25 to 40% to below 10% [E], saving `0.20*40 = 8 s` mean and 1 to 4 min P90 [E], and curbs multi-group reruns on 35 of 150 PRs, or 23.3% [M]. Q-AN5 removes GitHub's `19 + 26 = 45 s` queue floor [M] | Speculative pair runs write content-keyed `T2` `candidate` records only. Authoritative `merge_group` verifies its exact SHA under ALLGREEN and falls back to normal twin execution on a miss. Q-AN5 defaults to GitHub's queue | The matrix fits. Owner question Q-AN5 on D18, Decision 7, and ruleset for CAS merge train | 8 h |

**N01. Layered CLI, Cargo shim, and host daemon.** Velnor combines all three entry points so agents
produce receipts without prompt edits or per-call startup cost. Explicit verification commands live
on the `velnor` CLI. Meanwhile `mise.toml` lines 43 to 45 [C] and `hk.pkl` lines 20, 35, 47, and 58
[C] route every `cargo check`, `clippy`, `nextest`, `doc`, and `fmt` call through `velnor shim` over
a Unix socket to `velnor-agentd`. That host daemon keeps warm mTLS cluster streams, writes a
temporary `GIT_INDEX_FILE` to hash the worktree without touching `.git/index`, and matches arguments
against the base commit's released `velnor-ci` inventory. On Apple Silicon Macs, `mbx` misses 10 of
10 workspace units across worktrees at distinct paths, which leaves 229.7 MiB of dead output per
checkout and a 485 GiB cache [M]. Routing calls through `velnor-agentd` removes that local cache
growth and feeds N02 and N06 automatically. Custom test filters, `CARGO_INCREMENTAL=1`, un-scrubbed
overrides, or mid-run file edits fail the post-run tree check, stay `T0`, and fall back to CI.

**N02. Remote-by-default cluster runs against local sandboxes.** Running sealed containers locally
through `velnor-local` contends for one 18-core Apple M5 Max at load 40 to 110. On that loaded Mac,
a cold `velnor-runner` nextest pass takes 1,530 s total, split into 176 s of `mbx` compile and
1,353.7 s across 430 tests [M], while plain cold `cargo build` takes 265.4 s [M] and outputs stay
`arm64`. Instead, `velnor-agentd` seals a content-addressed tree snapshot locally and executes
obligation commands remotely by default on the 480-thread EPYC cluster. It diffs blobs against the
node's bare mirror per Decision 24, uploads missing zstd chunks under 50 KiB in 0.15 to 0.3 s [E],
clones an XFS generation at `/w/velnor-new` in 0.25 s [M], and checks the staged-tree SHA in 0.1 to
0.3 s [E]. The sandbox scrubs shell environment variables, `.git`, and network access per C5. It
streams diagnostics back and signs a `T1` receipt via `velnor-node`, which cuts Slot 1 PR compute by
6 to 32 runner-min across 2 heads [E]. Fast `INCR=1` edits on `S-gen` take 1.5 to 2.8 s locally [M]
at `T0`, and `velnor-local` Docker serves as an offline `T0` fallback.

**N03. Four receipt tiers and 2-witness gate.** Modeling provenance as four strict tiers in
`velnor-ledger` gives agents single-run speed during edits while keeping the v3 gate intact. `T0`
covers native Mac and local Docker runs and never fills a witness slot. `T1` covers one
cluster-observed, `CARGO_INCREMENTAL=0`, Ed25519-signed execution from N02, which answers the agent
and fills Slot 1 of a `candidate` record. `T2` adds a second passing witness on a distinct worker
node, attempt, `--order-seed`, and twin or cold generation per Q12. The status writer posts `Velnor
/ Verified = success` only when `T2` or `T3` records cover every base-inventory obligation plus
fresh `T2` runs on C5 checks. A record reaches `T3` only after it lands on `main` and passes cold
Audit A1 as the 3rd total execution, backed by 2% ongoing cold sampling per C6. Only `T3` seeds
generations or releases per Decision 8. Across 10 edit-test loops of 3 runner-min each, running `T1`
and promoting once to `T2` spends 33 runner-min instead of 60 and saves 45% [E].

**N04. Split `arm64` and `amd64` execution policy.** Agent Macs run `darwin/arm64` and local Linux
containers run `linux/arm64`, whereas cluster workers and release binaries are `linux/x86_64`.
Running `linux/amd64` containers on Apple Silicon under QEMU or Rosetta slows builds 3 to 8x [E] and
alters kernel and SIMD behavior. Trusting local Darwin runs would also miss Linux-only checks such
as `check-native-image-inventory`, which `mise.toml` line 115 skips on Darwin [C], and case-sensitive
path bugs hidden by APFS. Velnor binds `os`, `kernel`, `arch`, and `cpu_features` into every key per
C2. It keeps fast `INCR=1` typechecks on local `arm64` at `T0`, runs all receipted obligations on
`linux/x86_64` EPYC workers via N02, and cross-compiles `aarch64-unknown-linux-musl` on the cluster
per item R03. **Owner question Q-AN1.** Under Decisions 9 and 11, when macOS and `arm64` support
returns, should Velnor add a `linux/arm64` cluster worker so native `arm64` obligations and R03
binaries run without QEMU smoke tests and save about 4 s per release build [E]? The default keeps
`x86_64` only.

**N05. Continuous `cpu.idle` checks on unpushed commits.** Across 1,009 commits, the median commit
sits locally for 11 s before push and 38% sit for at least 30 s [M]. Across 100 merged PRs, the
final commit waits 42 s at P50 and 320 s at P90 before CI starts [M]. A reflog watcher in
`velnor-agentd` can watch `.git/logs/HEAD`, debounce for 3 s, compute `merge-tree` against
`queue_tip`, and submit missing obligations to the cluster at `batch` `cpu.idle` priority before
push or label. Running both witnesses to `T2` hides the 25 to 35 s compile-test chain [E] inside the
agent's self-review and cuts label-to-verdict P50 from about 35 s to 1 to 3 s [E]. Because pre-runs
write only content-keyed `candidate` records and never touch GitHub statuses, unlabelled and draft
PRs remain blocked. **Owner question Q-AN2.** Decision 12 forbids unlabelled pushes from starting CI
because 45% of PR runs were cancelled [M]. Does the owner allow `cpu.idle` pre-runs on local commits
at about 9 `T1` or 18 `T2` preemptible idle runner-min per PR [E], or keep N05 off?

**N06. Fleet-wide ledger queries and in-flight joins.** Twelve parallel builder agents that rebase
onto `main` or stack branches otherwise repeat identical workspace builds and test suites. Before
spawning work, `velnor shim` and `velnor ledger query` compute the obligation keys from the released
planner and batch-query `velnor-ledger` across `T3`, `T2`, `T1`, live `in_flight` leases, and
same-arch `T0` failure hints. Before per-root enrolment, 12 agents checking a new `main` tip or a
shared stacked base reuse one whole-tree verification and save up to `11 * 38 = 418` runner-min [E].
After per-root enrolment V08, concurrent agents editing disjoint folders also skip untouched roots
at 3 runner-min per witness [E]. When an update touches `Cargo.lock` on 10.5% of PRs [M], later
agents join the single in-flight cluster build instead of each spending 43.4 to 67.4 s [M]. Known
failed keys return their failing test IDs and log tail in under 0.2 s. Gate safety holds because
`Velnor / Verified` still demands `T2` and fresh C5 runs, and ledger outages trigger fresh runs.

**N07. Inner-loop flake trap and 5x5 bisector.** Full CI runs today hit an 8.1% false-failure rate,
with 10 of 124 push runs failing on a SHA whose merge group passed [M]. Two pre-merge witnesses at
`retries = 0` raise pair false failures to 15.5% per Q12. That causes about 9 false blocks per day
at 58 merges per day [M, E] and wastes 1 to 2 agent fix cycles of 26.7 min each [M]. Velnor adds a
deterministic `--order-seed` flag to pinned `cargo-nextest` and flags `MixedOutcome` whenever
Witness 1 and Witness 2 disagree or a manual rerun flips. That attempt stays red because `retries =
0` never bends, while a manual rerun creates a fresh 2-witness attempt counted against the 3% to 1%
flake budget. In parallel, `velnor-ledger` runs 5 full-binary seeded passes on `HEAD` and 5 on
`main` at `batch` priority, taking 50 to 120 s on `velnor-ci` and 3 to 8 min on heavy suites [E].
`velnor-agentd` reports `inherited_flake` or `introduced_race` before a flaky PR can enter the queue
and disturb 29% of depth-4 entries [E].

**N08. Interactive agent priority band with fairness.** Cluster workers hit 92 to 95% CPU on peak
days [M], where unprioritised wait reaches `0.95 / 0.05 = 19x` service time [E] and stalls a 25 s
test for 8 minutes. Extending `velnor-dispatch` to five bands, `merge > gate > interactive > pr >
batch`, protects interactive speed without starving merges. Each active worktree gets at most one
concurrent 16-CPU `interactive` slot and a token bucket of 15 slot-min, or `240 CPU-min`, per hour.
Across 12 agents, this rule caps `interactive` usage at 12 of the cluster's 20 to 30 build slots and
10% of hourly cluster CPU. Witness-2 promotions, wide fan-out, and over-budget runs drop to `pr` or
`batch`. `interactive` preempts `batch` `cpu.idle` jobs, terminates and requeues them at the memory
watermark to free page cache, and holds foreground wait to at most 2 s P90 [T]. Reserved `merge` and
`A1` slots stay fenced, and an armed PR that joins an `interactive` run lifts that run's priority to
`pr`.

**N09. Isolated local signing against cluster signing.** Signing keys follow strict isolation
boundaries. On cluster workers, `velnor-node` runs an out-of-sandbox Ed25519 signer per node and
sandbox class, binds binary digests and observed test IDs per C4, and writes an append-only site log
for `T1` to `T3`. On Macs, `velnor local up` runs a dedicated local service user
`velnor-local-signer` outside the agent shell to sign `velnor-local` Docker runs as `T0`, with
future Apple Containerization support under Decision 21. That signature stops broken agent scripts
from spoofing shared `T0` failure hints and fast-red timings, while keeping zero gate authority on
`T0`. **Owner question Q-AN3.** Under Decision 11 and Q12, on a future `linux/x86_64` workstation
with TPM-measured boot and an isolated `velnor-node`, may a local `x86_64` receipt fill Witness Slot
1 to save 6 to 32 cluster runner-min per PR [E]? Cluster Slot 2 digest checks plus cold A1 still
catch deterministic bugs, but local tamper or skew cuts cluster-trusted pre-merge flake capture from
`1-(1-p)^2` to `p`. Keeping N02 remote-by-default is safer.

**N10. Pre-flight `velnor verify` for lookup verdicts.** Promoting `velnor verify` item V11 from Day
16 to 18 stretch into Phase 1 turns the PR verdict into a ledger read. Pairing V11 with injectable
clocks recorded in the key per item G18 for `io/mod.rs:210` `now_seconds` and the 5 clock-reading
test files [C], backed by nightly `+8` and `+30` day Audit A2 shifts, removes C5-forced reruns on
the 47% `ci-helper/` and `generator/` PR cohort. Before pushing, `velnor verify` computes `T_cand =
git merge-tree --write-tree queue_tip HEAD` and finishes `T2` on `T_cand`, reusing N02's `T1` Slot 1
whenever `queue_tip` stayed fixed or per-root keys match. The agent pushes immediately, turns on
auto-merge, and adds `velnor:verify` via its App token. `velnor-control` derives the identical
`merge-tree`, matches all `T2` and `T3` keys in about 0.5 s [E], and writes `Velnor / Verified =
success` in 1 to 3 s [E], plus fresh C5 runs on uncleaned roots. When GitHub forms the `merge_group`
19 s later [M], 67 to 83% of groups within 1 to 5 min share `T_cand` per Q11 [E] and pass by a 1 to
3 s lookup.

**N11. Cluster-resident XFS agent worktrees.** Because headless AI agents do all coding per Decision
15, hosting 12 worktrees on one Mac causes avoidable WAN traffic, `arm64` drift, and local disk
bloat. On Mac APFS, cloning a warm target takes 1.0 to 1.4 s, yet Cargo still spends 57 to 119 s
rebuilding all 6 workspace crates because the checkout path changed [M]. With `velnor worktree
create <task>`, agents run in unprivileged pods on a dedicated XFS worker where every worktree mounts
at `/w/velnor-new` inside its own mount namespace. A reflink clone plus warm build then takes `0.25
+ 0.31 = 0.56 s`, which is `100x` to `210x` faster [M], and the 485 GiB Mac cache [M] disappears.
Agent pods hold `T0` rights and zero signing keys. When `velnor shim` runs an obligation,
`velnor-node` clones the worktree in 0.25 s into a network-off `build` sandbox, checks its tree SHA
at `T1`, and runs Witness 2 on another worker for `T2`. **Owner question Q-AN4.** Reopening
decision-log item N08, approve 1 dedicated agent worker at about `$200` per month [E] outside the 5
CI workers?

**N12. Fleet merge-matrix oracle and CAS train option.** When two agents run `velnor verify` against
the same `queue_tip` together, the second to enqueue faces `queue_tip + A + B`. That clash leaves 25
to 40% of merge groups to miss on tree hash [E] and contributes to 35 of 150 queued PRs, or 23.3%
[M], running multiple merge groups. Because N02 and N10 register each active agent's tree in
`velnor-ledger`, the ledger can track at most 6 pre-flighted or armed trees, run in-memory `git
merge-tree --write-tree` across pairs in under 30 ms [E], warn colliding agents via `velnor shim`,
and pre-verify ordered pair trees at `batch` priority before enqueue. Pre-verifying pairs cuts group
misses below 10% [E]. **Owner question Q-AN5.** Reopening D18 against Decision 7, once N10 and N12
make PR and group checks 1 to 3 s lookups, GitHub's queue adds 19 s to form a group and 26 s to
merge, or `45 s` [M]. After a 200-PR shadow run, may Velnor replace the ruleset's `merge_queue` rule
with an App-only atomic ref compare-and-swap train N31, keeping human `bypass_actors` empty, to
reach about 7 to 12 s push-to-merge [E]?

## Risks

| id | how it could cause a false green | how it could cause waste | how Velnor contains both |
|---|---|---|---|
| N01 | Shim misparses Cargo flags, leaks dirty env vars, or hashes while a file changes mid-run | Hashing untracked build folders slows every local Cargo call | Match only exact `velnor-ci` inventory args, run in env-scrubbed remote snapshots via N02, and ignore `target/` via Git index rules |
| N02 | Corrupt upload chunk or stale node mirror stages a tree different from the PR commit | Uploading large generated fixtures on every edit saturates the Mac uplink | Verify the full staged-tree SHA before step 1, dedupe uploads with zstd chunks at about 1% delta [M], and keep fast `INCR=1` checks local at `T0` |
| N03 | Buggy status query counts a single-witness `T1` receipt as passing `Velnor / Verified` | Agents discard 90% of intermediate `T1` trees and strand dead blobs | Separate `Slot1` and `Slot2` types so the status writer compiles only against `T2` or `T3`, keep 2% cold sampling, and expire unpromoted `T1` blobs after 24 h |
| N04 | Cross-arch `T0` hint or miskeyed `arm64` node masks an `x86_64`-only failure | Running a check locally at `T0` and again remotely at `T1` burns double CPU | Bind `os`, `kernel`, and `arch` in every key so `arm64` never satisfies `amd64`, scope `T0` hints by arch, and dispatch each `velnor shim` call to one side only |
| N05 | Pre-verified commit status leaks onto an unlabelled or draft PR | Commit-amend bursts flood `batch` workers and repeat today's 45% cancel waste [M] | Reflog runs write `candidate` ledger rows only and never call GitHub's status API, guarded by a 3 s debounce and instant `cgroup.kill` above 80% CPU |
| N06 | Narrow per-root key misses a parent read across 79 of 86 `CARGO_MANIFEST_DIR` files [M] | Twelve agents miss on a new `main` tip simultaneously and start 12 builds | Keep whole-tree keys until eBPF read-audit enrolment enforces every path per C2, and coalesce identical concurrent keys via `in_flight` joins |
| N07 | Shared bisector key labels PR B's new race as PR A's `inherited_flake` | One flaky binary spawns 10 `batch` runs per agent across all 12 agents | Keep `retries = 0` hard-coded on gates, key `HEAD` bisectors by `binary_id` plus `head_key` and `main` by `binary_id` plus `main_sha`, and cap daily flake CPU |
| N08 | Scheduler bug lets `interactive` jobs starve `merge_group` or cold Audit A1 | Twelve agents spawn parallel loops and pin 20 build slots at `interactive` | Fence reserved `merge` and `A1` slots in `velnor-node`, cap each worktree at 1 `interactive` slot and 15 slot-min per hour, and demote excess to `pr` or `batch` |
| N09 | Root tamper on a Q-AN3 workstation patches the kernel to forge Witness Slot 1 | Hung `velnor-local-signer` socket blocks local container runs | Keep `T0` non-gating by default, require cluster Slot 2 binary-digest equality plus cold A1 if Q-AN3 ever turns on, and enforce a 500 ms signer IPC timeout |
| N10 | `queue_tip` moves during `velnor verify` or a date-sensitive test slips past fixed clocks | Agent runs `velnor verify` repeatedly before rewriting PR commits | `velnor-control` recomputes `merge-tree` on authoritative PR and `merge_group` SHAs, runs uncleaned C5 checks fresh, and runs nightly clock-shifted Audit A2 |
| N11 | Agent process mutates a shared base generation, or 12 clones exhaust `125.5 GiB` RAM via `14.29 GB` per-clone page caches [M] | Idle agent pods hold memory on CI workers | Mount bases read-only, share page cache via a Lab B2 read-only `overlayfs` lower layer, isolate agents on a dedicated tainted node, and run Witness 2 on another node |
| N12 | Speculative tree `tip + A + B` misses because the queue orders `tip + B + A` | Pairwise `merge-tree` builds grow quadratically when many PRs arm at once | Authoritative `merge_group` always verifies its exact SHA under ALLGREEN, and speculation caps at most 6 ordered prefixes plus single-ejection bypass trees |

Systemic containment backs all twelve ideas. If post-merge cold Audit A1 disagrees with any
agent-originated `T2` record, `velnor-control` freezes status writes at once and revokes the active
epoch. It marks open heads and queued groups that used that class as `failure` so GitHub ejects
them, and it cold-audits every commit landed in the window per C6. When the cluster or
`velnor-ledger` is unreachable, `velnor-agentd` falls back to local `T0` runs and full post-push
cluster verification. That fallback path is slower and never wrong.

## The first three steps

These three work items fit inside binding Decisions 1 through 24 and wait on zero open owner
questions. They take 62 agent-hours, or about 16 to 24 wall-clock hours across 12 builders with
reviews, and deliver immediate pre-push reuse that compounds with Day 7.5 to 10 graduation C07 and
per-root keys V08.

1. **Ship `velnor verify`, remote blob-delta runs, and `velnor-ci` clock cleanup in Phase 1 across
   N02, N03, and N10 in 30 agent-hours.** Right after `velnor-ledger` items V01 to V04 and XFS warm
   generations C05 land around Day 5, add the missing-blob upload endpoint in `velnor-dispatch` and
   `velnor-node`. Enforce `T0` to `T3` in `velnor-ledger` and ship `velnor verify` item V11. Replace
   `SystemTime` calls in `ci-helper/crates/velnor-ci/src/io/mod.rs:210` and the 5 clock-reading test
   files with key-recorded injectable clocks per G18 plus nightly Audit A2 item V06. **Verification
   and payoff.** Pass planted-failure suite S1 including mid-run edits and `queue_tip` shifts.
   Before C07 graduates, `velnor verify` shifts the 6 min cold Witness 2 run pre-push so a prompt
   push and label resolves the PR verdict in 1 to 3 s [E]. Once C07 and V08 land, `velnor verify`
   drops to about 25 to 35 s and 67 to 83% of merge-queue runs finish as 1 to 3 s lookups [E].
2. **Replace `mbx` in `mise.toml` with `velnor shim`, `velnor-agentd`, and the `interactive` band
   across N01 and N08 in 18 agent-hours.** Point `mise.toml` line 44 and `hk.pkl` lines 20, 35, 47,
   and 58 at `velnor shim`. In the same commit per Decision 18, update `scripts/verify-local.sh`,
   `scripts/check-freshness.sh`, and `weaken.rs` lines 300 to 320. Delete the legacy engine's
   `mbx`-wrapper gate in `native_tool_input.rs:104-119` and `build_tasks.rs:74-75`. Add the
   `interactive` band with 1 slot and 15 slot-min per hour per worktree in `velnor-dispatch`.
   **Verification and payoff.** Pass `scripts/verify-local.sh` and hold `interactive` wait to at
   most 2 s P90 under 85% load in K04. Agent `velnor-runner` tests speed up `7.1x` from `1,353.7 s`
   to `192 s` [M], the `485 GiB` Mac cache retires [M], and Slot 1 `T1` fills during normal coding.
3. **Turn on fleet ledger queries, in-flight joins, and the `MixedOutcome` bisector across N06 and
   N07 in 14 agent-hours.** Wire `velnor shim` to batch-query `velnor-ledger` across `T1` to `T3`,
   `in_flight` joins, and same-arch `T0` failure hints before spawning work. Add `--order-seed` to
   pinned `cargo-nextest`. Trigger 5x5 full-binary `HEAD` and `main` `batch` bisection on mixed
   witness outcomes with `retries = 0`. **Verification and payoff.** Prove 12-worktree join
   coalescing and 100% red retention on seeded flaky suites in K08. Duplicate post-rebase and
   `Cargo.lock` builds across the 12 agents stop, and agents no longer burn 27 to 53 min [M, E]
   chasing inherited `main` flakes.
