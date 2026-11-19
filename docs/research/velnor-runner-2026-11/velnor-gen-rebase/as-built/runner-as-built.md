# As-built report: the runner side

| Field | Value |
|---|---|
| Model | Claude Opus 5.5 (`claude-opus-5-5`) |
| Client | Claude Code 2.1.325 (general-purpose subagent) |
| Reasoning effort | high |
| Date | 2026-11-20 |
| Repository state | main at `4fc0ede34e`–`a619478fb2` unless the text says otherwise; #1747 (old engine deleted), #1274, #1335 and #1748 merged after it |

Read-only evidence for the velnor-gen rebase round. Written by a subagent from the code, `gh` and git; unedited except for this header and replacing home-directory paths with `~`.

---

# B. Runner side as built: velnor-new main tip 4fc0ede34e (2026-11-20) vs research base 95de9b704b (2026-11-18)

Checkout: `~/.cache/ci-ambition/main-tip` (detached). All paths below are relative to it.
Compared with `git diff/log 95de9b704b..HEAD` (269 commits, 7,049 files; most of the file count is the
crate-layout restructure and regenerated goldens).
Read-only. No GitHub or cluster state was queried; "open" status of issues is therefore NOT verified
(only what docs/commits say). Items I could not establish are marked UNKNOWN.

Confidence tags: [C] read from code at the cited path; [D] stated in a doc (not proven by code);
[E] my inference from code/docs; [M] measured number quoted from the in-tree evidence pack.

---------------------------------------------------------------------------------------------------

## 0. Headline (what is true today)

1. **Velnor has no Rust job executor and no Rust dispatcher in production.** Every job that runs on a
   Velnor-labelled runner is executed by GitHub's unmodified official C# runner (`Runner.Listener`/
   `Runner.Worker`, pinned 2.338.0, `.velnor/runner.lock`). `docs/goals.md:16` says so as a rule:
   "Jobs are always executed by GitHub's official runner". `docs/proposed/macos-scaleset-runner.md:37-39`:
   "Velnor does not implement an Actions interpreter and does not ship a Go sidecar."
2. **Production CI for velnor-new itself runs on ARC, not on velnor-host.** Evidence pack
   (`docs/research/velnor-runner-2026-11/evidence.md:21-22, 533-553`) [M, snapshot 2026-11-18]: Hetzner k3s,
   5 workers (96 threads, ~125 GiB), Go `gha-runner-scale-set` chart 0.15.1 + controller in `arc-systems`,
   three scale sets (tailrocks max 60, ChainArgos 60, jackin 40), each runner pod = official runner
   container (`ghcr.io/tailrocks/velnor-ci-runner:2026-11-16.2`) + privileged `docker:dind` sidecar, all
   volumes `emptyDir`. velnor-new's generated `rust-ci.yml` targets label `ubuntu-26.04-scale-set` for
   same-repo events and falls back to `ubuntu-latest` for fork PRs (`.github/workflows/rust-ci.yml:57`,
   `.velnor/rust-ci.toml:4`). None of the ARC/Helm/Kustomize deployment lives in this repo (no `deploy/`
   dir exists; the evidence pack cites an outside checkout `~/Projects/tailrocks/velnor-runners`).
3. **What the Rust runner code (`crates/velnor-runner-*`) is:** `velnor-host`, a macOS-first controller
   that implements the *scale-set listener side* in Rust (register scale set, open session, long-poll
   messages, acquire, mint JIT) and then launches ONE official-runner container + ONE private dind
   container per job ("pair") on ONE local Docker engine, under a very heavy crash-safe journal and
   ownership-proof regime. ~190k lines incl. tests (host alone 181k). It is "built and heavily unit
   tested", live qualification last recorded 2026-10 with G4/G7/G8 not passed
   (`docs/implemented/macos-scaleset/checklist.md:21,27,28`). I found no in-repo evidence that
   velnor-host is serving any production queue on 2026-11-20 [E].
4. **Kubernetes/ARC are explicitly out of scope of the active runner spec**
   (`docs/proposed/macos-scaleset-runner.md:318-323`), but a *proposal* to add a `Runtime` trait with a
   Kubernetes backend landed 2026-11-19 (`docs/proposed/kubernetes-runtime.md`, design only, 495 lines,
   PR #1324), and a second, much larger *proposal* (`docs/proposed/velnor-runner-architecture.md`, 1159
   lines + `velnor-user-experience.md` 447 lines + `docs/research/velnor-runner-2026-11/`, PR #1365)
   describes the target in the brief (Rust dispatcher replacing ARC, Native+Official executors, platform
   trait, RustFS tiers, ledger, App status, `velnor verify`). **Zero lines of that target design are
   implemented.** The proposed new Cargo root `executor/` does not exist.
5. **velnor-local is a local GitHub emulation that runs the OFFICIAL runner against ChristopherHX's C#
   `runner.server`** (patched in a Dockerfile) plus Rust stand-ins (cache server, API stub, authority). It
   is a macOS/Apple-Silicon/OrbStack developer tool, not an agent verification service.
6. **Biggest change since 95de9b704b:** (a) four nested workspaces (`generator/`, `ci-helper/`, `local/`,
   `crates/velnor-runner/`) collapsed into one root workspace and flat `crates/velnor-*`
   (`23bed02e7e`, #1290); (b) the two design proposals above; (c) ~55 hardening commits to the runner
   (journal v52 -> v58, daemon-lock/lineage fixes, scale-set delete on disconnect, org-level scale set,
   Linux credential file); (d) velnor-local grew 16.4k -> 21.5k lines (resources, storage sizes, plain
   image, velnor-new scenario, dev `velnor-ci` injection, `up`/`down` fixes); (e) CI-side helpers grew a
   node-shared MBX store and PR build-delta reuse. No change to the "official runner executes jobs" rule.

---------------------------------------------------------------------------------------------------

## 1. Processes, who executes jobs, which protocols are in Rust

### 1.1 Process inventory

| # | Process | Where it runs | Language | Status | Evidence |
|---|---|---|---|---|---|
| P1 | ARC controller + 3 listener pods (`gha-runner-scale-set-controller` 0.15.1) | k3s ns `arc-systems` | Go (upstream) | PRODUCTION for tailrocks/ChainArgos/jackin | `evidence.md:536` [M] |
| P2 | ARC runner pod: official runner (`/usr/local/bin/velnor-runner-start`) + privileged `docker:dind` init-sidecar; emptyDir work/tmp/sock; requests 2 CPU/4Gi, mem limit 32Gi, no CPU limit | k3s ns `arc-runners*` | C# runner + Go dockerd | PRODUCTION; executes velnor-new CI jobs | `evidence.md:551-562` [M] |
| P3 | `crates-mirror` nginx DaemonSet (200 GB hostPath proxy cache of index.crates.io/static.crates.io, HTTPS 8443) | k3s ns `velnor-system` | nginx | PRODUCTION (opt-in via `[crates_mirror]`) | `evidence.md:574-576`; `.velnor/rust-ci.toml:24-27` |
| P4 | `velnor-host daemon run` (scale-set listener + dispatcher + launcher + journal + IPC controller, single process, per-user macOS LaunchAgent) | one Mac host, one Docker engine | Rust | BUILT; live gates not passed; no evidence of use on 11-20 | `crates/velnor-runner-cli/src/args.rs:106-231`; `docs/proposed/macos-scaleset-runner.md:104-106`; checklist rows G4/G7/G8 |
| P5 | `velnor-host` operator CLI (`connect`, `status`, `doctor [--probe]`, `logs`, `drain`, `resume`, `credential import-release-read`, `service install/start/stop/uninstall`, `daemon run`, `compare`, `journal show`, `disconnect`, hidden `local`) | same Mac | Rust | BUILT | `args.rs:21-107` |
| P6 | Per-job pair: official runner container (unprivileged, 14 caps, custom seccomp, pids/mem/cpu limits) + private dind container (privileged) + nonce-named volumes + network | same Docker engine | image from `images/runner`, `images/dind` | BUILT | `docs/proposed/macos-scaleset-runner.md:195-277`; `docs/proposed/kubernetes-runtime.md:51-58` |
| P7 | `velnor-job-start-hook` (runs INSIDE the runner container as `ACTIONS_RUNNER_HOOK_JOB_STARTED`; asks the host for a decision over a root-owned control dir `/opt/velnor/source-proof/{control,inbox}`, 120 s timeout) | in pair | Rust (`crates/velnor-resource-probe`) | BUILT | `images/runner/ubuntu-26.04/entrypoint.sh:68`; `crates/velnor-resource-probe/src/job_hook.rs:10-20` |
| P8 | `velnor-resource-probe` / `velnor-storage-init` guest helpers (cgroup, mountinfo, statvfs sampling; storage layout initializer; execution-channel publication) | in pair / probe container | Rust | BUILT | `crates/velnor-resource-probe/src/` (75 files, 11.9k lines) |
| P9 | `velnor-runner-attestation-helper` (Sigstore bundle + Fulcio claim check against pinned production TUF root; run as a separate process, never linked) | host | Rust | BUILT | `docs/proposed/macos-scaleset-runner.md:53,55-58`; `crates/velnor-runner-attestation/` (1.8k lines) |
| P10 | `velnor-ci` (in-workflow helper: plan, gate, report, delta, crates-mirror, mbx-store, release-gate, ...) and `velnor-gen` (workflow generator) | inside jobs / dev | Rust | PRODUCTION (installed by `actions/setup-velnor`) | `crates/velnor-ci/src/cli.rs:36-96`; `actions/setup-velnor/action.yml` |
| P11 | `velnor-local` (`up/run/down/build/doctor/status/conformance`, internal `supervise`, `cache-server`, `api-stub`, hidden `jit`) + containers `<name>-standin`, `-cache`, `-api`, runners | developer Mac | Rust + C# runner.server | BUILT, dev tool | `crates/velnor-local/src/cli.rs:20-60`; `local/README.md` |
| P12 | Old engine `velnor-actions-*` + `velnor-archive-guard` ("maintained, not developed") | CLI/CI | Rust | legacy generator, still builds | `AGENTS.md` (root) edit in diff; `docs/goals.md:12` |

There is no process in the repo that is a Rust dispatcher over a cluster, a node agent, a ledger service,
a GitHub App webhook receiver, or a daemon an agent can call.

### 1.2 Who executes jobs today

- **Production (velnor-new CI, ChainArgos, jackin):** the official runner in ARC pods (P2). Job
  selection, scaling, JIT minting, listener long-poll are done by ARC's Go listener (P1) [M].
- **Mac (`velnor-host`) path:** the official runner in a pair container; Velnor owns only the outside
  (listener, admission, ownership, cleanup). `docs/proposed/macos-scaleset-runner.md:37-39`,
  `docs/deferred/README.md:3-9`.
- **Local (`velnor-local`):** the official runner (2.338.0, linux/arm64) fed by runner.server (C#) in
  either `pair` mode (velnor-host `local` subcommand) or `plain` mode (bare runner image).
  `local/README.md:3, 91-100`.
- **Historical "native" runtime is explicitly not built.** `docs/deferred/self-hosted-runner.md:3, 19, 22,
  276-297, 366` (V2-native "Not implemented"; "MUST NOT be simulated by launching the official runner"),
  superseded by active spec which chooses official-runner-only. The evidence pack independently records
  "Its V2-native stage is documented as superseded and not implemented" (`evidence.md:27`).
  velnor-legacy (804k lines, native broker/run-service/Results client) is archived and not in this tree
  (`evidence.md:28`).

### 1.3 GitHub protocols implemented in Rust (velnor-new main tip)

Everything below is in `crates/velnor-runner-github` (8.2k lines, pure wire types, no HTTP stack, `serde`
only; HTTP is `reqwest 0.13`/`hyper 1.12` in `crates/velnor-runner-host/Cargo.toml:104-107`, transport
trait in `session/`).

| Protocol / endpoint | Rust? | Where |
|---|---|---|
| Scale-set CRUD: `_apis/runtime/runnerscalesets` (create with `disableUpdate`, get by name/id, delete, runner groups, label equality checks) | YES | `paths.rs:4`; `registration/scale_set.rs`; `registration/groups.rs`; `api-version=6.0-preview` at `session/retry.rs:15` |
| Registration/admin token (repo/org/enterprise paths) | YES | `registration/token.rs`, `registration/admin.rs`; org+runner-group added 2026-11-19 (#1330 `7be003b42a`) |
| Message session: `POST/PATCH/DELETE .../runnerscalesets/{id}/sessions[/{sid}]`, 401 single-flight refresh, reopen | YES | `session/open.rs:70,92`, `session/reopen.rs`, `session/close.rs:28`, `refresh.rs` |
| Message long-poll with `lastMessageId`, `X-ScaleSetMaxCapacity`, 202=empty, envelope `RunnerScaleSetJobMessages`, inner kinds JobAvailable/Assigned/Started/Completed, statistics | YES | `poll.rs`, `paths.rs:64`, spec `macos-scaleset-runner.md:123-163` |
| `acquirejobs` (array of int64 request ids, partial success, uncertainty classification) | YES | `paths.rs:14`, `acquire.rs`, `session/jobs.rs` |
| `generatejitconfig` (scale-set JIT; NOT classic repo `generate-jitconfig`) | YES | `paths.rs:19`, `runner.rs`, `secret.rs` (`EncodedJit`, max 131040 bytes) |
| Runner agent removal/absence proof (`_apis/distributedtask/pools/0/agents/{id}`) | YES | `paths.rs:6`, `registration/runners/absence.rs` |
| Actions REST used for *source authorization* of an assigned job: `actions/runs/{id}`, `.../attempts/{n}`, `git/commits`, repo identity, PR source, workflow content/ref, timeline of the run; plus a PR "nonce commit" (`velnor-pr-validation-nonce:v1:`) | YES (read-mostly) | `workflow_run/*.rs` (execution, source/pull_request, hook_source, nonce_commit.rs:11) |
| Broker long-poll (job message fetch after JIT) | NO (the official runner does it) | grep for `broker`/`runservice` in runner crates and velnor-local: no hits |
| Run-service `acquirejob`/`renewjob`/`completejob` (job lock, 60 s renew) | NO | `evidence.md:29` documents it, as the official runner's duty |
| Results twirp service (logs, timeline, step summaries) | NO | same |
| OIDC token service | NO | no hits |
| Cache service v2 (twirp `CreateCacheEntry`/`GetCacheEntryDownloadURL`/`FinalizeCacheEntryUpload` + Azure-style blob PUT/GET) | YES, but only as a LOCAL emulation (not production) | `crates/velnor-local/src/io/mod.rs:144`, `protocol.rs:22`; rules in `crates/velnor-cache-rules`; production cache is GitHub's |
| Artifacts v4 | NO (local mode forwards to runner.server's C# `ArtifactControllerV2`, one Dockerfile patch for `application/zip`) | `local/docker/runner-server/Dockerfile:9-10` |
| GitHub App auth for the Velnor service itself | NO. Credential = PAT-like secret in macOS Keychain or (new, Linux) a 0600 file; ARC uses its own App `tailrocks-arc-runners` | `crates/velnor-runner-host/src/keychain.rs`, `credential_file.rs`, `config/keychain_reference.rs:24-27`; `evidence.md:537` |
| Commit status / check-run WRITES | NO (only the old engine reads check runs; `velnor-ci release-gate` reads statuses) | `crates/velnor-ci/src/cli.rs:54-56` |

Pinned protocol authority: `actions/scaleset` `e6daac702355cdb5b880b4fbdcf6d85dcd9e48e5`
(`.velnor/runner.lock`, `macos-scaleset-runner.md:119`). The evidence pack used `af394d86`/v0.5.0
(`evidence.md:1121`), i.e. the repo pin and the panel's read are different scaleset commits [E: minor].

Notable behaviours of the Rust listener side (all [C] from the spec and code):
- One host-wide capacity authority, `max_jobs` default 1, qualification at N>1 still required
  (`macos-scaleset-runner.md:108-115`; `crates/velnor-runner-core/src/capacity.rs:45`).
- Single connection per repository, one daemon per state directory, one Docker engine
  (`macos-scaleset-runner.md:79-83`).
- It *adopts* a pre-existing matching scale set after a lost create response (#1110 `5387bc894a`), refuses
  one that names another runner group (#1260), and now deletes a set it created when disconnecting
  (#1412, journal v58).

---------------------------------------------------------------------------------------------------

## 2. velnor-local

### 2.1 What it is

`local/README.md:3`: "The local GitHub environment, a development tool that is not shipped. It runs
GitHub's official runner, unmodified, against `runner.server` on an Apple Silicon Mac, in OrbStack
containers, with a cache that follows GitHub's cache rules." Goals rule 11 ("Local first",
`docs/goals.md:75`): workflow verified on the developer's machine with the official runner started by the
Velnor runner, a local stand-in for GitHub and a local cache that enforces GitHub's rules; real GitHub
confirms afterwards.

Components (`local/README.md:5-10`, `local/docker/*`):
- **Stand-in = ChristopherHX `runner.server`** (C#/.NET 8), pinned commit
  `992ccbbbf9afcde477c38c316e053b1af457ad40`, built by `local/docker/runner-server/Dockerfile`
  with sed-based patches (anchor-verified, build fails when anchor moves): artifact `Content-Type`
  (line 9-10), `refs/pull/<n>/merge` ref_name/ref_type (11-20), `workflow` string-vs-object converter
  (`WorkflowObjectConverter.cs`), signed per-job cache-admin grant claim (`job-grant.cs.snippet`),
  and a timeout/cancel fix (`Task.Delay` instead of `WaitAnyCancellationToken`). So **there IS a C#
  runner-server in `local/docker/runner-server/`**, but it is third-party code patched at build, not code
  Velnor owns.
- **Runner:** actions/runner 2.338.0 linux-arm64 (`local/docker/runner/Dockerfile`) in `plain` mode, or
  the production runner image rebuilt for arm64 (`images/runner/ubuntu-26.04/README.md`, `velnor-local
  build`) in `pair` mode.
- **Rust pieces:** `cache-server` (Twirp+blob, answers from `velnor-cache-rules`, owns `github.localhost`,
  proxies everything else to runner.server), `api-stub` (answers the few GitHub REST calls `velnor-ci`
  makes: PR files, compare, contents, tarball, pulls list, actions caches list/delete, run/jobs/logs from
  runner.server; everything else 501 with JSON naming the path; `README.md:257-279`), `supervise`
  ("authority"), and the driver that posts events through runner.server's `Runner.Client`.
- **Authority** (`supervise`, 127.0.0.1 only, bearer token replaced on each `up`): ledger of dispatched
  events (`POST /ledger`), JIT mint for local runners (`POST /jit`), run facts, agent delete.
  `README.md:135`. NOTE the name clash: this "ledger" is the local event ledger that feeds the
  job-start check, NOT a verification-result ledger.
- **Pair mode** = `velnor-host --state ... local` (feature `local-mode`, refused in release builds)
  starting single-use pairs (`crates/velnor-runner-host/src/local/`, 3.9k lines) behind the SAME
  job-start check as production but fed by the authority instead of api.github.com
  (`README.md:91-100, 104-106`).

### 2.2 Scenario vocabulary (what "pair", "hub", "fork", "story" mean)

All are in `local/scenarios/*.json` ([C]; descriptions are the `description` fields).

| Word | Meaning |
|---|---|
| **pair** | A runner container + private dind container started by velnor-host per job. `--runner pair` (default) uses them; `--runner plain` runs the bare official runner image with no job-start check. `README.md:94, 131`. Pair = the same pair that production velnor-host runs. |
| **trusted / untrusted side** | Two networks, each with its own runner.server and cache server. Untrusted = fork PRs: restore-only cache, no secrets, no token. `README.md:137-139` |
| **warm** | push to default branch (saves caches), then PR touching `rust-toolchain.toml` (selects all lanes, no source change): expects no 3rd-party crate download/compile. `scenarios/warm.json` |
| **cold** | empty cache: PR appends toolchain comment, is squash-merged, push to default branch runs cold. |
| **docs** | PR changes only `docs/notes.md`: Rust jobs should be skipped. |
| **leaf** | PR touches `app/src/main.rs`, the crate nothing depends on. |
| **hub** | PR touches `core/src/lib.rs`, the crate that `lib` and `app` depend on (reverse-dep fan-out). |
| **lock** | PR bumps the single external crate (`fnv`) only the `tool` crate uses (lock-file-only change; `update_lock` step). |
| **fork** | one same-repo PR and one fork PR on the same workflow: runs on trusted and untrusted pair; fork job cannot reach the API stub. |
| **story** | merge PR1, open PR2, push to PR2 again, open PR3, close PR2: a multi-event, multi-PR session. |
| **hello / scheduled-run** | one push; used for `--policy` admission and for proving a job-posted second run is refused (`not_dispatched`). |
| **velnor-new** | push to default, PR touching `crates/velnor-archive-guard/src/lib.rs`, second push: recipe for velnor-new's own `rust-ci.yml` (added 2026-11-19, #1718). |
| **probe-*** | replays the lab's conformance probes locally (see 2.3). |

Hub/leaf/lock/docs/warm/cold are the lab (`tailrocks/velnor-lab`) scenarios of the *selection and cache*
promises of `docs/goals.md` (rule 2, rule 8, targets table). Steps vocabulary at `README.md:161-170`:
`push`, `commit`, `update_lock`, `open_pull_request {fork}`, `merge_pull_request`, `close_pull_request`,
`merge_group`, `schedule`, `workflow_dispatch`.

### 2.3 Conformance probes

- `local/conformance/*.json`: GitHub's real behaviour as measured by the lab's `lab probes` (checked there
  weekly): `cache-other-pr, cache-own-pr, cache-pr-reads, cache-restore-keys, cache-save-existing,
  event-payload, needs-result, token` (`README.md:230`).
- Replayed through `velnor-cache-rules` in `crates/velnor-cache-rules/tests/it/conformance.rs` (cache
  family + token) and through `velnor-local conformance --dir` against logs of a run
  (`README.md:232-243`): legs print `reproduced` / differences / `not observed`.
- Reproduced locally: `event-payload`, `cache-restore-keys`, `cache-pr-reads`, `cache-other-pr`,
  `cache-own-pr`. NOT reproduced (known, issue #632, lab-side fixes tailrocks/velnor-lab#289):
  `cache-save-existing` (probe reads api.github.com), `needs-result` (cancellation not modelled),
  `token` (runner.server signs different claims) (`README.md:240-243`).
- This is a conformance oracle for the *cache + event emulation*, not a differential harness between two
  executors.

### 2.4 Relationship to "agents verify locally"

- It is the only existing "verify before pushing" path, and it is human/dev-shaped: macOS Apple-Silicon
  only (`doctor` requires linux/arm64 daemon and a `unix://` socket, `README.md:39-41, 83-90`), needs
  OrbStack/Docker Desktop VM >= 9 GiB by default, ~minutes to build images (`velnor-local build`),
  Ubuntu labels all map to one image (Ubuntu 26.04 pair / 24.04 plain) so OS differences are invisible
  (`README.md:145`).
- It still reaches the internet for actions downloads, mise tools and the pinned `velnor-ci` release
  (`README.md:87-89`); `actions/checkout` is replaced by a local checkout of the copy.
- Verdict honesty: every verdict ends with "Executed / Stood in / Not exercised / Known difference" lines
  (`crates/velnor-local/src/verdict.rs:69-80`). E.g. pair mode: "Not exercised: scale set acquire, GitHub
  JIT, journal, GitHub runner deletion proof."
- The approved-workflows (job authority) check is OFF by default locally; `--policy` turns it on
  (`README.md:114-125`).
- It is a CLI tied to one scenario/run at a time (parallel scenarios in separate environments,
  `--parallel`), not a daemon with a socket/API an agent calls, no result cache keyed by tree, no
  signing, no status. It does not run on the cluster and cannot produce a "verification record".
- Production-vs-local parity gap: the helper `velnor-ci` is installed by `actions/setup-velnor`
  (release archive, SHA-256 pinned). Local mode substitutes a local variant via `--velnor-ci-binary`
  with a rewritten pin in the copy only (`README.md:56-62`, #1415/#1718).
- The README's "Run velnor-new's workflows / What does not work yet" section (`README.md:173-196`) is
  STALE against main: it still says there is no `--label` flag (line 147 documents it), no crates-mirror
  override (lines 198-206 document `VELNOR_CRATES_MIRROR=off`), and `rust-ci.yml` is not in the tree
  (`.github/workflows/rust-ci.yml` exists), and #1221 (race on replacement pair) is closed by #1706
  (`18be5b6d08`). No recorded run of the velnor-new scenario exists (`README.md:175`).

---------------------------------------------------------------------------------------------------

## 3. Platform backends, storage, caches, mirrors

### 3.1 Platform backends

| Backend | Status | Evidence |
|---|---|---|
| Docker (bollard 0.21.1) on one local engine, `unix://` socket only (embedded `unix://` in a path rejected, #1273) | BUILT; the ONLY backend of velnor-host | `Cargo.toml:50`; `docker_client.rs:98` (`connect_unix`); `docker_spec.rs` (`ContainerPlan`, `audit_plan`, `HOST_NEEDLES` at :47) |
| Docker as dind-per-job ("pair") | BUILT | see P6 |
| Kubernetes via velnor-host | NOT BUILT; proposal only (PR #1324) | `docs/proposed/kubernetes-runtime.md:3` "Design only; no code accompanies this document" |
| Kubernetes via ARC | PRODUCTION but outside this repo and not Velnor code | `evidence.md:533-566` |
| A platform trait | ABSENT. `docs/proposed/kubernetes-runtime.md:26`: "There is no trait today", ~5,150 lines mention docker, ~50 non-test files import `bollard`. `git grep "trait Runtime"` -> no hits in crates | [C] |
| Linux host | PARTIAL: Linux credential-from-file (`credential_ref = "file:/abs/path"`, 0600, O_NOFOLLOW, 4 KiB; #1329 `586faa21b8`), `cfg(target_os="linux")` branches in controller IPC / diagnostics. But the lineage anchor path is hard-coded `~/Library/Application Support/Velnor/runner-lineage` (`daemon_lock.rs:167`), service = launchd plist (`plist.rs`), and the spec calls a Linux-host controller out of scope (`macos-scaleset-runner.md:320-321`) | [C] |
| Apple Containerization / microVM | ABSENT | none |

Pair shape as built (`docs/proposed/kubernetes-runtime.md:51`, `macos-scaleset-runner.md:195-277`):
runner container: not privileged, 14 whitelisted caps, `worker/seccomp/runner-v1.json`, pids limit, tmpfs
`/tmp` & `/dev/shm`; dind container: privileged, own pids limit; shared `/run` socket volume; home, work,
dind-data, metadata volumes `w<nonce>[-home|-work|-docker|-metadata]` created per job with `NoCopy`, tmpfs
backed for home/work; storage layout initialized by an owned initializer before the pair starts; raw JIT
only on stdin/fd3 -> short-lived `ACTIONS_RUNNER_INPUT_JITCONFIG` on `exec` (never in env/labels/argv).
Four per-job budgets are mandatory host config (`runner_cpu_millicores`, `runner_memory_bytes`,
`dind_cpu_millicores`, `dind_memory_bytes`, `operational_memory_reserve_bytes`, no defaults;
`macos-scaleset-runner.md:202-211`). Docker named volumes have NO per-job size limit; "enforced storage
isolation remains unproven" (`:222-227`).

### 3.2 Storage

- **Per-job storage:** Docker named volumes / tmpfs, journalled (`StorageRole` RunnerRoot/Home/Work/Tmp/Shm,
  SharedRun, DindRoot/Data/Tmp/Shm; `StorageMedium` Disk|Tmpfs; `StorageFilesystemKind` Btrfs|Tmpfs|Other;
  `StorageInodePolicy` Finite|DynamicBtrfs|Unknown) at
  `crates/velnor-runner-host/src/journal/connection/reservation/storage_budget/role/types.rs:7-107`.
  btrfs awareness comes from the resource probe (a 2026-11-19 fix verifies the bound btrfs subvolume
  device before publication writes, #1389 `8518fbeb7a`) [C]. No reflink clone, no warm snapshot, no
  pool, no NVMe tier.
- **Storage abstraction (NVMe/RustFS/S3):** ABSENT. `git grep -i "rustfs\|nvme\|reflink"` outside
  `docs/research`, `velnor-runner-architecture.md`, `velnor-user-experience.md` finds only mentions in
  `docs/hard-decisions.md` (reflink in a different context) and old freshness evidence. The only "S3"
  concept is mbx's own cache modes (`mbx-cache`), consumed by `mise`/MBX not by Velnor.
- **Cluster storage today:** k3s `local-path` StorageClass exists but no PVs/PVCs; every runner volume is
  `emptyDir` on the node's XFS/LVM (`evidence.md:568-572, 560`) [M].

### 3.3 Cache backends actually in use for velnor-new

| Cache | What | Where defined |
|---|---|---|
| GitHub Actions cache (`actions/cache` v2 protocol served by GitHub) | four families: Tools (`velnor-tools-...`), Sources (`velnor-src-...`), Target (`velnor-tgt-...`), PR Delta (`velnor-dlt-pr<N>-...`); budget <= 8 GB of GitHub's 10 GB (goals targets, `docs/goals.md:59`) | `docs/rust-ci-cache-keys.md:7-14`; `crates/velnor-model` (`CacheKey`), `crates/velnor-render` (`render_key`) |
| PR build-delta reuse (new since base) | cumulative per-PR delta overlay, mtimes preserved, forks save nothing, >500 MB delta not saved, trusted `workflow_run` `rust-trim` job deletes older deltas | `7f16e1fe15` (#1107, part of #609); `velnor-ci delta` (`crates/velnor-ci/src/delta_cli.rs`) |
| MBX (`mr-boxington`) compiler cache | per-job local store; NEW opt-in `mbx_shared_store = true`: `velnor-ci mbx-store` points `MBX_CACHE_DIR` at `$VELNOR_MBX_SHARED_DIR` only on trusted events (push/schedule on default branch, merge_group) when the dir is absolute, symlink-free, writable; pod-private `MBX_SHIMS_DIR` 0700. Intended for "one persistent MBX compiler cache on the cluster's node NVMe" exported by the runner pod | `e79060d249` (#1411); `crates/velnor-ci/src/mbx_store.rs:10-50`; `.velnor/rust-ci.toml:7` (`mbx_shared_store = true`) |
| crates.io mirror | nginx DaemonSet in `velnor-system`, `sparse+https://crates-mirror.velnor-system.svc.cluster.local/index/`, CA at `/opt/velnor/crates-mirror/ca-bundle.pem`; `velnor-ci crates-mirror` writes cargo config; `VELNOR_CRATES_MIRROR=off` disables (used by velnor-local) | `.velnor/rust-ci.toml:24-27`; `crates/velnor-ci/src/io/crates_mirror*.rs`; doc says `evidence.md:576`: was rolled back from default 2026-11-16, opt-in wiring merged (#1002) and now enabled in velnor-new's own config |
| Local cache server | GitHub-rule emulator for local runs only | `crates/velnor-local`, `crates/velnor-cache-rules/RULES.md` |

No native `actions/cache` server in production; no content-addressed chunk store; no cache-key
prefetch; no L0/L1/L2 tiering.

### 3.4 Git mirrors

ABSENT. `git grep -i "git mirror\|--reference"` in crates/docs (excluding research) finds only
`docs/proposed/velnor-user-experience.md`. Checkout in production is `actions/checkout` over the network.
The architecture proposal's "node-local mirrors of the repository and every upstream" (`§7.4`, decisions
22-24) is design-only. The only mirror shipped is the crates.io nginx proxy above.

### 3.5 Warm pools / warm snapshots

ABSENT in code. ARC `minRunners` idle pods (4/4/2) are the only warm capacity (`evidence.md:541-544`);
velnor-host launches the pair on demand per job ("pairs are single-use", `README.md:89`). The Kubernetes
proposal's `NodeCache` PVC slots (`kubernetes-runtime.md:268-310`) are design-only.

---------------------------------------------------------------------------------------------------

## 4. State, journal, leases, doctor probes, lineage lock, cleanup proofs

### 4.1 State directories and locks (macOS-oriented)

- State: `~/Library/Application Support/Velnor/` (spec `macos-scaleset-runner.md:104-106`); logs
  `~/Library/Logs/Velnor/`; private Unix socket (`SOCKET_DIR_MODE`, `MAX_FRAME = 1 MiB`,
  `crates/velnor-runner-host/src/ipc.rs:6`); launchd per-user LaunchAgent running `daemon run` in the
  foreground, no double fork.
- **`daemon.lock`:** `DaemonLock::try_acquire_daemon(state)` takes an exclusive flock on the state
  DIRECTORY plus `daemon.lock` so a second daemon fails even if the file is unlinked (#1263 `a619478fb2`,
  closes #1117; `daemon_lock.rs:50`, `is_held` at :85, `lib.rs:71`).
- **Lineage lock ("EngineLineageGuard"):** per-engine lock + anchor file
  `…/Velnor/runner-lineage/engine-<key>.{lock,anchor}` binding {engine_id, journal_path, instance_id,
  revision} (`daemon_lock.rs:36-38, 136-170`; `ANCHOR_VERSION=1`, anchor <= 16 KiB). Purpose: one journal
  lineage per Docker engine; engine identity change stops automatic mutation
  (`macos-scaleset-runner.md:178-179`). Hardened in this window: fails closed when the lock file is
  replaced/unlinked (dev/inode check, #1289 `1d6195881c`), first bind survives leftover anchor temp file
  (#1294), refuses unresolvable `HOME` (#1249). `journal_meta.lineage_pinned` mirrors this in SQL
  (`journal_schema.rs:46`).
- Additional per-purpose locks (`admission.lock` and generic `DaemonLock::try_acquire`).

### 4.2 SQL engine and schema

- **Engine: Turso (`turso 0.8.3`, the Rust SQLite rewrite), not rusqlite/libsql**
  (`Cargo.toml:51`; `crates/velnor-runner-host/Cargo.toml:117`; `journal_schema.rs:4`). Dev profile
  compiles `turso_core`/`turso_parser` at opt-level 3 to make tests tolerable
  (`Cargo.toml:112-119`; #1474 `36b5e415b5`: slowest tests 387 s -> 61 s via shared journal fixture).
- **Schema:** table `intents` (row = one durable lifecycle intent; columns: kind, subject, state,
  docker_id, dind_id, launch_id, assignment_key, github_runner_id, acquire_attempted/resolved, acquired,
  jit_requested, runner_completed, worker_cleanup_proven, cleanup_proven;
  `journal_schema.rs:18-40`), `journal_meta` singleton (instance_id, engine_id, revision,
  lineage_pinned; :46), unique index on active `assignment_key` where `cleanup_proven = 0` (:80),
  `resource_probe_operations` with partial unique `one_active` per role
  (`journal_schema/resource_probe.rs:18`), `host_operation_leases` singleton lease row (:30), plus ~45
  schema modules (`journal/schema/*.rs`) for connection scope, request ledger/reservations, JIT
  claims & tombstones, post-JIT retirement, unbound retirement, pre-JIT cleanup, scale-set create/delete,
  registry image import, PR-source transition, header/capacity allocations, execution publication.
  Revision triggers increment `journal_meta.revision` on every write (v18/v20 triggers).
- **Versioning:** `PRAGMA user_version`, base = 52, now = **58** (`journal/schema.rs:26`). v53-v58 added
  in this window (e.g. v55 post-JIT effect fences `schema/post_jit_effect_fences.rs:9`, v56 bootstrap
  containers recorded absent only after proven gone #1398, v57 uncertain Doctor probe with aborted child
  #1399, v58 scale-set delete #1412). Unknown versions and malformed migrations fail closed
  (`macos-scaleset-runner.md:176-178`). Tests include "historical" migration fixtures
  (`bootstrap_historical_v45..v48`).
- **Size:** `journal/` alone 65.8k lines, `launch/` 48.8k, `host_controller/` 13.6k, `worker/` 6.8k,
  `controller_ipc/` 5.5k (line counts of `crates/velnor-runner-host/src/*/`, incl. test modules).
- **Protocol rule** ("persist intent, perform external call, record success or uncertainty; no DB
  transaction across HTTP/Docker; `Drop` does not free a durable slot"): `macos-scaleset-runner.md:165-168`.

### 4.3 Lifecycle, capacity, reconcile

- Worker states (`velnor-runner-core/src/lifecycle.rs:9`): Reserved, Acquiring, AcquisitionUncertain,
  **Acquired (distinct state, #1144 `580b4e1a3b`)**, Provisioning, IdleAssignable, Running, Finishing,
  Cleaning{owned}, Quarantined (keeps owned container+volume ids, #1145).
- `Capacity::reserve` never replaces a live slot (#1142 `f788d15cad`); a permit covers the job until cleanup
  is proven (`macos-scaleset-runner.md:108-115`).
- Startup reconcile: `reconcile::before_advertise` -> `Reconcile::{Hold{adopt,occupied}|Advertise}`
  (`reconcile.rs:138-141, 169`), adopts still-running owned workers; admission is opened only when the gate
  says Advertise (#1206 `17e8b85c86`); reconcile releases a slot only on PROVEN runner absence (#1309
  `c320d9d597`, `release_permitted` at :156).

### 4.4 Doctor probes and leases

- `velnor-host doctor` is read-only; `doctor --probe` starts a short-lived **owned probe container**
  (`velnor-resource-probe`) to measure guest CPU/memory/storage; tracked as `resource_probe_operations`
  (phases prepared ... removed|aborted, one active per role) under a single `host_operation_leases` row
  with states reserved/active/uncertain/recovering/released (`journal/doctor_probe/api.rs:104-186`).
  `ready` requires a bounded controller-owned snapshot (Docker ownership + journal + GitHub session
  agreement); a bare empty journal can never report ready (`macos-scaleset-runner.md:92-96`).
  Fixes this window: recover a settled probe child with no lease row, refuse missing v56 guard (#1527
  `cee8ecafa7`), uncertain probe with aborted child recovers (#1399).
- "Leases" in this codebase = (a) the `host_operation_leases` singleton for probe/helper operations,
  (b) per-reservation capacity permits, (c) `ProbeLeaseState`. There is NO lease on a GitHub job lock
  (the official runner does job-lock renewal) and no cluster-level lease.

### 4.5 Ownership and cleanup proofs

- `CleanupProof::covers` requires a full 64-hex container id AND a non-empty named volume equal to the
  owned ids (`velnor-runner-core/src/ownership.rs:31-34`; short-id acceptance fixed #1275 `227673da85`).
  `OwnershipFailure::{IdMismatch,MissingResponse,ForeignObject}`.
- Rules (`macos-scaleset-runner.md:275-277, 110-115, 181-190`): delete only objects whose immutable id
  matches the journal; names are not delete authority; foreign objects survive; missing delete response is
  not success; no host-wide prune, no prefix delete; Docker 404 or explicit `exited`/`dead` required
  before absent/stopped; empty `Status` = uncertain; pre-JIT bootstrap recovery records an immutable target
  identity per object and retires with fences so a late-published create can never bind.
- `docker_spec::DeleteDecision` {Delete, KeepForeign, NotDeleted}; spec audit forbids host-path mounts,
  `docker.sock` outside a private volume, JIT material in env/cmd/labels, privileged runners
  (`docker_spec.rs:47 HOST_NEEDLES`, `docs/proposed/kubernetes-runtime.md:51`).
- Cleanup observer: `StorageCleanupObserverWitness` unlocks storage-removal journal transitions; its
  absence proof first re-captures engine identity == journaled `engine_id` + `docker_root_digest`
  (`kubernetes-runtime.md:181-187`).
- velnor-local has a separate lighter audit: `cleanup.jsonl` with intent + confirmed outcome per removal,
  label-filter-only deletion, repair of a torn final record, refuse on a corrupt complete record
  (`README.md:106-110`; `crates/velnor-local/src/io/cleanup.rs`; #1174, #1343 `75f8119299`).
- Job trust inside the pair: the job-start hook challenge/decision files carry reservation id, runner id,
  container ids, engine id, root digest, policy generation/digest, channel digest
  (`velnor-resource-probe/src/job_hook.rs:30-48`); the "approved job authority policy" JSON binds repository
  id + workflow path + sha256 + event/ref (+PR rules) and is configured at `connect`
  (`args.rs:121-200`). Fork PRs and `merge_group` are never approved under policy (local README :125).

---------------------------------------------------------------------------------------------------

## 5. What changed since 95de9b704b, and what is still open

### 5.1 Structural

| Change | Evidence |
|---|---|
| One root workspace: `crates/velnor-runner/crates/*` -> `crates/velnor-runner-*`; `local/crates/*` -> `crates/velnor-local`, `crates/velnor-cache-rules`; `generator/`+`ci-helper/` -> `crates/velnor-gen,-ci,-model,-render,-rust,-release,-workflows,-freshness`; `.config/nextest.toml` overrides only for the generator crates | `23bed02e7e` (#1290/#1594); `local/AGENTS.md` (new, 13 lines); `docs/goals.md:12` still says `generator/`, `ci-helper/`, `crates/velnor-runner`, `local/` [stale]; root `AGENTS.md` and `docs/roadmap.md:61,85`, `docs/deferred/*` updated by the same commit |
| Old engine's Rust CI lane code deleted after velnor-gen cutover (#1523) | `5d175c6b7f` (#1420); `fa1344bab0` moves freshness out of the old engine |
| Release tags: tools-v<ver> and runner-v groups via release-plz | `a463d33d56` (#731), `c5fe3184fe` (#1429), `0d1e600d3d` |

Line deltas (rs only, incl. tests): runner-host 171.1k -> 181.4k; runner-cli 4.0k -> 4.8k; runner-core 0.9k
-> 1.35k; runner-github 7.9k -> 8.2k; runner-qualification 3.6k -> 4.1k; resource-probe 11.5k -> 11.9k;
attestation 1.8k -> 1.8k; image-manifest 2.6k unchanged; velnor-local 16.4k -> 21.5k;
cache-rules 0.7k unchanged; velnor-ci now 76k.

### 5.2 Runner (functional)

- Org-level scale set with named runner group (#1330 `7be003b42a`; CLI flags `--org`, `--runner-group`
  at `args.rs:176-179`).
- Scale-set lifecycle: delete a set the controller created on disconnect (#1412, v58); keep id of refused
  created set (#1402 `96238ab67f`); adopt matching set after lost create response (#1481); a rejected
  create can post again (#1731 `0d49facba6`); reject responses naming another runner group (#1260).
- Linux credential source `file:` (0600) for a Kubernetes-Secret-style mount (#1329 `586faa21b8`).
- Daemon safety: unlinked `daemon.lock` (#1263), lineage lock replaced (#1289), noncanonical probe
  Docker directory (#1269), embedded `unix://` in socket path (#1273), host-wide drain counts unresolved
  session refreshes (#1696 `cb33e929c2`), drain waits ignore own bound session (#1401), accepted control
  sockets blocking on macOS (#1140), crashed/removed runner container fails the local host (#1185).
- Release/attestation: per-file attestations and reruns of runner release admitted (#1146/#1148 `f81765f5f6`).
- Multiplexed docker log frames with missing/truncated payload rejected (#1346 `4f9711990f`).
- Admission/recovery: legacy-gate Advertise (#1206), proven-absence release (#1309), post-JIT retirement
  claim stops a runner starting (#1283), storage initializer handoff failure recorded uncertain (#1361).
- `velnor-host` local-mode test lane added to the repo's own Rust CI (#1742 `25a1ddfdf6`;
  `.velnor/rust-ci.toml:44-50`).

### 5.3 velnor-local

#1415/#1718 (P4 of #627): `--label`, origin remote rewrite, plain image on the production base, cache
persistence flags, `--velnor-ci-binary` dev helper (linux-arm64 ELF checked, pin rewritten only in the
copy), `down` without `ps`, arm64; `--runner-cpus/--runner-memory`, `--home-size/--work-size/--tmp-size`,
Docker VM memory check in `doctor`, docker-only (`unix://`) refusal, `VELNOR_CRATES_MIRROR=off`,
`velnor-new` scenario; #1706 wait for replacement pair before next event (closes #1221); #1695 pin every
scenario branch, signed cache-admin grant from the job's calculated `actions` permission, split cache list
from delete; #1384 reject runtime tokens before `nbf`; #1388 plain-mode `up` waits for runners online; #1722
dind-driver recipe; #1174/#1343 down/inventory safety.

### 5.4 Docs/design added (all "proposed", none implemented)

- `docs/proposed/kubernetes-runtime.md` (#1324, `953f9a58f3`): one velnor-host, `Runtime` trait with
  `RuntimeKind {Docker, Kubernetes}`, `WorkloadRef/StorageRef/NetworkRef` (with UID), `Observation<T>
  {Present, AbsentProven, Unknown}`, `AbsenceProof`/`OwnershipProof`; Pod-per-job with `runner` + `dind`
  containers; node-local PVC cache slots (`ReadWriteOncePod`, `local-path`), `PlacementHint` node affinity;
  12-PR work breakdown (:446-470) and 9 owner questions (:472-). Keeps the official runner; keeps
  `docker_*` journal phases -> `runtime_*`. Mac stays Docker-only.
- `docs/proposed/velnor-runner-architecture.md` (v3, 1159 lines), `velnor-user-experience.md` (447), and
  `docs/research/velnor-runner-2026-11/` (#1365 `3e55bb3ea0`, 2026-11-19): owner decisions 1-24
  (`owner-decisions.md`): Rust-only, Native default + Official reference executor, K8s+Docker platform
  trait with capabilities, RustFS tiers, native `actions/cache`, node git/crates/registry mirrors, ledger,
  App `velnor-verify` writing `Velnor / Verified`, label flow, `velnor verify`/`ship`, ARC deleted for
  velnor-new at "day 6.5". The architecture doc says the Docker journal/launch machine is to be DELETED at
  the final milestone and its observation model ported (`architecture.md` §5.10, table row "velnor-host
  journal (Turso) ... Delete at the final milestone"), which conflicts with `kubernetes-runtime.md` §5
  ("Journal ... same SQLite tables ... stays identical"). Two incompatible directions are in-tree.
- `docs/implemented/job-trust-boundary.md` (new): "A step whose result or credentials matter runs in a job
  where no repository code has executed"; enforced for generated workflows by
  `crates/velnor-gen/tests/it/job_trust_boundary.rs`; lists remaining same-job gaps (cache saves, upload
  tokens, helper after repo code).

### 5.5 Open roadmap items (from docs; open/closed state NOT verified on GitHub)

`docs/roadmap.md` (132 lines) is a PROCESS document for the fast-Rust-CI roadmap issue **#574**; it contains
no item list (state is in GitHub issues labelled `fast-ci`, board "Velnor fast Rust CI", `roadmap.md:35-50`).
Its edits in this window: only layout wording (`:61,85`). Issue numbers visible in runner/local docs and
commits:

| Issue / PR | What | Status per repo |
|---|---|---|
| #574 | fast Rust CI roadmap root | open (roadmap run unattended) |
| #627 | P4 "velnor-local local runs" | #1415, #1718 merged "Part of #627" |
| #632 / velnor-lab#289 | conformance legs that differ (cache-save-existing, needs-result, token) | listed as not reproduced, README:240-243 |
| #1221 | race on replacement pair | closed by #1706 (`18be5b6d08`) |
| #1223, #1228-#1230 | add real `rust-ci.yml` to tree | file exists now (`.github/workflows/rust-ci.yml`); README text stale |
| #609 | cache P-series (PR delta) | #1107 merged "Part of #609" |
| #610 | FAST-4.4 budget-sized saves | `6ef4af0fd3` closes "gaps" |
| #624 | crate AGENTS.md etc. | `4e3667b5a9` "Done-when" |
| #1290 | restructure crates | merged `23bed02e7e` |
| #1324 | Kubernetes runtime proposal | doc merged; implementation 0 of 12 PRs (`kubernetes-runtime.md:446-470`) |
| #1365 | runner design panel | docs merged; implementation 0 |
| #1429 | tools-v / runner-v release groups | merged transition |
| #1435 | `velnor-ci plan` PR/fork/local diff selection | `d500c85085` merged |
| #1485, #1523 | generator validation / old-engine Rust lane delete | merged |
| #1263, #1289, #1527, #1399, #1412 ... | runner hardening | merged |

Still undone for the runner per the in-tree gates: live qualification G4-live (BLOCKED_EXTERNAL), G7-paired
and G8-merge (NOT_RUN), R1 hosted MBX roundtrip (FAIL), R3 daemon lifecycle / N>1 backfill / cleanup (FAIL:
Keychain ACL partition list could not be set non-interactively), R4-R8 (NOT_RUN)
(`docs/implemented/macos-scaleset/checklist.md:21,27,28,38-48`; last dated 2026-10-03/10-26).
`max_jobs > 1` qualification (`macos-scaleset-runner.md:108`) is unproven.

---------------------------------------------------------------------------------------------------

## 6. Gap analysis vs the brief's target design

Legend: EXISTS = shipped/usable code; PARTIAL = adjacent code or doc design that does a piece; ABSENT = nothing
in code (design doc may exist). "Design doc" refers to `docs/proposed/velnor-runner-architecture.md` (ARCH),
`velnor-user-experience.md` (UX), `kubernetes-runtime.md` (K8S), `docs/research/velnor-runner-2026-11/owner-decisions.md` (OD).

| # | Target element | Verdict | What exists | What is missing |
|---|---|---|---|---|
| 1 | **Rust dispatcher replacing ARC** | PARTIAL (client library + single-host controller only) | Rust scale-set client: sessions, long-poll, acquire, JIT, ack, refresh (`velnor-runner-github`); host controller with journal and reconcile (`velnor-runner-host`); org-level set (#1330). Design: ARCH §5.2 `velnor-dispatch` (6k-line budget, "one service per site"), OD 20 ("ARC is still deleted: Velnor's own dispatcher drives both executors") | No multi-node placement, no Kubernetes pod creation, no per-site service, one Docker engine and one repo connection only, `max_jobs>1` unqualified; runs only on a Mac; never run against the production queue; Rust does NOT implement broker long-poll, run-service acquire/renew/complete or Results (needed if the dispatcher is to run Native executor or to drive Official with Velnor-synthesised job messages, ARCH §5.3). ARC still serves all prod CI |
| 2a | **Executor trait with 2 executors behind one interface** | ABSENT | Only a hint in the deferred spec: `docs/deferred/self-hosted-runner.md:208-210` ("explicit `OfficialRuntime` and `NativeRuntime`... explicit `RuntimeKind` match") superseded; ARCH §5.3 shows the trait signature (`Executor::identity/run`, `Work`, `Role`) | No `Executor` code; `executor/` Cargo root absent |
| 2b | **Velnor Native Rust executor** (expressions, steps, composites, post steps, masking, cancellation, run service, Results; closed action set, no Node) | ABSENT | None. velnor-legacy (archived, outside tree) had broker/run-service/Results clients + expression engine (`evidence.md:28`). `docs/deferred/self-hosted-runner.md:366` "Native protocol: Not implemented". `docs/goals.md:16` and `macos-scaleset-runner.md:37` currently FORBID it (rule text must change; "Changing this file requires the owner's explicit approval", `goals.md:3`) | Everything |
| 2c | **GitHub Official as reference/selectable executor** | PARTIAL, and it is today's ONLY executor | Official runner 2.338.0 in pair containers, image pipeline (`images/runner`, `.velnor/runner.lock` with digests/checksums, `velnor-runner-image-manifest`, attestation helper), `velnor-host compare` fail-closed lane comparison (`args.rs:72-82`) hosted-vs-scale-set, local mode runs it with runner.server | No "run both and diff" differential harness against a native executor; no sampled shadow runs; no per-job-type executor choice in `.velnor/` (`.velnor/rust-ci.toml` only has `runner = "ubuntu-26.04-scale-set"`) |
| 3 | **Platform trait: Kubernetes + Docker** | ABSENT (design only) | K8S §2 is a concrete trait proposal (`Runtime` with `create_worker/start_worker/deliver/exec_helper/wait/logs/stop/delete/inspect/prove_owned/list_owned/cgroup_subjects`, `Observation`, `AbsenceProof`); ARCH §5.4 + OD 13/21 want capabilities (reflink, shared page cache, privileged dind, snapshot) and microVM extensibility. Docker side is implemented with direct `bollard` calls | No trait (`git grep "trait Runtime"` empty); no `kube` dependency; ~50 files import bollard; K8S plan PR1..PR12 not started. K8S proposal models "pair" (runner+dind) which ARCH §5.4 rejects ("core describes environments generically, never pod/container concepts") |
| 3b | Kubernetes backend actually running Velnor jobs | ABSENT in Velnor code; ARC provides it | ARC pods (see P2) | Velnor-owned pod creation, UID-precondition deletes, Pod-per-job with dind, per-node cache PVCs, NetworkPolicy/PSA hardening, seccomp distribution |
| 4 | **Storage abstraction (NVMe / RustFS / S3)** | ABSENT | Only per-job Docker volume accounting (`storage_budget/`), btrfs/tmpfs awareness, `VELNOR_MBX_SHARED_DIR` node-dir convention, GitHub Actions cache | No blob-store trait, no tiers, no RustFS/S3 client, no chunked content-addressed store, no site replication. Design: ARCH §7.1 |
| 5 | **Native actions/cache server** (GitHub semantics, branch scoping, cache v2) | PARTIAL (emulator, local only) | `velnor-cache-rules` (728 lines; read own ref/base/default, write own ref, immutable keys, evictions, fork PR read-only, `RULES.md`), `velnor-local cache-server` serving Twirp v2 + blob (archives as files + `index.json` under flock), conformance vs real GitHub for 6 probes, test data from real `@actions/cache` 6.1.0 | Not a production service; not backed by any storage tier; no authoritative index across nodes; the official runner on ARC pods still talks to GitHub's cache; no way (yet) to point production runner at it (ARCH OD 22 "Open: can Runner.Worker be pointed at a Velnor v2-compatible endpoint"); no prefetch, no chunk dedup |
| 6 | **Node git mirrors** | ABSENT | crates.io nginx mirror per node (DaemonSet) only | git repo mirrors, registry mirror, tool store; ARCH §7.4 |
| 7 | **Warm pools / warm snapshots / reflink clones** | ABSENT | ARC `minRunners` | all |
| 8 | **Verification ledger** (result records keyed by tree/toolchain/executor, witnesses, joins, promotion) | ABSENT | Adjacent: `velnor-ci gate` (required-check logic from `needs`, `crates/velnor-ci/src/gate.rs`), `velnor-ci plan` selection with explicit affected-selection rules, `merge-group-passed`, `release-gate` (reads statuses of exact commit), cache key model `CacheKey`; velnor-local authority "ledger" of dispatched events (name collision, different purpose); journal `request_ledger` (scale-set message ledger) | `velnor-ledger` crate, keys, records, signed receipts, `ensure`, epochs. ARCH §6 is design-only |
| 9 | **App-written commit status** (`Velnor / Verified`) | ABSENT | `.velnor/rust-ci.toml` `required_check = "Required"` (a normal Actions check produced by `velnor-ci gate`); old engine reads check-runs for review completion; `docs/implemented/job-trust-boundary.md` rule that verdict/token steps live in a job with no repo code | GitHub App, webhook receiver, label state machine (`velnor-control`), status writer, merge-queue handling. UX §1.1 lists App `velnor-verify` perms. Not started |
| 10 | **`velnor verify` / `velnor cargo` daemon for agents** | ABSENT | `velnor-local run` (dev CLI) is the nearest; `velnor-ci` helpers; `scripts/verify-local.sh` (a script that sets test env, referenced `roadmap.md:84`; diff shows `scripts/verify-local.sh` modified/trimmed in this window) | No `velnor` CLI/daemon, no request-only socket, no mTLS targets, no sealed sandbox, no snapshot-of-worktree upload; no `velnor cargo` (cargo wrapper through node cache). `git grep "velnor cargo"` finds nothing; `velnor verify` appears only in ARCH/UX |
| 11 | **Fork PR fence / trust scopes** | PARTIAL | Generated workflows route fork PRs to `ubuntu-latest` (hosted), same-repo to scale set (`.github/workflows/rust-ci.yml:57`); `trusted_event` rule for MBX shared store (`mbx_store.rs:30-38`); job-authority policy refuses fork/merge_group; velnor-local two-side model | OD 8 says DROP fork handling (only own people/agents open PRs); code and docs still carry fork handling heavily; conflict to resolve |
| 12 | **Conformance oracle** | PARTIAL | velnor-lab probes + `local/conformance/` + cache-rules replay (cache/event/token behaviour); `docs/proposed/rust-verification-contract.md` etc. | Executor differential harness `velnor-conformance`; official-vs-native corpus |
| 13 | **Observability (OTel traces per ensure)** | ABSENT | `velnor-ci report` writes `ci-metrics.json`; `velnor-ci bench` | OTLP exports |

### 6.1 Design-vs-code contradictions worth resolving before building

1. `docs/goals.md:16,20,22,75` and `docs/proposed/macos-scaleset-runner.md:37-39,318-323` assert
   official-runner-only and Kubernetes/ARC out of scope; OD 1/20 and ARCH assert Native default and ARC
   deleted. Per `goals.md:3` and `AGENTS.md`, goals changes need explicit owner approval.
2. K8S (keep journal and Docker "pair" -> Pod "pair") vs ARCH §5.10 (delete the Turso journal and launch
   machine at the final milestone; port only the observation model).
3. `velnor-host` is a macOS+Keychain+launchd-shaped single-host controller; its hardest invariants
   (single engine lineage, `~/Library/...` anchor, Keychain ACL) are not shaped for a cluster dispatcher.
4. The Rust wire crate has scale-set-level types only; broker/run-service/Results types would be new.
5. `goals.md:12` and several `docs/proposed/*` still name the old layout (`generator/`, `ci-helper/`,
   `crates/velnor-runner`, `local/`).
6. `local/README.md:173-196` is stale (see 2.4). `AGENTS.md`-adjacent: `docs/roadmap.md` (the process) says
   nextest `slow-timeout` setting exists for six crates.

### 6.2 What is genuinely reusable if the target is built (my [E] assessment, not a decision)

- Scale-set wire client and its uncertainty classification (`acquire.rs` `Certainty`, `TransportFail`).
- `velnor-runner-core` lifecycle/capacity/ownership types (small, pure; ARCH §5.10 "Port and refactor").
- Pair spec audit (`docker_spec.rs`), seccomp profile `worker/seccomp/runner-v1.json`, runner/dind images
  with pinned digests/checksums (`images/`, `.velnor/runner.lock`), job-start hook channel design.
- `velnor-cache-rules` + local cache-server + conformance data as the executable spec of GitHub cache
  semantics for a native cache service.
- velnor-local as the Docker-backend, Mac-local lane of "Docker platform" and as a conformance bench
  (runner.server is the only available local stand-in that issues real job messages to an official runner).
- `velnor-ci` / `velnor-gen`: plan/selection/gate/delta/mbx-store/crates-mirror are executor-independent.

---------------------------------------------------------------------------------------------------

## 7. Unknowns and cautions

- Whether `velnor-host` currently serves any queue anywhere: UNKNOWN; last recorded live evidence is
  2026-10 (ChainArgos on a Mac, G6 launchd PASS, G4/G7/G8 not).
- ARC scale-set settings, node tuning and the `VELNOR_MBX_SHARED_DIR` export are out-of-repo; the only
  in-repo observation is the 2026-11-18 evidence snapshot. The `ubuntu-26.04-scale-set` label used by
  `rust-ci.yml` is served by ARC per that snapshot (evidence.md:40, 541-544 label `[velnor]` +
  scale set name), not by velnor-host [E, consistent with `crates_mirror` hostname being cluster-internal].
- GitHub issue open/closed states, label `owner-action`/`blocked` lists, and the #574 board were not
  queried.
- `docs/research/velnor-runner-2026-11/*` reports facts as of 2026-11-18/19 and is itself a design panel
  output, not product documentation; numbers tagged [M, proxy] there are not re-verified here.
- Line counts include test modules (velnor-runner-host is 181k lines of which production code is a
  minority); do not read them as production size.

## 8. Key file index

- Active runner spec: `docs/proposed/macos-scaleset-runner.md` (323 lines)
- Superseded roadmap: `docs/deferred/self-hosted-runner.md` (382), `docs/deferred/README.md`
- K8S proposal: `docs/proposed/kubernetes-runtime.md` (495)
- Target-design proposals: `docs/proposed/velnor-runner-architecture.md` (1159), `velnor-user-experience.md` (447),
  `docs/research/velnor-runner-2026-11/{owner-decisions,decisions,delivery-plan,evidence}.md`
- Gate status: `docs/implemented/macos-scaleset/checklist.md`
- Rules: `docs/goals.md` (95), `docs/roadmap.md` (132), `docs/host-deployment-decision.md` (23; Ansible deploys
  stay operator-run, no generated deploy job, unrelated to runner scheduling)
- Runner code: `crates/velnor-runner-{core,github,host,cli,qualification,image-manifest,attestation}`,
  `crates/velnor-resource-probe`
- Local: `crates/velnor-local`, `crates/velnor-cache-rules`, `local/{README.md,AGENTS.md,conformance,
  scenarios,docker,testdata}`
- Images: `images/{runner,dind,job,resource-probe}`, pins `.velnor/runner.lock`
- Consumer config: `.velnor/rust-ci.toml`, `.github/workflows/rust-ci.yml`
- In-job helper: `crates/velnor-ci`, action `actions/setup-velnor/action.yml`
