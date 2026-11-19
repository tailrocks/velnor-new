# Reimagining the runner for AI agents: Gemini 4 Neon

| Field | Value |
|---|---|
| Model | Gemini 4 Neon |
| Model id | `gemini-4-neon` |
| Client | Antigravity CLI 1.5.9 (agy -p) |
| Reasoning effort | high |
| Date / run | 2026-11-19 |
| Brief | agent-native brief v1 (2026-11-19): the final design, the user-experience draft and the owner decisions |

This is the model's own answer to the owner's prompt to reimagine the runner when AI agents do the work, especially running agents' local verification through Velnor so CI can skip what was already verified, unedited except for formatting.

---

# Velnor: Agent-Native Verification & Execution

*Proposal grounded in Velnor v3 (`velnor-runner-architecture.md`), UX draft (`velnor-user-experience.md`),
and binding owner decisions 1–24. Tags: **[M]** measured, **[E]** estimated, **[T]** target, **[C]** read
from code.*

## The picture

When 12+ AI coding agents drive almost every change in parallel, treating "local dev" and "remote CI"
as separate worlds wastes both laptops and servers. Today agents burn Apple Silicon CPUs compiling and
testing worktrees under `darwin/arm64`, push, and wait while Hetzner `linux/amd64` nodes redo every
byte from scratch. An agent-native Velnor collapses both worlds into **one remote-by-default
verification fabric**: an agent's `cargo nextest`, `cargo clippy`, and `velnor verify` route through
a thin local daemon onto warm Hetzner cluster sandboxes, streaming structured diagnostics back in real
time while minting signed **Tier-2 candidate receipts** into `velnor-ledger`. Agents query the ledger
before executing so parallel worktrees never duplicate baseline or shared-base work, and surface flakes
during development before they eject merge-queue batches. Local Macs keep an offline/fast-red Docker
sandbox (`Tier-1`, never gate-eligible) for instant feedback only. When an agent pushes and applies
`velnor:verify`, the PR and merge-group verdicts become **a 2–5 s ledger lookup** rather than a
build—preserving two independent cluster witnesses, cold Audit A1, and zero false greens.

## Ideas

| id | idea | mechanism in Velnor | what it removes (with arithmetic) | correctness argument (tier & fallback) | fit with owner decisions | cost (agent-h) |
|---|---|---|---|---|---|---|
| N01 | `velnor` CLI, Cargo shim & `velnor-agentd` | `PATH` shim (`cargo`, `cargo-nextest`) + `velnor verify` talk to per-host `velnor-agentd`, mapping canonical workspace commands onto `velnor-ci obligation exec` and the base commit's released inventory. | Eliminates 100% of post-push repeat of agent pre-push checks: ~30–35 s at v3 goal (~5.5–6 min at day 5.2) and ~6–12 runner-min/PR [E]. | Only canonical commands over clean, full-tree snapshots on the released inventory mint receipts; ad-hoc/dirty runs stay **T0** (never recorded). Fallback: local unminted run; CI runs fresh. | Fits (10c, 11) | 16 |
| N02 | Remote-by-default on warm cluster nodes vs. local sandboxes | `velnor-agentd` syncs a compressed Git tree delta (~5–50 KiB) over QUIC/WireGuard to `velnor-dispatch`/`velnor-node` on Hetzner XFS warm pools; streams logs back. `--local`/offline uses `velnor-platform-docker`. | Eliminates 12-worktree Mac throttling & ~60–120 GiB local `target/` churn [E]; turns 61 s Mac / 92 s cold build [M] into 0.25 s clone + ~15 s warm compile [E] that counts as CI. | Remote runs use the exact CI sandbox, eBPF read audit, `linux/amd64` fingerprint, and node signer (**T2**). Local Docker runs stay **T1/T0** and never gate. Fallback: offline local Docker; CI runs both witnesses. | Fits (10c, 11, 13, 24) | 22 |
| N03 | Four-tier receipt trust ladder & witness quorum | `velnor-ledger` enforces **T0** (raw host), **T1** (local sealed container), **T2** (cluster-witnessed candidate), **T3** (A1-audited `main`). Gate requires **2 independent T2/T3 witnesses** on distinct nodes/orders/generations. | Remote agent run fills Slot 1 of 2 during dev, cutting post-push CI compute 50% (~6 runner-min at goal; ~19–38 at day 5.2 [E]); `--quorum` fills both slots, cutting post-push compute 100%. | Preserves `1 - (1 - p)^2` detection (§1.2). T0/T1 fill 0 gate slots; only distinct-node T2/T3 count; A1 audits every landed commit cold. Fallback: missing/revoked slots run fresh at gate. | Fits (8, 10c, 11); **Owner Q1** on T1+T2 | 10 |
| N04 | `arm64` Mac vs. `linux/amd64` cluster split | `ExecutionFingerprint` binds `(os, arch, kernel, cpu_features)`. Minting witnesses run natively on Hetzner `linux/amd64` (N02); local Apple Silicon runs only as a T0/T1 fast-red pre-filter (no Rosetta/QEMU). | Avoids 3–10× Rosetta/QEMU slowdown [E], eliminates double-compilation (`arm64` local + `amd64` CI), and catches Linux/case-sensitive XFS mismatches during dev rather than in CI. | Disjoint keyspaces make cross-arch reuse structurally impossible; even static checks (`rustfmt`, `velnor-gen check`) need `linux/amd64` T2 receipts to merge. Fallback: local `arm64` fast-red only; CI runs `amd64`. | Fits (9, 11, 13, 21) | 8 |
| N05 | Continuous verification of committed-but-unpushed work | `velnor-watch` (`post-commit` hook / ref watcher in `velnor-agentd`) computes `merge-tree(queue_tip, HEAD)` after a 15 s debounce and submits speculative **batch (`cpu.idle`)** T2 verification, cancelling on new commit. | Hides 15–35 s (goal) / 3–6 min (day 5.2) compile+test behind agent push/PR prep, dropping label-to-green to ~2–4 s [E]; risks ~20–45% wasted batch CPU (~5–15 runner-min/PR [E]). | Writes only **T2 candidate** receipts on exact full trees; never writes `Velnor / Verified` until pushed + labelled `velnor:verify`. Fallback: preempted/amended commits miss and run at label. | **Owner question** (conflicts with D12; extends D19) | 12 |
| N06 | Cross-agent ledger query & in-flight join | `velnor ledger query` (auto-invoked by N01 shim) computes obligation keys via the released `velnor-ci` planner and checks `velnor-ledger`, returning cached passes/advisory reds or joining live in-flight runs. | Across 12 agents sharing `main`/stacked bases (~4 checks/agent/h), removes ~50–75% of duplicate unchanged-root & baseline checks (~40–80 runner-min/h saved [E]); < 0.5 s hit latency. | Serves only unrevoked, active-epoch **T2/T3** exact-key hits; C5 obligations always return `FRESH_REQUIRED`; failed records are advisory only and never skip re-runs. Fallback: ledger down → `MISS` → run fresh. | Fits (10b, 10c, 11) | 12 |
| N07 | Shift-left flake detection during development | `velnor-ledger` records same-key outcome disagreements across agent runs/witnesses into a flake stream; adds `velnor verify --stress N` (parallel seeded runs on idle nodes) before pushing. | Prevents merge-queue ejections (8.1% per run [M] → 15.5% per pair), each saving 1–4 group rebuilds (~6–40 runner-min & 3–15 min wall at goal; ~65–260 runner-min today [M/E]). | Zero retries stay enforced on all minting runs (C7); any disagreement fails the record immediately. Stress runs write to telemetry only, never to a gate slot. Fallback: standard two-witness zero-retry gate. | Fits (11, §1.3) | 14 |
| N08 | Interactive agent priority lane & fair queueing | `velnor-dispatch` uses 5 tiers: `merge > gate > agent-interactive > PR > batch`, keeping reserved merge/A1 slots untouchable; per-agent token bucket (max 1 interactive build slot/agent) + 90 s auto-demotion to `PR`. | Cuts interactive agent P90 wait under 70–95% load (~19× queueing [M, N38]) from ~2–5 min to ≤ 5–10 s [E], while keeping merge-queue P90 wait ≤ 10 s [T]. | Priority affects admission order only, never sandbox isolation, fixed 16-CPU key, or witness quorum. Fallback: batch preempted first at 109.5 GiB watermark; saturated interactive work queues or runs local Docker. | Fits (15, §9) | 10 |
| N09 | Cryptographic signing & attestation of local/remote receipts | Out-of-sandbox Ed25519 `velnor-signer` issues DSSE receipts under two disjoint CAs: Cluster CA (**T2/T3**, node+class keys, transparency log) and Workstation hardware key (Secure Enclave/TPM, **T1**). | Eliminates manual provenance checks at gate (~50 µs Ed25519 verify [E]); stops buggy agents or stale worktrees from poisoning shared local/cluster state. | Compile-time distinct `ClusterReceipt` vs `LocalReceipt` types: `velnor-control` rejects T1 keys structurally. Signer compromise bumps epoch and revokes open heads (§6.5). Fallback: invalid signature → miss → re-run. | Fits (8, 11, §6.5) | 14 |
| N10 | Lookup-only PR & merge-group verdict path | Agent dev runs fill Slot 1 (N02), `velnor verify` against `merge-tree(queue_tip, HEAD)` fills Slot 2 before push; on `labeled velnor:verify` and `merge_group`, `velnor-control` validates receipts and posts `Velnor / Verified`. | Drops label-to-green P50 from ~35 s (goal) / ~6 min (day 5.2) to **~2–5 s** [E] (when C5 = 0), pushing push-to-merge P50 to **~50–55 s** (~45 s GitHub MQ floor [M] + lookup). | Posts `success` only when every obligation of `merge-tree(queue_tip, HEAD)` has 2 valid T2/T3 cluster witnesses + fresh C5 runs + exact test IDs (C1–C5). Fallback: queue tip moved or C5 present → `ensure` runs exact delta. | Fits (7, 10, 11, 12) | 10 |
| N11 | Structured failure packets & fail-fast sibling cancel | `velnor-exec`/`velnor-node` stream NDJSON failure packets (failing test IDs, rustc JSON spans, `velnor repro <id>`, flake history) on first red from fast-red or either witness, cancelling unshared sibling tasks. | Cuts agent fix loop by 30–90 s/red (no waiting for full suite or scraping raw logs) and saves ~50–80% of doomed witness CPU on broken commits [E]. | Fast-red and failure packets can only abort or speed up reds; they can never fill a passing slot or write `success` (C4, §6.2). Fallback: raw stderr tail streamed if JSON parse fails. | Fits (11, 15, §6.2) | 10 |

**N01 — `velnor` CLI, Cargo shim & `velnor-agentd`.** Agents already call `cargo nextest run`,
`cargo clippy`, `cargo doc`, and `cargo fmt --check` dozens of times per PR; asking them to remember
a separate CI command invites drift and repeated work. A thin `PATH`-prepended shim (installed in the
agent shell environment alongside explicit `velnor verify` and `velnor check`) forwards invocations to
a persistent per-host daemon `velnor-agentd`. When an invocation matches a canonical obligation in the
base commit's released inventory (e.g., `cargo nextest run --profile ci --locked --offline` over a
clean or snapshotted tree), `velnor-agentd` translates it into a Velnor obligation request backed by
`velnor-ci obligation exec`. Non-canonical calls (single-test debug filters, `--nocapture`, custom
`RUSTFLAGS`, or dirty edits) run in non-minting **T0** mode so agents keep full flexibility while the
ledger never ingests uncanonical flags. Because the shim is an agent-side front-end rather than a
legacy CI compatibility layer, it honors Decision 18 while turning routine agent checks into CI proof.

**N02 — Remote-by-default on warm cluster nodes vs. local sealed sandboxes.** Running builds locally on
an Apple Silicon Mac across 12 parallel agent worktrees thrashes local SSDs (60–120 GiB of duplicated
`target/` trees [E]), triggers thermal throttling, and still produces zero gate-eligible `linux/amd64`
evidence. Under remote-by-default, `velnor-agentd` computes the diff against the latest `main` commit
already cached in the cluster's bare mirror (§7.4), ships a ~5–50 KiB compressed delta over a persistent
QUIC/WireGuard stream to the home site (`velnor-dispatch` → `velnor-node`), and executes inside a
pre-warmed Hetzner XFS reflink sandbox (0.25 s clone + ~15 s warm compile on 16–64 EPYC threads [M/E]).
Stdout and stderr stream back interactively over ~35–120 ms RTT, indistinguishable from a local shell
except that builds run faster and mint a real **T2** cluster receipt. Local sealed Docker sandboxes
(`velnor-platform-docker` via Docker Desktop/OrbStack) stay available for `--local`, offline work, and
conformance testing, but because they lack `linux/amd64` fidelity and eBPF read audits (`read_audit:
false`), they serve fast-red feedback only and never count toward `Velnor / Verified`.

**N03 — Four-tier receipt trust ladder & witness quorum.** Velnor needs a hard boundary between "useful
during dev" and "allowed to land on `main`." Every receipt carries one of four provenance tiers: **T0**
(raw host / incremental / ad-hoc, never stored), **T1** (local sealed Docker container signed by a
workstation hardware key, usable for local worktree dedupe and fast-red hints only), **T2** (remote
Hetzner sealed sandbox on the released inventory signed by a node signer daemon), and **T3** (T2
promoted after landing on `main` and passing cold Audit A1 with matching binary digests). Gate
admission (`Velnor / Verified`) requires **2 independent T2/T3 witnesses** on distinct cluster nodes,
attempts/test orders, and generations (or one cold), preserving §1.2's `1 - (1 - p)^2` detection rate.
During normal coding, an agent's remote run fills **Slot 1**, leaving only Slot 2 for push/label;
running `velnor verify --quorum` before pushing fills both slots so push+label executes nothing.
**Owner question (A1):** Could a PR head ever pass with 1 T2 cluster witness + 1 T1 local Linux-VM
witness (deferring the 2nd cluster witness to the merge queue) to save ~6 runner-min/PR [E]?
*Recommendation: No*—local Macs lack eBPF read audits and run `arm64`, breaking C2/C4 parity; keep 2
T2 cluster witnesses mandatory.

**N04 — Solving the `arm64` Mac vs. `linux/amd64` cluster mismatch.** Apple Silicon Macs run Darwin
`arm64` (and local Linux containers run `linux/arm64`), while all five Hetzner workers run Debian 13
`linux/amd64` (kernel `6.12.94+deb13-amd64`) and macOS releases are paused (Decision 9). Emulating
`linux/amd64` locally via Rosetta or QEMU is 3–10× slower and introduces memory-ordering and syscall
divergences incompatible with zero false greens; sharing keys across architectures would let APFS
case-insensitivity or `cfg(target_arch)` bugs slip past CI. Velnor solves this by binding `(os, arch,
kernel, cpu_features)` into every `ExecutionFingerprint` (§6.1) so `arm64` and `amd64` keys never
collide, and by making remote `linux/amd64` execution (N02) the default for agents. When an agent runs
locally (`--local` or offline), Velnor runs fast-red checks natively on `arm64` to catch syntax,
borrow-checker, and logic errors quickly, tagging output explicitly as `T1 (arm64, non-minting)` so
neither agent nor gate confuses it with `linux/amd64` evidence. If Decision 9 is later lifted and macOS
legs return, `arm64` simply becomes a second obligation vector in its own disjoint keyspace.

**N05 — Continuous verification of committed-but-unpushed work (Owner question — conflicts with
Decision 12).** Even with remote-by-default testing, agents often make cleanup commits, address review
feedback, or rebase locally before pushing and labelling. A `velnor-watch` loop inside `velnor-agentd`
(watching Git `post-commit` / ref updates with a 15 s debounce) can compute `merge-tree(queue_tip,
HEAD)` and submit speculative **batch-priority (`cpu.idle`)** T2 verification for committed-but-unpushed
trees, cancelling immediately on a new commit. **Conflict with Decision 12 (extends D19):** Decision 12
binds CI to on-demand `velnor:verify` labelling because 45% of PR runs were cancelled [M], and D19
already recommended against pre-label speculation on pushed PRs. *Gain at stake:* removes 15–35 s (goal)
to 3–6 min (day 5.2) from label-to-green wall time (yielding a ~2–4 s lookup even if the agent skips
`velnor verify`), at an estimated cost of ~20–45% wasted preemptible batch CPU (~5–15 runner-min/PR
[E]) when agents amend or squash commits. **Owner question (A2):** Allow opt-in `velnor-watch` for
committed-but-unpushed work strictly at `cpu.idle` batch priority (never writing `Velnor / Verified`
until labelled, capped per agent and paused above 60% cluster CPU)? *Recommendation:* Keep Decision 12
as default; rely on explicit N02/N10 first, and gate opt-in `velnor-watch` on measured idle headroom
after day 10.

**N06 — Cross-agent ledger querying & in-flight deduplication.** When 12 builders and 4 reviewers run
concurrently on `velnor-new`, they repeatedly re-verify the same `main` tip, shared lower commits of
stacked PRs, and untouched Cargo roots (`generator/`, `ci-helper/`, `local/`, `crates/velnor-runner`,
`executor/`). Integrating `velnor ledger query` into N01's shim lets `velnor-agentd` hash the local
tree (and enrolled per-root subtrees) via the released `velnor-ci` planner before launching any
sandbox. If `velnor-ledger` already holds 2 passing, unrevoked, active-epoch T2/T3 receipts for a key,
the agent gets an instant (< 0.5 s) verified answer; if another agent or PR is currently executing that
key, `velnor-agentd` joins the live log stream instead of spawning a duplicate build (§6.2). If another
agent already hit a failure on that key (e.g., a broken `main` test), the query surfaces that failure
immediately as an advisory warning while still allowing a fresh run (failed records are never reused for
verdicts). C5 obligations (clock/network/random) always return `FRESH_REQUIRED`.

**N07 — Shift-left flake detection during development.** Today's 8.1% per-run false-failure rate [M]
becomes 15.5% under two witnesses and triggers expensive merge-queue ejections that rebuild every entry
queued behind the failure. Moving agent execution onto the shared ledger turns development into a
continuous flake detector: whenever Slot 1 (an agent's remote run) and Slot 2 (a second witness or peer
agent's run) disagree on the exact same key, `velnor-ledger` immediately fails the record (zero retries
remain binding, §1.3), logs the disagreement with test ID, node, seed/order batch, and CPU load to the
flake telemetry stream, and hands the agent an exact repro command before the PR is opened. For tests
an agent edits or adds, `velnor verify --stress N` (default N = 5, `cpu.idle` batch priority) fans out N
independent seeded executions across idle nodes; any single failure blocks locally and emits a failure
packet, driving real flake rates toward §1.3's 1% target before code reaches the queue.

**N08 — Scheduler priority for interactive agent commands.** Under §9's four-tier scheduler (`merge >
gate > PR = other orgs > batch`), synchronous agent commands (`velnor verify` or remote `cargo nextest`)
would either starve behind background PR queues or compete unfairly with the merge queue. Inserting an
**`agent-interactive`** tier (`merge > gate > agent-interactive > PR > batch`) protects LLM agent turn
latency without touching the reserved merge lane (1 × 16-CPU build slot + 2 gate CPUs + 1 A1 audit slot
per site, ~7.7% CPU, §9). When an agent waits synchronously on a fast-red or Slot 1 build,
`velnor-dispatch` admits it ahead of async PR/batch traffic, cutting P90 wait under 70–95% load from
~2–5 min to ≤ 5–10 s [E]. Two hard guardrails prevent 12 parallel agents from hogging all 20 build
slots: (1) weighted fair queueing caps each agent identity to **1 concurrent interactive build slot**
(Slot 2 witnesses run at `PR` priority unless `--quorum` runs on a lightly loaded site), and (2) any
interactive task exceeding 90 s auto-demotes to `PR` priority.

**N09 — Cryptographic signing & attestation of local and remote receipts.** Even with trusted authors
(Decision 8), unsigned receipts invite silent corruption from buggy agent scripts, stale worktree state,
or mismatched toolchains. Velnor adopts a DSSE-envelope receipt signed by out-of-sandbox Ed25519 keys
with strict root-of-trust separation. On Hetzner workers, the node signer daemon (§6.5) signs
**ClusterReceipts (T2/T3)** using short-lived node+class keys chained to the site Cluster CA, binding
`key`, `tree_oid`, `obligation_id`, `inventory_version`, every executed binary's SHA-256 digest
(`CARGO_BIN_EXE_*` included), `execution_fingerprint`, `executor_id`, `generation_id`, `witness_slot`,
`epoch`, and exact expected test IDs, anchored in the site's append-only log. On developer Macs/Linux
hosts, `velnor-node` signs **LocalReceipts (T1)** using a hardware-backed device key (macOS Secure
Enclave / Linux TPM). In Rust, `ClusterReceipt` and `LocalReceipt` are distinct types: `velnor-control`
cannot accept a `LocalReceipt` when computing `Velnor / Verified`, while T1 signatures still give local
multi-worktree dedupe and peer bug reports tamper-evident provenance.

**N10 — How the PR verdict becomes mostly a lookup.** Combining N01–N09 turns the PR gate from a slow
batch job into a cryptographic lookup. During a task, an agent's remote test runs populate Slot 1 T2
receipts. Before pushing, the agent runs `velnor verify` (which computes `merge-tree(queue_tip, HEAD)`
via the site mirror), reusing every unchanged root and Slot 1 receipt while executing only missing Slot
2 witnesses—and critically, this delivers an exact-tree hit **even on day 5.2 with whole-tree keys**,
before per-root enrolment graduates. When the agent pushes, enables auto-merge, and adds `velnor:verify`,
`velnor-control` receives the webhook (~1–2 s), computes `merge-tree(queue_tip, HEAD)` (~0.5 s), checks
the two signed T2 receipts per obligation (~50 µs each), runs any remaining C5 obligations, and posts
`Velnor / Verified = success` in **~2–5 s** (once C5 clock reads are injected and keyed, clearing
today's 5 clock-reading test files [C]). When GitHub auto-enqueues the PR, the merge group matches the
already-verified `merge-tree` (~60–75% of groups [E]), turning the group check into a second 2–5 s
lookup and pushing push-to-merge P50 to **~50–55 s**—within ~5–10 s of GitHub's ~45 s queue floor [M].

**N11 — Agent-native structured failure packets & fail-fast streaming.** Human CI UIs dump megabytes of
ANSI text and force agents to poll, download archives, and regex-parse errors. Because AI agents consume
95%+ of CI output here, `velnor-exec` and `velnor-node` capture `rustc --message-format=json` and
`cargo-nextest` structured events directly, emitting a compact NDJSON **Failure Packet** (failing test
IDs, compiler spans, exact `velnor repro <receipt-id>`, diff against `main`'s last known state, and N07
flake history) on the first failure from either the non-minting fast-red lane (§6.2) or either witness.
`velnor-agentd` prints the packet directly to stdout/stderr and `velnor-control` attaches it to
`Velnor / Details`; simultaneously, `velnor-dispatch` cancels unshared sibling obligations for that
request. Agents start fixing 30–90 s earlier per failed attempt while shedding ~50–80% of doomed
witness CPU [E], with zero impact on green soundness because fast-red can never fill a passing slot.

## Risks

| id | false-green risk | waste / degradation risk | containment |
|---|---|---|---|
| N01 | Shim drops a flag or tests a stale dirty tree while claiming canonical coverage. | Shim overhead or broken UX causes agents to bypass Velnor and run raw local `cargo`. | Shim verifies exact canonical argv + clean tree OID against released inventory; mismatches downgrade to **T0** (unminted). Keep overhead ≤ 100 ms; bypasses still verified at CI gate. |
| N02 | Delta upload corrupts/omits a file vs. real Git commit, minting a receipt for the wrong tree. | 12 agents spamming remote builds saturate the 20 cluster build slots and evict XFS page cache (14.29 GB/clone [M]). | Node hashes the full staged tree and binds the exact Git tree OID into the receipt (§7.4); wrong trees cannot hit at push. Per-agent slot cap + N08 priority + local fast-red fallback protect cluster. |
| N03 | T1 local receipt leaks into a gate witness slot, or one node fills both slots. | Agents run `--quorum` on every tiny edit, doubling cluster CPU unnecessarily. | Distinct Rust types (`ClusterReceipt` vs `LocalReceipt`) + ledger invariant asserting `node_1 != node_2` and `attempt_1 != attempt_2`. Default agent runs fill Slot 1 only; `--quorum` is explicit pre-push. |
| N04 | Static/doc check passes on `arm64` Mac and is mistakenly reused for `linux/amd64` gate. | Agents burn Mac battery running local `arm64` builds that must still re-run on `amd64`. | `ExecutionFingerprint` binds `(os, arch, kernel, cpu_features)`; ledger forbids cross-fingerprint hits. Default N02 runs remotely on `amd64` so local Macs compile only when offline. |
| N05 | Speculative run on unpushed commit mints a verdict before label, violating D12. | Agents amend/squash rapidly, wasting 20–45% cluster CPU on discarded trees. | `velnor-watch` writes **T2 candidate** records only and never touches `Velnor / Verified`. Runs strictly at `cpu.idle` batch priority, debounced 15 s, cancelled on new commit, gated on Owner Question A2. |
| N06 | Stale/revoked record or C5 clock-dependent test served as a hit to another agent. | Ledger contention under 12 parallel agents adds query latency to every command. | Ledger verifies active epoch, non-revocation, and `C5 == false` on every lookup; read-only queries hit site-local ledger replicas (< 5 ms LAN). Degraded mode returns `MISS` (slower, never wrong). |
| N07 | Flake detection auto-retries a red and turns a real bug green. | Excessive `--stress N` sweeps starve cluster build slots. | Zero retries remain hard-coded on every minting path; stress runs write only to telemetry, never to witness slots, and execute strictly at `batch` (`cpu.idle`) priority with a cap on N. |
| N08 | Priority inversion starves Audit A1 or merge-queue witnesses, delaying revocation or landing. | One runaway agent loop monopolizes the interactive tier. | Merge lane (16 CPU + 2 gate CPU) and A1 slot are strictly reserved (§9); per-agent token bucket caps each agent at 1 interactive build slot with 90 s auto-demotion to `PR`. |
| N09 | Compromised workstation key or buggy agent forges a gate receipt. | Key rotation or transparency-log outage blocks all agent runs. | Workstation keys belong to T1 CA only (structurally rejected by `velnor-control`); Cluster CA keys live in out-of-sandbox node signers; epoch bump revokes bad signers (§6.5). Fallback: run fresh at gate. |
| N10 | `Velnor / Verified` written on a stale `merge-tree` after `main` advances, or skips C5 obligations. | Frequent `main` churn (42–71 merges/day [M]) lowers `merge-tree` hit rate, forcing post-label delta runs. | `velnor-control` recomputes `merge-tree(queue_tip, HEAD)` at webhook time, binds status to exact head SHA + active epoch, and always executes C5 obligations fresh. Misses degrade cleanly to v3's ~35 s path. |
| N11 | Fast-red lane (`CARGO_INCREMENTAL=1`) reports green and short-circuits a minting witness. | Over-eager sibling cancellation kills a shared obligation needed by another agent. | Fast-red is non-minting by type and can only post to `Velnor / Details` or cancel unshared sibling work; shared ledger records cancel only when their last live claimant drops (§6.2). |

**Systemic compound risks: C5 tainting and cluster page-cache ceiling.** Lookup-only verdicts (N10)
stall whenever even one C5 obligation (like `velnor-ci`'s `SystemTime::now()` tests [C]) forces a
10–30 s fresh run on every lookup, and 12 remote-by-default agents can hit the 4–6 sandbox/worker
page-cache ceiling (14.29 GB per XFS reflink clone [M]) before Lab B2's overlayfs sharing lands.
Containment: inject a keyed deterministic clock to bring C5 to zero on the fast class, enforce the
1-slot/agent interactive cap (N08), and fall back automatically to local Docker fast-red when cluster
memory exceeds 80%.

## The first three steps

Ordered to deliver the lookup-only PR experience as soon as v3's exact-tree ledger exists (day 5.2),
**without waiting for per-root key enrolment (day 10) or stretch milestones (days 16–18)**:

1. **Step 1 — Ship Remote-by-Default `velnor verify` & Trust-Tiered Receipts (N02 + N03 + N09,
   ~36 agent-hours).**
   Promote work item `V11` from stretch (days 16–18) to immediately after exact-tree ledger records land
   (day 5.2, aligning with `G04`). Implement the delta-sync client in `velnor`, the remote `ensure`
   endpoint on `velnor-dispatch`/`velnor-node` (streaming logs over QUIC/WireGuard into warm
   `linux/amd64` XFS sandboxes), and the compile-time `ClusterReceipt` (T2/T3) vs. `LocalReceipt` (T1)
   split in `velnor-ledger`.
   - *Value unlocked:* Even with day 5.2's whole-tree keys and cold second witness, an agent running
     `velnor verify --quorum` before pushing moves the entire ~6 min verdict off the PR critical path,
     turning `labeled velnor:verify` into a **~3–5 s lookup** [E] and immediately eliminating `arm64`
     vs. `amd64` surprises.
   - *Exit gate:* 50 consecutive pre-flighted agent PRs hit ≥ 95% exact-tree lookup at `labeled` (when
     the queue tip is unchanged), 0 T1 receipts accepted by `velnor-control` in negative tests, and
     delta upload + full-tree checkout hash ≤ 1.5 s P90.

2. **Step 2 — Add the Agent Cargo/Nextest Shim, Cross-Agent Ledger Query & Interactive Priority Lane
   (N01 + N06 + N08 + N11, ~48 agent-hours).**
   Build `velnor-agentd` with the `PATH` shim mapping canonical `cargo nextest` / `cargo clippy` /
   `cargo fmt --check` calls to `velnor ledger query` + remote Slot 1 execution, backed by the 5-tier
   priority queue (`agent-interactive` with 1-slot/agent token bucket and 90 s auto-demotion) in
   `velnor-dispatch`. Wire `velnor-exec`'s structured NDJSON failure packets and fail-fast sibling
   cancellation into the stream.
   - *Value unlocked:* Removes the friction of invoking `velnor verify` manually—routine agent test runs
     automatically populate Slot 1 receipts, deduplicate ~50–75% of redundant cross-agent baseline
     checks [E], cut interactive wait under load to ≤ 5–10 s [E], and free Apple Silicon Macs from
     12-worktree `target/` thrashing.
   - *Exit gate:* 12-agent concurrent replay at 80% cluster load holds merge-queue wait P90 ≤ 10 s,
     interactive agent P90 wait ≤ 10 s, ≥ 40% cross-agent ledger hit rate on stacked/shared bases, and
     100% of non-canonical/dirty shim invocations downgraded to T0.

3. **Step 3 — Drain C5 Clock Tainting & Wire Shift-Left Flake Detection (N07 + N10 completion,
   ~24 agent-hours).**
   Replace `SystemTime::now()` in the 5 `velnor-ci`/`velnor-gen` test files and `velnor-ci`'s
   `now_seconds()` (`io/mod.rs:210`) with an injectable clock fixed and recorded in the obligation key
   (keeping nightly Audit A2 at +8 d and +30 d to catch date bombs), and enable `velnor-ledger`'s
   same-key disagreement telemetry plus `velnor verify --stress N` at `cpu.idle` batch priority.
   - *Value unlocked:* Removes the last blocker to true 2–5 s lookup-only verdicts on rebases,
     pre-flighted PRs, and exact-tree merge groups (driving push-to-merge P50 to ~50–55 s [E], ~5–10 s
     above GitHub's queue floor), while surfacing flaky tests during agent development before they eject
     merge-queue batches.
   - *Exit gate:* C5 obligation-seconds = 0 on the `generator/`/`ci-helper/` class, exact-tree lookup
     verdict ≤ 5 s P50 / ≤ 10 s P90 across 100 live PRs, and 100% of seeded flaky tests (p = 0.05–0.2)
     surfaced in flake telemetry on disagreement with zero false greens.
