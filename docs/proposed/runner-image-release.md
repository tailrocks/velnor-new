# Runner image release contract

The owner approved GHCR distribution for Runner, DinD, and the resource probe
on 2026-10-28. This contract replaces their GitHub Release archive distribution.
macOS host and attestation-helper binaries retain their existing GitHub Release
distribution and exact-source compatibility checks.

## Image family

| Component | Repository | Platform |
| --- | --- | --- |
| Runner | `ghcr.io/tailrocks/velnor-runner` | `linux/amd64` |
| DinD | `ghcr.io/tailrocks/velnor-dind` | `linux/amd64` |
| Resource probe | `ghcr.io/tailrocks/velnor-resource-probe` | `linux/amd64` |

Build each image once from the selected source commit. Qualification and
publication use that same image. An Actions artifact may transport OCI bytes
between jobs; it is not a published image archive or a consumer fallback.
Build and integration qualification are separate CI steps. A normal local
build remains a standard command, for example:

```sh
docker build --platform linux/amd64 -t velnor-dind:29.9.1 images/dind
```

Publication reuses the existing fixed image-authority registry transport and
`publish_image` operation. Package scope is explicit and limited to the three
repositories above and the existing `tailrocks/velnor-job` package. Existing
job-image evidence and source/workflow authority retain their contracts.
There is no second registry client or independent Runner-family publisher.

Each image receives `sha-<full-source-commit>` after qualification. A tag that
already names the same digest is idempotent; a tag naming different bytes is
an error and must not be replaced. Readback must verify the published bytes
and exact digest. Runtime consumers never select an image through this tag.

Source-tag publication requires exclusive writer authority. Keep every
Runner-family tag writer under the protected product-release concurrency group
`${{ github.repository }}-product-release` with `cancel-in-progress: false`,
and confirm package write access does not introduce another publication path.
The shared publisher's HEAD/PUT checks are not atomic registry create semantics;
another HEAD does not prevent a competing writer. Do not claim immutable tags
against writers outside that serialized authority. Recorded digest references
remain the consumer identity.

## One immutable manifest

The `runner-${source_sha}` GitHub Release contains exactly
`RUNNER_IMAGE_MANIFEST.json`, `SHA256SUMS`, `LICENSE`, and `NOTICE`. Existing
checksum, asset-inventory, attestation, immutable-release, and source/workflow
verification apply to those exact bytes. Published historical releases remain
unchanged.

Manifest schema version 2 requires these fields:

| Field | Contract |
| --- | --- |
| `schema_version` | Integer `2`. |
| `repository` | `tailrocks/velnor-new`. |
| `source_ref` | `refs/heads/main`. |
| `source_commit` | Full lowercase 40-digit source SHA. |
| `signer_workflow` | `.github/workflows/product-release-images.yml`. |
| `signer_ref` | `refs/heads/main`. |
| `workflow_authority_sha` | The same source SHA under the current protected release authority. |
| `trusted_signing_identity` | Existing pinned OIDC issuer and image-workflow certificate identity, checked against verified provenance. |
| `platform` | `linux/amd64`. |
| `versions` | Required `runner`, `docker`, `buildx`, `compose`, and `node` versions from the selected source's `.velnor/runner.lock`. |
| `images` | Exactly `runner`, `dind`, and `resource_probe` entries. |
| `resource_probe` | Protocol version `1`, image user `65532:65532`, entrypoint `/velnor/resource-probe`, and working directory `/`. |

Each image entry requires `repository`, `tag`, `registry_digest`,
`platform_manifest_digest`, and `config_digest`. Repository and source tag must
match the component and source above. Digests use `sha256:<64 lowercase hex>`.
This single-platform profile publishes the selected OCI platform manifest
directly and requires `registry_digest == platform_manifest_digest`. Indexes
are not accepted in this profile. The config digest identifies different bytes
and must remain a separate field. No local Docker image ID belongs in the
portable manifest. Missing, duplicate, unknown, or incompatible fields fail
admission.

The manifest is not its own trust root. Verify its authenticated asset bytes
and provenance against the compiled source/helper identity before accepting its
claims. Do not weaken the existing main, canonical-caller, source, or workflow
checks to publish pull-request source.

## Host admission

Pull each recorded `repository@registry_digest` anonymously with explicit
`linux/amd64` selection. GitHub release/provenance credentials must not become
registry credentials. Keep bounded downloads, pull progress, inspection, and
daemon/root identity checks. Reject a changed daemon, changed Docker root,
unsupported platform, source/version mismatch, or failed pull before creating
an accepted runtime candidate.

Inspect the pulled image on the selected daemon and validate its config and
identity against the authenticated manifest. Registry digest, platform-manifest
digest, config digest, and local runtime ID have distinct roles. Preserve the
supported classic and containerd image-store checks; do not equate a registry
digest with every Docker image ID. Create containers using the verified local
immutable ID. Creation, ownership recovery, and audit paths must use the same
admitted Runner and DinD identities. Mutable local tags cannot select them.

The probe retains its fixed execution configuration, restricted runtime,
Docker-root binding, bounded six-field protocol, and existing failure behavior.

## Publication and qualification gates

Use the existing GitHub Actions publication mechanism with package write
permission only in the relevant publishing jobs and caller. Confirm package
association, permissions, and public visibility. A public repository alone does
not prove a public package. Anonymous pulls must use no stored user login.

Required live proof covers all three digest pulls on a GitHub-hosted runner
and the Mac, source/version/platform checks, Runner communication through its
private DinD socket, and different private and outer daemon IDs. Qualification
uses unique resource ownership and bounded cleanup. It removes only its own
containers and volumes and preserves unrelated sentinels and built images.

Complete producer and consumer integration before release. Retain unreachable
archive helpers only until GHCR publication and anonymous digest-pull proof;
then remove them. There is no permanent archive publisher, loader, or fallback.
Existing published releases and unrelated local recovery work are preserved.
