# Scale Set production resource admission

**Status:** Proposed design. It is not evidence of a qualified probe, a safe
resource budget, or a live runner installation.

This contract extends the active [macOS Scale Set runner](macos-scaleset-runner.md)
specification. The selected Docker daemon is the authority for guest CPU,
memory, and Docker-root measurements. macOS host totals and filesystem free
space are not substitutes for guest measurements.

The controller-owned probe projection, output record, admission attempt, and
operation-journal lifecycle are specified in the
[resource-probe controller contract](resource-probe-controller.md).

## Resource budgets and worker creation

Every new worker pair uses the validated `ResourceBudget` made from the four
required `[host.resources]` fields. The configured CPU and memory sums are the
per-job admission cost. The host configuration must reach the launch loop and
the final stage that builds both Docker create requests; there is no
unbudgeted production constructor or fallback. Docker receives the runner's
limits on the unprivileged runner container and the DinD limits on that
worker's private DinD container. Each container's swap limit equals its memory
limit. A failed or incomplete projection fails before JIT delivery.

The current [D21 decision](../implemented/macos-scaleset/decisions.md) and
implementation sample macOS host load, memory pressure, and `df /`; this
contract intentionally migrates those policy inputs to the selected Docker
guest. It preserves D21's one-slot-per-poll header adjustment, intended
initial advertisement of exactly one, and rule that a lower header never kills
running jobs. The current `advertise()` seeds the count at one but evaluates a
healthy first sample through the growth step, so it can emit two; the proposed
implementation closes this gap by sending one on the first header regardless
of sample. Only a later poll may grow the header. A missing or stale poll
sample holds the previous header. During
an active session, a missing or stale guest sample also holds that session and
blocks job acquisition and worker start; host measurements are never a
fallback. PSI is diagnostic only and does not gate capacity or admission.

The initial poll header of one is a protocol value, not a worker permit.
Resource-derived total pair capacity remains separate from that header. If
static guest CPU or memory totals cannot fit one configured pair, preserve the
zero-fit result even while the header remains one; deny new acquisition, JIT,
and worker start until a fresh static calculation can fit a pair and the
current guest-sample gate passes. This denies new work only: completion intake,
reconciliation, and exact cleanup for existing rows must remain available.

The migrated D21 load thresholds remain 750 milli-load per guest CPU for
growth (`<= 750`) and 1150 milli-load per guest CPU for shrink (`> 1150`). The
memory thresholds remain 8 GiB for growth and 4 GiB for shrink. Disk is a
global pressure signal: below 10 GiB D21 shrinks the advertised header by at
most one slot; from 10 GiB through less than 20 GiB disk alone holds the
header; at or above 20 GiB disk permits one-slot growth when load, memory,
resource, and `max_jobs` bounds also permit it. Other load or memory pressure
may still shrink the header while disk is in its hold band. Separately, below
10 GiB of measured guest Docker-root available space is a hard no-new-start
gate. This admission gate does not replace D21's one-step header change.
Neither disk threshold is a per-job allocation or quota.

For each capacity calculation, convert CPU to one exact common unit.
Selected guest vCPUs become NanoCPUs with checked multiplication by
1,000,000,000; reserve exactly one guest CPU in that unit. Convert configured
pair millicores to NanoCPUs with checked multiplication by 1,000,000. Sum the
positive, finite Docker `NanoCpus` values of extant journal-owned containers
directly in NanoCPUs, with checked addition. Missing, zero/unlimited,
negative, or overflowing CPU values fail closed. The available CPU budget is
the guest total minus the one-CPU reserve, and the upper bound for new pairs
divides the remaining budget by the configured runner-plus-DinD pair cost.

The total-memory bound uses guest `MemTotal` and the configured pair memory
cost. The maximum total pair count is capped by `max_jobs` and the current D21
header. Available new slots are that ceiling minus all occupied journal
permits, floored at zero. For each additional pair, sum its configured cost
with the charges for all occupied rows and require the aggregate CPU and
memory charges to fit those guest totals. Charge each extant journal-owned
container its exact finite CPU, memory, and swap limits, including containers
created under an older configuration. The inspected swap limit must match the
supported no-extra-swap policy; it is validated and recorded but is not added
a second time to physical memory accounting. A truly not-yet-created half is
charged at the current configured limit. A pending permit with neither
container is charged a current pair only after bounded, exact Docker
inspection proves that neither container exists; absent journal IDs after an
ambiguous create are not proof of absence. Unknown ownership, engine identity,
container configuration, or limits fail closed for new admission. Existing
jobs continue, and a lower capacity never kills a worker or releases a permit
before journal cleanup proof.

For each new admission attempt, `MemAvailable` must be at least one current
configured pair's memory cost. This is a separate headroom check; do not
subtract occupied pairs from `MemAvailable` a second time. The synchronous
sample used by that attempt is also bound to the selected engine identity and
canonical Docker-root path. The engine identity and root are checked again
after sampling and must match the values observed before the probe ran. A
missing, malformed, unsupported, stale, or unbound sample blocks
acquisition/start. A sample from a previous poll or attempt cannot be reused.

Named volumes remain unmetered and have no per-job storage quota. The free
space sample can pause future admissions through the existing pressure policy;
it cannot prevent a running workflow from consuming remaining space. The
controller must not describe this policy as enforced storage isolation or as
a bound on disk growth.

## Guest sample and controller-owned probe

The selected Docker daemon's `info` response supplies guest CPU count, total
memory, and `DockerRootDir`. It does not supply live free blocks or
`MemAvailable`. A separate controller-owned probe samples the guest's
`/proc/loadavg` and `/proc/meminfo` plus the selected Docker root. The workflow
runner image and runner `CreateProjection` are not probe mechanisms.

Every admission attempt runs one synchronous, bounded probe against the
selected engine. Its result is for that attempt only and is bound to the
engine identity and canonical `DockerRootDir` observed by that same attempt.
The probe returns guest load and `MemAvailable`; CPU count and total memory
come from that engine's `info` response. It may report memory PSI for
diagnostics, but PSI has no admission threshold under this contract. Docker
root available bytes are `f_bavail * f_frsize`, converted and multiplied with
checked arithmetic. Zero is valid and activates the low-water policy; the
result must be no larger than checked total filesystem bytes and must fit the
host's numeric type. CPU count, total memory, load, and `MemAvailable` are
range-checked; `MemAvailable` may not exceed guest total memory. Missing or
stale data never falls back to a host filesystem or prior sample.

The probe is a small source-built, static, non-root executable in a separate
immutable image. Its expected source identity is repository
`tailrocks/velnor-new`, source ref `refs/heads/main`, and one exact full source
commit. The host binary's provenance must identify the immutable
`binary-${source_sha}` release, source ref, and signer
`tailrocks/velnor-new/.github/workflows/product-release-binary.yml`. The
release coordinator is
`tailrocks/velnor-new/.github/workflows/product-release.yml@refs/heads/main`;
the image-asset signer is
`tailrocks/velnor-new/.github/workflows/product-release-images.yml`, verified
with the exact `workflow_authority_sha` and source digest accepted by that
coordinator. The host build may record its known source commit as build
metadata, but never a post-build artifact digest. The source-bound image
release is the immutable
`runner-${source_sha}` release. It contains
`velnor-resource-probe-linux-amd64.tar`, `RESOURCE_PROBE_MANIFEST.json`, and
`SHA256SUMS`. The manifest binds the archive SHA-256, OCI index digest, selected
`linux/amd64` image-manifest digest, config digest, repository, signer
workflow/ref, workflow authority digest, source commit, and trusted signing
identity. These digests name different bytes and are never interchangeable.
The image release workflow builds the runner, DinD, and probe artifacts from the
same source commit, then signs/attests the manifest and assets. The host
release's verified source commit selects exactly this image release manifest;
neither the host binary nor the manifest embeds a post-build digest that would
create a circular build input. The controller verifies the manifest,
attestation, and archive checksum against that exact repository, release,
workflow, ref, source commit, and signing authority before local image load.
It validates the archive descriptor chain from `index.json` through exactly
one `linux/amd64` image manifest to its config and layers. Any separate
provenance/referrer descriptor must bind to that image manifest and is not an
image platform candidate. Version 1 accepts only the release-produced
BuildKit OCI-layout archive with its Docker compatibility record; duplicate,
unsafe, ambiguous, unrecognized, or unsupported archive entries fail closed.

The controller then inspects the loaded image with an explicit `linux/amd64`
platform and checks its OS, architecture, configuration, and immutable ID
against the validated archive chain. A Docker image ID is store-specific: on
the tested Docker Engine 29.4.0 containerd profile the ID and inspect
`Descriptor.Digest` were the selected image-manifest digest; on the tested
classic profile the ID was the config digest and no descriptor was returned.
The same BuildKit archive loaded in both disposable profiles. Therefore the
host checks the manifest digest for the first inspect shape or the config
digest for the second, and rejects an unknown or inconsistent inspect shape;
it does not require image ID to equal config digest on every engine. The probe
is created by the immutable ID from the verified inspect response, never by a
mutable tag, and no image is pulled from a registry. The exact platform and
store behavior must remain covered by consumer qualification. If the existing
official release chain cannot authenticate and publish this manifest and
asset, probe readiness fails closed until that source-owned release path is
implemented.

The controller accepts only the normalized absolute, non-root `DockerRootDir`
returned by the selected daemon's `info` response; it does not accept a caller
override. It creates the probe through a distinct typed controller-only
projection with that exact path mounted read-only at one fixed target. This
does not relax runner `CreateProjection`, its bind allowlist, or the runner and
DinD mount rules. The probe runs under a reviewed, fixed non-root UID with a
read-only root filesystem, no network, all capabilities dropped,
no-new-privileges, a fixed executable, and no job arguments or environment.
It reads only the fixed procfs metrics and calls `statvfs` on the fixed mount
target; it does not traverse or open Docker-root files. If that UID cannot
obtain the required metrics, readiness fails rather than retrying as root.
Relative paths, noncanonical paths, and `/` are rejected.

Probe output is one versioned, size-limited record of numeric measurements.
The host rejects extra output, missing fields, non-finite values, arithmetic
overflow, out-of-range values, and trailing records. It does not retain or
print the Docker-root path or raw probe output. A bounded monotonic deadline
covers sample execution; the sample is not cached beyond its admission
attempt.

Before any probe create/start/stop/remove request, the journal durably
records the operation intent, selected engine identity, exact
source/manifest/image identity, operation-scoped exact name, role, and
canonical create-configuration identity. The controller binds each successful
create response to its immutable container
ID before advancing the journal. On a lost or ambiguous response and after a
restart, recovery inspects only the exact intended name and verifies the
engine, role labels, full create configuration, and source/image identity. It
may clean up only one fully matching owned ID. No match after an ambiguous
create, multiple candidates, changed engine, or any configuration mismatch is
quarantined and blocks new admission; it is not treated as proof of absence.
Lost start, stop, and remove responses use the same durable intent and exact
ID reconciliation. A remove is complete only when a bounded exact inspection
proves that ID absent on the same engine. No host-wide prune, name-prefix
deletion, runner bind broadening, or volume mutation is allowed.

The read-only whole-root mount is a sensitive read capability over data that
may be readable to container processes; the probe code's use of only
`statvfs` does not remove that capability. It is never mounted into a workflow
runner or DinD container. The probe's source, release binding, image identity,
actual UID, entrypoint, mount, network, capabilities, output parser, timeout,
journal/recovery lifecycle, and cleanup path require independent review before
implementation or installation.

## Qualification requirements

Qualification must use the source-bound probe image and the selected Docker
backend, not a synthetic sample. It must demonstrate that the non-root probe
can read its permitted procfs metrics and measure the actual mounted guest
Docker root, and that missing permissions or malformed data fail closed.
Admission tests must cover configured pair limits on the actual staged runner
and DinD requests; exact aggregate CPU/memory charges for occupied and
uncertain journal rows; old finite, unlimited, and changed-budget containers;
exact proof of absent containers; sample freshness and engine/root binding;
guest low-water and D21 boundaries, including healthy, missing, and saturated
first samples producing an initial header of exactly one; a zero-fit static
capacity retaining that header without acquiring or starting work; and shrink
without killing or releasing existing workers while completion and cleanup
remain available. Probe lifecycle tests must cover every crash boundary and
lost create/start/stop/remove response, including mismatched image, source,
engine, role, labels, configuration, and duplicate candidate quarantine. An
isolated integration qualification must use the exact verified image through
offline load and immutable ID, and prove the non-root UID can read the allowed
procfs metrics and stat the actual mounted guest Docker root. Missing
permissions, malformed records, stale samples, and arithmetic overflow must
fail closed. The no-quota storage limitation remains explicit in all evidence.

## Initial source ownership map

| Concern | Source owner |
|---|---|
| Resource policy and guest sample | `crates/velnor-runner/crates/velnor-runner-host/src/{config.rs,guest.rs,launch.rs,launch_blocking.rs,launch/capacity.rs,launch/pressure.rs,launch/resource_capacity.rs,launch/turn.rs,worker/resource_budget/,worker/resources/guest.rs}` |
| Runtime budget propagation and Docker create projections | `crates/velnor-runner/crates/velnor-runner-cli/src/daemon_run.rs`; `crates/velnor-runner/crates/velnor-runner-host/src/{stage.rs,worker.rs,worker/mounts.rs}` |
| Probe binary and immutable image | a new nested runner probe crate and a separate minimal image source; never the workflow runner image |
| Image asset publication and source binding | `crates/velnor-actions-workflow-renderer/src/schema2_product_release_family.rs`, the source-owned official release workflow and manifest/attestation producer, generated-output capture owner, and `crates/velnor-actions-orchestrator/tests/snapshots/` |
| Probe operation journal, recovery, and cleanup | `crates/velnor-runner/crates/velnor-runner-host/src/{journal.rs,journal/launch.rs,launch/recovery.rs,launch/completion/container.rs}` plus a distinct typed probe projection |
| Behavioral evidence | nested runner host tests for configuration, admission, exact Docker projections, journal recovery, cleanup, and probe execution; an isolated integration check against the offline-loaded verified image |

This map is for design review only. Generated workflows and snapshots must be
updated by their supported renderer and capture procedures. No generated
workflow, image, service, or live configuration is changed by this proposal.
