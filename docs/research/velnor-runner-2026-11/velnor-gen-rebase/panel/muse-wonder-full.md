# velnor-gen rebase by Muse Wonder Full

| Field | Value |
|---|---|
| Model | Muse Wonder Full (`muse-wonder-full-1.0`) |
| Client | Muse Code 1.4.20 (muse exec) |
| Reasoning effort | max |
| Date | 2026-11-20 |
| Main tip analysed | `0eb2d76a5c` |
| Brief | [velnor-gen rebase brief v1](../briefs/velnor-gen-brief-v1.md) |

Independent answer from the Velnor runner research panel, written without seeing other members' answers. Unedited except for this header and replacing home-directory paths with `~`.

---

# Rebase the Velnor plan on velnor-gen (panel member report, 2026-11-20)

**Base.** `tailrocks/velnor-new` main tip `0eb2d76a5c` (2026-11-20), read-only checkout `~/.cache/ci-ambition/main-tip`; plan base `95de9b704b`. Reports `regen/A`–`E` were re-checked against the checkout; two live facts were re-read with read-only `gh` calls (open PRs, latest runs). Citations are `path:line` or a commit hash; `R` = `.github/workflows/rust-ci.yml` (666 lines). Brief correction verified: `57b65af0db` (#1747) deleted `velnor-actions-*` (18 members remain, `Cargo.toml:5-24`); `qualification.yml:1` is velnor-gen-generated from `.velnor/workflows.toml:1-2`; no `qualification-gen.yml`, `rust-ci-timings.json` or `release.toml`; `.velnor/config.toml` survives only as `[stacks.rust]`; #1274 (`61067c17fc`), #1335 (`47ef96f8ea`), #1748 on main. Open PRs (live): #1608, #1200, #1026, #1297, #1751; #1320 CLOSED unmerged (#1073 orphaned). Tip push run `44584861409` failed: main is still red.

## 1. Verdict in ten lines

1. **The final design still stands on velnor-gen — its centre is stronger, its sequence is
   wrong.** The ledger, the label flow, two witnesses, build-once-promote and the App gate
   (`docs/proposed/velnor-runner-architecture.md` §6, §8.2, §8.4) all survive, because
   velnor-gen already contains their hard parts as typed Rust shared between generation
   time and run time: selection (`crates/velnor-rust/src/select.rs`), the plan
   (`crates/velnor-rust/src/plan.rs:41`), cache keys (`crates/velnor-model/src/cache.rs:95`)
   and the verdict (`crates/velnor-rust/src/verdict.rs:278` `decide`).
2. **The single most important change to the plan: move the verification contract into the
   generator.** `velnor-gen` must emit the obligation inventory and the verdict-key vector
   from `RepoFacts` at plan time, digested and freshness-checked like `rust-ci.yml` itself
   (`crates/velnor-render/src/header.rs:23-45`); `velnor-ci plan` then resolves that vector
   against the ledger at run time. The ledger becomes a cache of generator obligations,
   never a second planner. Generator owns meaning; runner owns speed.
3. **Phase 0 is no longer a ruleset edit and two engine releases.** Engine releases are
   gone with #1747. Phase 0 is now: green main, an honest `Required`, a revived queue.
   Nothing else can be measured or gated until those three hold (section 6).
4. **The gate is currently not gating, and that outranks every speed item.** Post-cutover
   `Required` failed on 226 of 268 PR runs and 157–167 of ~173 merges landed with a failing
   `Required` (admin bypass inferred, `regen/C` §1, §4); the tip's own push run failed.
   Correctness first (owner decision 11) means the next work is repair, not executors.
5. **The as-built label flow inverts owner decision 12 on the decisive point.** An
   unlabelled ready PR and a draft both get a GREEN `Required` (`docs/rust-ci.md:84-85`;
   `R:584`, `R:608-609`), so an unlabelled PR can merge without tests. Fixing this inside
   velnor-gen is the first generator change of the rebased plan (sections 3, 6).
6. **One reuse mechanism already exists and is off.** `velnor-ci merge-group-passed`
   (`crates/velnor-ci/src/merge_group.rs:46`) plus `[ci] skip_push_after_merge_group`
   (`generator/CONFIG.md:26-39`) is the design's "reuse only on proof, else run everything"
   in shipped form. Revive the queue, turn it on, measure it: that is the first ledger,
   and it needs no new crate.
7. **The Native executor stays the goal but leaves the critical path.** On the new
   baseline the hosted queue is already gone (scale-set queue P50 2–13 s, `regen/C` §3.3)
   and the long pole is compilation in the single `rust` job (`R:34`). An executor saves
   seconds per Actions job; verdict reuse saves whole runs. Order: correctness → reuse
   inside Actions → App status → Native executor (sections 4, 5).
8. **The `executor/` Cargo root and per-root keys are dead.** #1290/#1594 collapsed the
   repo to one workspace (`Cargo.toml:5-24`) and #1747 removed the reason for a separate
   root. New code goes in `crates/velnor-*`. Keys are per-package from the start, split
   into a compile key and a verdict key because HD11 compiles `--workspace` while tests
   run selected (`docs/hard-decisions.md:486-512`).
9. **One owner decision blocks roughly a third of the plan and must be asked as one
   pack:** rewriting `docs/goals.md:16` ("Jobs are always executed by GitHub's official
   runner") and its dependent lines to admit two executors behind one interface. Exact
   wording is in section 7. Until then, X/O/P/ST/F items stay gated; everything in
   sections 3 and 6 proceeds without it.
10. **What velnor-gen makes possible that the old design did not imagine:** the same
    `WorkflowPlan` driving Actions runs, `velnor-local` scenarios and future Native
    obligations; verdict keys computed by `velnor-ci plan` before any step runs; and a
    generator-owned benchmark where `velnor-ci timings` feeds the shape decision that
    `crates/velnor-rust/src/shape.rs` already writes into the workflow. Section 8 develops
    this.

## 2. The design mapped onto velnor-gen

### 2.0 The pipeline the design must plug into

Generation is pure except `io`: `RepoView` → `detect()` → `RepoFacts` → `plan()` →
`WorkflowPlan{Job{Step{StepKind}}}` → render → `check_rules` → `seal()`
(`crates/velnor-gen/src/lib.rs:77-105`; `crates/velnor-rust/src/plan.rs:41`;
`crates/velnor-render/src/lib.rs:72-122`). Run time is one binary: every generated `run:`
is one `velnor-ci …` command, one of ~45 subcommands (`crates/velnor-ci/src/cli.rs:35-175`),
installed from a digest-pinned release (`.velnor/rust-ci.lock`; `R:71-82` re-verifies the
binary SHA-256 on every invocation). Three consequences shape everything below:

- **A new behaviour needs four coordinated edits, never one:** config key
  (`crates/velnor-rust/src/config_keys.rs:4-38`, unknown keys rejected at
  `crates/velnor-rust/src/config.rs:75-77`), model type (`crates/velnor-model/src/`),
  planner (`crates/velnor-rust/src/`), renderer (`crates/velnor-render/src/`), plus a
  helper-version floor in `crates/velnor-gen/src/helper_gate.rs:8` when a new `velnor-ci`
  command is required. The precedent chain for `skip_push_after_merge_group` (config →
  `Step.skip_after_merge_group` → `merge_group_skip.rs` → gate exception) is the template
  for every ledger step.
- **The render rules are already the design's "no logic in YAML" as code:** 16 SHA-pinned
  actions (`crates/velnor-render/src/rules.rs:16`), each `run:` one simple command of at
  most 2,000 characters (`rules.rs:38`), workflow at most 500,000 bytes (`rules.rs:39`).
  Anything the design wants in a workflow must fit those rules or change them deliberately.
- **The plan and the gate already come from a released helper** (goals rule 12,
  `docs/goals.md:76`): the PR under test cannot change the `velnor-ci` that plans or gates
  it. The design's C3 requirement (inventory from a released helper and the base commit,
  architecture §1.2) is therefore half-built; what is missing is the inventory itself.

### 2.1 Verification ledger and verdict keys

| Decision | Home | Extend exactly |
|---|---|---|
| Obligation inventory (versioned, released) | **Generator-time** | New module in `velnor-rust` beside `plan.rs`: derive obligations from `RepoFacts` + `RustCiConfig` (`crates/velnor-model/src/repo.rs`); new types in `velnor-model` next to `VerdictSpec` (`crates/velnor-model/src/verdict.rs:64-75`) and `RustSelection` (`crates/velnor-model/src/selection.rs:45-84`). Emitted as a generated, digested file beside `.velnor/rust-metadata.json` (same `seal()` discipline, `header.rs:23-34`). |
| Verdict key (canonical struct, DG01) | **Generator-time type, run-time value** | Key struct in `velnor-model` next to `CacheKey` (`cache.rs:95`); field values computed in `velnor-ci plan` (`crates/velnor-ci/src/io/plan.rs:37-90`) from the same inputs `prepare` already hashes for `compat` (`crates/velnor-rust/src/compat.rs`; `docs/rust-ci-cache-keys.md:67`). `velnor explain` reads the struct, never a string. |
| Lookup / join / witness scheduling | **Run-time** | New `velnor-ci` subcommand after `select_event` (`crates/velnor-rust/src/select_event.rs:12-44`); a hit turns `RustSelection::{Some,All}` into reused only with digest equality; new `SelectionReason`/`AllTrigger` variants so `--explain` names it (`selection.rs:73-84`). Per-step skip generalises `Step.skip_after_merge_group: Option<PlanFrom>` (`crates/velnor-model/src/plan.rs:110`). |
| Gate accepting "reused" | **Run-time** | `velnor_rust::decide` (`verdict.rs:278`) gains a reused outcome only with evidence outputs; `tests_expected` (`crates/velnor-rust/src/required_job.rs:152`) and the `tests_run/listed/ignored/by_package` outputs must still match. Today the sole reused case is `push && skip_push == 'true'` (`generator/CONFIG.md:37`). |
| Ledger storage, epochs, signing | **Runner-side (new crate `crates/velnor-ledger`)** | Library linked by `velnor-ci` (in-Actions lookups read through an HTTPS endpoint with the job token; writes only from trusted events, the `mbx_store::trusted_event` rule, `crates/velnor-ci/src/mbx_store.rs:28-37`) and by the future node agent. No second Cargo root. |

**Free from velnor-gen:** content addressing below the verdict already exists and must be
reused as key fields before inventing new digests — `compat` (16 hex of SHA-256 over
rustc/target/profile/image/workspace/driver), `lock_root` (lockfile hash ignoring
workspace-own versions, helper ≥ 0.1.86, `docs/rust-ci-cache-keys.md:20`), `inputs
save/restore` blob ids + mtimes (`docs/rust-ci-cache-keys.md:38`), delta source identity
(`docs/rust-ci-cache-keys.md:55-57`), helper archive+binary SHA-256 (`.velnor/rust-ci.lock`),
and the generated-file body digest (`R:2`). **Split the key in two, which E misses:** a
*compile key* (whole-workspace by construction, HD11: clippy/test-build/doctest/doc run
`--workspace`, `docs/hard-decisions.md:488-492`) and a *verdict key* (per-package test
execution, narrowed by `-E`). A single per-package key would claim a compile reuse that
HD11 forbids; a single whole-tree key would throw away the selection `select()` already
proves. Compile reuse graduates on digest equality of binaries (architecture §7.3);
verdict reuse graduates on the 3,000-comparison shadow (§6.4). They are separate reuse
classes with separate epochs.

### 2.2 The `Velnor / Verified` App status

**Runner-side, new crate `crates/velnor-control`** (webhook receiver, label state machine,
fenced status writer, reconciler — architecture §8.2). Nothing of it exists: no App, no
webhook receiver, no commit-status write anywhere in `crates/` (`velnor-ci release-gate`
only reads statuses). Two generator-side obligations come first, and they are cheap:

- **Name discipline.** Until the App ships, `Required` IS the gate (`required_check =
  "Required"`, `.velnor/rust-ci.toml:5`; gate at `R:608-609`). The transition must not
  rename it: `velnor-ci release-gate` and the ruleset both read `Required`. The App adds
  `Velnor / Verified` as a second required context (shadow → required → sole), exactly the
  design's steps 2–4 (architecture §8.2), and only then does the Actions `Required` job
  shrink to the push/A1 reporter. Renaming first would break the only working gate.
- **The inventory the App judges** is the generator-emitted vector of §2.1, read from the
  base commit. `velnor-control` never plans; it resolves keys. That keeps goals rule 12
  (plan from a released version) true after PR/group Actions runs disappear.

**Free:** `Velnor / Details` has a precursor — `velnor-ci report`'s `ci-metrics.json`, rendered by the `Cache` check (`R:627-666`). Extend that schema with reused/run/fresh counts; invent no new format.

### 2.3 Label flow

**Generator-time, entirely inside velnor-gen.** The machinery is `PrRun`
(`crates/velnor-model/src/plan.rs:120-127`: Always/Ready/Quick/Related/Requested/
Unrequested), `CI_RUN_LABEL`/`CI_AUTO_LABEL` (`plan.rs:115-116`), `quick()`/`full()`
(`crates/velnor-rust/src/plan.rs:330-343`), the trigger/draft rendering
(`crates/velnor-rust/src/plan_budget.rs:49-70`) and the `Required` job builder
(`crates/velnor-rust/src/required_job.rs`). The rebased flow (section 3, gap 2) needs:

1. A `Required` job that **fails** with "CI not requested" on a non-draft PR without the
   label and on a draft, instead of passing (`R:584` notice today). A skipped required
   job passes on GitHub, so failure is the only honest encoding — the design says the
   same (architecture §8.2 transition).
2. `unlabeled` added to the `pull_request` types (today: opened, synchronize, reopened,
   ready_for_review, edited, labeled — `R:5-12`), cancelling the in-flight run; a written
   verdict stays true for its commit (ruling Q9).
3. One label, not two. `ci:auto` (every push, cancel previous — concurrency at `R:22-24`)
   already implements owner steps 3–4; `ci:run` (one shot, auto-removed by
   `ci-run-label`, `R:613-626`) contradicts step 4 and should be retired. Rename to the
   owner's `velnor:verify` or keep `ci:auto` — owner question Q-N2, recommendation in §7.

The quick set may keep running unlabelled for fast feedback; only `Required`'s honesty changes. Agents must label with an App/user token (`GITHUB_TOKEN` changes trigger nothing) — already true today.

### 2.4 Reuse across agent run → PR → queue → main → release

Ordered by increasing machinery; each step is independently shippable and measurable:

| Hop | Mechanism | Home | Status |
|---|---|---|---|
| Queue → push | `merge-group-passed` + `skip_push_after_merge_group` | Run-time (`crates/velnor-ci/src/merge_group.rs:46-86`; steps skipped per `crates/velnor-rust/src/merge_group_skip.rs:8-55`) | **Built, off.** No `[ci]` table in `.velnor/rust-ci.toml`; no `merge-group-passed` in `R`. Needs helper ≥ 0.1.82 (`helper_gate.rs`); pinned helper is 0.1.95 (`4fc0ede34e`). |
| PR push N → N+1 | PR delta layer (#1107, `7f16e1fe15`): cumulative per-PR `target.velnor-delta`, base-key-bound (`docs/rust-ci-cache-keys.md:53-63`) | Run-time (`velnor-ci delta {prepare,apply,stage}`) | **Built, unmeasured** ("No timing measured", #1742). Compile reuse only, correctly isolated per PR (goals rule 8). |
| PR → queue (same tree) | Exact-tree verdict record | Generator key vector (§2.1) + ledger lookup in `velnor-ci plan` | Not built. First verdict-level reuse; whole-tree key only. |
| PR → queue (different tree) | Per-package verdict records + C5 re-run | Same, plus C5 classifier in the planner (V12) | Not built. Needs enrolled contracts (§8) before narrowing below whole-tree. |
| Agent run → PR | `velnor verify` fills candidate records (§2.5) | Runner-side daemon + ledger | Not built. Only T2 (cluster, sealed) fills a slot. |
| Main → release | Promote bytes by digest (§2.5 below in table order: §2.6) | Generator release jobs + `velnor-ci release-gate` | Gate exists in weak form; promotion does not. |

**Trust rule, already the codebase's own:** writers are trusted events only (`push`/
`schedule` on the default branch), PR writes stay in the PR's scope. `mbx-store` enforces
exactly this for the compiler cache (`mbx_store.rs:28-37`); the delta layer enforces it
for build outputs (forks restore-only, main/queue never read deltas,
`docs/rust-ci-cache-keys.md:61`). The ledger adopts the same scopes (`candidate` vs
`trusted` after A1) instead of inventing new ones. **HD3 caveat:** baseline-gated skipping
was removed by owner decision for the old engine (`docs/hard-decisions.md:223-230`); HD
binds the engine only (`hard-decisions.md:16-19`, `docs/goals.md:14`), but Gate 5's history
means verdict reuse still needs an explicit owner approval (Q-N8). `skip_push_after_merge_group`
does not: it reuses GitHub's own run verdict on the identical SHA, not a Velnor-computed key.

### 2.5 Agent-native `velnor verify` / `velnor cargo` / `velnord`

**Runner-side, new crates `crates/velnor-cli` + `crates/velnord`** (the name `velnor-host`
is taken by `crates/velnor-runner-cli`; the design's single `velnor` binary needs the
naming ADR of E-N16 before any rename). What velnor-gen gives for free is decisive here:
`velnor verify` does not need a new planner. `velnor-ci plan --explain` already computes,
from a checkout, the selected packages, the nextest filter, the package args and the
doctest/docs needs (`crates/velnor-ci/src/io/plan.rs:37-161`; outputs at
`crates/velnor-ci/src/plan.rs:19-63`), and `velnor-local` already runs a checkout's
workflows against a local stand-in (`local/README.md:3`). The honest v0 of `velnor verify`
is therefore: snapshot the worktree as a tree (AN02), run the released `velnor-ci plan`
for that tree on a cluster node, execute the obligations in the gate's sandbox, write
candidate records. `velnor cargo` count-only (AN01) maps argv to the same plan outputs;
a wrong mapping is a miss, never a false hit. The `interactive` class, trust tiers
T0–T3 and the request-only node socket (DG06) are unchanged from the design; they are
scheduling policy in the node agent, not generator concerns. One ordering constraint E
gets right and the plan must keep: `velnor cargo` cannot take `mise.toml`'s wrapper slot
from `mbx` (`mise.toml:44-46`) until the generator stops emitting `mbx-store`/`mbx-seed`
steps around it — generator change first, wrapper second.

### 2.6 Release promotion by digest

**Generator-time + run-time; the hand-written release workflow is the thing to delete.**
Today `release-rust-ci-tools.yml` (253 lines, hand-written) rebuilds both tools from
source on hosted runners per target with no cache, and its `gate` job checks only ancestry
(`release-rust-ci-tools.yml:87-105`: compare API `behind|identical`), not that `Required`
passed — contrary to `docs/goals.md:77`. Meanwhile velnor-gen already generates a full
`release.yml` for consumer repos (`crates/velnor-render/src/release*.rs`; config
`.velnor/release.toml`, `generator/CONFIG.md:267-376`) with `family_tag`, `[[binaries]]`,
`[[checks]]`, attestation and `velnor-ci release {record,publish,preflight,family-tag}`
— this repo simply does not use it (no `.velnor/release.toml`). The rebased path:

1. Land #1608 (gate requires `Required` from the push run) — correctness, no redesign.
2. Adopt `.velnor/release.toml` with `family_tag = "tools-v{{ version }}"` (#733) and let
   velnor-gen emit `release.yml`; delete `release-rust-ci-tools.yml`. Migrate and delete,
   no side-by-side.
3. Promotion: release bytes become one obligation keyed by closure/lockfile/toolchain/
   profile/target (+ group SHA while the commit is compiled in), built in the release PR's
   merge group, qualified once (`--version`, `check-tag` — `velnor-gen check-tag` exists,
   `crates/velnor-gen/src/lib.rs:121-131`), attested once; the tag job only fetches by
   digest, re-hashes, verifies and publishes.

**Conflict to settle with the owner (Q-N5):** generated release verification pins hosted
runners (`HOSTED_RUNNER = "ubuntu-latest"`, `crates/velnor-render/src/release.rs:31`) and
verifies with `--deny-self-hosted-runners` (`release_attest.rs:70`). "Releases on Velnor"
(R01) requires dropping that flag, which weakens an existing, deliberate guarantee.
Recommendation in §7: keep hosted-attested publication; build bytes wherever is fastest,
attest and verify on hosted runners. Promotion by digest does not need self-hosted
publishing to save the rebuild. **Also missing entirely:** the `runner-v` release group
is declared (`release-plz.toml:49-71`, 8 crates) but no workflow builds or publishes it.
F01 (delete runner crates) and N07 (a `runner-v` workflow) cannot both be right; the owner
pack must pick one before either is scheduled.

### 2.7 Velnor Native executor vs GitHub Official

**Runner-side, new crates in `crates/velnor-*`, gated on the goals rewrite (§7).** Today
nothing in Rust executes an Actions job: the official C# runner (pinned 2.338.0,
`.velnor/runner.lock:17-18`) executes every job, in ARC pods in production and in
`velnor-host` pairs on macOS. `crates/velnor-runner-github` implements only the scale-set
side (sessions, long-poll, acquire, JIT — `regen/B` §1.3); broker long-poll, run-service
acquire/renew/complete, Results and OIDC do not exist in Rust in this tree. The design's
`Executor` trait (architecture §5.3) therefore splits into three unequal parts on
velnor-gen:

- **Acquired-job Official** (release, scheduled, image): mostly exists. `velnor-host`
  already launches the official runner per job with a journal and ownership proofs
  (`regen/B` §4); RV05 reviews the wrapper for `velnor-official`. This is the cheapest
  executor work and the only part the verdict path never needs.
- **Obligation-mode Official** (synthetic job message, node-agent run-service/Results/
  cache endpoints, `velnor-ci obligation exec`): entirely new. Note the generator
  interaction: an obligation is a `velnor-ci` command line from the released inventory,
  so obligation mode needs a new `HelperCommand` (`crates/velnor-model/src/steps.rs:158-167`),
  a `velnor-ci` subcommand, a `helper_gate.rs` floor, and a `HELPER_COMMANDS` entry if
  declared workflows may call it (`crates/velnor-workflows/src/helper_call.rs:6`, 38
  commands at tip).
- **Native** (expressions, steps, composites, masking, post steps, closed action set):
  entirely new, and its closed set must be reconciled with the generator's actual set
  before a line is written. `ALLOWED_ACTIONS` has 16 entries (`rules.rs:16`) including
  `docker/login-action`, `docker/setup-buildx-action`, `docker/build-push-action` and
  `actions/github-script` (JavaScript) — all absent from the design's native table
  (architecture §5.6), while the design's `asamarts/alint` is not in the generator list
  (it runs via `[[lint]]`, `generator/CONFIG.md:216-258`). Either the generator stops
  emitting docker/github-script actions for Native-routed jobs (a `velnor-rust` planner
  + `velnor-render` rules change with goldens), or Native refuses those jobs forever.
  Decide in the naming/set ADR (E-N20); do not discover it at cohort 2.

**Free:** executor choice is already a label in the model — `runner`, `fork_runner`,
`test_groups.runner`, `crates_mirror.runner`, `[[workflow]] runner` (A §7.2). A first
`velnor-native` label needs no model change; a typed executor choice (G02/G05) is a new
config key through `config_keys.rs` read from the base commit, with the executor identity
folded into every verdict key (§2.1) so results never cross executors (owner decision 20).

### 2.8 Platform trait

**Runner-side.** There is no trait: `grep "trait Runtime"` over `crates/` is empty; ~50
files import `bollard` directly (`regen/B` §3.1). Two incompatible proposals are in tree:
`docs/proposed/kubernetes-runtime.md:3` ("Design only; no code accompanies this document")
keeps the Turso journal and maps the Docker "pair" to a Pod pair, while the architecture
(§5.10) deletes the journal at the final milestone and forbids pod/container concepts in
the core (§5.4). Reconciling them (E-N12) is a precondition for P01/P02, not a detail:
the journal is 65.8k lines of the runner's hardest invariants (`regen/B` §4.2), and
"port only the observation model" (Present/AbsentProven/Unknown, Pod-UID proofs) is the
right merge — keep `velnor-runner-core`'s lifecycle/capacity/ownership types (small, pure;
`CleanupProof::covers` now requires a full 64-hex id and named volume,
`crates/velnor-runner-core/src/ownership.rs:31-36`, fixed by `227673da85` #1275), port the
Docker spec audit (`docker_spec.rs` forbids host mounts and the outer socket), delete the
pair-shaped launch machine only when F01 deletes its last user. Generator interaction is
deliberately thin: a backend advertises node-local features through the env vars the
helper already reads (`VELNOR_MBX_SHARED_DIR`, `VELNOR_MBX_SEED_DIR`,
`generator/CONFIG.md:111-115`), because the workflow `env` context cannot see runner
process env. A backend offering more (reflink, read audit) plugs in the same way — a new
helper command + config table following the `crates_mirror` pattern (helper floor 0.1.93)
— never by editing generated YAML semantics per backend.

### 2.9 Storage; actions/cache server; git and crate mirrors; warm snapshots; mbx

| Component | Home | Exists | Missing / extend |
|---|---|---|---|
| Storage abstraction (`BlobStore`, tiers) | Runner-side `crates/velnor-storage` | Nothing: no blob trait, no RustFS/S3 client (`regen/B` §3.2). Cluster storage is `emptyDir` on node XFS/LVM. | ST01/ST02 as designed; routes stay `local` until the phase-1 cluster test (ST06) passes. Gated with Q-N1 only because `docs/goals.md:59` budgets GitHub's cache; the trait itself needs no goals change. |
| Native `actions/cache` server | Runner-side service + `crates/velnor-cache` index | The executable spec exists: `velnor-cache-rules` (scope, immutable keys, eviction — `crates/velnor-cache-rules/src/lookup.rs:8-41`) + the `velnor-local` Twirp-v2 cache server + lab conformance probes (`local/conformance/*.json`). Production cache is GitHub's, four typed families (`CacheKey::{Tools,Sources,Target,Delta}`, text only via `render_key`, `crates/velnor-render/src/cache.rs:20-61`). | Production service, authoritative cross-node index, chunk dedup, prefetch; ST05 endpoint so Official can use it (owner decision 22's open question — architecture §7.2 answers it via `ResultsServiceUrl`, still unbuilt). **Generator rule:** keep the single-`render_key` discipline; the native server consumes the same `CacheKey` values, it does not re-derive key text. |
| Crates mirror | Run-time (`velnor-ci crates-mirror`) + cluster DaemonSet | **Shipped and on** for this repo (`[crates_mirror]`, `.velnor/rust-ci.toml:11-14`; loud failure, no silent crates.io fallback, `generator/CONFIG.md:45-68`). Hosted/fork jobs never use it. | Per-lockfile read-only registry on top (C02 rewritten); nothing else. Owner decision 24's "currently unused" is stale. |
| Git mirrors | Runner-side (node agent) | Absent. Checkout is `actions/checkout` at `fetch-depth: 2` (`R:65`; `PLAN_DEPTH = 2`, `crates/velnor-rust/src/plan.rs:37`). | C03/C10 as designed. Note the generator coupling: native checkout must reproduce `fetch-depth`/`sparse-checkout` inputs the planner emits, and the full-tree hash (architecture §7.4) reuses the delta layer's source-digest code path, not a new hasher. |
| Warm snapshots / generations | Runner-side (builder + node agent) | The compile-reuse substrate half-exists: `inputs save/restore` mtime restoration, delta overlays with fingerprint invalidation on source-snapshot mismatch (`docs/rust-ci-cache-keys.md:57`), MBX CAS (next row). No reflink clone, no sealed generation, no pool. ARC `minRunners` idle pods are the only warm capacity. | C05/C12 as designed, with one correction from as-built: the changed set Δ must be computed against the generation manifest using the delta layer's full tracked-source digest, which already handles the "main moved past G" case the design's verbatim-`inputs restore` port would get wrong (architecture §7.3 step 2). |
| mbx | Run-time (`velnor-ci mbx-store`/`mbx-seed`) | **Shipped:** `mbx_shared_store = true` (`.velnor/rust-ci.toml:6`); trusted events write the node store, PR/queue seed a private copy read-only (`generator/CONFIG.md:111-115`). `[cache] mbx_remote` is off. No sccache anywhere. | Measure before redesigning (C08 rewritten): there is no hit-rate number for the shared store at helper 0.1.95. `velnor cargo` replacing the mbx wrapper is ordered after the generator stops emitting mbx steps (§2.5). |

**What velnor-gen gives for free, summarised:** a typed plan shared by generator and
helper; selection with fail-closed unknown paths; a gate that already refuses
"scheduled but skipped" and "tests expected but none ran" (`decide`, `tests_expected`);
four content-addressed cache families with one key renderer; PR-isolated build deltas;
a node-shared compiler store with the right trust scopes; digest-pinned released helpers;
golden-sample tests for every generated byte (38 samples, `crates/velnor-gen/tests/it/golden.rs`);
and per-run metrics (`velnor-ci report`/`bench`/`timings`). The design's remaining work
is the ledger, the App, the executors, the platform/storage backends and the agent CLI —
in that order of value, not the delivery plan's order of construction.

## 3. Correctness first

Ranked by harm: a false green or an ungated merge outranks a slow run, always (owner
decision 11; `docs/goals.md:40-41`). "Needs owner" means a decision or a settings change
only the owner can make; everything else is agent work under existing rules.

**Gap 1 — Red main merged around, not through (worst).** Evidence: post-cutover 226 of
268 PR runs failed `Required`; 157–167 of ~173 merges carried a failing `Required` on the
head commit, median 0.6 min after the last push (`regen/C` §1); the tip `0eb2d76a5c`'s own
push run `44584861409` failed. The ruleset requires only `Required` with no
`integration_id` and an admin bypass actor (`regen/C` §4), so bypass is both possible and,
inferred, routine. Root causes sampled by C §2: `velnor-ci plan` step failures (33/60,
incl. helper-install SHA mismatch), `velnor-ci gate` failures (36/60), lint (~58), tests
(~40), Format (17), `Cache: Check cache waste` (17), plus `ci.yml` Alint 400-line cap and
cargo-deny RUSTSEC-2026-0543 (#1218). **Fix:** fix-forward to green first (item 1, §6) —
no ruleset tightening until green exists, or the repo deadlocks; then bind `Required` to
integration 15368, empty `bypass_actors`, ALLGREEN (item 5, §6). **Where:** code fixes in
the failing crates; ruleset via owner-applied JSON + field-by-field read-back. **Owner:**
yes for bypass retirement (Q-N3) — but note `docs/roadmap.md` already forbids `--admin`
merges; the owner is being asked to make the written rule enforceable, not to invent it.

**Gap 2 — `Required` green without tests.** Unlabelled ready PR: quick set only, gate
skipped, `Required` passes (`docs/rust-ci.md:85`; `generator/CONFIG.md:548`). Draft:
`Required` passes with a notice (`R:584`; `docs/rust-ci.md:84`). After label removal the
new head is again green without tests, because `unlabeled` is not a trigger (`R:5-12`).
This directly violates owner decision 12 step 6 ("an unlabelled PR never has a green
required check") and goals never-compromise 2 ("green means verified",
`docs/goals.md:41`). It is worse than gap 1 in kind — gap 1 is a red gate bypassed by a
human; gap 2 is the gate itself lying — but gap 1 is worse today because it is exercised
on every merge. **Fix:** §2.3 (fail "CI not requested", add `unlabeled`, retire `ci:run`).
**Where:** `velnor-model` (`PrRun`), `velnor-rust` (`plan_budget.rs`, `required_job.rs`),
`velnor-render` conditions, `generator/CONFIG.md` `pr_ci` table, `docs/rust-ci.md:78-90`,
goldens. **Owner:** yes for the label name and `ci:run` retirement (Q-N2); no for the
honesty fix itself, which goals already mandate. Recommend changing `pr_ci = "label"`
semantics in place (with a helper-floor bump) rather than adding a third mode: the current
semantics are a bug against the product's own goals file, and the brief forbids keeping
old and new side by side. Consumers who relied on advisory-green get a loud migration
note in `CONFIG.md`, not a compatibility flag.

**Gap 3 — Release gate checks ancestry, not verification.** `release-rust-ci-tools.yml:87-105`
accepts any tag whose commit is `behind|identical` to the default branch; the declared
`checks: read` permission is unused. `docs/goals.md:77` requires release only from a
default-branch commit "whose required check passed". Combined with gap 1, tools 0.1.95
could be (and the process allows being) released from a red commit. **Fix:** land #1608
rebased (item 4, §6), then the generated-`release.yml` migration (§2.6). **Owner:** no.

**Gap 4 — Merge queue configured, unused; auto-merge off; HEADGREEN.** Zero `merge_group`
runs and zero enqueues post-cutover (`regen/C` §1); `allow_auto_merge = false`; grouping
HEADGREEN with min wait 5 min. Under HEADGREEN a group can land commits whose own checks
failed — the design's C1 evidence, now at 157× scale. The queue is also the only place a
fork PR ever gets the full suite (`docs/rust-ci.md:90`), so while it sleeps, the fork
story is fiction. **Fix:** revive with gap 1's ruleset step (ALLGREEN, depth 2, timeout
90, wait 0), enable auto-merge, agents stop catch-up merges (B04 survives verbatim).
**Owner:** yes (repo setting + ruleset, Q-N4 bundled into the gap-1 ask).

**Gap 5 — Two selection authorities can disagree (E understates this).** Runtime selection
is `velnor_rust::select` over `.velnor/rust-ci.toml` + `rust-metadata.json`
(`crates/velnor-ci/src/io/plan.rs:37-90`). But `.velnor/config.toml` still carries
`[stacks.rust]` `[[inputs]]` rules with `complete = true` — HD3's "selected only by its
declared inputs" semantics (`docs/hard-decisions.md:118-121`) — read by the ported
`velnor-ci plan --mode` path (`crates/velnor-ci/src/plan_request*`) for workflows deleted
in #1550/#1747. Two files now claim to say what a path selects, with opposite failure
modes (`complete = true` narrows; `select()` fails closed to everything). Nothing checks
they agree. **Fix:** delete the ported engine ops (`plan --mode`, `write-request`,
`resolve-qualification`, `image`, `admit-preview`, `run-authority`, `qualification compare`
callers are gone) and `.velnor/config.toml` with them (E-N14), after a grep-proof that no
live workflow calls them. **Owner:** no — #1747 already decided the engine is deleted;
this is finishing it (AGENTS.md: "No legacy code").

**Gap 6 — Dead `GeneratorCheck`: generated files can drift silently.**
`StepKind::GeneratorCheck` exists (`crates/velnor-model/src/steps.rs:108`) and renders
(`crates/velnor-render/src/steps.rs:279`) but no planner emits it (grep over `velnor-rust`:
no hit; only a rules test constructs it, `crates/velnor-gen/tests/it/rules.rs:322`). No
in-CI step runs `velnor-gen check`; freshness relies on out-of-repo scripts
(`regen-main.sh`, #1739/#1742) and on `check-metadata`, which covers only
`rust-metadata.json`. A hand-edited `rust-ci.yml` with a forged digest, or a config change
without regeneration, reaches main undetected by CI — the digest is unkeyed and stops
accidents, not forgers (`crates/velnor-render/src/header.rs:20-22`). **Fix:** emit
`GeneratorCheck` in the `rust` job (or a tiny gate job) for every generated workflow
(item 3, §6); DG08's derived-freshness obligations then generalise it. **Owner:** no.

**Gap 7 — `fetch-depth: 2` silently degrades selection to "everything".** Push/queue
planning diffs `before..after` / `base_sha..head_sha` and requires the base to be an
ancestor in a depth-2 checkout (`crates/velnor-ci/src/io/git.rs:12-79`); any failure
selects everything (`io/plan.rs:170-179`). Inference (A §2.3, unverified by a run):
multi-commit pushes and multi-PR groups therefore always select everything. This is
fail-safe (slow, never wrong) — a speed defect wearing a correctness costume — but it also
means the queue, once revived at depth 2, will look slower than designed and the
`skip_push` measurement will be polluted. **Fix:** first write the failing test (a
two-commit push fixture in `velnor-local` scenarios or `velnor-ci` plan tests), then fetch
the base explicitly (`git fetch --depth=1 origin <base>` in `velnor-ci plan`) or deepen
only the plan checkout. **Owner:** no.

**Gap 8 — Unknown workflow paths select everything (safe, expensive, and now common).**
Only the exact generated `rust-ci.yml` has a named trigger (`AllTrigger::GeneratedWorkflow`,
`crates/velnor-rust/src/classify.rs:69`); edits to `ci.yml`, `qualification.yml`,
`release-rust-ci-tools.yml`, `release-plz.toml` or `.github/**` otherwise are
`UnknownPath` → select everything (`crates/velnor-model/src/selection.rs:79`). Correct by
goals rule 2, but in a repo whose daily work IS workflow edits, "safe" means most PRs pay
full-workspace lint (and full tests when labelled). Note `.velnor/config.toml`'s
`[stacks.rust]` maps `.github/workflows/release-rust-ci-tools.yml` to five generator
crates — for the dead authority of gap 5, not for `select()`. **Fix:** map the remaining
workflow files explicitly via `[[inputs]]`/`non_inputs` in `.velnor/rust-ci.toml` (config,
not code), keeping fail-closed for genuinely unknown paths. **Owner:** no.

**Gap 9 — Duplicated, serialised, non-blocking policy.** `cargo-deny` and `cargo-machete`
run twice per event: hand-written `ci.yml:68-134` (not required by the ruleset, failing on
205 of 295 PR runs per `regen/C` §2 without blocking anything) and the generated `policy`
job (`R:406-505`), which `needs: rust` although deny/machete need no build — they need only
the plan outputs `rust_deny`/`rust_machete`, which in the one-job shape live inside the
`rust` job. So the cheap checks wait for the expensive job, and the loud checks gate
nothing. **Fix:** migrate `ci.yml`'s four jobs (Alint, Cargo Deny, Cargo Machete,
Jactionlint — `ci.yml:41,68,118,135`) into generated `[[lint]]` entries + `policy`
(`generator/CONFIG.md:216-258` already supports `jactionlint`/`alint`/`mise-task` kinds),
run `policy` unconditionally in parallel (deny/machete are seconds; the plan gate saves
nothing worth serialising for), delete `ci.yml`. This *strengthens* the gate: lint jobs
enter `Required`'s spec (CONFIG.md:258). **Owner:** no, but land it after gap 1's green,
because it makes four currently-ignorable reds blocking.

**Gap 10 — No speed budget is armed.** `[budget]` is unset in `.velnor/rust-ci.toml`, so no
`rust-budget` job exists; "No merged check compares wall time with a limit yet"
(`docs/rust-ci.md:114`). Goals targets (first cargo ≤ 30 s, no-crate run ≤ 1 min) are
reported nowhere enforceable (`velnor-ci bench budget` exists and fails closed,
`generator/CONFIG.md:180-184`). A correctness-adjacent gap: without it, every future speed
regression is invisible until someone redoes report C by hand. **Fix:** item 10, §6 (set
budgets from the green baseline's median + 25%, per CONFIG.md's own rule). **Owner:** no.

**Gap 11 — `Cache` advisory but noisy.** Goals rule 3 (visible, non-blocking, `docs/goals.md:67`) is correctly implemented (`cache_check_blocks = false`), but a red `Cache` job marks whole runs failed in lists (17 sampled), training everyone to ignore red. **Fix:** none to the gate; reporting (item 10) must count `Required`, not run conclusions. Never make Cache blocking to silence it. **Owner:** no.

**Gap 12 — The rule books contradict the owner's executor decision and the tree.**
`docs/goals.md:16` (official runner always) and `docs/proposed/macos-scaleset-runner.md`
("Velnor does not implement an Actions interpreter", active specification) forbid what
owner decisions 1/20 command; `docs/goals.md:11-12` still locates code in `generator/`,
`ci-helper/`, `crates/velnor-runner` and calls the deleted engine "maintained, not
developed". `AGENTS.md:31-32` already records the deletion, so the repo's own books
disagree with each other. **Fix:** one owner-approved goals amendment (§7 gives exact
wording), including the stale layout lines. **Owner:** yes, explicitly (`docs/goals.md:3`).

Not gaps, verified sound and to be left alone: helper pinning and per-invocation binary
verification (`R:71-82`); fail-closed selection on any doubt (`select()` whole-tree
triggers); the gate's refusal of scheduled-but-skipped jobs and zero-test passes
(`decide` + `tests_expected`); PR-delta isolation (forks save nothing, main never reads
deltas); private-source token hygiene (mint-fetch-revoke before repo code,
`generator/CONFIG.md:85`); the publish job checking out nothing and verifying digests
(`release-rust-ci-tools.yml:228-253`, goals rule 13's second half — the first half is gap 3).

## 4. Speed on the new baseline

### 4.1 Where the time goes now (and what C cannot tell us about tip)

Post-cutover measured numbers (`regen/C`, 4.3 h sample, 2026-11-19 13:31–17:50Z; treat as
small-sample and red-shortened throughout):

| Measure | Value | Reading |
|---|---|---|
| Rust CI PR run, all | P50 1.1 / P90 5.8 min (268 runs, 2 success) | Dominated by quick-set and early failures; not a verdict time. |
| Rust CI PR run, heavy lanes ran | P50 2.9 / P90 6.2 min (n=141; 58 failed, 83 cancelled) | Closest thing to a PR verdict baseline; still shortened by failure. |
| Rust CI PR run, plan selected nothing | P50 0.8 / P90 1.2 min (n=127; 85 still failed) | Selection works; plan/gate failures dominate the red. |
| Rust CI push, heavy | P50 7.5 / P90 11.3 min (n=74; 72 failed) | Post-merge full set: + `cargo doc`, alone-checks, prebuild, saves. |
| Queue (job created→started), scale set | P50 2–13 s, P90 13–57 s; 0 hosted jobs in 2,660 sampled | The pre-cutover 79–97% hosted queue share is **gone**. Queue is solved; do not spend on it. |
| Long pole | `rust-root` run P50 67 s (PR) / 371 s (push) | Compile-bound. See the staleness warning below. |
| Per merged PR, summed run wall | PR 2.5 + push 3.9 = 6.4 min (was ~117 min pre-cutover across PR+queue+push) | Twice-verified (queue leg vanished), not deduplicated. |
| Push-to-merge | P50 0.6 / P90 3.8 min post-cutover | **Not a baseline.** It measures bypass latency. Any honest target must be re-derived after gap 1 closes. |

**Staleness warning (verified, E does not flag it):** C's per-job decomposition
(`rust-plan`, `rust-root`, `rust-helper`, … 11 jobs per run) describes the workflow shape
*during* its window. At tip `0eb2d76a5c`, `rust-ci.yml` is one `rust` job + `policy` +
`rust-trim` + `required` + `ci-run-label` + `cache` (`R:34,406,506,571,613,627`), because
`shape = auto` with no `.velnor/rust-ci-timings.json` resolves to `one-job` (`R:32`
comment; `crates/velnor-rust/src/shape.rs:28-33`), and the 31 "Check crate alone" steps of
report A are 18 at tip (one per surviving workspace member, `Cargo.toml:5-24`; post-merge
only, `check_alone = "post_merge"`). C's run-level P50/P90 and queue facts remain valid;
its critical-path attribution does not map onto tip. **Every lever below therefore starts
with re-measurement on a green main (item 10, §6), using `velnor-ci bench collect/compare`
and `velnor-ci timings --repo`, which already exist.** Goals never-compromise 5 ("every
speed claim has a measurement", `docs/goals.md:44`) forbids doing otherwise.

Within the one-job shape the PR path is: checkout → helper → `plan` → restores (+ delta) → Format → clippy `--workspace` → test build `--workspace` → nextest `-E` → lane → doctests. Selection narrows only which *tests run* (HD11), so a one-crate PR pays a workspace-sized compile bill and a crate-sized test bill. That asymmetry, not queueing, is the baseline's shape — compile reuse outranks executor work.

### 4.2 The next five levers, in order of saved minutes per merge

**Lever 1 — Delete the second verification: queue + `skip_push_after_merge_group`.**
Saves most of the push run's check time on every queue-merged PR. The push heavy run is
P50 7.5 min; the skip keeps cache-warming builds and saves but drops Format, Lint, Docs,
alone-checks, test setup, tests and doctests (`generator/CONFIG.md:37`). Conservative
saving: 3–6 min of wall per merge and the larger part of 3.9 summed push minutes, at zero
new code — one config table (`[ci] skip_push_after_merge_group = true`) plus regeneration.
*Measure:* `velnor-ci bench compare` on 20 push runs before/after; verify `skip=true`
only on SHAs with a completed successful `merge_group` run of the same workflow file
(`crates/velnor-ci/src/merge_group.rs:46-86` already fails safe to `skip=false`).
Prerequisite: gaps 1+4 (a queue that is used and a main that can go green).

**Lever 2 — Parallelise what the one job serialises: timings file → shape.**
`velnor-ci timings --repo owner/name` writes `.velnor/rust-ci-timings.json` from medians
of ≥3 default-branch runs; `shape = auto` then chooses `lint-test` when the shorter build
exceeds 5 s (`docs/rust-ci.md:24-30`), writing the reason into the workflow (goals rule 5).
Today clippy (`--workspace`) and the test build run back-to-back in one job; splitting
them overlaps the two workspace compiles the PR pays regardless of selection. If one test
binary dominates (runner-host), `velnor-gen suggest-groups` already proposes a
`[[test_groups]]` split from the same timings file. *Measure (goals rule 14, experiment
written first):* PR-verdict P50/P90 and runner-minutes for `one-job` vs `lint-test` on the
same 30 labelled heads; adopt only if verdict improves without runner-minutes regressing
(goals target: neither gets worse, `docs/goals.md:60`). Expected saving: up to the shorter
of (clippy, test-build) per PR — plausibly 1–3 min at P50; the experiment decides.

**Lever 3 — The test and compile floor: runner-host + the velnor-ci monolith.**
Two concrete floors survive #1747. (a) `velnor-runner-host`'s suite (the design's 924.8 s
`receipt_integrity` still lives there; the crate grew to ~181k lines incl. tests,
`regen/B` §5.1) sets the P90 for any PR touching it, and its `local-mode` lane re-runs
with features (`.velnor/rust-ci.toml:26-32`, lane guard now emitted by #1335).
(b) `velnor-ci` at ~33.7k source + ~42.5k test lines (`regen/E` T07 row) is now the
workspace's largest compile unit: every `--workspace` build pays it, so splitting it along
module lines (T07 rewritten, §5) cuts the non-incremental edit-compile-test loop for
*every* PR, not just helper PRs. *Measure:* per-binary wall/CPU inventory on a worker
(BM08 rewritten) before touching anything; then loop-split (T02 rewritten: engine suites
are gone, only surviving loops count), private temp dirs/ports to drop `test-threads=1`,
prefix-clone fixtures. Target unchanged from the design: no test over 60 s alone.

**Lever 4 — Unserialise policy and delete ci.yml (gap 9).** `policy` currently starts only
after `rust` succeeds (`R:406` `needs`), adding its full setup+run (~1–1.5 min in C's
samples) to every verdict tail, twice over with `ci.yml` duplicating deny/machete.
Running deny/machete unconditionally in parallel and folding the four `ci.yml` jobs into
`[[lint]]` removes the tail and one whole workflow's runner-minutes (C: `ci.yml` PR runs
P50 0.8 min × 295 runs in 4.3 h). *Measure:* `Required` latency (last needed job end minus
run start) before/after on labelled PRs; runner-minutes per merged PR including `ci.yml`
before, excluding it after (it must be deleted in the same change, not left as a twin).

**Lever 5 — Measure the delta and the shared MBX store, then tune; fix depth-2 selection.**
The PR delta (#1107) and `mbx_shared_store` (#1411) shipped with no timing (A §3.2:
"No timing measured"). Until `ci-metrics.json` hit rates (`hit exact|fallback|miss` per
family) and `first_cargo_seconds` are reported over a green week, any warm-snapshot or
native-cache investment is guessing — the design's §7.3 numbers (0.25 s reflink clone,
0.31 s no-op build) were measured on idle nodes against a different substrate. In the
same item, fix gap 7 so multi-commit pushes stop selecting everything. *Measure:* second-
push-to-same-PR compile counts (goals target: only what changed since the first push,
`docs/goals.md:56`) and `Cache`-check waste rate, delta on vs off (pin an older helper in
a lab branch — the generator silently drops delta steps below 0.1.95,
`generator/CONFIG.md:43`, which is a ready-made A/B switch).

Deliberately **not** in the five: the Native executor (seconds per Actions job, off the
verdict path), RustFS/native cache (unmeasured substrate, lever 5 first), warm pools
(queue is 2–13 s), and per-package verdict reuse (needs the ledger + shadow evidence;
it is lever 1 of the *next* epoch, §8).

### 4.3 Targets, honestly re-derived

Definitions (unchanged from the design, architecture §1.1): **PR verdict** = label (or
labelled push) → green required check on that head. **Push-to-merge** = final labelled
push → merge, reported gross and net of GitHub's queue share (~19 s to group + ~26 s to
merge, measured medians, no runner controls them).

| | PR verdict P50 / P90 | Push-to-merge P50 / P90 | Basis |
|---|---|---|---|
| Pre-cutover (history) | 26.7 / 92.3 min [M, proxy] | 85 / 457 min [M, proxy]; C window: 122 / 701 min | evidence pack; C §1 |
| Post-cutover as measured | heavy 2.9 / 6.2 min (red-shortened) | 0.6 / 3.8 min (**bypass, void**) | C §1–§2 |
| **Honest green baseline (to measure, item 10)** | expect ~4–6 / ~8–12 min [E: heavy-run P50/P90 lengthened to completion] | expect verdict + queue residence at depth 2 | Re-measure; do not publish until n ≥ 50 green labelled heads |
| **After levers 1–5 (Actions era, no ledger)** | **≤ 3 / ≤ 7 min [T]** | **≤ 6 / ≤ 12 min [T]** (≈ verdict + ≤ 2 min queue at depth 2 with reuse) | Levers 2+4 cut the PR path; lever 1 cuts push, not verdict |
| Design goal (ledger era, kept) | ≤ 90 s / ≤ 4 min [T] | ≤ 3 / ≤ 8 min [T] | Needs per-package verdict keys enrolled + compile reuse graduated + floor fixed |
| Design stretch (kept) | 30 s / 100 s [T] | 100 s / 4 min [T] | Needs per-test receipts + stamp-after-link; floor is GitHub's ~45 s + C5 path |

Two honesty rules. First, the design's intermediate row ("App gate, whole-tree keys, cold
second witness ≈ 6/18 min verdict") is **already beaten by the dumb baseline** — a heavy
Actions run finishes in ~3 min at P50 — because it was costed against 39 jobs and hosted
queues that no longer exist. Whole-tree-keys-with-cold-second-witness would be a
*regression* if shipped as specified; the ledger must enter at per-package verdict keys
with warm compile reuse, or not at all (see §5, V-stream). Second, runner-minutes per
merged PR must be re-baselined too: C's summed wall is 6.4 min/PR post-cutover vs ~117
pre-cutover, but wall ≠ runner-minutes and red runs understate both. Item 10 publishes
the green number; until then, quote no runner-minute target tighter than "not worse than
the previous version" (`docs/goals.md:60`), enforced by the `[budget]` job.

## 5. What to keep, change, drop

E classified all 174 delivery items (VALID 86, CHANGED 50, CONFLICT 20, DONE 10,
OBSOLETE 8) and proposed N01–N24. I agree with E's per-item evidence almost everywhere;
where I disagree is on **sequence and centre of gravity**, plus five things E missed.
Verdicts below are by group (E's item table is the index; only deltas are argued).

**Agree, with E's actions:** MARK-DONE (B03, B08, B10, B11, B12, A09, G04-cutover, T07-gen,
T06-mostly) — all verified at tip (0 hosted Rust CI jobs in C's sample; no Darwin legs;
`ALLOWED_ACTIONS` enforced at render, `rules.rs:16,386`; one `tests/it` in the generator
crates). DROP (B02, B13, B14, X13, T04, T05, S04) — engine releases, per-job engine
profiles, 105 `background: true` uses (0 at tip), the 89 s fixture and
`non_inputs_differential` (both deleted with the engine), image census (workflows deleted
#1550/#1554). GATE on Q-N1 (P07, X01–X16 executor core, ST03's GitHub-cache replacement,
R01's self-hosted release, F01, V04's verdict reuse on Q-N8) — correct, and §7 asks them
as one pack so the gate lifts once, not 22 times.

**Disagree / re-sequence:**

1. **X-stream order.** E keeps the executor stream intact behind the gate. On the new
   baseline that misorders value: X02–X05 (broker/run-service/Results) buy nothing until
   obligations exist, and obligations need the generator inventory (G03 rewritten, §2.1)
   first. Re-sequence: G03 → V01–V04 (ledger inside Actions via `velnor-ci plan`) → L02
   (App) → O03/X (executors run obligations). Acquired-job Official (O01) can proceed in
   parallel any time after RV05; Native (X06–X13) is last, not first. The delivery plan's
   "coding does not set the dates" was true when CI took 94 min/PR; with ~3 min verdicts,
   evidence counts (3,000 comparisons, 300 heads) set the dates even harder — start the
   shadow counters on day one of the ledger, not after the executor.
2. **V08/V13 (keys).** E rewrites V08 as per-package. Agree, but split compile vs verdict
   keys (§2.1) or HD11 makes the per-package key unsound for compile reuse. V13 (stretch
   per-package) is then deleted as subsumed; the stretch becomes per-test-case receipts
   (AN09) only.
3. **B07 (interim label step).** E: REWRITE as a new `pr_ci` mode. I go further (§3 gap 2):
   change `pr_ci = "label"` semantics in place. A third mode keeps a known-false-green
   product behaviour alive for every consumer; goals forbid it. This is the one place I
   recommend *more* breakage than E.
4. **G01 cohorts.** E: rebuild cohorts on today's jobs. Agree, but cohorts now have only
   six generated jobs + four lint jobs + qualification + release to move — cohort
   migration collapses from "26 crate jobs" theatre to: (1) gate/policy/lint jobs,
   (2) the one `rust` job template, (3) Docker-class test binaries, (4) release. The
   per-type pair counts (D11) should be re-derived for four types, not eleven.
5. **T07.** E marks the generator split DONE and rewrites the `velnor-ci` part "with real
   numbers". Promote it: the `velnor-ci` split is now a top-5 speed lever (§4.2 lever 3),
   not housekeeping, because `--workspace` compilation makes the largest crate everyone's
   tax.

**What E missed (additions to N01–N24):**

- **N25 — C's job-level data is stale at tip** (§4.1). Any plan item citing `rust-root`/
  `rust-plan` timings must re-measure against the one-job shape before sizing.
- **N26 — Dual selection authority** (§3 gap 5). E lists retiring `[stacks.rust]` as
  cleanup (N14); it is a correctness item: two files, opposite narrowing semantics, no
  agreement check.
- **N27 — #1320 closed unmerged; #1073 orphaned.** D recommended "rewrite #1320 on
  velnor-gen"; the PR is gone. The per-root inputs-audit work (no double workspace build
  on scheduled runs) needs a fresh issue-sized PR on the `crates/` layout, simplified by
  #1713's floor (0.1.81+) deleting the older-helper branches it was gated on.
- **N28 — Qualification is a weak oracle seed as built.** `qualification.yml` (1,303 lines,
  64 jobs, dispatch-only) has `compare` as `echo compare-lanes` in both its engine and
  generated forms (A §6). H04 cannot seed a differential corpus from an echo; wire
  `velnor-ci qualification compare` (exists as a ported command) or record real
  hosted-vs-scale-set pairs first. Also record one run of the `velnor-local` `velnor-new`
  scenario (E-N22): it has none.
- **N29 — Golden sample `generator/samples/velnor-new` is stale** (differs from live
  `rust-ci.yml` at least in `labeled` type and cron, A §1.4). Either record it fresh with
  `record-sample` or delete it; a stale self-portrait in the test suite teaches the wrong
  workflow. Pairs with gap 6.

**Streams that survive, and how their first items change:**

| Stream | Verdict | First item(s) after rebase |
|---|---|---|
| B (bootstrap) | **Replaced** by new Phase 0 (§6). B01/B05 rewritten (owner pack, §7); B04 kept; B15 rewritten onto `velnor-ci bench/timings` counting `Required`, not run conclusions (gap 11). | §6 items 1–5. |
| RV (reviews) | Keep all five; RV03's scope moves to tip paths (`crates/velnor-ci/src/gate.rs`, `crates/velnor-render/src/release.rs:31`). RV01 must decide journal-vs-observation-model with N12 in the same review. | RV03 first (it unblocks G03/R02). |
| G (workflows/inventory) | **Becomes the lead stream.** G03 rewritten as generator-emitted inventory (§2.1); G05 rewritten onto `config_keys.rs` (`.velnor/config.toml [velnor.*]` is dead — the file is `[stacks.rust]`-only and slated for deletion, gap 5); G02 after the set ADR (§2.7); G04 remainder = "push `Required` reports A1". | Gap-2 label honesty, GeneratorCheck, G03. |
| V (ledger) | Keep, anchored in `velnor-model` types (E §3), entered at per-package verdict keys (§4.3 honesty rule 1). V05 renamed (collision with `velnor-ci inputs audit`): call it "cold audit", keep A1/A2 as log tags only. V10 after queue revival. | `skip_push` measuring (lever 1) as V00; then V01 key struct. |
| AN (agent-native) | Keep; AN01–AN04 re-based on `velnor-ci plan` outputs (§2.5). AN09 stays stretch. | AN01 count-only after G03. |
| C (compile reuse) | Keep; C01/C02 rewritten as extensions of shipped `tools-local`/crates-mirror; C05 builds on delta+inputs (§2.9); C08 baseline = shared MBX store; C09 target = the four GitHub families only (seal/prune already deleted). | Lever-5 measurement (C08 data first). |
| X/O/P (executors/platform) | Keep behind Q-N1; re-sequenced per disagreement 1. X01's `executor/` root → `crates/velnor-*`. P03 = refactor of existing Docker code per RV01, not greenfield. | RV01/RV02/RV05, then O01. |
| A (native actions) | Keep with recounts (E's tip counts stand: checkout 49, cache/restore 14, save 9, upload 10, download 6, mise-action 15, github-script 5). A06/A07 deferred until a consumer in *this* repo exists (attest/app-token appear only in generated consumer `release.yml`). | A08 (replace github-script) — pure generator work, needed by both executors. |
| H (conformance) | Keep; H04 seed rewritten (N28); H03 reuses `velnor-ci qualification compare` + `velnor-local conformance` instead of a third comparator. | N28 wiring. |
| L (App/gate) | Keep; L06 inventory from `WorkflowPlan`/`VerdictSpec`; L07/L08 rewritten to one existing context (§2.2 name discipline). | After V01–V03 shadow. |
| ST/S (storage/cluster) | Keep; ST03 gated on Q-N1's cache line only. S04 dropped. Cluster manifests still have no repo home (E-N11) — decide before P04/S01 write any. | ST01 after Q-N1. |
| T (test floor) | T02/T03 rewritten to surviving suites; T04/T05 dropped; T06 residual = runner-host only; T07 promoted (above); T08 refs updated (`now_seconds` consumers at `crates/velnor-ci/src/cache.rs:19` et al.). | BM08 inventory, then T02. |
| R (release) | R02 rewritten (land #1608 first; `--app-check` is the only genuinely new flag — push-run evidence is already required and non-Actions App checks are currently *refused*, so R02 must also lift that refusal deliberately); R04 rewritten onto generated `release.yml` (§2.6); R01 gated on Q-N5. | #1608. |
| BM (benchmarks) | Keep; BM03 baseline redefined (today's CI = green velnor-gen Actions, not the 85-min proxy); BM04 trace refreshed post-cutover; BM06/BM08 single-workspace; BM14 needs the revived queue. Build on `velnor-ci bench` (E-N10), do not write a second harness. | Item 10 baseline. |
| DG (Dagger-derived) | Keep; DG07 names from generated step names (`Format`, `Lint`, `Test`, `Check <crate> alone`); DG08 = gap 6 first; DG11 extends `ci-metrics.json` to OTLP, not a parallel trace format. | DG08 with item 3. |
| F (final deletion) | Keep last; scope now includes `release-plz.toml` runner group, tag rulesets and the `local-mode` CI lane (`.velnor/rust-ci.toml:26-32`) in the same change, per E. Blocked on the N07-vs-F01 owner pick (§2.6). | — |

## 6. Phase 0 and the first ten work items, rewritten

Phase 0 used to be: ruleset JSON, engine 0.1.81, gate jobs off hosted runners, engine
0.1.82 batch. All of it is done or deleted. **New Phase 0 (definition): main is green,
`Required` tells the truth, merges go through the queue, and a clean baseline exists.**
Exit criteria: (a) 20 consecutive push runs green without bypass; (b) an unlabelled PR
cannot show green `Required` (lab-proven); (c) 10 consecutive merges through the queue
with `skip_push` measured; (d) item 10's baseline published. Only then does any
ledger/executor milestone clock start. Mmerged since the plan and therefore *removed*
from the queue: #1274 (`61067c17fc`), #1335 (`47ef96f8ea`), #1747 (`57b65af0db`), #1748
(`0eb2d76a5c`), #1263 (`a619478fb2`). Open-PR dispositions are folded into items 4 and 9.
Sizes are agent-hours of building, review excluded (review model/effort per row). Model
names follow the repo's own executor labels (`exec:sonnet/opus/haiku`).

**Item 1 — Green main (E-N01).** *Goal:* `Rust CI` and `CI` green on `main` for 20
consecutive pushes; tip run `44584861409`'s failures diagnosed to root cause (why did the
architecture allow each: helper-install SHA mismatch class, plan/gate fragility, Alint
400-line cap, #1218 chrono-via-turso deny failure). *Files:* failing crates named by the
run logs; `.alint.yml` only if the cap itself is the defect (prefer splitting files);
`deny.toml`/lockfile for #1218 (bump or replace the `chrono` path — never silence deny).
*Done-when:* criterion (a) above; each fixed failure has a test that was red before and
green after (AGENTS.md test rule), or an explicit "no test possible" note. *Size:* 12–20 h
(unknown until logs are read; re-size after 2 h of triage). *Impl:* sonnet @ high (opus @
medium if triage shows a planner/gate design fault). *Review:* opus @ medium.

**Item 2 — Honest `Required` under `pr_ci = "label"` (gap 2, E-N02/B07 rewritten).**
*Goal:* unlabelled ready PR and draft show failing `Required` ("CI not requested");
`unlabeled` cancels; `ci:run` retired, one label remains (name per Q-N2; build with
`ci:auto` so the item never blocks on the rename). *Files:*
`crates/velnor-model/src/plan.rs` (PrRun/labels), `crates/velnor-rust/src/plan_budget.rs`,
`crates/velnor-rust/src/required_job.rs`, `crates/velnor-rust/src/plan.rs` (`quick`/`full`),
`crates/velnor-render/src/` (conditions, `ci-run-label` job deletion), `generator/CONFIG.md`
`pr_ci` section, `docs/rust-ci.md:78-90`, `generator/samples/*` goldens, `.velnor/rust-ci.toml`
+ regenerated `R`. Helper floor bump in `crates/velnor-gen/src/helper_gate.rs`.
*Done-when:* lab/velnor-local scenario proves: draft red, ready-unlabelled red, label →
gate, push-while-labelled reruns, unlabel cancels, written verdict survives unlabel for
its commit; goldens updated; `velnor-gen check` clean. *Size:* 10–14 h. *Impl:* sonnet
@ high. *Review:* opus @ medium (gate semantics; security-adjacent).

**Item 3 — Wire `GeneratorCheck` (gap 6, E-N04/DG08).** *Goal:* every generated workflow
is drift-checked in CI: planner emits `StepKind::GeneratorCheck`
(`crates/velnor-model/src/steps.rs:108`; renderer at
`crates/velnor-render/src/steps.rs:279` → `helper_steps::generator_check_step`) in the
`rust` job after helper install; `velnor-ci` gains nothing (the step runs the pinned
`velnor-gen`… verify which binary `generator_check_step` invokes and pin it the same way
the helper is pinned — if it assumes an unpinned `velnor-gen`, fix that first).
*Files:* `crates/velnor-rust/src/plan.rs` (+ root_jobs), goldens, `docs/rust-ci.md`.
Also refresh or delete the stale `generator/samples/velnor-new` sample (N29) in the same
PR. *Done-when:* a PR that edits `.velnor/rust-ci.toml` without regenerating fails CI
naming the drifted file; a hand-edited `R` fails likewise. *Size:* 4–6 h. *Impl:* sonnet
@ high. *Review:* sonnet @ high.

**Item 4 — Land #1608, then retire the dual authority (gaps 3+5; E-N06/N14/N26).**
*Goal (4a):* rebase #1608 (release gate requires `Required` from the push run; +35/−13, 4
files, clean locally per D) onto tip, regenerate goldens if the workflow moved, merge
through the (revived-in-item-5 or direct-with-green) path. *Goal (4b, separate PR):*
delete `velnor-ci plan --mode` + `write-request`/`resolve-qualification`/`image`/
`admit-preview`/`run-authority` ports and `.velnor/config.toml`, with a grep-proof of no
live callers in `.github/workflows/` and `qualification/`. *Files (4a):*
`.github/workflows/release-rust-ci-tools.yml`, `crates/velnor-gen/tests/it/release_workflow.rs`,
fixtures/MANIFEST per the PR. *Files (4b):* `crates/velnor-ci/src/plan_request*` + CLI
wiring (`crates/velnor-ci/src/cli.rs`), `.velnor/config.toml`, `crates/velnor-gen/src/drifts.rs`
(the `engine_configured` tolerance that lets unmarked workflows coexist must go with it —
after item 7 deletes `ci.yml`, nothing needs the tolerance; sequence 4b after item 7).
*Done-when:* tag on a red commit is refused by the gate job (dry-run in the lab repo);
4b: `rg "plan --mode|stacks\.rust"` over workflows is empty and selection has exactly one
implementation. *Size:* 4a 3–5 h; 4b 6–8 h. *Impl:* sonnet @ high. *Review:* opus @ medium
(release authority + deletion scope).

**Item 5 — Ruleset + queue revival, owner-applied (gaps 1+4; B05/B04 rewritten).**
*Goal:* agents prepare, owner applies: `PUT rulesets/24396608` with
`required_status_checks: [{context: "Required", integration_id: 15368}]`,
`bypass_actors: []`, `merge_queue: {SQUASH, ALLGREEN, build 2, merge 1..10, wait 0,
timeout 90}`; repo `allow_auto_merge = true`; field-by-field read-back + canary PRs
(H09-lite: no-status PR and foreign-source status PR must stay `blocked`). Agent procedure
change in the same change: auto-merge at creation, no catch-up merges (update
`docs/roadmap.md` merge procedure text). *Files:* none in-repo except `docs/roadmap.md`;
the JSON lives in the owner-pack issue. *Done-when:* read-back equals the JSON; 10 merges
land through the queue with zero bypass (item 6 measures); a PR with failing `Required`
provably cannot enqueue. *Size:* 2 h agent + owner application. *Impl:* sonnet @ high
(prep + verification script in strict TypeScript/Bun if scripted). *Review:* opus @
medium. **Blocked on item 1** (never tighten onto a red main) **and Q-N3/Q-N4.**

**Item 6 — Enable and measure `skip_push_after_merge_group` (lever 1).** *Goal:* add
`[ci] skip_push_after_merge_group = true` to `.velnor/rust-ci.toml`, regenerate, and
publish the before/after: push-run wall, skipped-step list, `skip=true` rate, and proof
that cache saves still ran (Target/Sources entries present per `velnor-ci cache verify`).
*Files:* `.velnor/rust-ci.toml`, `R` (generated), measurement note in the PR (numbers from
`velnor-ci bench compare`, not run-page eyeballing). *Done-when:* 20 queue-merged PRs
show `skip=true` pushes with green `Required` and warm caches on the next PR; one forced
`skip=false` path (direct push commit) still runs everything. *Size:* 3–4 h + measurement
wall time. *Impl:* haiku @ high (config + measurement). *Review:* sonnet @ high.

**Item 7 — Migrate `ci.yml` into the generator; unserialise policy (gap 9, lever 4).**
*Goal:* `[[lint]]` entries for jactionlint/alint (+ probe) reproducing `ci.yml:41-157`
exactly; `policy` (deny/machete) loses its `needs: rust` (run unconditionally in
parallel; if unconditional policy proves wasteful on docs-only PRs, the fallback is a
tiny plan job — measure, don't guess); delete `ci.yml` in the same PR. Drop `edited`
from the deleted file's trigger set by deletion (B06 remainder) and decide `edited` for
`R` (keep: base-retarget re-planning is load-bearing, `R:35`; title/body edits already
run nothing). *Files:* `.velnor/rust-ci.toml`, `crates/velnor-rust/src/lint_jobs.rs` /
`lane.rs` (policy needs), `crates/velnor-render/src/lint_steps.rs`, goldens, `ci.yml`
(deleted), `docs/rust-ci.md:47`. *Done-when:* the four checks appear in `Required`'s gate
spec; a planted alint/deny failure blocks merge; verdict latency measured per lever 4.
*Size:* 8–12 h. *Impl:* sonnet @ high. *Review:* opus @ medium (gate membership change).

**Item 8 — Timings file + shape experiment (lever 2).** *Goal:* run `velnor-ci timings
--repo tailrocks/velnor-new` on green default-branch runs, commit
`.velnor/rust-ci-timings.json`, run the written experiment (goals rule 14: question,
measurements, picking rule recorded before the first run) for `one-job` vs `lint-test`
(and `suggest-groups` output for runner-host if it proposes one). *Files:*
`.velnor/rust-ci-timings.json`, `.velnor/rust-ci.toml` (shape/groups only if the rule
picks them), `R` (generated; its header comment must state the new reason). *Done-when:*
experiment report with raw `bench` files committed; shape adopted only per the picking
rule. *Size:* 4–6 h + run wall time. *Impl:* sonnet @ high. *Review:* sonnet @ high.

**Item 9 — Clear the open-PR queue to zero (except housekeeping).** Dispositions:
**#1200** (mise platform pins; on `crates/` layout, 2 behind at D-time): rebase, fix the
non-#1218 failures, merge — highest value, closest. **#1026** (inputs sibling/target-dir;
+5,792 lines, 96 commits, stacked): do not rebase wholesale; replay as two fresh PRs on
tip (velnor-ci input handling, then generator rendering at the current floor), closing
#1026 with links; carry #1730 follow-ups into the new PRs' checklists. **#1297**
(Renovate): close as superseded; let Renovate recreate against the single workspace
(`ci-helper/Cargo.lock` no longer exists). **#1073** (orphaned by #1320's closure, N27):
fresh small PR for per-root `inputs audit --root` without the separate audit job,
dropping the version-gated branches #1713 made dead. **#1751**: owner's research PR, not
ours to disposition. *Files:* per PR. *Done-when:* open PRs = {#1751} ∪ {new split PRs
in flight}; each closed PR names its successor or its obsolescence reason (never
"stale"). *Size:* #1200 4–6 h; #1026-split 12–16 h; #1297 0.5 h; #1073-fresh 6–8 h.
*Impl:* sonnet @ high (#1026-split: opus @ medium for the design call on what survives).
*Review:* sonnet @ high per PR; opus @ medium for the #1026 split plan.

**Item 10 — Green baseline + armed budget (E-N09/N10, gap 10, B15 rewritten).** *Goal:*
one clean measurement pass on green main: PR verdict and push-to-merge (gross + net of
GitHub) per PR class from `velnor-ci bench` + GitHub API, runner-minutes per merged PR
(completed/cancelled/superseded split), cache hit rates per family, `first_cargo_seconds`
vs the 30 s target; then set `[budget]` in `.velnor/rust-ci.toml` (median + 25% per
`generator/CONFIG.md:184`) so `rust-budget` guards it, and refresh or retire every "Today"
number in the architecture/explainer docs (label the old ones pre-cutover history, per
AGENTS.md evergreen rule move dated numbers to the measurement issue, not the docs).
*Files:* `.velnor/rust-ci.toml`, `R` (generated budget job), measurement issue with raw
files; doc touch-ups limited to removing false "today" claims. *Done-when:* §4.3's
"honest green baseline" row has measured values with n and intervals; a planted 2× slow
step fails `rust-budget` in the lab. *Size:* 6–8 h. *Impl:* sonnet @ high. *Review:*
opus @ medium (baselines become targets; get them right).

Order: 1 first; 2, 3, 4a, 7, 8 parallel after 1 (sequence regen commits for goldens); 5 after 1 (+owner); 6 after 5; 9 throughout; 10 last. Total ≈ 65–95 agent-hours + ~15 review. Wall time is set by green traffic (20 pushes, 20 queue merges, ≥50 labelled heads), not code size or agent count.

## 7. Owner questions, rewritten

### 7.1 D1–D26 disposition

Sources: `docs/research/velnor-runner-2026-11/owner-questions.md` (21 open listed; D5
absent; D6, D10, D12, D13 marked settled there). "Answered" means events on main settled
it; the recommendation column is mine where the question survives.

| Id | Verdict | Why / recommendation |
|---|---|---|
| D1 release engine 0.1.81 | **Answered — delete.** | #1345 shipped; engine deleted `57b65af0db`. |
| D2 auto-merge, no catch-up | **Reworded** → item 5/Q-N4. | Still `allow_auto_merge = false`, queue unused, merges direct. Ask as one bundle: auto-merge on + queue on + bypass off. Recommend yes to all three. |
| D3 first ruleset update | **Reworded** → item 5. | The prepared JSON names `Required verdict`, a context nothing emits since #1523. New JSON: one context `Required` bound to 15368, bypass `[]`, ALLGREEN, depth 2, timeout 90, wait 0 — with the precondition "main green first" (item 1), which the original lacked. Recommend (a) as rewritten. |
| D4 evaluator A or B | **Keep, deferred.** | No repo dependency until X06. Recommendation unchanged: B (decision 3 compliance), default A until answered; only ask when Q-N1 passes and X06 is next — asking now wastes the owner's attention on a gated stream. |
| D6 lab reference executor | Settled (decision 19). | No change. |
| D7 `velnor-verify` App + agent identity | **Keep + extend.** | Add: "Until the App ships, does `Required` stay the sole gate name?" Recommend yes (§2.2 name discipline) and create both identities now — item 2's agents already need a non-`GITHUB_TOKEN` labelling identity. |
| D8 deploy identity + node changes | **Reworded.** | Add the missing half: where do cluster/ARC manifests live? (No `deploy/` in repo; ARC lives in an outside checkout, `regen/B` §0.2.) Recommend: approve identity + XFS now; RustFS volume rebuild only with ST06; manifests move into this repo (new top-level dir) or stay external — pick one explicitly. |
| D9 pre-approve CI-only engine releases | **Answered — delete.** | No engine. Successor question (tools releases via release-plz) needs no pre-approval: they are ordinary PRs + tags under existing rulesets. |
| D10 two witnesses, ALLGREEN | Settled by rulings; **note the gap:** ruleset is still HEADGREEN in fact. | Fold the factual fix into D3-rewritten; do not re-ask the principle. |
| D11 cutover counts | **Reworded after G01-rebuilt.** | Four job types now (§5 disagreement 4), not eleven. Recommend keeping N per type (150/300/150/30) but re-deriving wall time; ask with the cohort table, not abstractly. |
| D12 where release bytes are built | Settled (merge group, promote by digest). | Implementation path changed (§2.6) but the decision holds. |
| D13 park old runner PRs | **Re-ask as Q-N9.** | Premise false: those PRs merged, hardening continued (~55 commits, journal v58, CI lane #1740). The live question is runner-host policy until F01: continue, freeze, or fix-only. Recommend fix-only (correctness fixes yes, features no) until RV01 reports. |
| D14 delete engine crates | **Answered — delete.** | Done by #1747. Successor: delete the ported ops + `.velnor/config.toml` (item 4b) — no owner ask needed, #1747 is the approval. |
| D15 bad-release recovery | **Keep.** | Sharper now: today's de facto recovery *is* the admin merge gap 1 relies on. Recommend (b) redeploy + break-glass verifier, and sequence it before bypass retirement bites: item 5 must not land until the break-glass path exists on paper and the previous-helper redeploy is rehearsed once (pin-helper to N−1 in the lab). |
| D16 draft-carried label arms at ready | **Absorbed into Q-N2.** | Cannot be answered sensibly apart from the whole label flow (§2.3). My recommendation inside Q-N2: yes, arm at ready (the reconciler reads state; literal decision-12 step 2 costs a round trip for no correctness). |
| D17 who approves a reduction | **Keep.** | Recommend owner-only, signed inventory entry (as E). Note the generator hook: reductions are diffs to the emitted inventory (§2.1), reviewable like any generated file. |
| D18 Velnor merge train | **Keep, precondition added.** | Needs GitHub's queue in use to measure (BM14). Recommend not now (as E); re-ask with BM14 numbers after item 6. |
| D19 compile before the label | **Keep.** | Recommend no (as designed). Note: the quick set already compiles clippy `--workspace` pre-label today — if the owner answers "no" strictly, item 2 must also stop the quick set, not just redden `Required`. Put that consequence in the question text. |
| D20–D22, D24–D26 | **Keep, unasked for now.** | No repo dependency; ask at their plan points (BM15, BM03 arms, S01, DG09). D26 (arm64 lane): note release already builds natively on `ubuntu-24.04-arm` — the question is only about a *gating* lane. Recommend advisory-only if ever. |
| D23 auto-verify unpushed commits | **Keep.** | Recommend not yet. Cheaper to revisit once AN01 count-only data shows real agent command volume. |

### 7.2 New questions (E's Q-N1..Q-N11, with my recommendations)

| Id | Question (short) | My recommendation |
|---|---|---|
| Q-N1 | Approve rewriting `docs/goals.md` (executor, layout, fork, cache, local-first lines) + macOS spec to admit Native + Official behind one interface, K8s, ARC deletion? | **Yes, with the exact wording in §7.3** — and split the vote: (i) executor wording now; (ii) native-cache goals line only when ST03 is ready to replace GitHub cache (don't pre-weaken `docs/goals.md:59`); (iii) fork line: keep the fence (below). |
| Q-N2 | Label flow: name, retire `ci:run`, quick set allowed?, unlabelled red? | Name: rename to `velnor:verify` (owner's own decision-12 example; one rename, done in item 2's regeneration). Retire `ci:run`: yes. Quick set: yes, run it (fast reds are valuable) but `Required` stays red until the gate passes. Unlabelled red: yes — non-negotiable per decision 12. Draft-carried label arms at ready: yes (D16). |
| Q-N3 | Retire admin bypass + bind `Required` to 15368 now, accepting a blocked main until green? | Bind + ALLGREEN now (after item 1); bypass retirement in the same edit **only with D15(b) approved**. Blocking main is the point; a bypass that exists will be used (157 proofs). |
| Q-N4 | Is the merge queue part of the flow? | Yes — GitHub's queue stays (decision 7). Depth 2 until group verdict P90 ≤ 5 min and paired false-failure ≤ 3%, then 4/15 min (design's condition, kept). |
| Q-N5 | Releases: self-hosted, hosted-attested, or both? Drop `--deny-self-hosted-runners`? | **Hosted-attested publication, build anywhere.** Promotion (§2.6) captures the saving without surrendering the attestation guarantee `release_attest.rs:70` encodes. Revisit only if hosted minutes ever gate a release. |
| Q-N6 | Fork PRs: drop handling (decision 8) or keep the fence (`docs/goals.md:43`)? | **Split the answer.** velnor-new the repo: no fork support work, rely on repo settings (fork approval + runner-group scoping, `generator/CONFIG.md:11`). velnor-gen the product: **keep** the fence — it is one rendered expression (`R:57`-style) plus `fork_runner` validation, and public consumer repos need it; deleting a safety feature from a generator to save zero lines of velnor-new config is the wrong trade. Decision 8 should be amended to say exactly this. |
| Q-N7 | Parallel lanes with separate targets (design §8.1) vs one job per workspace (goals rules 4–5)? | Goals win until measured otherwise: lever 2's experiment (item 8) is the arbiter. The design's citation (`hard-decisions.md:56-62`) is HD1, which bound the deleted engine only. Ask the owner to confirm "experiment decides" rather than picking a side today. |
| Q-N8 | Allow a verification-result ledger despite HD3? | Yes, scoped: reuse only on whole-key equality with the §2.1 soundness cases (shadow → 3,000 → 2% sampling → epoch revocation), C5 always fresh. HD3 binds the engine; this approval is the goals-side equivalent Gate 5's history demands. Without it, V04+ do not start; `skip_push` (item 6) needs no approval and proceeds regardless. |
| Q-N9 | Runner-host policy until F01? | Fix-only (correctness/security), no features, until RV01's verdict table lands; then re-decide with evidence. (Replaces D13.) |
| Q-N10 | Keep the 40k-word plan under `docs/` despite the evergreen rule? | Keep under `docs/research/` as a dated snapshot with a "superseded facts" banner (E §0.9 dead links fixed in the same PR); evergreen contracts move to `docs/proposed/` only when adopted. AGENTS.md's rule targets contract docs masquerading as current; a labelled research snapshot is honest. |
| Q-N11 | Backlog executors: Claude-only or multi-vendor? | Claude-only in `docs/roadmap.md` terms (`exec:*` labels) until the roadmap loop itself is amended; normalise the TSV model ids to the label set (E §5c). Multi-vendor panels (like this one) are research, not the delivery loop. |

One question E does not list and the pack needs: **Q-N12 — `runner-v` release or F01
deletion?** (§2.6: the group is declared, unpublished, and slated for deletion by two
different items.) Recommendation: decide with Q-N9/RV01 evidence; until then, build no
`runner-v` workflow (N07 paused) and delete nothing (F01 paused). Publishing a runner the
plan deletes, or deleting a runner other repos might install, are both one-way doors.

### 7.3 Exact proposed wording change to `docs/goals.md` (for the owner to approve or reject)

Change 1 — the executor rule, `docs/goals.md:16`. Delete:

> New work serves Rust projects on GitHub Actions, on GitHub-hosted or self-hosted runners. Jobs are always executed by GitHub's official runner: on GitHub's machines, or on the project's own machines inside the containers that `velnor-runner` starts.

Insert:

> New work serves Rust projects on GitHub Actions, on GitHub-hosted or self-hosted runners. Jobs are executed by the executor the project chooses in its configuration: Velnor Native (written in Rust, the default) or GitHub Official (GitHub's official runner, driven by Velnor). The executor's identity is part of every result, so a result is never reused across executors. GitHub Official is also the permanent reference executor: it proves Velnor Native behaves the same, and a reference run never produces a verdict, a cache entry or a release. Velnor never switches executor on its own when a job fails.

Change 2 — local first, design rule 11, `docs/goals.md:75`. Replace "with GitHub's
official runner, started by the Velnor runner" with "with the project's chosen executor,
started by Velnor". (One clause; the rest of rule 11 is untouched. This also unblocks
P07 without weakening the rule's intent: local verification still precedes real GitHub.)

Change 3 — stale layout, `docs/goals.md:11-12` (factual, ask in the same approval so the
file is opened once). Row 1 becomes: "New work: the Rust generator and helper in
`crates/velnor-gen`, `crates/velnor-model`, `crates/velnor-rust`, `crates/velnor-render`,
`crates/velnor-release`, `crates/velnor-workflows` and `crates/velnor-ci`, the runner in
`crates/velnor-runner-*`, the local environment in `crates/velnor-local`, and the
workflows the generator writes". Row 2 becomes: "Everything else that is left of the old
engine (`crates/velnor-archive-guard`, `.velnor/config.toml`, the qualification scripts):
maintained, not developed; the engine itself is deleted and `velnor-gen` writes every
generated workflow".

Explicitly **not** changed in this ask: `docs/goals.md:43` (fork sentence — Q-N6 keeps
the fence), `docs/goals.md:59` (8 GB GitHub cache budget — amended only when a native
cache is ready to replace it), `docs/goals.md:20-22` ("the runner is developed as a
product in its own right" — stays true under either executor; F01 does not need it
removed, it needs Q-N12 answered), and every promise, never-compromise line and target.
If the owner rejects Change 1, the consequence must be stated plainly in the pack: Native
(X-stream), ARC deletion and obligation-mode verification cannot be built; the plan
collapses to sections 3, 4 and 6 of this report (correctness + Actions-era speed +
ledger-inside-Actions up to Q-N8's limit), which is still a coherent, valuable plan —
but it is not the design the owner approved in decisions 1 and 20.

## 8. Be ambitious: what a generator-first foundation makes possible

The old design treats velnor-gen as a YAML printer the runner tolerates. Inverted, it is
the strongest part of the system: a deterministic, golden-tested function from repository
facts to a complete verification plan, already trusted to be byte-exact
(`crates/velnor-gen/tests/it/golden.rs`, 38 samples). Six possibilities follow, ordered
from "build next" to "this is the endgame"; each names its hardest problem, because the
easy versions are worthless.

**1. The generator emits the verification contract itself.** The obligation inventory
(§2.1) should be a generated file — digested by the same `seal()` as `rust-ci.yml`
(`crates/velnor-render/src/header.rs:23`), drift-checked by the same `GeneratorCheck`
(item 3), reviewed as a diff. Then "what does CI check?" stops being archaeology across
666 lines of YAML and becomes one reviewable artifact; an inventory reduction (D17) is a
visible deletion in a PR, not a silent skip. Hardest part: expected test IDs churn on
every test addition, so the inventory must record *derivation rules* (package ×
configuration × filterset) and let `velnor-ci plan` expand IDs at run time, failing on
any expansion the base commit's rules would not produce. Get that boundary wrong and the
inventory is either stale daily or too loose to catch a dropped test.

**2. Verdict keys at plan time — instant verdicts before any executor exists.**
`velnor-ci plan` already runs before tools install and emits the full selection
(`crates/velnor-ci/src/io/plan.rs:37-90`). Emitting the key vector there means a
rebase-only push, a stacked PR, or an identical queue tree can resolve to "all reused,
only C5 to run" within the existing Actions job — steps skip, `Required` greens in under
a minute, no App, no Native, no new infrastructure beyond a ledger read. The generator
can even emit a dedicated fast-path shape: plan + resolve + gate as the whole workflow
for repos that opt in. Hardest part: the lookup must happen with the *released* helper's
key definition against records written under possibly older key versions; key-version
negotiation (miss on version bump, never approximate) is the entire soundness story.

**3. One plan, three drivers.** `WorkflowPlan` (`crates/velnor-model/src/plan.rs:22-113`)
is already an IR with one backend (YAML render). Add two more consumers of the same
value: `velnor-local` scenarios (today they re-derive intent from workflow files) and
Native obligations (today imagined as a separate command graph). Conformance then has a
precise meaning the design currently lacks: *the same plan, driven three ways, must
produce the same verdict and the same test-ID set*. That is a stronger and cheaper oracle
than replaying YAML through two executors, because plan-level divergence is findable
without running anything. Hardest part: `StepKind` (60+ variants,
`crates/velnor-model/src/steps.rs:29-132`) leaks Actions concepts (cache actions, artifact
uploads); the IR needs an obligation-level view beside the step-level view, generated
from the same planner pass so the two can never disagree.

**4. A generator-owned benchmark that tunes its own output.** The loop is half-built:
`velnor-ci timings` → `.velnor/rust-ci-timings.json` → `shape.rs` decision written into
the workflow → `velnor-ci bench budget` guards regressions. Close it: emit the benchmark
as a generated declared workflow (`.velnor/workflows.toml` `[[workflow.case]]` already
generates hosted-vs-scale-set job pairs — the differential harness's skeleton, once N28
replaces its `echo` compare), and let shape, test-group splits (`suggest-groups`) and
`[budget]` values all derive from the same measured file. CI that re-plans itself from
evidence, with every change a generated diff a human reviews, is something no
hand-written workflow can do. Hardest part: feedback stability — a shape change alters
the timings that justify it; the picking rule needs hysteresis (adopt only on >10%
verdict gain sustained over two timing windows) or the workflow will oscillate.

**5. Generated narrowing contracts, revoked by revert.** Per-package keys need contracts
naming every path an obligation may observe (architecture §6.1). Instead of hand-writing
them, generate *proposals*: `velnor-ci inputs audit` already discovers compile-time reads
(`.velnor/rust-inputs.lock`); the read audit (V07) adds run-time reads. The generator
emits both as candidate contract TOML; a human reviews and lands it; enrolment,
suspension and revocation are then ordinary config commits with `velnor-gen check`
enforcing freshness. The trust decision becomes explicit, diffable and revertible in one
PR. Hardest part, unchanged and worth stating plainly: no trace proves completeness
(untaken branches, vDSO clock reads, hash-seed-dependent fixtures — architecture §1.2
C2). Generation cannot fix blindness; it can only ensure the bet is written down,
bounded to one obligation, and automatically re-examined by key-independent sampling
(A1, the nightly reuse-free run). Any proposal that hides the bet inside a service is
strictly worse than one that commits it to the repo.

**6. The endpoint: YAML becomes the legacy adapter.** When the App judges generator
inventories and executors run generator obligations, `rust-ci.yml` shrinks to the push/A1
reporter and a compatibility surface for GitHub's UI. The generator's primary outputs
become the inventory, the key schema and the release definition — artifacts that survive
any change of CI host, because none of them mention Actions. At that point the hardest
remaining problems are not speed: witness independence when both witnesses share node
stores (correlated MBX/target state can correlate false greens — twin generations, C12,
are the mitigation and must be proven, not assumed); speculation storms under ALLGREEN
when a popular base commit invalidates dozens of queued prefixes at once; and the fenced
status writer as a single point of merge failure, which decision 18 forbids answering
with a fallback gate — it must be answered with writer availability (standby in the home
site, epoch-fenced failover) rehearsed like the break-glass verifier. A generator-first
Velnor can plausibly be the fastest honest CI for this repository. Whether it stays
honest at that speed depends entirely on those three problems, and the plan should fund
them before it funds another second off the verdict.
