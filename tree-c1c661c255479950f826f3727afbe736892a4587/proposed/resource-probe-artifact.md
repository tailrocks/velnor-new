# Resource probe image artifact

**Status:** Proposed. This document defines an artifact contract; no probe,
image, publication, or host-consumer qualification is claimed yet.

**Design baseline:** `657443419928de54512fb788c45440ce8cc46949`.

## Purpose and boundaries

The resource probe is a small Linux/amd64 container image that returns bounded
measurements from the private Docker guest used by one Scale Set worker. It is
an observation tool, not a quota, storage limit, scheduler, or admission
controller. The host controller remains responsible for selecting the exact
official release, verifying its source and attestation, creating the isolated
container, validating its response, and applying resource policy. This proposal
does not change host sampling, admission, or image-cryptography code.

Velnor Actions continues to render the image release workflow; it does not run
the probe or implement its protocol. The current image-family publisher is
`runner-${source_sha}`. The probe archive and its manifest become two more
assets in that existing immutable release family, alongside the runner and
DinD image archives. They are not generator release assets or a replacement
for the generator's release manifest.

The current Scale Set specification says free-space and memory measurements
describe headroom, not durable quotas, and storage isolation remains unproven.
This artifact must preserve that limitation.

## Probe protocol

The executable takes no arguments and reads no environment variables. On
success it writes exactly one compact JSON object followed by a newline to
stdout. All six keys are required; the first five are unsigned integers and
the PSI value is either an unsigned integer or `null`. The host rejects
malformed JSON, duplicate or unknown keys, missing values, overflow, trailing
output, and output above 512 bytes. A failed mandatory read or conversion
exits nonzero without writing a partial record. Diagnostics go to stderr only.

| Field | Meaning and source |
| --- | --- |
| `schema_version` | Integer `1`. |
| `docker_root_free_bytes` | Checked `f_bavail * f_frsize` from `statvfs` on `/velnor/docker-root`, a read-only bind of the selected daemon's exact canonical `DockerRootDir` returned by that same daemon's `info` response. The controller binds the sample to the engine identity and root path observed before the probe, then checks both again after it; the probe accepts no path override. |
| `docker_root_total_bytes` | Checked `f_blocks * f_frsize` from `statvfs` on the same mounted filesystem. This describes the actual mounted filesystem, not a value inferred from Docker info. The probe rejects multiplication overflow and any sample where available bytes exceed total bytes. |
| `memory_available_bytes` | `MemAvailable` from the fixed `/proc/meminfo` file, in kB multiplied by 1024 with checked arithmetic. |
| `load_milli` | The first `/proc/loadavg` value (one-minute load), multiplied by 1000 and rounded upward using checked integer arithmetic. The host normalizes this value by the guest CPU count for the 750/1,150 milli-load-per-CPU policy thresholds. |
| `memory_psi_some_avg10_bps` | Either `null` when PSI is unavailable, disabled, unsupported, or malformed, or `some avg10` from `/proc/pressure/memory` represented in hundredths of a percent (10,000 = 100%). It is diagnostic only and does not gate admission. |

Every key is present in the version-1 record. `schema_version`,
`docker_root_free_bytes`, `docker_root_total_bytes`,
`memory_available_bytes`, and `load_milli` are required unsigned integers; PSI
is the sole nullable field. Reads from
`/proc` use fixed paths and small explicit byte caps. Parsers accept only the
documented decimal forms, reject signs, exponent notation, missing required
fields, duplicate fields, invalid ranges, and arithmetic overflow, and never
fall back to another file or source. Failure to obtain a valid mandatory
measurement fails the sample; PSI read or parse failure produces `null` and
does not deny an otherwise valid sample. The probe does not accept a path
from a caller. The runtime contract limits stdout to 512 bytes; the host
enforces the same limit before parsing and rejects unknown keys or missing
keys.

The probe uses safe Rust APIs only. The filesystem query uses the exact pinned
`rustix` registry dependency in the nested runner workspace; no FFI or
`unsafe` code is allowed. The executable is statically linked for
`x86_64-unknown-linux-musl` and has no dynamic runtime dependencies.

## Image execution contract

The separate `images/resource-probe` build context produces a `scratch` image
containing only the probe executable. Its fixed image configuration is:

- platform `linux/amd64`;
- numeric user and group `65532:65532`;
- working directory `/`;
- entrypoint `/velnor/resource-probe`, with no command and the exact
  `Config.Env` produced by the pinned image builder (the tested Docker 29.4.0
  fixture contains only its default `PATH` entry);
- an OCI revision label equal to the source SHA used for the release build.

The host-side runtime owner must run the image with a read-only root filesystem,
no network, all Linux capabilities dropped, no-new-privileges enabled, no
arguments or environment overrides, and exactly one read-only bind mount of the
selected daemon's canonical `DockerRootDir` to `/velnor/docker-root`. The host
gets this source path from that daemon's `info` response, rejects a
noncanonical or root path, and rechecks the same engine identity and path
after sampling. It must not mount the outer
Docker socket, job workspace, host home, credentials, or another host path.
Those options and the image's release identity are enforced and tested by the
host-side work, not by this image's Dockerfile. The image itself cannot verify
that the mount is read-only; the controller must create it that way.

That mount exposes the selected worker's whole daemon data root to the probe
process. Read-only prevents writes; it does not prevent reads that the
container's UID is allowed by filesystem permissions to perform. The fixed
probe implementation only calls `statvfs` on the mount and never opens,
enumerates, or emits data from Docker-root files, but this is not a kernel
confidentiality boundary against a compromised probe. The trust boundary is
the controller-only, source-bound and attested image running as a fixed
non-root UID with no network, dropped capabilities, no job-provided input,
numeric bounded output, and no retained raw output. The controller never
passes this image or mount to the job or DinD container. A narrower same-
filesystem anchor is a possible future design only if its creation and exact
binding to the selected daemon's `DockerRootDir` are proven without adding an
unreviewed write or accepting a substitutable path; this proposal does not
depend on such an anchor.

Images built before the explicit `WORKDIR /` profile was added do not satisfy
this contract and must be rebuilt and requalified before use. The image build
workflow must inspect the built image's OS, architecture, user, working
directory, entrypoint, absent command, exact builder-produced environment, and
revision label. It must also run the actual image as UID/GID 65532 with the same
isolation options and a disposable read-only directory mounted at the fixed
path, then validate one bounded protocol record. This proves the packaged
static executable starts under the declared profile; it does not qualify the
host's security options or resource policy.

## Build, manifest, and publication

At the design baseline, the renderer's active image path is
`schema2_product_release.render` →
`schema2_product_release_family_jobs::family_document(Family::Images)` →
`schema2_release::image_release`. `Family::Images::asset_paths` supplies the
exact inventory used by preparation, publication, checksums, and attestation
verification. The generated `.github/workflows/product-release-images.yml`
must not be hand-edited.

The image build job will use the existing pinned Rust toolchain plus typed
Mise-adapter commands to install the musl target and build the nested probe
package with its lockfile. It stages only that executable and the tracked
Dockerfile into a temporary build context, builds the probe image with the
exact release source SHA label, inspects its fixed configuration, runs the
isolated smoke check, and saves the archive. It does not build from a floating
Rust image or execute a post-build tool from an unpinned package source.
It reloads the exact saved archive into the selected daemon and inspects the
loaded `linux/amd64` image before writing the manifest. The later release
assembly step saves only the existing runner and DinD images; it verifies the
probe archive and manifest exist, then checksums those same bytes without
rewriting the probe archive.

The build job produces `RESOURCE_PROBE_MANIFEST.json` from validated metadata
of the image it actually built and saved. The manifest has a fixed schema and
field order, contains no timestamp, and includes these identity fields:

| Field | Required value and verification |
| --- | --- |
| `repository` | `tailrocks/velnor-new`; match the attested source repository. |
| `source_ref` | `refs/heads/main`; match the attested source ref. |
| `source_commit` | Full source commit SHA; match the source digest accepted by `product-release.yml` and the attested digest. |
| `signer_workflow` | `.github/workflows/product-release-images.yml`; match the exact image-asset signer workflow. |
| `signer_ref` | `refs/heads/main`; match the signer identity. |
| `workflow_authority_sha` | Exact workflow authority digest accepted by `product-release.yml`; match the signed attestation's signer digest. |
| `trusted_signing_identity` | Object with `oidc_issuer` and `certificate_identity`; both must match the verified attestation certificate and the pinned Velnor policy. |

The version-1 `trusted_signing_identity` values are:

- `oidc_issuer`: `https://token.actions.githubusercontent.com`;
- `certificate_identity`:
  `https://github.com/tailrocks/velnor-new/.github/workflows/product-release-images.yml@refs/heads/main`.

The coordinator and host verifier must check those values against the verified
attestation certificate, not accept them from the manifest as authority. The
remaining identity fields bind the exact repository, source ref, source
commit, signer workflow/ref, and workflow authority accepted by the protected
coordinator for that run.

The manifest also binds `platform`, `archive_format`, `archive_sha256`,
`oci_index_sha256`, `image_manifest_digest`, `config_digest`,
`protocol_version`, `image_user`, and `entrypoint`. These digests identify
different layers of the saved artifact and must never be treated as
interchangeable:

| Field | Identity |
| --- | --- |
| `archive_format` | Exact versioned profile `buildkit-oci-layout-docker-compat-v1`; other values are unsupported. |
| `archive_sha256` | SHA-256 of the complete published tar archive. |
| `oci_index_sha256` | SHA-256 of the exact `index.json` bytes in the archive. |
| `image_manifest_digest` | Digest of the one selected `linux/amd64` OCI image manifest in the index. |
| `config_digest` | Digest of the image configuration JSON referenced by that image manifest. |

The build validates the descriptor chain from `index.json` to the selected
image manifest, its config, and every layer before it writes the manifest.
It also checks that the config says `linux/amd64` and matches the inspected
user, entrypoint, absent command, exact builder-produced environment, and
revision label. The probe itself reads no environment values, and the host
passes no runtime environment overrides. A root index may
also contain a separate provenance/referrer descriptor. That descriptor is
not an image platform candidate: the build and host validate its subject
against the selected image manifest and keep its identity separate from the
image manifest and config digests. The version-1 archive parser accepts only
the release-produced BuildKit OCI-layout archive with its Docker compatibility
record, requires a unique `linux/amd64` image candidate, validates every
descriptor, size, and digest, and rejects duplicate or unsafe archive paths,
links, unrecognized entries, ambiguous image candidates, and unsupported
archive forms. It cross-checks the Docker compatibility record against the
selected OCI image. The classic store's separately generated save archive is
not a version-1 input format; format extensions require a reviewed parser
change and new saved-archive fixtures.

The loaded Docker image ID is not a portable substitute for either digest.
Docker Engine 29.4.0 returned the selected image-manifest digest as the ID in
its containerd image store, and the config digest as the ID in its classic
image store. The same BuildKit archive loaded successfully in both disposable
store profiles. Therefore the host validates the archive identities first,
then performs `image inspect --platform linux/amd64` on the selected daemon and
checks the returned OS, architecture, and image configuration against the
manifest. It validates the immutable inspected ID against the matching
store-specific identity: the image-manifest digest when the inspected result
exposes the containerd descriptor, or the config digest for the tested classic
result without a descriptor. Descriptor and ID fields, when present, must
agree with the validated archive chain. An unrecognized inspect shape or
identity fails closed. The host creates the probe using the immutable ID from
that verified inspect result; it never creates by the archive tag. The exact
Docker Engine version and inspect behavior must remain covered by consumer
qualification before this proposal is implemented.

The archive SHA-256 is separately authenticated by the published asset
inventory and is not the Docker image ID, manifest digest, or config digest.
The manifest is data about the archive, not an independent trust root: the
host verifies each identity field against the accepted coordinator and signer
identity extracted from the verified attestation, never against the manifest
alone. The release job computes `SHA256SUMS` over the runner archive, DinD
archive, probe archive, and `RESOURCE_PROBE_MANIFEST.json`.

The observed Docker Engine 29.4.0 mapping is evidence for this design
correction, not a product or consumer qualification:

| Disposable/observed store | `image inspect --platform linux/amd64` ID | Descriptor digest |
| --- | --- | --- |
| containerd snapshotter | `sha256:055b3124b01a1b4b5b1c06fcd8b2b27859948c0c9812129121157b8ed78fed48` (image manifest) | same image-manifest digest |
| classic VFS store | `sha256:2fb80254177669698c443a8414897d0afcf40f1eac64456cb149339e599c66bb` (config) | absent |

For that archive the config digest was
`sha256:2fb80254177669698c443a8414897d0afcf40f1eac64456cb149339e599c66bb`,
distinct from the selected manifest digest. The configured outer OrbStack
daemon also returned the manifest digest for explicit `linux/amd64` inspect
and an index digest for default multi-platform inspect; the host must always
specify the required platform. These observations do not qualify the eventual
consumer or any engine version beyond the tested fixtures.

The version-specific identity behavior is corroborated by the pinned engine
source: [containerd image inspection](https://github.com/moby/moby/blob/docker-v29.4.0/daemon/containerd/image_inspect.go)
returns the selected target descriptor digest, while [classic image
inspection](https://github.com/moby/moby/blob/docker-v29.4.0/daemon/images/image_inspect.go)
returns the image ID derived from its config. [Containerd archive
export](https://github.com/moby/moby/blob/docker-v29.4.0/daemon/containerd/image_exporter.go)
and [classic archive export](https://github.com/moby/moby/blob/docker-v29.4.0/daemon/images/image_exporter.go)
use different store paths. These sources support keeping archive, manifest,
config, and loaded image identities distinct; the saved-archive fixtures are
the evidence for the exact load and inspect behavior in the tested profiles.

The exact five-file release inventory is:

1. `velnor-runner-linux-amd64.tar`
2. `velnor-dind-linux-amd64.tar`
3. `velnor-resource-probe-linux-amd64.tar`
4. `RESOURCE_PROBE_MANIFEST.json`
5. `SHA256SUMS`

The existing image attestation job attests every file in this inventory. The
existing immutable publisher verifies each checksum, asset digest and
attestation against the exact source SHA and image-workflow authority, then
publishes the full inventory at `runner-${source_sha}`. Preparation and
publication continue to reject an occupied tag or release whose metadata,
immutability, asset set, or proofs do not match; no asset is overwritten or
added after publication. The manifest alone never authorizes loading or
running an image.

## Planned source ownership

| Path | Responsibility |
| --- | --- |
| `crates/velnor-runner/Cargo.toml`, `Cargo.lock` | Add the probe package to the existing nested workspace and lock its exact dependencies. The root generator workspace remains independent. |
| `crates/velnor-runner/crates/velnor-resource-probe/` | Minimal Rust executable, fixed-path numeric parsers, bounded output schema, and focused unit tests. |
| `images/resource-probe/Dockerfile`, `README.md` | Separate scratch-image context and its build/runtime contract. |
| `crates/velnor-actions-workflow-renderer/src/schema2.rs`, `schema2_release.rs`, `schema2_product_release_family.rs`, and a small probe-release helper module | Compose the pinned build, verification, archive, manifest, and exact image-family asset inventory. |
| `crates/velnor-actions-orchestrator/src/product_release_pins.rs` and renderer pin fixtures | Resolve the exact pinned musl-target installation and nested-package build argv through existing typed Mise adapters rather than handwritten tool resolution. |
| Renderer/orchestrator release tests and generated snapshots | Verify the fixed build profile, manifest contents, exact asset inventory, attestation inputs, and rendered workflow. Generated `.github` files and snapshots are refreshed only through the supported CLI and capture procedures. |
| `docs/proposed/README.md` | Index this proposal after independent design review accepts its scope and contract. |

The later host consumer owns container creation, read-only bind enforcement,
bounded stdout collection and strict parsing, image signer/source verification,
sample freshness, admission thresholds, and resource lifecycle. The release
artifact work must not add host modules or claim those controls are complete.

## Required proof before implementation is called ready

- Unit cases cover exact numeric parsing, upward load rounding, byte conversion,
  checked Docker-root total/free multiplication and ordering, zero and
  boundary values, malformed and oversized input, and overflow.
- Image inspection rejects wrong platform, user, working directory, entrypoint, command,
  environment, or source label. The actual smoke run proves the static image
  starts as the numeric non-root user with the required restrictions and emits
  one record within the cap.
- Saved-archive fixtures cover every enabled Docker image-store profile. They
  verify the archive descriptor/config chain before load and verify that each
  supported daemon returns the expected immutable inspect identity for the
  explicit `linux/amd64` platform; config, manifest, and archive digests must
  remain distinct fields.
- Workflow tests prove the build uses locked nested dependencies and pinned
  Rust/Mise target commands, builds only for linux/amd64, creates the manifest
  from actual image/archive metadata, and includes exactly the five named
  assets in checksum, attestation, prepare, and publish paths.
- The generated workflow passes supported generation, actionlint, shellcheck,
  security scanning, snapshots, and repository gates. No generated workflow or
  snapshot is edited by hand.
- A separate host-side review confirms the protocol and isolation contract
  against the consumer before any claim of end-to-end resource-probe
  qualification. Live Scale Set execution, capacity high-water evidence, and
  consumer adoption remain separate proof obligations.
