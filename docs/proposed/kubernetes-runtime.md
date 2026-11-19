# Proposed: Kubernetes runtime for velnor-host (one runner, two runtimes)

**Status:** Proposal. Design only; no code accompanies this document.

## 0. Goal and non-goals

velnor-host (`crates/velnor-runner-*`) drives one scale-set runner per job on a
Docker engine. The owner wants the same host to also run jobs on the tailrocks
Hetzner k3s cluster at full capacity with warm caches.

Goal: ONE runner. Docker and Kubernetes sit behind a single `Runtime` interface.
The scale-set protocol, the journal, the official runner binary, the launch
state machine and the cleanup proof discipline stay one implementation.

Non-goals: a second host binary, a second journal, a Kubernetes operator or CRD,
replacing Docker as the default, or running the Mac (macOS) path on Kubernetes.

Evidence base: a read of the current Docker coupling. Counts below come from a
grep of `crates/velnor-runner-*` (about 5,150 lines mention "docker"; roughly 50
non-test source files import `bollard`). Names in backticks are real files or
types; where this document proposes a name it says so.

## 1. Current Docker coupling

Everything funnels into `bollard::Docker`, passed by value or reference.
There is no trait today. Call frequency in `velnor-runner-host/src`:

| bollard call | uses | where it matters |
|---|---|---|
| `info` | 30 | engine identity capture (`SelectedEngineContext::capture`), cleanup and recovery gates |
| `inspect_container` | 18 | `launch/inspect.rs` (`container_running`, `classify_inspect`), reconcile gate, recovery |
| `remove_container` / `stop_container` | 14 / 11 | `launch/retirement.rs`, `bootstrap/cleanup`, `local/remove.rs` |
| `inspect_volume` / `create_volume` / `remove_volume` | 11 / 3 / 5 | `bootstrap/containers/storage_object*`, `cleanup_witness/volumes.rs` |
| `inspect_image` | 7 | `verified_images.rs`, qualification `docker.rs` |
| `wait_container`, `start_container` | 6, 6 | `containers/pair/production*`, `completion` |
| `create_container` | 5 | `containers/pair/{spec,config}.rs`, local pair |
| `attach_container`, `create/start/inspect_exec` | 5, 4/3/5 | worker helper protocol (`storage/observation.rs`), resource probe |
| `logs` | 2 sites x4 files | `resource_probe/ops.rs`, `publication/execute/*`, `local/channel.rs` |
| `*_network` | 3 each | `containers/network/{create,remove}.rs` |
| `upload_to_container` | 2 | seeding files into the pair |
| `list_containers` | 2 | owned listing for reconcile and local remove |

Other coupling that is not a bollard call:

- **Config.** `HostConfig.docker: DockerConfig` (`config.rs`) has `endpoint`
  (must be a unix socket; `docker_client::connect_unix` rejects `tcp://`),
  `context`, `platform`. `validate_docker` enforces this.
- **Spec audit.** `docker_spec.rs` (`ContainerPlan`, `audit_plan`,
  `reject_mount`, `HOST_NEEDLES`) forbids host-path mounts, docker.sock mounts
  except a private volume, JIT material in env/cmd/labels, privileged runners.
- **Pair shape.** `containers/pair/config.rs`: two containers per job, a runner
  (no privilege, 14 whitelisted capabilities, custom seccomp from
  `worker/seccomp/runner-v1.json`, pids limit, tmpfs `/tmp` and `/dev/shm` sized
  from the `StorageProvisioningPlan`) and a dind sidecar (privileged, own pids
  limit). They share a socket volume (`/run`), plus home, work, dind-data and
  metadata volumes named `w<nonce>[-home|-work|-docker|-metadata]`
  (`worker/names.rs`, `WorkerVolumeRole`).
- **Journal phases.** `reconcile::LaunchPhase` includes `docker_provisioning`,
  `docker_requested`, `docker_uncertain`; storage rows carry container ids,
  volume names, engine id and `docker_root_digest`.
- **Ownership proofs.** See section 2.4.
- **Qualification.** `velnor-runner-qualification` (`docker.rs`, `probes.rs`,
  `resource_dind.rs`, `host_config.rs`) inspects images and runs probe
  containers; `velnor-resource-probe` runs inside the pair and reads cgroups and
  mountinfo.
- **Local mode.** `local/` (`pair.rs`, `remove.rs`, `channel.rs`, `storage.rs`)
  uses the same `connect_unix` engine and the same pair plan for developer
  machines. It must stay Docker-only and must keep compiling against the trait.

## 2. The runtime interface

### 2.1 Principle

The trait speaks in the host's own domain (a worker pair, its storage, its
ownership), not in Docker or Kubernetes verbs. Journal code never sees a
container id string without a `RuntimeKind`. Names below are proposed.

### 2.2 Types

```rust
/// Selected by host.toml; stored in the journal next to every identity.
enum RuntimeKind { Docker, Kubernetes }

/// Opaque, runtime-minted, journal-persisted. Replaces bare container-id and
/// volume-name strings in rows.
struct WorkloadRef { kind: RuntimeKind, scope: String /* engine id | cluster+ns */,
                     name: String, uid: String /* container id | pod UID */ }
struct StorageRef  { kind: RuntimeKind, scope: String, name: String,
                     uid: String /* volume name+created | PVC UID */ }
struct NetworkRef  { kind: RuntimeKind, scope: String, name: String, uid: String }

/// Identity of the engine or cluster; compared to the journal (today:
/// SelectedEngineContext engine_id + docker_root_digest).
struct RuntimeIdentity { kind: RuntimeKind, id: String, root_digest: String,
                         platform: RunnerPlatform }

/// What the host asks for. Built from StorageProvisioningPlan + budget +
/// verified images. Contains NO secrets; JIT goes through `deliver`.
struct WorkerSpec {
    names: WorkerIdentityNames, owner: OwnerLabels,   // see 2.4
    runner: ContainerSpec, engine: ContainerSpec,     // runner + dind/docker
    storage: Vec<StorageSpec>, network: NetworkSpec,
    tmpfs: Vec<TmpfsSpec>, budget: ResourceBudget,
    placement: PlacementHint,                          // cache affinity, k8s only
}
struct ContainerSpec { image: VerifiedImage, env: Vec<EnvVar>, cmd: Vec<String>,
    caps: CapabilitySet, seccomp: SeccompProfile, pids_limit: i64, privileged: bool,
    mounts: Vec<MountSpec>, log_limit: LogLimit }
enum StorageClass { Ephemeral, NodeCache /* warm, node-local */, SharedRun }
struct StorageSpec { role: StorageRole, class: StorageClass, bytes: u64, inodes: u64 }

enum Liveness { Live, Exited{code:i64}, Gone }        // Gone only via a proof
enum Observation<T> { Present(T), AbsentProven(AbsenceProof), Unknown(RuntimeError) }
struct AbsenceProof { subject: ProofSubject, runtime: RuntimeIdentity, how: AbsenceHow }
enum AbsenceHow { NotFoundById /* docker 404 by id; k8s 404 by name+uid precondition */,
                  ListedEmptyByOwnerLabel }
struct OwnershipProof { subject: ProofSubject, labels_match: bool, uid_match: bool,
                        owner_chain_ok: bool, runtime: RuntimeIdentity }
```

`Observation::Unknown` is the "uncertain" branch the journal already models
(`*_uncertain` phases). Timeouts map to `Unknown`, never to `AbsentProven`.

### 2.3 Trait

```rust
#[async_trait] trait Runtime: Send + Sync {
    fn kind(&self) -> RuntimeKind;
    async fn identity(&self) -> Result<RuntimeIdentity, RuntimeError>;       // docker info
    async fn verify_image(&self, r: &ImageRef) -> Result<VerifiedImage, _>;   // inspect_image

    // lifecycle of one worker pair
    async fn create_worker(&self, s: &WorkerSpec) -> Result<CreatedWorker, _>; // net+vols+2 ctrs
    async fn start_worker(&self, w: &WorkloadRef) -> Result<(), _>;
    async fn deliver(&self, w: &WorkloadRef, role: Role, f: &Delivery) -> Result<(), _>; // upload_to_container
    async fn exec_helper(&self, w: &WorkloadRef, role: Role, argv: &[String])
        -> Result<HelperStream, _>;        // create/start/inspect_exec + attach (stdin/stdout)
    async fn wait(&self, w: &WorkloadRef, role: Role) -> Result<ExitStatus, _>;   // wait_container
    async fn logs(&self, w: &WorkloadRef, role: Role, o: LogOpts) -> Result<LogStream, _>;
    async fn stop(&self, w: &WorkloadRef, grace: Duration) -> Result<(), _>;
    async fn delete(&self, w: &WorkloadRef) -> Result<(), _>;                  // idempotent
    async fn delete_storage(&self, s: &StorageRef) -> Result<(), _>;
    async fn delete_network(&self, n: &NetworkRef) -> Result<(), _>;

    // observation and proof
    async fn inspect(&self, w: &WorkloadRef) -> Observation<WorkloadState>;   // live/exited/gone
    async fn inspect_storage(&self, s: &StorageRef) -> Observation<StorageState>;
    async fn inspect_network(&self, n: &NetworkRef) -> Observation<NetworkState>;
    async fn prove_owned(&self, w: &WorkloadRef, expect: &OwnerLabels)
        -> Result<OwnershipProof, _>;
    async fn list_owned(&self, scope: &OwnerSelector) -> Result<Vec<OwnedItem>, _>;
    async fn cgroup_subjects(&self, w: &WorkloadRef) -> Result<OwnedCgroupSubjects, _>;
}
```

Notes on shape:

- `create_worker` returns refs for both containers, every storage object and the
  network in one value, because the journal records them together (the
  `StorageCleanupIdentity` already holds both container ids, the volume
  inventory and the engine id).
- No generic "run any container" verb. Qualification and the resource probe use
  `create_worker` with a probe spec (or a `run_probe` helper layered on it), so
  nothing can escape the audit in `docker_spec.rs`.
- `exec_helper` exists because the worker helper protocol
  (`storage/observation.rs`) writes a request on stdin and reads a record line
  from stdout of an exec. Kubernetes has `pods/exec` (SPDY or WebSocket), so it
  maps. This is the single most protocol-sensitive method.
- Pure decisions stay out of the trait: `reconcile::before_advertise`,
  `audit_plan`, name derivation, resource budgeting, journal writes.

### 2.4 Ownership proofs and "proven gone"

Today, in prose:

1. `reconcile_gate` (`launch/gate.rs`) reads journal rows, asks Docker which
   recorded container ids are running (`inspect_container` then
   `classify_inspect`), and feeds `before_advertise` with the intersection of
   journal, GitHub runner ids and observed ids. HTTP 404 => `false` (not live);
   any other error => `EnsureError::Unexpected`, i.e. hold.
2. `observe_docker_cleanup_absence` (`bootstrap/cleanup/production.rs`) first
   re-captures engine identity and requires it to equal the journaled
   `engine_id` and `docker_root_digest` (else `HostError::Ownership`), then
   requires a 404 for each container id AND container name, and a 404 for each
   volume name. The result `ObservedDockerCleanupAbsence` is what unlocks
   `StorageCleanupObserverWitness` in the journal.
3. Creation is labeled `velnor.role`, `velnor.volume`, `velnor.worker`
   (`docker_spec::runner_plan`) and `DeleteDecision` distinguishes `Delete`,
   `KeepForeign`, `NotDeleted`.

Kubernetes equivalent, with the same strength:

| Docker proof | Kubernetes proof |
|---|---|
| engine id + docker root digest | cluster id (`kube-system` namespace UID) + namespace UID + a velnor ConfigMap/Lease holding the host instance id |
| container id | Pod UID (`metadata.uid`), recorded at create from the API response |
| 404 on id | GET by name returns 404, OR returns a Pod whose UID differs (name reuse by a new Pod is "ours is gone" only if the new one is not ours; treat as gone for the old UID, foreign for the new) |
| 404 on name | same GET; plus list by label selector returns zero items |
| labels | labels `velnor.io/{role,worker,host-instance}` AND `ownerReferences` to a per-job owner object (a Secret or ConfigMap named `w<nonce>` created first, whose UID is journaled) |
| volume absent | PVC GET 404 by name and UID mismatch or 404; PV not required |
| remove with filter | DELETE with `preconditions.uid` so a recreated object is never deleted |

Rules carried over unchanged: 404 proves gone only after the identity check
passes; timeouts, 5xx, RBAC 403, and watch gaps are `Unknown`, never gone;
`terminating` (deletionTimestamp set, or finalizers pending) is live, matching
`Status::REMOVING` in `still_live`. A Pod in `Succeeded` or `Failed` is
`Exited`; `Unknown` phase is `Unknown`.

Important difference: Docker 404 is authoritative because the engine is a
single writer. The API server is also linearizable for GET by name (a quorum
read), so a GET is acceptable; list/watch results are not used as absence
proof without `resourceVersion=""` (quorum) semantics. This is stated as a
requirement, not assumed.

## 3. Mapping existing Docker call sites to the trait

| Call site (file) | Trait method |
|---|---|
| `docker.info` + `SelectedEngineContext::capture` (`launch/engine_context.rs`, `bootstrap/cleanup/runtime.rs`) | `identity` |
| `inspect_image` (`launch/verified_images.rs`, qualification `docker.rs`) | `verify_image` |
| `create_volume` / `inspect_volume` (`containers/storage_object*`, `cleanup_witness/volume*`) | part of `create_worker`; `inspect_storage` |
| `create_network` / `remove_network` (`containers/network/*`) | part of `create_worker`; `delete_network`; `inspect_network` |
| `create_container` x2 (`pair/production`, `pair/spec`, `pair/config`) | `create_worker` (spec built by a runtime-neutral `WorkerSpec` builder; each runtime lowers it) |
| `start_container` (`pair/production`) | `start_worker` |
| `upload_to_container` | `deliver` |
| `attach_container`, `create/start/inspect_exec` (`storage/observation.rs`, `pair/production/attach.rs`) | `exec_helper`, `wait` |
| `wait_container` (`completion`, `drive`) | `wait` |
| `logs` (`resource_probe/ops.rs`, `publication/execute/*`, `local/channel.rs`) | `logs` |
| `stop_container`, `remove_container` (`retirement.rs`, `completion/cleanup.rs`, `local/remove.rs`) | `stop`, `delete` |
| `inspect_container` (`launch/inspect.rs`, `gate.rs`, `pre_jit_recovery`, `unbound_retirement`) | `inspect`, `prove_owned` |
| `list_containers` (reconcile, local remove) | `list_owned` |
| `remove_volume` (`cleanup`) | `delete_storage` |
| `owned_cgroups.rs`, `resource_probe` cgroup reads | `cgroup_subjects` |
| `connect_unix`, `docker_deadline` | `DockerRuntime::connect`, per-method deadline wrapper shared by both runtimes |
| `ContainerPlan`, `audit_plan`, `HOST_NEEDLES` | runtime-neutral `audit_spec(&WorkerSpec)`, plus runtime-specific lowering audit |
| `RUNNER_CAPABILITIES`, `runner_security_opt` | `CapabilitySet`, `SeccompProfile` in the spec |
| `HostConfig.docker` | `HostConfig.runtime` (section 6) |
| `local/*` | stays on `DockerRuntime` via the trait; no k8s |

## 4. Kubernetes mapping

### 4.1 Object per job

One Pod per job (not a Job). Rationale: velnor already owns retry, ack and
exactly-once semantics in the journal and GitHub JIT configs are single-use; a
Job controller would restart the runner and re-use a consumed JIT config. Pod
`restartPolicy: Never`.

The pair becomes one Pod with containers `runner` and `dind` (the existing
"pair" is two cooperating containers sharing `/run` already, so a single Pod
is the natural unit, and it removes the network object: containers in a Pod
share localhost). Resource accounting moves from two cgroup subjects to the
Pod cgroup plus per-container limits.

Lowering of `ContainerSpec`:

- `caps` to `securityContext.capabilities.drop: [ALL]` + `add: [...]` (same 14).
- `seccomp` to `securityContext.seccompProfile`: `type: Localhost` with a
  profile distributed to nodes (the runner-v1 JSON), or `RuntimeDefault` as a
  fallback if node distribution is not accepted (open question).
- `privileged` for dind only: see 4.4.
- `pids_limit` has no Pod field; set kubelet `podPidsLimit` on the velnor node
  pool (or accept a cluster-wide value). Flagged as a gap.
- tmpfs `/tmp`, `/dev/shm` to `emptyDir{medium: Memory, sizeLimit}`; inode limit
  has no equivalent (gap; the probe in 4.6 must say so).
- `log_limit` is a kubelet setting (`containerLogMaxSize`), not per Pod.

### 4.2 Storage and caches

| Docker volume (`WorkerVolumeRole`) | Kubernetes |
|---|---|
| Socket (`/run`) | `emptyDir` (shared between runner and dind) |
| Home | `emptyDir` |
| Work (`_work`) | `NodeCache` or `emptyDir` per class below |
| DindData (`-docker`) | `NodeCache` (image layers; the biggest warm-cache win) or `emptyDir` |
| RunnerMetadata | `emptyDir` |
| action archive / seed | read-only ConfigMap or an init container from the verified image |

Warm caches (the point of this effort), two options, recommend A:

- **A. Local PersistentVolumes via a node-local provisioner** (k3s ships
  `local-path`; or `topolvm`/`openebs-localpv`). One long-lived PVC per
  (node, cache family), NOT per job, claimed `ReadWriteOncePod`. A job Pod
  mounts the cache PVC of the node it was scheduled to. Because a PVC bound
  to a local PV pins the Pod to that node, node affinity is automatic and
  scheduling reuses the warm node.
- **B. hostPath** under `/var/lib/velnor/cache/<family>`. Simpler and faster,
  but bypasses volume accounting, needs `hostPath` allowed by policy, and
  weakens isolation (any pod with hostPath could read it). Not recommended
  beyond a spike.

Because a cache PVC outlives a job, the per-job volume cleanup proof splits:
job-private storage (emptyDir, per-job PVC) is proven gone by 404 by UID;
shared cache PVCs are never deleted by job cleanup and are never in the
journal's `absent_volumes`. The journal records the cache PVC UID with a
`StorageClass::NodeCache` marker so reconcile knows "keep".

Concurrency: with `ReadWriteOncePod` only one job uses a cache at a time. For
N concurrent jobs per node, shard into K cache slots per node (slot index is a
label; slot selection is a pure function, like `local/slot.rs`). Cache
isolation across trust scopes follows the existing cache contract: one cache
slot family per trust scope, `pr`-scope never writes trusted caches
(`cache-contract.md`).

Placement: `PlacementHint { prefer_node: Option<NodeId>, cache_family }` lowers
to `preferredDuringSchedulingIgnoredDuringExecution` node affinity on a
`velnor.io/cache-<family>=<digest>` node label (set by a small node labeler or
by the host after first use) plus a node selector for the velnor pool and a
`tolerations` entry for a dedicated taint. Host-side selection prefers the node
that last ran the same repository+family (journal records it).

### 4.3 Identity and RBAC

Namespace per host instance (default `velnor-<host-instance>`), one
ServiceAccount `velnor-host` used by velnor-host (out-of-cluster via
kubeconfig, or in-cluster), and a separate no-token ServiceAccount
(`automountServiceAccountToken: false`) for job Pods. Role (namespaced, not
ClusterRole):

- `pods`: create, get, list, watch, delete
- `pods/exec`, `pods/log`: create, get
- `persistentvolumeclaims`: create, get, list, delete
- `configmaps`, `secrets`: create, get, delete (only for owner object and JIT)
- `events`: list (diagnostics)

Plus read-only cluster scope for identity: `get` on `namespaces` and
`nodes` (a tiny ClusterRole, bound read-only). Admission (Pod Security
Admission, or Kyverno/Gatekeeper) enforces the audit rules cluster-side in
addition to `audit_spec` host-side.

### 4.4 dind and container actions

The official runner binary stays unchanged in the `runner` container. For
Docker container actions, service containers and `docker build` steps it needs
a Docker daemon at `DOCKER_HOST=unix:///var/run/docker.sock`, exactly as today.
In the Pod this is the `dind` container (privileged, own cgroup) sharing the
socket `emptyDir`; no change to the runner image or hooks.

Alternatives, rejected for the first release:

- Sysbox / Kata / gVisor RuntimeClass: removes `privileged` but needs node
  support; listed as a hardening follow-up (`runtimeClassName` in spec).
- Rootless dind or buildkit-only: breaks arbitrary container actions.
- Host docker socket: forbidden today (`bad_socket`) and stays forbidden.

### 4.5 Start, wait, logs, exec

- Start: Pod creation starts it. `start_worker` becomes "wait until the Pod
  reaches Running or fails scheduling within a bounded deadline".
- Wait: watch the Pod with a bounded relist; completion is the `runner`
  container terminated state (`state.terminated.exitCode`), not Pod phase, as
  dind keeps running. `stop` then deletes the Pod (grace period preserved).
- Logs: `GET pods/log?follow=true&container=...`; `LogOpts` maps to
  `sinceTime`, `tailLines`. Kubelet log rotation can drop lines; the existing
  probe and publication readers must treat a gap as `Unknown`.
- Exec: `pods/exec` WebSocket (`v5.channel.k8s.io`) for the helper protocol.
  Needs stdin close semantics (half close) to match `attached.input.shutdown()`;
  a spike is required first (PR 2 below).
- Deliver (JIT config, seed): Secret mounted read-only into `runner` and owned
  by the Pod; written only after Pod UID is known. Never in env, args or
  labels (the same rule `contains_jit` enforces now).

### 4.6 Qualification and security probes

`velnor-runner-qualification` today inspects images and runs probe containers
to check capabilities, seccomp and dind privilege. Kubernetes needs a parallel
qualification suite behind the same trait: confirm Pod Security level, that
`capsh` inside `runner` matches the whitelist, that seccomp is `Localhost`,
that the SA token is not mounted, that hostPath/hostNetwork/hostPID are
rejected by admission, that dind alone is privileged, that egress policy is
as intended, and that the kubelet supports `podPidsLimit`. Results are stored
with `RuntimeKind` in the same journal tables (`probe_record.rs`).

## 5. What stays identical

- **Journal**: same SQLite tables and `LaunchPhase` machine. Phase names
  `docker_*` are historical; add a migration that renames to `runtime_*` with
  an alias on read (journal schema is versioned; see `journal_schema.rs`).
  Rows gain `runtime_kind` and `scope`; ids move from bare strings to
  `WorkloadRef`/`StorageRef`. Existing Docker rows read as `kind=Docker`.
- **Scale-set protocol** (`scale_set*`, `connect`, `listen`, `assign`): session,
  acquire, JIT, ack. Untouched.
- **Official runner binary** and the image contract (`velnor-runner:ubuntu-26.04-...`,
  entrypoint, job-start hook): untouched; same verified-image manifest.
- **Pure logic**: `reconcile::before_advertise`, resource budgeting, name
  derivation, `audit_plan` rules, completion and retirement ordering.
- **Local mode**: Docker only.

## 6. Security

- **Privileged pods**: only `dind`, only in a dedicated, tainted node pool,
  one job per Pod, never co-scheduled with other tenants (anti-affinity on
  the pool or dedicated nodes). The runner container is non-privileged with
  dropped capabilities. Goal state: replace privileged dind with a
  sandboxed `RuntimeClass`.
- **Namespace isolation**: a velnor namespace with Pod Security `baseline`
  enforced and a documented exemption for the dind container (PSA applies
  per Pod, so `privileged` needs a separate namespace, `velnor-dind`, or an
  admission policy that allows exactly this shape). NetworkPolicy default-deny
  ingress; egress allow-list to GitHub, registries, the cache backend; deny
  node and cloud metadata addresses (`169.254.169.254`) and the Kubernetes
  API from job Pods. ResourceQuota and LimitRange cap concurrency.
- **Secrets**: JIT config lives in a per-Pod Secret owned by the Pod, deleted
  with it, never in env or labels; host kubeconfig is a credential in the
  existing keychain abstraction (`keychain.rs`) or a mounted SA token; no
  Secret is readable by job Pods other than its own. etcd encryption at rest
  is required (k3s `secrets-encryption`). Cache PVCs hold no credentials.
- **Fork fence**: unchanged in meaning. Fork PRs and untrusted refs run only
  with `pr` trust scope: read-only cache, no write to trusted cache slots,
  dedicated cache slot family and node label so a fork job cannot land on or
  mount a trusted cache PVC. Enforced twice: host-side `audit_spec` refuses a
  trusted cache mount for an untrusted job; cluster-side admission restricts
  PVC and node selectors by Pod label. dind image layers cached on a node
  are per trust scope for the same reason.
- **Host exposure**: `HOST_NEEDLES` analogue for Kubernetes: reject hostPath
  (except the controlled cache path if option B), hostNetwork, hostPID, hostIPC,
  projected SA tokens, and any mount of `docker.sock`.

## 7. Migration

- Docker stays the default. Missing `[runtime]` means Docker; existing
  `host.toml` files parse unchanged (`[docker]` remains accepted as an alias).
- Opt-in:

```toml
[runtime]
kind = "kubernetes"            # default "docker"

[runtime.kubernetes]
kubeconfig_credential = "..."  # keychain ref, or in_cluster = true
namespace = "velnor-ci"
node_pool_label = "velnor.io/pool=ci"
cache_storage_class = "local-path"
cache_slots_per_node = 4
runtime_class = ""             # optional sandbox
```

- A host selects exactly one runtime at start; mixing within one host process is
  not supported in v1 (journal rows keep `kind` so a later multi-runtime host
  is possible). Switching runtime requires an empty journal or a proven-clean
  state (reuse the existing "unbound retirement" gates); the host refuses to
  start if rows of the other kind are not complete.
- Roll out: qualify in a non-production namespace, then one scale set with a
  low max, then raise; rollback is changing `kind` back after drain.

## 8. Work breakdown

Each PR is independently mergeable, red-first (the failing test lands in the
same PR first commit), and Docker behavior must stay byte-identical until
PR 7. Sizes: S < 300 lines, M 300-1000, L > 1000 changed.

| # | PR | Size | Risk | Red-first test |
|---|---|---|---|---|
| 1 | Introduce `Runtime` trait + types; `DockerRuntime` wraps existing `bollard` calls; no call-site change | M | Low | trait contract tests against a fake runtime; compile-time check that `DockerRuntime: Runtime` |
| 2 | Spike (doc plus ignored test): k3s exec half-close, log follow, Pod UID 404 semantics, local PV pinning | S | Low | ignored integration test that fails until the behaviors are verified on the cluster |
| 3 | Migrate cleanup/absence proofs and `reconcile_gate` to `Observation`/`prove_owned`/`list_owned` | L | High | table tests: 404, 5xx, timeout, foreign object, engine-identity mismatch each give `Unknown`/`Ownership`, never gone |
| 4 | Migrate create/start/wait/logs/exec/deliver call sites (pair, resource probe, publication) | L | High | existing `pair`/`storage` tests run on both fake and Docker; golden `WorkerSpec` lowering to `ContainerCreateBody` equals today's body |
| 5 | Journal: `runtime_kind`, `scope`, `WorkloadRef` columns; phase rename with read alias; migration of existing rows | M | High | v-N journal fixture opens, reads as Docker, and round-trips; downgrade-refusal test |
| 6 | Local mode on the trait (Docker only) and delete direct `bollard` imports outside `docker/` | M | Medium | lint test: `bollard` import outside the Docker module fails CI |
| 7 | `KubernetesRuntime` core: client, spec lowering (Pod, emptyDir, caps, seccomp), create/delete/inspect/proofs with UID preconditions, RBAC manifests | L | High | lowering golden tests; fake-apiserver tests for 404/UID mismatch/terminating; envtest or k3s integration job |
| 8 | Kubernetes exec/logs/wait/deliver incl. JIT Secret ownership | M | High | helper-protocol test over exec with half-close; JIT never in Pod spec test |
| 9 | Node-local cache PVCs, slots, affinity hints, trust-scope fencing | L | High | fork job refused trusted cache mount (host and admission); placement prefers last node |
| 10 | Kubernetes qualification suite and probes behind the trait | M | Medium | probe fails on a Pod that mounts hostPath or token |
| 11 | `[runtime]` config, validation, `[docker]` alias, runtime selection, docs and ops runbook | S | Low | old `host.toml` parses unchanged; unknown kind rejected |
| 12 | Cluster hardening: dedicated pool, NetworkPolicy, PSA, quotas; optional sandbox RuntimeClass | M | Medium | admission policy tests |

Order: 1, 2, 5, 3, 4, 6 (all Docker-only, shippable behind no flag), then 7, 8,
9, 10, 11, 12. PRs 3 and 4 are the riskiest because they touch crash recovery;
they should merge only with the existing recovery test suites green and each
change split per module if it exceeds review size.

## 9. Open questions for the owner

1. **dind**: accept privileged dind in a dedicated, tainted node pool for v1,
   or require a sandboxed RuntimeClass (Kata/gVisor/Sysbox) before enabling
   Kubernetes at all? It decides PR 12 timing and node image requirements.
2. **Cache model**: local PVs per node with slots (recommended) vs hostPath
   (faster, weaker isolation) vs a remote cache backend only? Also: how many
   concurrent jobs per node, i.e. slot count?
3. **Where velnor-host runs**: out-of-cluster (kubeconfig credential in the
   keychain) or as an in-cluster Deployment? This changes the credential and
   RBAC story and crash-recovery assumptions (host restarts with Pods alive).
4. **Cluster scope**: is the tailrocks k3s cluster dedicated to CI, or shared
   with other workloads? Shared means a hard requirement for namespace PSA,
   dedicated nodes and NetworkPolicy from day one.
5. **Seccomp distribution**: install `runner-v1.json` on every node
   (Localhost profile) or accept `RuntimeDefault`?
6. **Pids and inode limits**: kubelet-level `podPidsLimit` is cluster-wide;
   OK to set it on the CI pool, and accept no per-Pod tmpfs inode limit?
7. **Journal rename**: is renaming `docker_*` phases to `runtime_*` with a
   read alias acceptable, or should the old names remain forever?
8. **Mac**: confirm the Mac host path stays Docker-only (OrbStack) and out of
   scope for Kubernetes.
9. **Autoscaling**: fixed Hetzner node count, or cluster autoscaler? Warm-cache
   locality and node churn interact; scale-down would drop node-local caches.
