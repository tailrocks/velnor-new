# Controller-owned guest resource probe

**Status:** Proposed controller contract. Source-level implementation may
exercise injected test providers, but the production image provider remains
unavailable until its verifier and source-bound image release are qualified.
This document does not claim a qualified probe, live installation, or safe
Scale Set capacity.

This contract implements the controller boundary referenced by the
[resource-admission policy](runner-resource-admission.md). The selected Docker
engine is authoritative for guest measurements. Host CPU, memory, and disk
values are never substitutes.

## Boundaries and admission

The probe is a separate controller-owned container. It must not use or widen
the runner/DinD `CreateProjection`, runner bind allowlist, worker recovery
semantics, or worker mounts. It receives no runner workspace, Docker socket,
JIT, GitHub credential, job environment, or worker network access.

Each start attempt must obtain a fresh, single-use permit. A permit is private,
non-cloneable, non-serializable, and cannot be cached in a session or reused by
a later poll. Mint it only after the static resource calculation, selected
engine/root binding, successful probe and exact cleanup, post-sample identity
check, deadline, and all numeric admission rules pass. Consume it once at the
acquire/JIT/start boundary.

Both production start paths require separate attempts:

- Before the initial session-scale start, run a fresh sample after the
  completion worker starts. If unavailable on a verified engine, skip the new
  start and keep the session alive.
- In each later poll, preserve completion intake and completion-only
  acknowledgments first. Reconciliation, exited-worker release, and exact
  cleanup remain available without a probe. Before a new acquire/JIT/start,
  take a fresh sample for that attempt. If unavailable, leave the job
  unacquired and unacknowledged and keep polling completion.

A D21 poll-header sample never authorizes a later worker start. Missing or
stale pressure data holds the last header and blocks new starts while the
session continues to poll completion. The initial header remains exactly one;
it is a protocol value, not a worker permit. A zero static fit denies new
work even with header one. The D21 thresholds, disk bands, one-step behavior,
and no-quota limitation are defined in the resource-admission policy and must
remain in force. Lower capacity never kills running workers.

An unbound or mismatched engine is a hard stop for Docker mutations. On an
already verified engine, missing, malformed, stale, unsupported, or
timed-out probe data denies new work but does not suppress completion,
reconciliation, or exact cleanup for existing worker rows.

## Typed projection and sample

Use a private controller projection with its own constructor, serializer,
inspector, and tests. It must not be representable as a runner or DinD
projection. Build it only from current selected-engine `info` and a verified
image-provider result. Use an operation-scoped name and exact role, instance,
engine, and operation labels. The projection has a fixed executable and
configuration, no arguments or environment values, no writable mounts, no
restart, a fixed numeric non-root UID/GID, a read-only root, no network, no
privileges, all capabilities dropped, and no-new-privileges.

The only bind is the canonical, normalized, absolute, non-root
`DockerRootDir` reported by that selected engine, read-only at one fixed target.
This gives the small trusted probe a sensitive read capability over
world-readable cache and image data even though it only calls `statvfs`.
Independent qualification must inspect the effective UID, exact mount, image,
security options, and output boundary. Never use this mount in runner or DinD.
Do not accept a caller path override or print or persist the path itself.

The executable reads only fixed `/proc/loadavg`, `MemAvailable` from
`/proc/meminfo`, and `statvfs` metadata from the fixed target. It has no Docker
API access. The controller obtains `MemTotal`, engine identity, and
`DockerRootDir` from the selected daemon's `info` response. It binds the
sample to that engine/root for the same attempt and verifies both again after
sampling. A previous poll or attempt's sample is never reusable.

Schema 1 is one canonical JSON object with exactly six keys:
`schema_version`, `docker_root_free_bytes`, `docker_root_total_bytes`,
`memory_available_bytes`, `load_milli`, and `memory_psi_some_avg10_bps`.
The first five are unsigned integers; PSI is required but may be an unsigned
integer or `null`, and is diagnostic only. Producer, smoke validator, and
controller enforce the same maximum of 512 stdout bytes including the sole
terminating newline. Reject extra/missing/duplicate keys, malformed or
noncanonical output, trailing records, absent newline, overflow, and
out-of-range values; never persist or log raw output.

Convert `MemAvailable` kB to bytes with checked multiplication and require it
not to exceed `MemTotal` from engine `info`. Compute Docker-root available
bytes as checked `f_bavail * f_frsize` and total bytes as checked
`f_blocks * f_frsize`; require positive fragment size, available blocks no
greater than total blocks, free bytes no greater than total bytes, and
representable values. Parse load into checked fixed-point form. PSI read or
parse failure yields `null` and never gates admission. Any required metric
failure yields no permit.

One monotonic deadline covers the entire probe transaction (30 seconds for
isolated qualification); each Docker request is bounded by the smaller of 10
seconds and the transaction time remaining. Timeout, overflow, failed
inspection, or failed cleanup yields no permit. Exact probe cleanup must be
proven before the post-sample engine/root check and permit minting.

## Source-bound image dependency

The host accepts no mutable tag and never pulls the probe image. A private,
fail-closed provider must authenticate the official host binary's source
commit and the exact same-source image-release manifest, signer authority,
attestation, and archive checksum before offline image load. The archive
descriptor chain and Docker store-specific runtime identity must be validated
by the source-owned provider; the controller uses only the provider's
daemon-specific immutable runtime image ID and binding fingerprint. It must
not assume that Docker's runtime image ID always equals the config digest.

No qualified product host verifier or provider is currently available. Until
that source-owned API passes independent verification, production image
readiness is unavailable, so no production permit can be minted. Test fakes
may exercise projection and lifecycle behavior but do not qualify image trust
or production readiness. Do not introduce shell, PATH, mutable-tag, or
credential-based fallbacks.

## Durable operation lifecycle

Use a dedicated journal table/API, separate from runner launch and worker
volume state. Each row binds the journal instance, selected engine identity,
a domain-separated digest of the canonical Docker root, exact verified source
and image binding, operation ID/name, role, canonical projection identity,
phase, and immutable container ID once confirmed. Keep the root path only in
process memory. Never store raw output, tokens, or a reusable resource sample.
Restart always requires a new sample.

Commit operation intent before each external effect, with constrained
transitions:

1. `Prepared` records the verified binding and complete projection before
   container mutation. No Docker effect is allowed before `CreateRequested`.
2. On restart, a provably `Prepared` row may transition to `Aborted` without a
   Docker call because no effect can precede the durable `CreateRequested`
   transition. If that ordering cannot be proved, quarantine it.
3. `CreateRequested` precedes create. Persist the exact returned ID before
   start. A lost create response triggers exact-name recovery; never blindly
   create again.
4. `StartRequested` precedes start. Resolve an ambiguous response only by
   inspecting the exact ID on the same verified engine and checking labels,
   complete projection, and image binding. A result can authorize the current
   attempt only when the exact container has a successful exit and its bounded
   record validates for that same live attempt. A merely started, queued, or
   still-running container never supplies a permit. Unknown execution state is
   quarantined.
5. Observe and validate output once in memory. Do not journal a reusable
   sample. A sample interrupted by restart is discarded, even when output was
   already observed.
6. Persist `StopRequested` and `RemoveRequested` before those requests. A lost
   response is resolved only by bounded inspection of the exact recorded ID
   on the same engine.
7. Mark `Removed` only after exact same-engine inspection proves that ID is
   absent. Otherwise retain the row for recovery and block new probes.
8. Missing or multiple candidates after an ambiguous create, changed engine,
   role/label/config/image mismatch, unreadable state, unsupported status, or
   corrupt phase is `Quarantined`. Quarantine never triggers prefix deletion,
   prune, guessed ownership, or runner cleanup.

Resolve every unresolved row before a new probe. Inspect only its exact name
or immutable ID, and clean only one fully matching owned container. A
same-engine exact-ID not-found response is the only absence proof after a
remove. Probe recovery failure blocks new work but leaves verified-engine
completion processing and exact worker cleanup available.

## Qualification requirements

Tests must cover exact projection acceptance and rejection of extra mounts,
arguments, environment, job data, network, privilege, capabilities, mutable
image selection, or wrong UID; every parser, numeric, overflow, newline,
output-size, and deadline boundary; missing guest metrics; engine/root change;
duplicate or mismatched containers; and every journal transition and crash
point, including lost create/start/wait/log/stop/remove responses and
ambiguous execution. Cover nonzero exit, invalid record, and restart after
valid output without granting a permit. Include restart/migration tests that
preserve active runner rows and prove `Prepared` abort performs no Docker call.

Exercise both real production start paths. Missing/stale sample, unverified
image, static zero fit, insufficient guest memory, low Docker-root space, or
identity mismatch must produce no acquire/JIT/create/start. Completion intake,
completion-only acknowledgment, reconciliation, and exact cleanup must remain
available when a sample is unavailable on a verified engine. Then qualify the
exact source-bound image offline on the selected Docker backend and run a
representative consumer workflow with measured resource behavior. Source
tests, injected samples, and green CI do not replace those qualifications or
authorize publication or release.
