# Release Publication Decisions

**Release-asset Maven bundle choice.** Adopted per the split brief; owner may
revisit. GitHub Packages Maven is repository-scoped and cannot serve private
cross-repository consumers with a scoped CI credential, so Java libraries
ship as release-asset Maven bundles consumed through the same App-token fetch
as Cargo git dependencies. Rejected: a machine-user classic PAT in every
consumer.

**Owner-approved rolling preview exception (2026-10-20).** The consumer native
preview channel reuses one explicitly qualified existing mutable `preview`
release and advances its lightweight tag only by a normal descendant update.
It may replace that channel's generation-owned assets through the authenticated
rolling transaction. Repository immutability remains enabled. This exception
never permits deleting/recreating/redrafting/republishing the preview release,
force-moving a tag, changing permanent stable releases, or weakening image and
release-bundle publication. See the [native channel
contract](proposed/native-preview-contract.md); the other publication rules in
[hard decisions](hard-decisions.md) remain unchanged.

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
