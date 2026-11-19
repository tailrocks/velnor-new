# How you use Velnor (draft for the owner)

**Status:** draft. It describes Velnor as you and the agents use it, following the
[architecture](velnor-runner-architecture.md) and the
[rulings](../research/velnor-runner-2026-11/velnor-gen-rebase/rulings.md). Where the design leaves a
user-facing choice open, this draft proposes one and marks it **Proposed — owner to confirm**, with
the id of the question in the [owner sheet](../research/velnor-runner-2026-11/owner-questions.md).

Three kinds of statement appear here:

- **Phase 0.** The generated workflow after the phase-0 items of the architecture (section 3). This is
  the near-term contract. "Item 6" means the item with that number there; the trusted verifier is its
  last item.
- **After phase 0.** Agent commands and reuse that need records or the node agent. Nothing marked so
  is built.
- **After Q1.** Anything where Velnor, not GitHub's official runner, executes jobs. It waits for the
  owner to amend `docs/goals.md` (Q1).

Command names, configuration keys and example output are proposals unless the text says they
exist. Example output is illustrative; its numbers are not measurements.

## 1. What CI is

velnor-gen writes this repository's workflows from `.velnor/rust-ci.toml` and
`.velnor/workflows.toml`. The helper `velnor-ci`, pinned by `.velnor/rust-ci.lock`, runs every step.
On a pull request the workflow `Rust CI` (`.github/workflows/rust-ci.yml`) runs:

| Job | What it does |
|---|---|
| `rust` | Plans which packages the change selects, then runs format, lint, the tests of the selected packages and the `local-mode` lane |
| The lint jobs | Alint and Jactionlint, from `[[lint]]` (phase 0 moves them out of the hand-written `ci.yml`) |
| `policy` | `cargo deny` and `cargo machete`. The advisory scan also runs on every push to `main` and every night, whatever changed |
| `Required` | The one required check. It is green only when everything selected ran and passed |
| `Cache` | Reports whether the cache did its job. It never blocks a merge |

A push to `main` adds the documentation build, the "check crate alone" steps and the cache saves.
Phase 0 moves those checks into the merge queue or keeps them after the merge, as you decide (Q13).

## 2. Your day as the owner

### 2.1 From draft to merge

| Step | You or the agent do | What runs | `Required` on the latest commit |
|---|---|---|---|
| 1 | Open a draft and push commits | Nothing | Red: "CI not requested" |
| 2 | Mark it ready for review | Nothing | Red: "CI not requested" |
| 3 | Add the label `velnor:verify` | The latest commit runs: plan, format, lint, the tests of the selected packages, the lint jobs, policy | Pending, then green or red |
| 4 | Push again while labelled | The new commit runs; the previous run is cancelled | Pending, then green or red |
| 5 | Remove the label | The run in flight is cancelled | Red: "CI not requested", also when the commit was green |
| 6 | Edit the title or body, or add or remove another label | Nothing; the event publishes no check and cancels nothing | Unchanged |
| — | Change the base branch | The latest commit runs against the new base, if the PR is labelled | Pending, then green or red |
| 7 | Turn on auto-merge (agents do it when they open the PR) | When `Required` is green, GitHub puts the PR in the merge queue | — |
| 8 | Nothing: the queue runs | The merge group is checked (section 2.2) | The group's own check |
| 9 | Nothing: the PR lands | The push to `main` runs. Once you approve reuse (Q8), it reuses check by check what the queue ran and passed | The push run's check |

A label added while the PR is a draft arms it: when you mark it ready, the latest commit runs. You
never re-add the label after "ready".

A check on GitHub belongs to a commit, not to a pull request. Two PRs with the same head commit
therefore show the same newest `Required`. State per PR comes with the Velnor App.

Labels must come from you or from an agent identity, never from `GITHUB_TOKEN`: GitHub starts no
workflow for a label that `GITHUB_TOKEN` added.

**The cost of an honest gate.** An unlabelled PR shows one short red `Required` per push. That
remains until the Velnor App exists. A green check that verified nothing would be worse.

### 2.2 Merging

- **Every merge goes through the merge queue** (phase 0, item 6; Q3, Q4). There is no admin bypass.
  Nobody, you included, merges around `Required`. Until then you use the bypass only for a PR that
  repairs `main` (Q3).
- **The queue uses ALLGREEN.** Each group's own result must be green. HEADGREEN would let a later
  group's pass cover an earlier entry that was tested only on its own diff.
- **Auto-merge is on.** Agents turn it on when they open the PR and never merge `main` into their
  branch: the queue already tests against the latest `main`.
- **A red group** removes the PR from the queue. The PR's own green stays. Fix, push, and the PR
  re-enters when its new commit is green.
- **The push after a queue merge** reuses the queue's result check by check (phase 0, item 7), once
  you approve reuse (Q8). A check that the queue did not run, such as the documentation build if you
  keep it after the merge, still runs on the push. Builds and cache saves always run.
- **After phase 0:** a group whose tree equals a tree already verified is a lookup, not a run.

### 2.3 What red and green mean

| `Required` shows | It means | What to do |
|---|---|---|
| Green | Everything the plan selected for this commit ran and passed; tests that were expected ran (a test step that ran no tests fails); the generated files are current; the configuration checks no less than `main`'s | Nothing. The PR can merge once `Velnor / Verified` is green too |
| `Velnor / Verified` green (phase 0, last item) | A workflow taken from `main` checked this commit's tree: the generated files are current, nothing is reduced against `main` without your signature, the helper pin is not older, and the `Rust CI` run for this exact commit passed | Nothing |
| Red: "CI not requested" | This commit has no verdict | Add `velnor:verify` when the PR is ready |
| Red: a step failed | A check failed; the run names the step and the test | Open the run, fix, push |
| Red: generated files out of date (phase 0, item 3) | Someone edited a generated workflow by hand, or changed `.velnor/` without regenerating | Run `velnor-gen generate` and commit the result |
| Red: the PR checks less than `main` (phase 0, item 10) | The PR removes a test or a check, or loosens `.velnor/rust-ci.toml`, compared with the base branch | Undo the reduction, or you sign it for that exact diff |
| Pending | A run is in progress for this commit | Wait |

`Required` is never skipped. A skipped required job would count as a pass, so the workflow never
lets that happen on a pull-request event.

`Required` runs from the PR's own copy of the workflow, so its checks on generated files and on
reductions catch accidents only: a PR that edits the workflow could remove them. `Velnor / Verified`
runs from `main`'s copy, and a PR cannot change it. Until the ruleset requires it, the gate does not
protect against a PR that edits its own workflow.

The `Cache` check is advisory. Red means a run compiled a crate that the restored cache already held,
or a cache save failed. It does not block a merge.

A red push run on `main` means `main` is broken. It blocks the next tools release (section 2.4) and
stops the target cache from being saved, so every run starts cold until `main` is green again.

### 2.4 Releases

The tools (`velnor-ci` and `velnor-gen`) release together under one tag, `tools-v<version>`:

1. A release-plz pull request bumps the version.
2. The release App pushes the tag.
3. The generated workflow `release-rust-ci-tools.yml` builds both tools for Linux (x86-64 and arm64)
   on GitHub-hosted runners and publishes a pre-release with checksums.
4. An agent pins the new version with `velnor-gen pin-helper <version>` and regenerates.

Phase 0 (item 5) gates step 3 on the green push run of the tagged commit: the `Rust CI` workflow, the
exact commit, its latest attempt. A red, skipped, pending or older run blocks the release. A change to
the tools reaches CI only through steps 1 to 4, so an agent releases only from a green commit of
`main`; whether it may do so without asking each time is D9.

After phase 0, the push run on `main` builds the release binaries once, and the tag promotes those
exact bytes instead of rebuilding them. Releases stay on GitHub-hosted runners with attestation
until you decide otherwise (Q5). The runner crates have a release group but no release workflow yet
(Q17).

## 3. Setting it up

### 3.1 Phase 0 (you do this once)

1. **Confirm the plan.** No issue is created before you do.
2. **Freeze the bypass** (Q3): from now on you merge around `Required` only for a PR that repairs
   `main`. Phase 0 starts with this.
3. **Answer the phase-0 questions:** tool releases (D9), the label flow (Q2), the queue (Q4), the gate a
   PR cannot weaken (Q12), what blocks landing (Q13), reuse (Q8), and the targets and cost (Q18).
4. **Create two identities** (D7): one for agents, which labels and enables auto-merge but cannot
   change the ruleset; and the App `velnor-verify` with `statuses: write`, whose key is a secret of an
   environment that only `main` can use.
5. **Leave the labels alone.** Phase 0, item 2 creates `velnor:verify` and deletes `ci:run` and
   `ci:auto` in one change. Until then a PR needs `ci:auto` for its tests to run.
6. **Apply the ruleset change** once items 0 to 5 are done and `main` has been green with tests
   running (item 6). The change binds `Required` to the GitHub Actions app, empties `bypass_actors`,
   switches the queue to ALLGREEN, and keeps every other rule and field. The issue gives you the
   commands: they build the payload from the live ruleset, and a `diff` of the read-back must print
   nothing. A PUT replaces the whole ruleset, so never send a payload that lists fewer rules.
7. **Allow auto-merge.** It is a repository setting, changed and read back on its own.
8. **Require `Velnor / Verified`** as well, once the trusted verifier has passed its lab cases.

A generated drift check then fails if any of it is undone.

### 3.2 After Q1: the cluster

The Kubernetes manifests live outside this repository, and host deployments stay operator-run
(`docs/host-deployment-decision.md`).

1. Extend the GitHub App `velnor-verify`. Permissions: `statuses: write`, `checks: write`,
   `pull_requests: read`, `contents: read`, `metadata: read`. Webhooks: `pull_request`,
   `merge_group`, `push` and `check_run`.
2. Create a deploy identity for the `velnor-system` namespace and approve the node changes: XFS work
   volumes, node reservations, an eviction threshold, a PID limit, taints for the nodes that run
   Docker tests, and one drive or volume per worker for RustFS. **Proposed — owner to confirm** (D8).

| Part | Runs as | What it does |
|---|---|---|
| `velnor-control` | One per repository, in its home site | Receives webhooks, runs the label flow, writes `Velnor / Verified` |
| `velnor-dispatch` | One per site | Talks to GitHub for Actions jobs and places all work |
| Node agent | One per worker (DaemonSet) | Keeps warm sandboxes, mirrors, the tool store and build generations; observes and signs results |
| Signer daemon | On each worker, outside every sandbox | Holds the key that signs results |
| Cache index | Site service | Remembers every `actions/cache` entry |
| RustFS | One cluster per site | Stores large blobs: cache chunks, artifacts, release bytes |
| Mirrors | Per worker and per site | A Git mirror on every worker; a crates mirror (it exists today) and an image registry mirror per site |
| Official runner image | Kept | The reference executor, and the executor when you choose it |

ARC's velnor-new scale set goes only after the last job type moves (Q9). ChainArgos and jackin keep
their ARC scale sets.

### 3.3 On a Mac

Today `velnor-local` runs this repository's workflows on a Mac with Apple Silicon: `velnor-local up`,
`run` and `down`. It runs GitHub's official runner against a local stand-in for GitHub.

**After Q1, proposed — owner to confirm (Q1):** one `velnor` binary with `velnor local up`, which
starts the node agent, a few warm containers and a local store. A Mac result never counts as CI
evidence: the machine's kernel and OS are part of every key.

## 4. The configuration

In phase 0 the generated workflow runs from the PR's own tree, so a configuration change takes effect
in the PR that makes it. Two checks compare it with the base branch: the reduction check inside the
workflow (item 10), and the trusted verifier, which the PR cannot remove. After Q1, Velnor reads the
gate's configuration from the **base commit**, and a change takes effect only after it lands.

### 4.1 Phase 0: `.velnor/rust-ci.toml`

The generator rejects unknown keys. A new key is added in `velnor-rust` (`config_keys.rs`),
documented in `generator/CONFIG.md` and gated on the helper release that understands it.
`.velnor/config.toml` is deleted in phase 0 (item 3); its path rules were not read by CI.

```toml
runner = "ubuntu-26.04-scale-set"
required_check = "Required"
mbx_shared_store = true
docs = "post_merge"            # or "always" if you decide docs block landing (Q13)
check_alone = "post_merge"
pr_ci = "label"                # one label, velnor:verify; no quick set (Q2)

[ci]
skip_push_after_merge_group = true   # on only after Q8 and the shadow proof (phase 0, item 7)

# Alint and Jactionlint move here from ci.yml; deny and machete stay in policy (phase 0, item 4).
[[lint]]
kind = "alint"
config = ".alint.yml"
probe = "scripts/alint-line-cap-probe.sh"

[[lint]]
kind = "jactionlint"
config = ".github/jactionlint.yaml"

# A path the plan cannot place selects everything until it is mapped (phase 0, item 9).
# The workflow, .velnor/ configuration, the toolchain file and cargo configuration cannot be mapped.
[[inputs]]
glob = "generator/samples/**"
packages = ["velnor-gen"]

[budget]                       # 0 means "no limit yet"; item 8 sets a field only from enough runs
pr_wall_seconds = 0
main_wall_seconds = 0
```

The `[crates_mirror]`, `[[test_setup]]` and `[[test_lane]]` tables stay as they are. The exact
`[[inputs]]` entries come from replaying 400 commits; the one above is an example.

**Proposed — owner to confirm (Q2):** the label name `velnor:verify` is fixed by the generator, not a
configuration key, so every Velnor repository uses the same label.

### 4.2 After Q1: runner settings

**Proposed — owner to confirm (Q1):** runner-side settings for the repository go into a new
`.velnor/velnor.toml`, site settings into `.velnor/sites.toml`, and personal Mac settings into
`~/.config/velnor/local.toml`. All keys are proposals.

```toml
# ---- .velnor/velnor.toml -----------------------------------------------------------------
[executor]
default = "native"                   # "native" (Velnor Native) or "official" (GitHub Official)

[executor.job_types]                 # per job of the generated workflows
"release-rust-ci-tools" = "official" # releases stay on hosted runners until Q5

# Each data class picks its tiers, fastest first. A tier the platform lacks is skipped.
[storage.routes]
actions_cache = ["local", "site", "cross-site"]
artifacts     = ["local", "site"]    # also uploaded to GitHub, so `gh run download` works
generations   = ["local", "site"]    # every route stays "local" until the RustFS test passes

[mirrors.repositories."tailrocks/velnor-new"]
refs = ["refs/heads/*", "refs/pull/*/head", "refs/heads/gh-readonly-queue/*", "refs/tags/*"]
refresh = "webhook"                  # plus a periodic catch-up

# ---- .velnor/sites.toml ------------------------------------------------------------------
[sites.de]
platform = "kubernetes"
namespace = "velnor-system"

[sites.de.storage.tiers]
local = { backend = "filesystem", path = "/var/lib/velnor/l0" }
site  = { backend = "s3", kind = "rustfs-cluster", endpoint = "http://rustfs.velnor-system.svc:9000" }

[sites.de.capacity]
priorities = ["merge", "gate", "pr", "batch"]   # only batch is preempted
build = { cpus = 16, memory = "24GiB", max_per_node = 4 }
```

The executor identity is part of every result key, so a switch re-verifies once and never reuses
the other executor's results. A wrong storage route can only make things slower: a missing or bad
blob is a miss.

## 5. The agent's day

### 5.1 Phase 0

1. **Push freely.** Drafts run nothing, so intermediate commits cost nothing.
2. **Check locally before pushing.** Run the tests of the crates you touched with the CI profile
   (`cargo nextest run --profile ci -p <crate>`). After a change under `.velnor/` or to a crate's
   manifest, run `velnor-gen generate` and commit what it writes; `velnor-gen check` must pass.
3. **Open the PR ready, with auto-merge on.** Add `velnor:verify` with the agent identity when the
   change is ready to verify. A label added during draft arms the PR at "ready". Before item 2 lands,
   the label is `ci:auto`.
4. **On red,** read the failing step in the run, fix, and push. The label stays; the new commit runs.
5. **Never** merge `main` into the branch, edit a generated workflow by hand, weaken
   `.velnor/rust-ci.toml` without the owner's signature, or merge around the gate.
6. **Stacked PRs.** Label the upper PR at any time. It merges only after the lower PR lands and GitHub
   retargets it to `main`, because the ruleset guards only `main`. **Proposed — owner to confirm
   (Q2).**

### 5.2 After phase 0: verify before pushing

The agent loop becomes: ask (`velnor resolve`), edit, run `cargo` as usual, `velnor verify`, then
`velnor ship`. The label then finds almost everything done. **Proposed — owner to confirm:** one
`velnor` binary with the commands below, their names and their output. The names are settled in one
decision record beside `velnor-ci`, `velnor-local` and `velnor-host`.

- **`velnor verify`** snapshots the worktree as a Git tree (tracked and untracked files, never ignored
  ones), sends only the new objects to the node mirror, and runs the tree merged with the queue tip in
  the gate's own sealed sandbox, on two cluster nodes. The results are records with the gate's keys,
  so the label later reads them instead of running them. `--witnesses=1` runs one and leaves the
  second to the label.
- **`velnor ship`** verifies, pushes, adds the label with the agent identity and turns on auto-merge.
- **`velnor wait`** follows the verdict and prints the failing test, its signature and the last log
  lines; **`velnor repro`** reruns that failure the same way.
- **`velnor resolve`** asks, without running anything, which checks are done, running or still
  needed. **`velnor why-red`** says whether a failure on an unchanged key is also red on `main`, a
  known flake, or new.
- Every command takes `--json`.

```console
$ velnor verify
Verifying 3f9a2c1 (merged with queue tip 5ae9378) on site de
  reused  38   ran 3 (2 witnesses each)   fresh 1 (reads the clock)
  ✓ fmt  ✓ clippy:velnor-ci  ✓ test:velnor-ci (412 tests)
Verified in 41 s. Push and label: the PR will reuse these results.
```

**Run one check by name.** Names come from the verification contract at the base commit and follow
velnor-gen's step and package names. An unknown name is refused.

```console
$ velnor check -l
fmt  clippy:velnor-ci  clippy:velnor-gen  test:velnor-ci  test:velnor-gen  fresh:rust-ci.yml  …
$ velnor check test:velnor-ci --target local
✓ test:velnor-ci (412 tests) on local (advice only: a Mac result never counts)
```

**Why something ran or was reused:**

```console
$ velnor explain 1412 test:velnor-ci
test:velnor-ci  RAN (2 witnesses: velnor-12, velnor-31)
  changed input  crates/velnor-ci/src/plan.rs
$ velnor explain 1412 clippy:velnor-gen
clippy:velnor-gen  REUSED  record from an earlier PR, trusted, epoch 4
  same: tree, selection, workflow digest, helper, toolchain, executor
$ velnor explain 1415 test:velnor-gen
test:velnor-gen  RAN  miss on one field: toolchain changed
```

**`cargo` through Velnor.** `velnor cargo` may take `mise.toml`'s cargo wrapper slot from `mbx` once
the generator no longer emits mbx steps. A command that matches a CI check asks first, then runs on
a warm cluster node and shows a trust tier; anything else runs on the Mac as before, as advice. Its
first release only counts, so the data show whether agents use it.

**Trust tiers** (shown on every result):

| Tier | Where it ran | Counts at the gate? |
|---|---|---|
| T0 | The agent's Mac, plain | No: advice |
| T1 | A sealed Linux sandbox on the Mac | No: advice, used only to spot duplicates and reds |
| T2 | A cluster node, the gate's sealed sandbox | Yes: fills one of a key's two witness slots |
| T3 | On `main` after the cold audit A1 | Yes: trusted |

Nothing verifies on its own. Automatic verification of unpushed commits is an open question in the
owner sheet, default off.

### 5.3 After Q1: jobs on Velnor

**Run a generated job locally** on the Docker backend, and **run both executors and diff** (reference
mode; it writes no verdict, cache entry or release):

```console
$ velnor run rust-ci.yml:rust
Using executor native@sha256:ab12…, platform docker (local)
  ✓ checkout (0.4 s)  ✓ setup-velnor  ✓ plan  ✓ cargo nextest run (88 passed)
Job succeeded. This local result is not CI evidence.
$ velnor diff rust-ci.yml:rust
native   succeeded  31.2 s
official succeeded  44.8 s
Compared outcomes, outputs, env, annotations, cache and artifact effects: 0 differences.
```

Velnor Native runs only what velnor-gen writes: the actions in the generator's fixed list, pinned by
SHA, and one-command `run:` steps. A job outside that set is refused **before its first step**, with a
message naming the action. Velnor never falls back to the official runner on its own; choosing it is
a configuration change that lands through the gate.

## 6. What you see

**Today and in phase 0:** the Actions run of `Rust CI` with its job summary, the `Required` check and
the `Cache` check. `velnor-ci report` writes `ci-metrics.json` and the lane summary for each run.

**After phase 0,** proposed — owner to confirm: a check run `Velnor / Details` with the full report.
No ruleset lists it, so it never gates.

```markdown
### Verified 3f9a2c1 in 38 s
| | Checks | Why |
|---|---|---|
| Reused | 29 | same key verified in an earlier PR and on main |
| Ran twice | 12 | inputs changed (crates/velnor-ci) |
| Fresh | 1 | reads the clock: always runs |
Dropped tests: none. (A test missing compared with main fails the check unless the owner signed it.)
```

**Metrics.** The daily report builds on `velnor-ci bench` and `timings`. **Proposed — owner to
confirm:** after Q1, each Velnor service also exports Prometheus metrics, and `velnor status` reads
them.

| Metric | What it tells you |
|---|---|
| PR verdict time, P50 / P90 | Label or labelled push to a green `Required` |
| Push-to-merge, P50 / P90 | The main metric. Proposed target 3 / 8 min (Q18); trial budget 9 / 20 min after phase 0 |
| Time to the first actionable red | How fast a failure reaches you |
| Enqueue time, queue residence, landing | Where merge time goes |
| Runner-minutes per merged PR | Split into completed, cancelled and superseded work |
| Reuse rates | Results reused against run, per rung of the ladder |
| False failures (flakes) | Per required run, with an interval |
| A1 audit lag (after Q1) | Time from landing to the cold audit |

No dated speed target is published before the first green, test-running baseline (phase 0, item 8).
The trial budgets are about 4 / 9 min for the PR verdict and 9 / 20 min for push-to-merge. The baseline
also reports the targets `docs/goals.md` sets, and what the honest gate costs an ordinary PR (Q18).

## 7. When things go wrong

**`main` is red.** Open the push run and find the failing step. Known causes: a lint failure; a
manifest edit without `velnor-gen generate` (the plan step names the stale file); a generated step
that the pinned helper cannot run (re-pin to a helper that has it, or regenerate with the pinned
one). After phase 0 items 0 to 3, each of them fails `Required` before it can merge.

**A red check on a PR.** Open the run. It names the failed step and the failing test IDs. Fix and
push: with the label on, the new commit runs.

**A flaky test.** There are no automatic retries. Re-run the failed job; if it passes, it counts as a
false failure. Two false failures of one test in seven days open an issue. The test is fixed, or
quarantined only if another blocking check covers the same thing, or under an owner exception that
expires.

**A bad helper release.** The fix is a pull request that pins the previous release. Its own run uses
the helper it pins. An older pin counts as a reduction, so you sign it once `Velnor / Verified` is
required. There is never an admin merge. An emergency stop on the queue and tags
is Q16.

**A stuck queue.** A group with no answer times out after 90 minutes and GitHub removes it.

**After Q1:** a broken Velnor release is fixed by redeploying the previous image digest. If the
records are unreachable, checks run fresh: slower, never wrong. If storage stalls, reads become
misses. A cold audit 2 hours late alarms; at 6 hours it pages you and holds releases. A reused result
that turns out wrong makes Velnor stop writing `success`, mark every open head or queued group that
relied on it as `failure`, re-check them, and audit every commit that landed in the window.

## 8. Switching things (after Q1)

| You switch | How | What changes | What never changes |
|---|---|---|---|
| Executor for a job type | Edit `.velnor/velnor.toml` and land it | That type re-verifies once, then runs on the other executor | The gate and the audits |
| Storage tier for a data class | Edit the storage routes | Speed and hit rates | `actions/cache` behaviour and scoping; PR writes never reach `main`'s scope |
| Storage backend | Edit `.velnor/sites.toml` (RustFS, MinIO, AWS S3, R2) | Speed, operations | Content is checked on every read |
| Platform | Run on Kubernetes or Docker | Speed; which shortcuts exist (no file-access audit on Docker means whole-tree keys) | Results from different platforms never mix |

No switch can make a check pass that would fail. The worst a misconfiguration can do is make things
slower.

## 9. Proposed — owner to confirm

| Item | Proposal | Question |
|---|---|---|
| The bypass freeze, now | Bypass only for a PR that repairs `main` | Q3 |
| The label flow | One label `velnor:verify`; no quick set; a draft, an unlabelled PR and a PR whose label was removed are red; a draft-carried label arms at "ready"; one short red job per push until the App | Q2 |
| The label name | Fixed by the generator, not configurable | Q2 |
| Stacked PRs | Label any time; merge after the lower PR lands | Q2 |
| No bypass, `Required` bound to the GitHub Actions app, auto-merge allowed | After a test-running green `main` | Q3 |
| The queue groups with ALLGREEN | Yes | Q4 |
| What blocks landing | Docs and alone-checks run in the queue, alone-checks only for released crates, decided after the baseline | Q13 |
| A gate the PR cannot weaken | The reduction check, then the trusted verifier and `Velnor / Verified`; you sign any reduction for its exact diff | Q12 |
| Reuse of a verified result | The push after a queue merge, then a rerun of a verified commit; nothing before you approve | Q8 |
| Tool releases by agents | From a green commit of `main`, without asking each time | D9 |
| Release hosting | GitHub-hosted with attestation | Q5 |
| A `runner-v` release workflow | After the runner-crates decision (Q9) | Q17 |
| The agent identity and the status App, now | Yes | D7 |
| Velnor executes jobs; `velnor local up`; `.velnor/velnor.toml` and `.velnor/sites.toml` | After the goals amendment | Q1 |
| Automatic verification of unpushed commits | Off | D23 |
| The `velnor` command names and output, `Velnor / Details` layout, Prometheus metrics | As shown above | Decided in one decision record after phase 0 |
