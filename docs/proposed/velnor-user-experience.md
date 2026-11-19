# How you use Velnor (draft for the owner)

**Status:** draft. It describes Velnor as you and the agents will use it, following the
[architecture](velnor-runner-architecture.md) and the
[delivery plan](../research/velnor-runner-2026-11/delivery-plan.md). Where the design leaves a
user-facing choice open, this draft proposes one and marks it **Proposed — owner to confirm**.
Command names, config keys and example output are proposals unless the text says the design names
them. Example output is illustrative: its numbers are not measurements.

## 1. Setting it up

### 1.1 On the Hetzner Kubernetes cluster

**What you do once.**

1. Create the GitHub App `velnor-verify` (owner question D7). Permissions: `statuses: write`,
   `checks: write`, `pull_requests: read`, `contents: read`, `metadata: read`. Webhooks:
   `pull_request`, `merge_group`, `push` and `check_run`.
2. Create an agent identity that cannot change the ruleset (D7), and a deploy identity for the
   `velnor-system` namespace (D8).
3. Approve the node changes (D8): XFS work volumes, node reservations (3 CPU, 6 GiB), a 2 GiB
   eviction threshold, a PID limit of 16,384, taints for the nodes that run Docker tests, and one
   drive or partition per worker for RustFS.
4. Deploy with `velnor deploy --site de`, then `--site fi`. It applies digest-pinned, attested
   images to `velnor-system` and reads back what it applied. **Proposed — owner to confirm** (the
   command; the design says only "deploy to `velnor-system`").

**What gets installed.**

| Part | Runs as | What it does |
|---|---|---|
| `velnor-control` | one per repository, in its home site | Receives webhooks, runs the label flow, writes `Velnor / Verified` |
| `velnor-dispatch` | one per site | Talks to GitHub for Actions jobs (release, image, scheduled) and places all work |
| Node agent | one per worker (DaemonSet) | Keeps warm sandboxes, mirrors, tool store and build snapshots; observes and signs results |
| Signer daemon | on each worker, outside every sandbox | Holds the key that signs results |
| Ledger and cache index | site services | Remember every verified result and every `actions/cache` entry |
| RustFS | one cluster per site | Stores large blobs: cache chunks, artifacts, release bytes |
| Mirrors | per worker and per site | Git mirror on every worker; crates mirror and image registry mirror per site |
| Official runner image | kept | Used as the reference, and when you choose the official executor |

ARC is removed for velnor-new at the final milestone (day 6.5). ChainArgos and jackin keep their
ARC scale sets.

### 1.2 On a Mac with Docker

1. Install Docker Desktop.
2. Install the `velnor` binary with `cargo install`, as you install other tools on macOS.
3. Run `velnor local up`. It starts the node agent as a launchd service, creates a few warm
   containers and a local store, and mounts the tool store read-only.

**Proposed — owner to confirm:** the commands `velnor local up` and the install by `cargo install`.

What is different on a Mac: storage is `local` only (no RustFS); there is no file-access audit, so
keys stay whole-tree; Docker tests get a private daemon container or are refused, never the host's
daemon. A Mac is for running jobs locally and for conformance. **A Mac result never counts as CI
evidence**, and its keys differ anyway, because the machine's kernel and OS are part of every key.
The same steps work on any Linux host with Docker.

## 2. The configuration

Velnor reads its configuration from the **base commit** (the `main` your PR targets), never from
the PR under test. A change to it takes effect only after it lands, through the normal gate.

**Proposed — owner to confirm:** repository settings go into the existing `.velnor/config.toml` as
new `[velnor.*]` tables; site settings (platform, storage backends, capacity, warm pools) go into a
new `.velnor/sites.toml`; personal Mac settings go into `~/.config/velnor/local.toml`. All keys
below are proposed (work item G05 writes the real schema). Values come from the design unless a
comment says "proposed value".

```toml
# ---- .velnor/config.toml (Velnor part) -------------------------------------------------
[velnor]
schema = 1
home_site = "de"                     # proposed value: where the status writer and cache index live

[velnor.label]
name = "velnor:verify"               # the design's working name
arm_from_draft = false               # D16 default; the recommendation is true

[velnor.executor]
default = "native"                   # "native" (Velnor Native) or "official" (GitHub Official)

# Only certified identities are accepted. The identity is part of every result key,
# so switching re-verifies once and can never reuse the other executor's results.
[velnor.executor.native]
image = "ghcr.io/tailrocks/velnor-exec@sha256:<digest>"
[velnor.executor.official]
runner_lock = ".velnor/runner.lock"  # the official runner pin that exists today
image = "ghcr.io/tailrocks/velnor-official@sha256:<digest>"

[velnor.executor.job_types]          # Actions jobs that stay jobs: release, image, scheduled
"release-rust-ci-tools" = "native"
"product-release"       = "native"
"job-image-candidate"   = "native"

[velnor.executor.obligations]        # PR and merge-group checks, per kind
fmt = "native"
clippy = "native"
doc = "native"
test = "native"                      # "official" is refused until its obligation mode passes conformance

# Each data class picks its tiers, fastest first. A tier the platform lacks is skipped.
# A wrong route can only make things slower: a missing or bad blob is a miss.
[velnor.storage.routes]
actions_cache = ["local", "site", "cross-site"]
artifacts     = ["local", "site"]    # also uploaded to GitHub, so `gh run download` works
release       = ["site"]
mbx_cache     = ["site"]
generations   = ["local", "site"]    # all routes stay "local" until the RustFS cluster test passes

[velnor.mirrors.repositories."tailrocks/velnor-new"]
refs = ["refs/heads/*", "refs/pull/*/head", "refs/heads/gh-readonly-queue/*", "refs/tags/*"]
refresh = "webhook"                  # plus a periodic catch-up
catch_up = "5m"                      # proposed value
seed = "site-bundle"                 # new or wiped nodes seed from the site store, not GitHub

[velnor.mirrors.crates]
verify = "lockfile-checksum"

[velnor.mirrors.toolchains]
from = [".velnor/rust-toolchain.toml", ".velnor/mise.lock"]   # pinned by digest

[velnor.mirrors.images]
pin = "digest"                       # through a registry mirror per site

# ---- .velnor/sites.toml -----------------------------------------------------------------
[sites.de]
platform = "kubernetes"
namespace = "velnor-system"

[sites.de.storage.tiers]
local      = { backend = "filesystem", path = "/var/lib/velnor/l0" }   # node NVMe on XFS
site       = { backend = "s3", kind = "rustfs-cluster", endpoint = "http://rustfs.velnor-system.svc:9000",
               durability = "strict", credentials = { secret = "velnor-rustfs" } }
cross-site = { backend = "replication", from = "fi" }                    # background warmth only
# Any S3 store works the same way, e.g. { backend = "s3", kind = "aws", bucket = "..." }.

[sites.de.capacity]
priorities = ["merge", "gate", "pr", "batch"]       # only batch is preempted
build = { cpus = 16, memory = "24GiB", max_per_node = 4 }
gate  = { cpus = 1, memory = "1GiB" }               # memory: proposed value
reserved = { merge_build_slots = 1, merge_gate_cpus = 2, audit_build_slots = 1 }

[sites.de.warm_pool]                                 # proposed values
gate = 8
build = 2

[sites.de.budgets]
generations_per_root = 3
data_volume_share = 0.40
alert_at = 0.70
mbx_cache = "300GiB"

[sites.fi]                                           # same shape as "de"
platform = "kubernetes"

# ---- ~/.config/velnor/local.toml (a Mac) ----------------------------------------------------
[local]
platform = "docker"
docker_host = "unix:///var/run/docker.sock"
docker_tests = "private-daemon"      # or "refuse"
storage = { local = { backend = "filesystem", path = "~/Library/Caches/velnor" } }
build = { cpus = 8, memory = "16GiB" }               # proposed value
warm_pool = { build = 1 }                            # proposed value
```

**Proposed — owner to confirm:** the values marked "proposed value": home site `de`, a 5-minute
mirror catch-up, 1 GiB per gate sandbox, warm pools of 8 gate and 2 build sandboxes per site, and
8 CPU / 16 GiB with one warm sandbox on a Mac.

## 3. Workflows

**What stays the same.** Workflows are GitHub Actions YAML. `actions/cache` is used exactly as on
GitHub (`key`, `restore-keys`, `path`, `cache-hit`), with GitHub's branch rules. Artifacts use
GitHub's v4 service, so `gh run download` and the API still see them. Logs and summaries of Actions
jobs appear in GitHub's UI.

**What the generator does.** `velnor-gen` writes the workflows. It routes each job type to Velnor,
uses only actions from the qualified set at qualified commit SHAs (a generator test enforces this),
and replaces the three `actions/github-script` steps with `velnor-ci` commands. After the cutover
(day 5.2), PRs and merge groups have **no Actions run at all**: Velnor checks them directly. Release,
image and scheduled workflows stay Actions jobs that run on Velnor. A push to `main` keeps one small
Actions job, `Required`, that waits for the cold audit of that commit.

**The closed native action set.**

| Action | How Velnor Native runs it |
|---|---|
| `actions/checkout` | From the worker's mirror, with every file checked; sparse checkout and `fetch-depth` supported |
| `actions/upload-artifact`, `download-artifact` | GitHub's v4 service, cached on site storage |
| `actions/cache`, `/restore`, `/save` | Velnor's cache with GitHub's rules, stored on local disk and RustFS |
| `jdx/mise-action` | Pinned tools from the read-only tool store |
| `jdx/mr-boxington-action` | Plain cargo, until `velnor-gen` drops it |
| `actions/attest-build-provenance`, `actions/attest` | Native signing and upload |
| `actions/create-github-app-token` | A scoped token, masked, revoked after the job |
| `asamarts/alint` | The pinned release |
| Composite actions in `.github/actions/` and `actions/setup-velnor` | Full composite rules, including background steps |

**An unknown action.** Velnor Native refuses the job **before its first step**: an action, a SHA or
an input outside the qualified set. The job fails with a message naming the action. The fix is to
qualify the action (add a native implementation with conformance cases) or not use it. Velnor never
falls back to the official runner on its own. The official executor runs JavaScript actions on
Node by nature, but choosing it is a deliberate config change, and the generator still emits only
qualified actions.

## 4. The PR flow, step by step

| Step | What happens | What `Velnor / Verified` shows |
|---|---|---|
| Open a draft, push many commits | Nothing runs | No status. GitHub shows "Expected — waiting for status", which blocks |
| Mark it ready for review | Nothing runs | No status |
| Add the `velnor:verify` label | Velnor checks your head merged with the merge-queue tip (or `main`) | `pending`, then `success` or `failure` |
| Push again while labelled | The old run is cancelled at once; the new head is checked, reusing every unchanged result | No status on the new commit until written |
| Remove the label | Velnor cancels and stops. Nothing is written | Unchanged: a verdict stays true for its commit |
| "Re-run" on `Velnor / Details` | Re-runs only the failed checks of that commit | `pending`, then the result |

**The four states of the check.**

| State | GitHub status | Description text (**Proposed — owner to confirm**) |
|---|---|---|
| Not requested | none (blocks) | `Velnor / Details` is `neutral`: "Not requested: add velnor:verify" (**Proposed — owner to confirm**) |
| Running | `pending` | "Verifying: 12 to run, 29 reused" |
| Passed | `success` | "Verified: 41 checks (29 reused, 12 run twice)" |
| Failed | `failure` | "2 checks failed: velnor-ci tests" |

`Velnor / Details` is a check run with the full report. No ruleset lists it, so it never gates.

**Merge queue and auto-merge.** Turn on auto-merge when you open the PR. When the head is green,
GitHub puts the PR in the queue. Velnor checks the group's commit, reusing every result whose key is
the same (about 60–75% of groups match the tree the PR passed, estimate), and runs the rest twice. Checks that read the clock or network always run. On `success`
the group lands. On `failure` GitHub removes the PR from the queue; the PR's own green stays, and
you fix and re-enter. After landing, Velnor rebuilds the commit cold (audit A1, within 2 hours at
P90, target). Releases wait for that audit.

**Until the App gate is on (about day 4.2).** The label is checked by an Actions job that fails with
"CI not requested". In that period any label change re-runs the workflow, and removing the label
turns a verified head red. So agents add the label only when they create the PR.

## 5. The CLI

The design names `velnor verify` and the "run both and diff" mode. **Proposed — owner to confirm:**
one `velnor` binary with the commands below, their names and their output.

**`velnor verify` before pushing** (a stretch item, about day 16–18). It runs your commit, merged
with the queue tip, in the same sealed Linux sandbox and with the same keys as CI, on the cluster.
Its results are stored as candidate records, so when you push and label, the PR reuses them and
the verdict follows within about 10 s (target). **Proposed — owner to confirm:** the commit reaches
the cluster as an uploaded bundle, not as a pushed branch; and `--local` runs on your Mac for quick
feedback that never counts.

```console
$ velnor verify
Verifying 3f9a2c1 (merged with queue tip 5ae9378) on site de
  reused  38   ran 3 (2 witnesses each)   fresh 1 (reads the clock)
  ✓ fmt  ✓ clippy:ci-helper  ✓ test:velnor-ci (412 tests)
Verified in 41 s. Push and label: the PR will reuse these results.
```

**Run one check by name** (from the released inventory at the base commit; an unknown name is
refused). `--target` picks where it runs: `local` (Docker on this machine), `k8s://de`, `k8s://fi`;
the trust tier comes from the node that ran it, never from the target name:

```console
$ velnor check -l
fmt  clippy:ci-helper  clippy:generator  test:velnor-ci  test:velnor-gen  fresh:workflows  …
$ velnor check test:velnor-ci --target local
✓ test:velnor-ci (412 tests) on local (advice only: a Mac result never counts)
```

**Agent environments** (proposed; Docker first, cluster environments wait on capacity): `velnor env
new <task>` makes a worktree and a sticky sandbox with its own target directory, temp dirs and
ports, and a command log; `velnor env push` publishes the branch. Environments never mint.
`velnor mcp` offers `env`, `check`, `verify` and `explain` as MCP tools beside `--json`.
`velnor lock update` is the only command that resolves inputs live; it opens a PR.
`velnor trace <sha>` renders the request's trace (every stage timed).

**Run a job locally** (on the Docker backend; work item P07):

```console
$ velnor run ci.yml:check-generator-samples
Using executor native@sha256:ab12…, platform docker (local)
  ✓ checkout (0.4 s)  ✓ mise  ✓ cargo nextest run (88 passed)
Job succeeded. This local result is not CI evidence.
```

**Run both executors and diff** (reference mode; it writes no verdict, cache entry or release):

```console
$ velnor diff ci.yml:check-generator-samples
native   succeeded  31.2 s
official succeeded  44.8 s
Compared outcomes, outputs, env, annotations, cache and artifact effects: 0 differences.
```

**Why something re-ran or was reused:**

```console
$ velnor explain 1412 test:velnor-ci
test:velnor-ci  RAN (2 witnesses: velnor-12, velnor-31)
  changed input  ci-helper/crates/velnor-ci/src/plan.rs
$ velnor explain 1412 clippy:generator
clippy:generator  REUSED  record from PR #1409, trusted, epoch 4
  same: tree for generator/ (contract v3), toolchain 1.99.1, executor native@sha256:ab12…
$ velnor explain 1415 test:velnor-gen
test:velnor-gen  RAN  miss on one field: toolchain 1.99.1 → 1.99.2
```

**Caches and mirrors:**

```console
$ velnor cache ls --ref refs/pull/1412/merge
cargo-registry-3b1e…   212 MiB   L0 de/velnor-12, L1 de   last used 4 min ago
$ velnor cache evict cargo-registry-3b1e…          # owner identity only
$ velnor mirror status
tailrocks/velnor-new   de 9/9 workers current   fi 6/6 current   last fetch 3 s ago
crates                 0 downloads in the last hour (lockfile unchanged)
```

**Status:**

```console
$ velnor status
Gate        velnor-control de (epoch 4, writer lease held)   canaries: all blocked ✓
Queue       2 groups building, 1 waiting                     A1 lag 22 min (SLO 2 h)
Capacity    de 61% CPU, fi 48%; merge lane free
Conformance native = official: 100% (de, fi, docker)        last official release check 2 d ago
```

## 6. What you see

**The check summary** (`Velnor / Details`). **Proposed — owner to confirm:** the layout.

```markdown
### Verified 3f9a2c1 in 38 s (would have been ~9 min without reuse)
| | Checks | Why |
|---|---|---|
| Reused | 29 | same key verified in PR #1409 and on main |
| Ran twice | 12 | inputs changed (ci-helper/crates/velnor-ci) |
| Fresh | 1 | reads the clock: always runs |
Dropped tests: none. (A test missing compared with main fails the check unless the owner signed it.)
```

**Logs.** Actions jobs log to GitHub as today. For checks that are not Actions jobs, the summary
shows the failing tail and links to the full log; `velnor logs <sha> <check>` prints it.
**Proposed — owner to confirm.**

**Dashboards and metrics.** The design defines a daily report (work item B15). **Proposed — owner to confirm:**
each service also exports Prometheus metrics, and `velnor status` reads them.

| Metric | What it tells you |
|---|---|
| PR verdict time, P50 / P90 | Label or labelled push to `success` |
| Push-to-merge, P50 / P90 | The main goal |
| Time to first actionable red | How fast a failure reaches you |
| Enqueue time, queue residence, landing | Where merge time goes |
| Runner-minutes per merged PR | Split into completed, cancelled and superseded work, twins, audits, releases |
| Cache hit rates | Per tier (L0, L1, L2) and per data class; bytes from GitHub against bytes from the LAN |
| Reuse rates | Records reused against run, per check kind |
| Conformance | Pass rate per backend and executor; mutants caught; days since the last official release check |
| False failures (flakes) | Per required run, with an interval; budget 3%, then 1% |
| A1 audit lag | Time from landing to the cold audit |

## 7. When things go wrong

**A red check.** Open `Velnor / Details`. It names the failed check, the failing test IDs and the
log tail. Run `velnor run` locally to reproduce, fix, and push: with the label on, the new head is
checked automatically. Failed results are never reused.

**A flaky test.** There are no automatic retries. Press "Re-run" on `Velnor / Details`; if it
passes, it counts as a false failure. Two false failures of one test in seven days open an issue
for the owner. The test is fixed, or quarantined only if another blocking check covers the same
thing, or under an owner exception that expires.

**A broken executor** (Velnor itself fails jobs). Velnor never switches executor on its own.
Changing the executor in config also cannot help at once, because that change must pass the gate
too. The recovery path (D15): (1) redeploy the previous image digest; this is a deploy, not a code
change, and then the fix PR is checked like any other; (2) if no earlier image can check it, the
pre-approved break-glass verifier runs every check cold on one node with the reference-qualified
executor, and writes `success` only if all pass. There is never an admin merge.

**A stuck queue.** Run `velnor status`. Velnor re-reads GitHub's state every 10 s, so a lost webhook
heals itself. A merge group that gets no answer times out after 90 minutes (15 minutes once group
verdicts take 5 minutes or less at P90) and GitHub removes it. If the ledger is unreachable, checks run fresh: slower, never wrong. If
storage stalls, reads become misses. A cold audit late by 2 hours alarms; at 6 hours it pages you
and holds releases.

**A reused result turns out wrong.** Velnor stops writing `success`, marks every open head or queued
group that relied on it as `failure` (GitHub ejects those groups), re-checks them, and audits every
commit that landed in the window cold.

## 8. Switching things

| You switch | How | What changes | What never changes |
|---|---|---|---|
| Executor for a job type or check kind | Edit `[velnor.executor.*]` and land it | That type re-verifies once (the executor is in the key), then runs on the other executor | The gate, the two runs, the audits |
| Storage tier for a data class | Edit `[velnor.storage.routes]` | Speed and hit rates | `actions/cache` behaviour and scoping; PR writes never reach `main`'s scope |
| Storage backend | Edit `.velnor/sites.toml` (RustFS, MinIO, AWS S3, R2) | Speed, operations | Content is checked on every read |
| Platform | Run on Kubernetes or Docker | Speed; which shortcuts are available (no file-access audit on Docker means whole-tree keys) | Results from different platforms never mix: the machine is in the key |

No switch can make a check pass that would fail. The worst a misconfiguration can do is make
things slower.

## 9. What agents do differently

- **Push freely.** Drafts and unlabelled PRs run nothing, so intermediate commits cost nothing.
- **Verify before pushing** with `velnor verify` (once it exists, about day 16–18), so the PR's
  verdict is mostly reuse.
- **Label deliberately.** Add `velnor:verify` when the PR is ready, with the agent's App identity,
  never `GITHUB_TOKEN`. Under D16's default, a label added during draft does not start CI at
  "ready"; re-apply it. Nothing adds the label automatically.
- **Turn on auto-merge at creation, and never merge `main` into the branch.** The queue already
  tests against the latest `main`.
- **Editing the PR text is free.** The `edited` trigger is gone.
- **Stacked PRs.** The upper PR reuses every result of the lower one with the same key, so it runs
  only its own changes. **Proposed — owner to confirm:** label the upper PR at any time (it is
  checked against its head merged with the queue tip). Merge it only after the lower PR lands and
  GitHub retargets it to `main`, because the ruleset guards only `main`.
- **Never bypass.** Agents use an identity that cannot change the ruleset, and no procedure uses an
  admin merge.
