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
stdout. All fields are required unsigned integers; the host rejects malformed
JSON, duplicate or unknown fields, missing values, overflow, trailing output,
and output above 512 bytes. A failed read or conversion exits nonzero without
writing a partial record. Diagnostics go to stderr only.

| Field | Meaning and source |
| --- | --- |
| `schema_version` | Integer `1`. |
| `docker_root_free_bytes` | Checked `f_bavail * f_frsize` from `statvfs` on the fixed `/velnor/docker-root` mount. The probe never opens or enumerates files under this path. |
| `memory_available_bytes` | `MemAvailable` from the fixed `/proc/meminfo` file, in kB multiplied by 1024 with checked arithmetic. |
| `load_milli` | The first `/proc/loadavg` value (one-minute load), multiplied by 1000 and rounded upward using checked integer arithmetic. The host normalizes this value by the guest CPU count for the 750/1,150 milli-load-per-CPU policy thresholds. |
| `memory_psi_some_avg10_bps` | `some avg10` from `/proc/pressure/memory`, represented in hundredths of a percent (10,000 = 100%). It is diagnostic only and does not gate admission. |

Reads from `/proc` use fixed paths and small explicit byte caps. Parsers accept
only the documented decimal forms, reject signs, exponent notation, missing
fields, duplicate fields, invalid ranges, and arithmetic overflow, and never
fall back to another file or source. The probe does not accept a path from a
caller. The runtime contract limits stdout to 512 bytes; the host enforces the
same limit before parsing.

The probe uses safe Rust APIs only. The filesystem query uses the exact pinned
`rustix` registry dependency in the nested runner workspace; no FFI or
`unsafe` code is allowed. The executable is statically linked for
`x86_64-unknown-linux-musl` and has no dynamic runtime dependencies.

## Image execution contract

The separate `images/resource-probe` build context produces a `scratch` image
containing only the probe executable. Its fixed image configuration is:

- platform `linux/amd64`;
- numeric user and group `65532:65532`;
- entrypoint `/velnor/resource-probe`, with no command or environment values;
- an OCI revision label equal to the source SHA used for the release build.

The host-side runtime owner must run the image with a read-only root filesystem,
no network, all Linux capabilities dropped, no-new-privileges enabled, no
arguments or environment, and exactly one read-only bind mount of the private
DinD daemon's Docker root to `/velnor/docker-root`. It must not mount the outer
Docker socket, job workspace, host home, credentials, or another host path.
Those options and the image's release identity are enforced and tested by the
host-side work, not by this image's Dockerfile. The image itself cannot verify
that the mount is read-only; the controller must create it that way.

The image build workflow must inspect the built image's OS, architecture, user,
entrypoint, empty command/environment, and revision label. It must also run the
actual image as UID/GID 65532 with the same isolation options and a disposable
read-only directory mounted at the fixed path, then validate one bounded
protocol record. This proves the packaged static executable starts under the
declared profile; it does not qualify the host's security options or resource
policy.

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

The build job produces `RESOURCE_PROBE_MANIFEST.json` from validated metadata
of the image it actually built and saved. The manifest has a fixed schema and
field order, contains no timestamp, and binds the exact source SHA, platform,
Docker image config digest, archive name and SHA-256, probe protocol version,
numeric image user, and fixed entrypoint. The manifest is data about the
archive, not an independent trust root. The release job computes
`SHA256SUMS` over the runner archive, DinD archive, probe archive, and
`RESOURCE_PROBE_MANIFEST.json`.

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
  zero and boundary values, malformed and oversized input, and overflow.
- Image inspection rejects wrong platform, user, entrypoint, command,
  environment, or source label. The actual smoke run proves the static image
  starts as the numeric non-root user with the required restrictions and emits
  one record within the cap.
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
