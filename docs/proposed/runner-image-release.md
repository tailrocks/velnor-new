# Runner image release contract

The owner approved GHCR distribution for Runner, DinD, and the resource probe
on 2026-10-28. This is the target for their GitHub Release archive distribution.
macOS host and attestation-helper binaries retain their existing GitHub Release
distribution and exact-source compatibility checks.

`[runner_image_family]` is supported only by the Velnor repository policy with
`main` as the resolved default branch. Consumer repositories and other default
branches must leave this option absent.

The current product-release image producer still writes Docker-save archives,
attests the shared `image-assets` artifact, and has no per-image publication
record or `qualify-images` job. The native OCI publisher therefore rejects its
current Runner-family output. Keep the existing release workflow active until
the producer emits the closed records below and OCI-layout archives from the
same image builds. Do not add a rebuild or a Docker-save conversion in the
privileged publisher.

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

Publication uses the shared Rust `image-maintenance publish-image` operation
for these three packages and `ghcr.io/tailrocks/velnor-job`. It reuses the
existing pinned Crane tool, credential login, bounded process runner, and OCI
archive guard. Job-image publication authenticates a successful CI producer
and a separate existing candidate attestor that signs the exact CI artifact,
record, and source closure. The attestor must consume that artifact without
rebuilding it. The current GitHub Release workflow remains active until the new
Runner-family producer and consumer are wired together; it is not a fallback
after GHCR cutover.

The producer writes one `velnor.runner-image-publication.v1` record per image:
`runner-image-publication.json`, `dind-image-publication.json`, or
`resource-probe-image-publication.json`. Each closed record binds its archive
and OCI graph to the selected source inputs and exact committed runner lock.
Its `workflow` identifies the successful default-branch push of `ci.yml` at
that exact source commit. The publication record does not assert qualification.

A separate default-branch push of `runner-image-qualification.yml` downloads
the exact CI artifact and qualifies the same Runner and DinD image identities
and the resource probe. After qualification succeeds, a no-checkout attestation
job calls the pinned `runner-image-qualification-attest.yml` signer. It signs
`RUNNER_IMAGE_QUALIFICATION.json`, which contains the source commit and ref,
runner-lock SHA-256, producer run ID and attempt, qualification run ID and
attempt, qualification job ID, and the archive and OCI identities for all
three images. The publisher authenticates the qualifier run and job, verifies
the exact receipt subject and dedicated signer, and compares each candidate's
archive, OCI graph, source and lock to the signed receipt. A workflow run tuple
or an artifact name alone does not bind the job to image bytes. The qualifier
workflow must use an unambiguous job dependency and artifact path so its signer
attests only the receipt produced by the successful qualification job. The CI
producer remains credential-free, and candidate images run without signing or
publication credentials.

Each image receives `sha-<full-source-commit>` after qualification. A tag that
already names the same digest is idempotent; a tag naming different bytes is
an error and must not be replaced. Readback must verify the published bytes
and exact digest. Runtime consumers never select an image through this tag.

`velnor-actions image-maintenance admit-runner-images` inspects the three OCI
archives and writes schema-2 descriptors for the native manifest formatter.
The descriptor includes a lowercase SHA-256 digest of the exact committed
runner-lock bytes. The formatter checks that digest against one bounded lock
read before parsing that same buffer. The selected source must match the clean
checkout and the committed runner lock. The admission command derives manifest
and config digests from authenticated archive bytes and preserves the
[probe's closed execution profile](resource-probe-artifact.md).
These descriptors are
intermediate evidence, not publication authority or runtime qualification.
The publisher must bind the same archives and lock to successful qualification
before publishing or producing the release manifest.

Native OCI materialization uses an absent destination inside a caller-owned
private scratch directory and creates it exclusively. The caller keeps that
scratch directory private and owned through Crane consumption, and consumes
the projection only after materialization returns successfully. On failure,
the materializer removes only the directory it created; if cleanup fails, the
error identifies the destination for recovery. Existing destinations are
never reused, overwritten or removed.

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
| `trusted_signing_identity` | Exactly `oidc_issuer` and `certificate_identity`, checked against verified provenance. |
| `platform` | `linux/amd64`. |
| `versions` | Required `runner`, `docker`, `buildx`, `compose`, and `node` versions from the selected source's `.velnor/runner.lock`. |
| `images` | Exactly `runner`, `dind`, and `resource_probe` entries. |
| `resource_probe` | Protocol version `1`, image user `65532:65532`, entrypoint `/velnor/resource-probe`, and working directory `/`. |

`oidc_issuer` is `https://token.actions.githubusercontent.com`.
`certificate_identity` is
`https://github.com/tailrocks/velnor-new/.github/workflows/product-release-images.yml@refs/heads/main`.

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

The native family publication and verification commands each have a 40-minute
deadline. The publication job has a 50-minute limit, leaving a combined
10-minute allowance for its other setup and release work when those commands
run within the job. This allowance does not guarantee completion for every
cold network or setup duration.

Required live proof covers all three digest pulls on a GitHub-hosted runner
and the Mac, source/version/platform checks, Runner communication through its
private DinD socket, and different private and outer daemon IDs. Qualification
uses unique resource ownership and bounded cleanup. It removes only its own
containers and volumes and preserves unrelated sentinels and built images.

## Additional production Mac qualification

Final manifest attestation and release require production Mac qualification of
the three published digests. This is an additional gate: retain the existing
CI artifact authentication, prepublication qualification, source provenance,
Required check, main reachability and canonical publication authority. A private
workflow dispatch never authorizes image or release publication.

The operator may use a dedicated private repository-scoped official runner.
Enumerate and trust its complete writer, workflow and action surface; labels
alone do not establish admission. Keep private fork workflows disabled, audit
repository access, and enroll the unmodified runner through the operator
deployment path.
Pin the repository and owner numeric IDs, reviewed workflow path and commit,
permitted ref, and server-assigned runner ID in the public authority policy.
Do not use an OIDC smoke workflow as the qualification authority.
The operator controller dispatches the private workflow at a branch or tag ref
and checks that the actual run and workflow commit equal the reviewed pin.
The dispatch API does not accept a raw commit SHA. The controller credential
stays outside both workflows.

The operator stages bounded manifest, lock and request data in the private
repository. Treat that transport as untrusted. The source-free Mac operation
verifies exact bytes and anonymously pulls the three recorded GHCR digests.
It uses the same supported classic/containerd identity and configuration checks
as host admission, including independent registry graph verification. It must
prove the production daemon/root/store binding, platform and versions, private
Runner/DinD communication, distinct inner and outer daemon IDs, and cleanup.
Candidate containers receive neither controller nor Actions credentials, host
home directories, Keychain access, SSH agents, or controller configuration.
The native workflow is a reviewed, immutable-pinned trusted surface with no
checkout or untrusted source execution. Do not pass a local PAT to the workflow
or qualifier. Strip GitHub and Actions token variables before qualification.
A separate native service account or sandbox is optional defense in depth;
same-user execution does not claim isolation from a compromised trusted native
workflow or SDK.

Record owned mutation intent durably before Docker changes in a private,
operator-owned persistent directory outside job workspace and temporary
cleanup. Bind recovery to the selected daemon and exact resource ownership.
Caller cancellation retains the operation owner until settlement. Process
restart reconciles durable state and preserves unrelated resources. An unknown
create outcome cannot be declared settled from inspection alone, whether the
resource is missing or visible; retain it unresolved until actual settlement
is durably recorded.
No receipt is emitted while cleanup or recovery is uncertain. The operation
has one 80-minute deadline and a separate 10-minute cleanup allowance within a
105-minute Mac job limit.

A separate source-free hosted Ubuntu 26.04 job directly depends on the Mac job
and authenticates its successful execution and exact same-run receipt artifact
through its own repository API. The closed workflow has exactly one possible
producer of that receipt artifact: the Mac job. Run association in artifact
metadata alone does not establish uploader job identity.
It checks the actual job runner ID, source/workflow commit, run and attempt,
then signs only the validated receipt with GitHub OIDC and pinned Cosign.
The Mac job has only the read permissions needed for its staged inputs and no
OIDC signing or publication permission. The hosted attestor has only actions
and contents read plus OIDC token write permission. Neither job receives a
broad cross-repository credential.

The signed receipt envelope binds both public and private repository IDs,
public source commit,
canonical release run and attempt, original producer artifact identity, exact
manifest and lock hashes, all three image identities, SDK source/asset identity,
private workflow commit, run/attempt/job/runner identities, daemon observations,
and successful cleanup. Reject duplicate, unknown, missing or mismatched fields.
The exact qualified manifest bytes must remain unchanged before final signing.

The operator may return the receipt and Sigstore bundle through a bounded,
content-addressed, data-only non-default Git branch in the public repository.
The branch must contain no workflow or executable payload and must not trigger
publication. Transport credentials, Git commits and receipt filenames confer
no qualification authority. The public verifier authenticates the bundle's
signature, chain, transparency evidence and signing-time validity, then checks
its exact issuer and workflow identity plus verified certificate extensions:
repository/owner IDs, private visibility, hosted signer environment, workflow
and source/configuration digests, ref, and invocation run/attempt. Missing or
inconsistent claims fail closed. Compare every public source, artifact, manifest,
lock and image binding to the canonical job's own authenticated inputs; reject
replay from another source, run, attempt or manifest.

The public signer remains
`product-release-images.yml@refs/heads/main`. The private signature authenticates
only this additional Mac evidence and cannot replace any original image-byte
provenance, qualification attestation, manifest trust root or release trigger.
A missing proof or unavailable private authority keeps final release closed.

The public verifier shares one 130-minute deadline across authentication,
proof polling, and verification. Polling does not reset that deadline. Its
hosted job has a 140-minute limit. These limits bound waiting and failure;
they do not guarantee private runner availability or successful completion.

Cosign verifies the original envelope and bundle against the SDK's compiled
Sigstore trusted root. The root's SHA-256 is
`6494e21ea73fa7ee769f85f57d5a3e6a08725eae1e38c755fc3517c9e6bc0b66`.
Refresh requires an authenticated Sigstore TUF target, independent review,
and a new qualified SDK. There is no runtime trust-root override or insecure
verification fallback. Chain, certificate transparency, transparency-log, and
signing-time verification remain required.

Successful verification emits the four validated asset digests. Final GitHub
attestation uses each asset's fixed subject name and verified subject digest.
It does not hash a mutable local pathname after verification. Before publishing
the draft release, the publisher compares the exact four server asset names
and digests with those verified subjects. A mismatch leaves the draft closed.
All supported Runner-family release writers share the product-release
concurrency group. This serialization does not protect against administrators
or applications writing outside that authority.

Complete producer and consumer integration before release. Retain unreachable
archive helpers only until GHCR publication and anonymous digest-pull proof;
then remove them. There is no permanent archive publisher, loader, or fallback.
Existing published releases and unrelated local recovery work are preserved.
