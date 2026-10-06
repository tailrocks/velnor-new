# macOS Scale Set runner

**Status:** Active specification. This document supersedes the conflicting
clauses of [the deferred self-hosted roadmap](../deferred/self-hosted-runner.md).
It is not a claim that live qualification or the ChainArgos rollout has passed.

**Repository:** `tailrocks/velnor-new`. **First consumer:** `ChainArgos/java-monorepo`.
**Host:** macOS, one explicitly selected local Docker engine. **Runtime:** the
unmodified official GitHub Actions runner, Ubuntu 26.04, `linux/amd64`, registered
with the Runner Scale Set protocol and JIT.

## 1. Selectors

```yaml
runs-on: ubuntu-26.04
runs-on: [velnor, ubuntu-26.04-scale-set]
```

The scale set name is `ubuntu-26.04-scale-set`. Register exactly the labels
`ubuntu-26.04-scale-set` and `velnor`. Do not register the hosted label
`ubuntu-26.04` on local workers. Do not implement Scale Sets as `--ephemeral`
plus workflow-job polling. The selector is routing, not authorization.

Host architecture, Docker VM architecture, container platform, and emulation are
separate facts. The primary profile is `linux/amd64`. On Apple Silicon the VM
may be `aarch64` and the container may run under emulation. Never record ARM64
as the x64 baseline. A later ARM profile needs its own selector, such as
`ubuntu-26.04-arm-scale-set`.

Velnor does not implement an Actions interpreter and does not ship a Go sidecar.
The official runner handles actions, steps, logs, credentials, post steps,
services, cancellation, and completion. A container is not a hosted VM.

## 2. Packages

Nested virtual workspace `crates/velnor-runner/`, excluded from the root
workspace members. Root `cargo --workspace` does not test it. Gates enumerate
both workspaces.

| Package | Owns | Must not own |
|---|---|---|
| `velnor-runner-core` | IDs, capacity, lifecycle transitions, ownership, evidence errors | HTTP, Docker, database, processes |
| `velnor-runner-github` | Scale Set wire client, auth refresh, registration, session, acquire, JIT, run census | Containers, task planning |
| `velnor-runner-host` | Journal, Docker ownership, workers, Keychain, launchd, IPC, recovery | Step interpretation, a second task graph |
| `velnor-runner-cli` | `velnor-host` parsing and presentation | Scheduling |

Direction: CLI to host to GitHub and core; GitHub to core. No runner crate
depends on the Rust, Tofu, Mise, renderer, or orchestrator crates. No generator
crate depends on runner crates. The host may use pure
`velnor-actions-contract` types at the evidence boundary only.

`velnor-actions` remains the only workflow generator.

## 3. Operator

Commands: `connect`, `status` (`--json`), `doctor` (read-only unless `--probe`),
`logs`, `drain`, `resume`, `service install|start|stop|uninstall`, `daemon run`,
`compare`, `disconnect`.

`connect` is idempotent. It persists the selected Docker context and engine
identity and never switches providers silently. Credentials are a Keychain
reference. No PAT on argv. No secret in TOML. Ready means bounded readiness,
not "a process exists".

One active repository connection. A second incompatible connection is rejected.
A second daemon fails before it opens another session.

`disconnect` drains and deletes only remote resources this controller recorded
as created. Adopting a set does not allow deleting it. Stop and restart do not
delete the set.

States include `waiting_for_engine`, `waiting_for_credentials`, `reconciling`,
`ready`, `draining`, and `degraded`.

The read-only `status` and `doctor` observer must not report `ready` from an
empty journal alone. It can report prerequisite failures or `reconciling`;
`ready` requires a bounded controller-owned snapshot proving Docker ownership,
journal state, and GitHub session/runner state agree. The CLI runs its current
partial observer in a private child process and kills and reaps that child at
the shared deadline; the child receives no secret argv, environment, or output.
Config reads require a regular file and size cap. Journal inspection has a
bounded file size, row count, and query deadline; exhausting any bound is
degraded. A confirmed missing configuration or absent credential may report
`waiting_for_credentials`; malformed, nonregular, oversized, and unreadable
configuration reports `degraded`.

Per-user LaunchAgent runs `velnor-host daemon run` in the foreground. No
double-fork. State under `~/Library/Application Support/Velnor/`. Logs under
`~/Library/Logs/Velnor/`. Private Unix socket. Not a root LaunchDaemon.

Default `max_jobs` is 1. Qualification must prove `N>1`. One host-wide capacity
authority. A permit covers the top-level job lifecycle until cleanup is proven.
Reserved, acquiring, uncertain, provisioning, idle, running, finishing,
cleaning, and quarantined states all occupy a slot.
A Docker 404 or explicit `exited`/`dead` status is required before a recorded
container can be treated as absent or stopped. A missing or empty `Status` is
uncertain even when `Running` is false; the permit stays occupied until pair
cleanup is proven.

## 4. Protocol

Authority: `actions/scaleset` `e6daac702355cdb5b880b4fbdcf6d85dcd9e48e5`
(`client.go`, `session_client.go`, `listener/listener.go`). Do not vendor the
Go module.

- Actions service base from registration. Query `api-version=6.0-preview`.
- Scale set path `_apis/runtime/runnerscalesets`.
- JIT is `POST /_apis/runtime/runnerscalesets/{id}/generatejitconfig`. Not the
  classic repository `generate-jitconfig` route.
- Acquire is `POST .../{id}/acquirejobs` with a JSON array of int64 request ids.
  Response is `{count, value}`. Partial success keeps only returned ids that
  were requested. An unusable success response or server failure keeps the
  reservation. After a requested id is acquired, a later JIT conflict does not
  settle that acquisition; keep the reservation uncertain and do not retry it.
- Poll header `X-ScaleSetMaxCapacity` is total capacity, not free slots.
- HTTP 202 is an empty poll, not an error, and is not acknowledged.
- Queue envelope `messageType` must be `RunnerScaleSetJobMessages`. `body` is a
  string containing a JSON array. Inner kinds: `JobAvailable`, `JobAssigned`,
  `JobStarted`, `JobCompleted`. Unknown inner kinds are visible and not silently
  acknowledged.
- `statistics` on the envelope may be null or omitted. Both mean absent. A
  present object always carries the seven counters. `totalAssignedJobs` is
  population authority. Do not add it to local reservations.
- Message id 0 is real. The `lastMessageId` query is omitted when the cursor is
  not greater than 0. Synthetic initial id `-1` is not acknowledged.
- Create sends `RunnerSetting` (Pascal-case JSON key) with `disableUpdate: true`.
  Re-read after create, adopt, and create races. Refuse the set if the server
  does not keep updates disabled, or if name, scope, or labels disagree.
- 401 on session calls refreshes once, single-flight. 403 is an authorization
  defect, not an infinite retry.
- Acknowledge only when the batch is replay-safe. Never acknowledge an
  unacquired offer that is the only retained copy of the work.

Persist intent, then perform the external call, then record success or
uncertainty. Do not hold a database transaction across HTTP or Docker. `Drop`
does not free a durable slot. Restart reconciles journal, exact Docker objects,
and GitHub state before advertising capacity, and adopts still-running owned
workers. Pending or uncertain rows lack enough durable stage and remote-operation
identity to settle an interrupted acquire or JIT request from Docker state alone;
retain their slot and owned volumes until an authoritative settlement mechanism
exists. On first open, the journal validates its version before schema mutation,
then migrates version-zero rows transactionally. Failed launch rows become
uncertain; pending and uncertain launch rows also have local cleanup proof
cleared. The old schema cannot distinguish a rejected request from a remote
effect with an unusable response, and Docker cleanup does not settle GitHub
state. This may conservatively retain an old definite rejection. Unknown journal
versions and malformed migrations fail closed without partial schema changes.
An engine identity change stops automatic mutation.

Disabled runner self-update is a registration invariant. Do not patch the
official runner. Do not spoof `/proc` to bypass `AssertCompatibleOS`.

## 5. Worker topology

Each admitted job gets one unmodified official runner container plus that
worker's private DinD engine. The runner container is not privileged. The job
does not receive the outer Docker socket, host home, Keychain, SSH agent,
controller config, or management tokens.

Every host configuration must set four per-job budgets under
`[host.resources]`: `runner_cpu_millicores`, `runner_memory_bytes`,
`dind_cpu_millicores`, and `dind_memory_bytes`. CPU is millicores; memory is
bytes. There are no resource defaults. The controller validates each Docker
limit, checked pair totals, and the selected daemon's CPU count before it opens
a job session. It applies the runner limits to the unprivileged runner
container and the DinD limits to the private privileged DinD container. Swap is
limited to each container's memory limit.

The configured pair CPU and memory totals are the per-job admission costs.
Admission must also account for already occupied jobs and measured guest CPU,
available memory, and Docker-root free space. Those measurements describe
current headroom; they are not durable quotas. In particular, Docker named
volumes have no per-job storage-size limit here. Free-space checks cannot stop
a job from consuming the remaining Docker-root storage, so enforced storage
isolation remains unproven.

`/var/run/docker.sock` inside the runner and inside the private daemon resolves
to that worker's socket, never the outer engine socket. Named volumes back
work, temp, actions, tools, and the socket. JIT is delivered on a short-lived
stdin channel after a durable provision intent. JIT is absent from Docker
`Config.Env`, `Cmd`, labels, image layers, host argv, journal, TOML, launchd
plist, and evidence archives.

JIT `workFolder` is `_work`, relative to the official runner root at
`/home/runner`, so the job work directory resolves to `/home/runner/_work`.
Mount the same per-worker named work volume there in both containers. The
runner and DinD images create that path as uid/gid `1000:1000`, mode `0755`,
before the first empty-volume mount; DinD starts first and Docker initializes
the volume from its image path. The entrypoint stages JIT only in its
container-local `/tmp` with mode `0600` and removes the file before starting
the listener; it must not persist JIT under the named work volume. Checkout,
tools, and job workspace use the shared writable volume for the runner user.

Delete only objects whose immutable id matches the journal. Names are not
delete authority. Foreign objects survive. A missing delete response is not
success. No host-wide prune. No prefix delete.

## 6. Generator routing

Schema 2 is the routing revision. Schema 1 is unchanged and still emits hosted
`runs-on: ubuntu-26.04` only. `velnor-actions config migrate --to 2` prints the
proposed config. `--write` persists that same text. Preview does not write.
Migration does not treat `runner_label` as a provider name.

Modes: `hosted`, `scale-set`, `both`. An explicit dispatch mode overrides
configured `execution.mode`. In `both`, every eligible verification workload
runs in both lanes with the same source, plan, task, and tool inputs. Trusted
control jobs and single-writer publish, deploy, release, and baseline promotion
stay single and hosted. A hosted catalog label cannot appear on a scale-set
selector.

Only `verification` entries in the shared `[[workflow.tasks]]` list follow
the paired-lane eligibility rule. Linux x64 verification tasks emit both
hosted and Scale Set jobs in `both`, and `Required` waits for both. In
`hosted` or `scale-set`, each Linux verification task emits only its selected
lane. Build and NativeImage variants remain hosted-only in every mode; a
Scale Set profile override for either variant is rejected.
macOS ARM64 tasks remain hosted in all modes because the Scale Set contract is
Linux/amd64; they stay in `Required` but do not provide paired qualification.

Paired qualification does not treat one lane's cached success as execution of
the other. Comparison fails or returns `NOT_PROVEN` for a missing lane,
duplicate or conflicting result, skipped, cancelled, timed-out, or failed job,
wrong source, attempt, plan, or profile, missing or swapped artifact, unknown
runner, omitted API page, cached-success substitution, or an unsafe archive.

Qualification, image-release, macOS-binary-release, and monitoring workflows are
emitted by `velnor-actions`, not patched after generation.

## 7. Gates

`docs/implemented/macos-scaleset/` records G0–G8. Each row is `PASS`, `FAIL`,
`BLOCKED_EXTERNAL`, or `NOT_RUN`. A skipped test is not a pass. Live GitHub,
launchd, signing, and ChainArgos verdicts stay external oracles. Local tests do
not mark G4, G6 launchd, G7, or G8 `PASS`.

## 8. Out of scope

Kubernetes, ARC, Velnor-managed VMs, Firecracker, libvirt, fleet provisioning,
a Linux-host controller, native Darwin or Xcode jobs, a dashboard or TUI, a Go
sidecar, a second workflow interpreter, a new distributed cache, multi-repository
capacity pools, and ChainArgos production deploys.
