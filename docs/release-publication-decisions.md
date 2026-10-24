# Release Publication Decisions

**Release-asset Maven bundle choice.** Adopted per the split brief; owner may
revisit. GitHub Packages Maven is repository-scoped and cannot serve private
cross-repository consumers with a scoped CI credential, so Java libraries
ship as release-asset Maven bundles consumed through the same App-token fetch
as Cargo git dependencies. Rejected: a machine-user classic PAT in every
consumer.

**Immutable per-source preview publication (2026-10-22).** The consumer native
preview channel publishes one immutable prerelease per full source commit SHA.
Version remains `<base>-preview.<full-history-count>+<source-sha7>` and the tag
is `preview-<full-source-sha>`. Repository-wide release immutability stays
enabled. Preview creation authenticates a source-specific tag before creating
the draft, then publishes the complete signed asset set once. It never replaces,
deletes, recreates or republishes a preview tag or release. This decision removes
the 2026-10-20 rolling-preview exception; manual stable version/tag authority,
image publication and permanent release bundles retain their contracts. See the
[native channel contract](proposed/native-preview-contract.md).

**Preview source/workflow separation (2026-10-22).** Let A be the successful
source CI commit, B the current preview workflow definition SHA, and H the
current protected `main` tip. Admission requires authenticated ancestry
`A <= B <= H`; A, B and H are distinct identities with distinct roles. A owns
the version, immutable tag, source configuration, package inventory and signed
release receipt. B owns the active workflow, signer certificate/run/job
identity, and trusted bootstrap/tool custody. A later protected tip does not
invalidate an ancestor A while the ancestry and latest successful CI attempt
for A remain valid. Every recovery still rechecks that exact-A CI attempt and
the current ancestry. Homebrew promotion is eligible only when A is the current
protected `main` tip; otherwise the native result is a typed clean skip. Stable
publication is unchanged. This supersedes the earlier same-SHA and newer-tip
revocation wording in the native preview design.

**Manual stable version and tag authority (2026-10-20).** The owner requires
existing release-plz version management and a permanent stable version tag before
exact-tag binary builds. The fixed qualified manual preparation phase may inspect
version metadata and Git state to propose the version/changelog PR, with native
config disabling tag/release/registry publication. It must not run repository
tasks, builds, hooks or repository-selected release-plz configuration. Permanent
tag creation uses native source-free exact-SHA authority after durable pre-tag
intent and protected human approval. This removes release-plz's mutable
associated-PR source selection from irreversible tag mutation. Tag, signer and
publisher jobs remain source-free; no general privileged checkout or execution
is authorized. The release is published once with complete signed assets and
stays immutable. See [manual stable coordination](proposed/binary-release-contract.md).

**Declared Sigstore-bundle provenance (2026-10-23).** Private repositories on a
plan without artifact attestations cannot persist an attestation (#364). The
attest job keeps the existing fixed `ArtifactAttest` role and keyless OIDC
signing through GitHub's Sigstore instance. A repository may declare
`[publish] provenance = "sigstore_bundle"` to keep the signed bundle and a
receipt as immutable release assets instead of in the attestation store,
verified with `gh attestation verify --bundle` before anything is published.
The default stays the attestation store and fails closed. The bundle never
goes to the public-good log (private identities stay private). The existing
Maven `attest = false` opt-out remains available with a validated reason and
warning; it takes precedence over either repository-level provenance mode.
See the
[package publication contract](proposed/package-publication-contract.md#10-provenance-store-publish-provenance).

**Native Homebrew distribution and adoption (2026-10-24).** Formula policy and
release selection belong to the source repository. Velnor owns the typed
projection, standalone formula renderer, independent literal-update admission,
whole-tree checks, binary install qualification, and protected promotion. A tap
contains conventional generated Homebrew formula DSL, not executable Python or
Ruby helper scripts. Public release verification uses read-only authority and
keeps authenticated archive custody through installation. Disabled stable policy
has a canonical non-installable sentinel; it never produces a fabricated
successful stable installation.

**Protected tap transition authoring (2026-10-24).** Adoption first replaces one
exact cataloged legacy base with a small authenticated SDK callee. The published
SDK source, external manifest digest, and Linux runner binary digest are separate
from the operator's current-host executable identity. Native authoring commands
emit a fresh reviewable overlay and exact deletion manifest; they never apply
files or create branches. The following structural cutover reuses the ordinary
workflow renderer and the same complete-tree validator used by Required. Neither
phase executes candidate configuration, legacy helpers, or candidate-selected
migration code to establish authority.

**Hosted Homebrew custody (2026-10-24).** Hosted qualification uses a freshly
materialized, exact pinned Homebrew source tree and private home. It preserves
the existing prefix and verifies the vendored Ruby archive before first runtime
execution. Mise owns the closed command and process-group execution adapters;
orchestration owns prefix changes, filesystem identities, and retained recovery
state. Strict command completion requires a held process-group anchor and proven
group absence. An uncertain cleanup result cannot authorize restoration. The
qualification job must prove the historical binary upgrade and both archived
recipe tests before SDK release publication.

**Native image maintenance during SDK publication (2026-10-24).** Automatic
job-image candidate builds and the resource-probe producer use source-built Rust
operations. The job candidate keeps its existing v1 wire format and declared
legacy input digests so the independently dispatched publisher remains compatible;
those input files are provenance data and are not executed by the native build.
A separately validated and attested source-closure record binds the native
implementation. Resource-probe production and admission share the same bounded
OCI validator. Required native fixture qualification precedes producer execution.

**Source Homebrew policy export (2026-10-24).** The source repository commits the
canonical release-policy projection used by protected tap verification. Native
export reuses the preview release-policy serializer and source preparation;
ordinary generation rejects a missing or stale policy. Export does not invoke
generation, and generation does not write outside its managed `.github` tree.
Adopters replace the legacy updater declaration with their native formula spec
before adopting this SDK version.
