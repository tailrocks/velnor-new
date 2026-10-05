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

Per-user LaunchAgent runs `velnor-host daemon run` in the foreground. No
double-fork. State under `~/Library/Application Support/Velnor/`. Logs under
`~/Library/Logs/Velnor/`. Private Unix socket. Not a root LaunchDaemon.

Default `max_jobs` is 1. Qualification must prove `N>1`. One host-wide capacity
authority. A permit covers the top-level job lifecycle until cleanup is proven.
Reserved, acquiring, uncertain, provisioning, idle, running, finishing,
cleaning, and quarantined states all occupy a slot.

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
  were requested. Uncertain transport keeps the reservation.
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
workers. An engine identity change stops automatic mutation.

Disabled runner self-update is a registration invariant. Do not patch the
official runner. Do not spoof `/proc` to bypass `AssertCompatibleOS`.

## 5. Worker topology

Each admitted job gets one unmodified official runner container plus that
worker's private DinD engine. The runner container is not privileged. The job
does not receive the outer Docker socket, host home, Keychain, SSH agent,
controller config, or management tokens.

`/var/run/docker.sock` inside the runner and inside the private daemon resolves
to that worker's socket, never the outer engine socket. Named volumes back
work, temp, actions, tools, and the socket. JIT is delivered on a short-lived
stdin channel after a durable provision intent. JIT is absent from Docker
`Config.Env`, `Cmd`, labels, image layers, host argv, journal, TOML, launchd
plist, and evidence archives.

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
