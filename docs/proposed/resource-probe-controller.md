# Controller-owned guest resource probe

**Status:** Proposed controller contract. The source includes code to check release files and load the image.
Released host builds lack the source commit, helper digest, and verifier program needed to check an image release.
The released provider is unavailable.
The provider and image release still need qualification.
This document does not prove that installation is live or that a Scale Set has safe capacity.

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

The current projection caps the probe at 100 millicores, 128 MiB memory and
swap, and four PIDs. The shared one-vCPU admission reserve includes that CPU
cap; the shared memory reserve separately includes the full 128 MiB cap plus
the required operator-set operational memory margin. These hard container
limits bound the probe itself. They do not establish that the additional
operational margin covers runtime startup growth; that requires live peak
measurement against the final image and selected engine.

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

### Implemented provider (source only, unqualified)

- **Build identity.** `velnor-runner-host`'s build script reads
  `VELNOR_SOURCE_SHA` (40 lowercase hex) and
  `VELNOR_ATTESTATION_HELPER_SHA256` (64 lowercase hex) and writes them into
  `OUT_DIR/compile_identity.rs`, which `compile_identity.rs` includes. Both or
  neither must be set; one alone, or a malformed value, fails the build. With
  neither, the identity is `None` and `provider_for_build` in
  `launch/resource_probe.rs` selects `UnavailableImageProvider`, which is how
  every local and current release build behaves. With both, a provider
  construction error is a probe error, never a silent fallback to unavailable.
- **Release admission.** `artifact_admission::NativeArtifactProvider` reads
  the GitHub release `runner-<source>` of `tailrocks/velnor-new` with the
  host's REST credential, accepts only redirects to
  `release-assets.githubusercontent.com`, and bounds every response. It
  requires exactly the `SHA256SUMS`, `RESOURCE_PROBE_MANIFEST.json`, and
  `velnor-resource-probe-linux-amd64.tar` rows; a strict-schema manifest whose
  source commit and workflow authority equal the compiled source; checksum,
  manifest, and archive digests that agree; and a deterministic OCI archive
  whose index, manifest, config, user `65532:65532`, entrypoint, and source
  label match the manifest. The verified release is cached in process per
  source, helper digest, and state directory.
- **Signature verification.** The `SHA256SUMS` attestation bundles are
  verified by the separate `velnor-runner-attestation-helper` binary
  (`crates/velnor-runner-attestation`, Sigstore with the pinned production TUF
  root). The helper must sit next to the canonical host executable, owned by
  the same user and not writable by group or others, and match the compiled helper
  digest. The host copies it into a private digest-bound snapshot under the
  service state directory (the journal's directory) and runs it with a cleared
  environment (only macOS `__CF_USER_TEXT_ENCODING` passes through), no arguments, one bounded JSON request on stdin, a 90-second
  deadline, and kill-and-reap on cancellation. The helper requires the GitHub
  Actions OIDC issuer, a URI identity for
  `.github/workflows/product-release-images.yml@refs/heads/main`, and the
  exact Fulcio source, ref, digest, build-config, and runner-environment
  claims for both the checksum file and each subject. Its TUF cache lives in
  `attestation-tuf-cache` under that state directory with rollback, size, and
  lock protection.
- **Offline load.** The archive is loaded into the selected engine with an
  explicit `linux/amd64` platform (Docker API 1.49 or later), inspected, and
  bound to the engine ID and `DockerRootDir` that were checked before and
  after inspection. The controller receives only the daemon-specific runtime
  image ID, platform, source revision, binding fingerprint, and inspected
  config, as `VerifiedCandidate`.

### Remaining before a production permit

The product release must build and ship the helper beside `velnor-host` and
compile `velnor-host` with the release source commit and the helper digest
(inside [hard decision 8](../hard-decisions.md#8-publication-only-from-the-default-branch-or-a-release-tag)).
Until then, and until the provider and image release pass independent
verification on the selected Docker backend, production image readiness is
unavailable and no production permit can be minted. Test fakes may exercise
projection and lifecycle behavior but do not qualify image trust or production
readiness. Do not introduce shell, PATH, mutable-tag, or credential-based
fallbacks.

Pull-request selection consequence ([hard decision 3](../hard-decisions.md#3-pull-requests-test-only-what-they-affect)):
a package with a build script is always `undeclared_reads`, and
`[[stacks.rust.inputs]]` cannot declare build-script inputs, so the
`velnor-runner-host` and `velnor-runner-cli` crate jobs now run on every pull
request.

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
8. Missing or multiple candidates after an ambiguous create,
   role/label/config/image mismatch, unreadable state, unsupported status, or
   corrupt phase is `Quarantined`. Quarantine never triggers prefix deletion,
   prune, guessed ownership, or runner cleanup.

A selected-engine identity or `DockerRootDir` change is a hard stop: perform no
Docker mutation and mint no sample or permit under the replacement binding.
Retain the journal's original engine/root binding so a later attempt can
reconcile or clean it only after fresh `info` confirms that exact original
engine and root again. A restarted or interrupted sample is never reused.
This temporary endpoint drift is distinct from a row/container ownership,
projection, configuration, or unsupported-state mismatch, which is
quarantined.

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
