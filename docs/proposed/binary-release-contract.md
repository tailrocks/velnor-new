# Manual stable binary release contract

**Status:** revised for the human owner's 2026-10-20 manual stable requirement.
This extends the shared [native channel contract](native-preview-contract.md);
implementation and actual hosted qualification are separate acceptance gates.
The old single-bin Apple-only stable workflow is removed, not retained as fallback.

## Configuration and release-plz ownership

Stable uses the shared `stacks.rust.binary_release` product, target, producer,
companion, evidence and version-owner fields. Its independently enabled `stable`
block selects the protected `release` environment. The existing
`stacks.rust.release` must also be enabled, select `registry = "git-tag"`, select
only `version_package`, use the same manifest and stable environment, and use the
literal version tag template `v{{ version }}`. Mismatches fail generation.
`release_pr` may enable manual version-PR preparation; it is not a stable publish
trigger. There is no implicit Cargo-registry publication.

The default release registry is `git-tag`; crates.io publication requires an
explicit `registry = "crates-io"` policy and its independent existing registry
authentication contract. Git-tag mode admits neither registry credentials nor
bootstrap authorization. An application with two native channels does not gain a
second version manager or a shell replacement for release-plz.

## One manual coordinator

The stable coordinator owns `.github/workflows/binary-release.yml` and starts only
by `workflow_dispatch`. Its closed operation is `prepare` or `release`. When release-PR preparation
is enabled, the default is `prepare`; otherwise the caller must explicitly select
`release` and preparation is unavailable. There is no automatic
stable publication on main push, CI completion, preview publication, a schedule,
or an arbitrary user-supplied tag/version input. Existing unrelated image and
package publication trigger families are unchanged.

`prepare` invokes the pinned release-plz `release-pr` command with the effective
native config. It may propose the selected package's version/changelog change,
but creates no stable tag, binary publication or Homebrew promotion. Its normal
PR gates and complete feedback disposition still apply.

`release` authenticates the actual manual default-branch workflow authority and
source, the selected Cargo package version, and successful full CI/Required for
that exact source. The protected stable environment must enforce a required
human reviewer, prevented self-review and protected-branch restrictions before
permanent tag authority is used. Configuration or a dispatch actor is not proof
that those protections exist. Missing authority fails before any tag mutation.

Release-plz owns version/changelog PR preparation, not permanent tag mutation.
Its native effective preparation config sets `git_only = true`, `publish = false`,
`git_tag_enable = false` and `git_release_enable = false`, selecting only the
version package. It cannot create an empty GitHub release. The qualified
metadata phase disables repository hooks and Git signing commands; no application
task or build receives its PR-writing token.

The release tag job is entirely source-free. It resolves authenticated source
version data, independently confirms the exact `v<P>` tag is absent, and retains
a native intent artifact bound to repository, version package/version, source/full
CI, static inventory policy and original protected manual workflow/run attempt.
Before upload, the native writer creates a detached annotated tag object with
no permanent ref. The annotation binds the canonical authorization hash; the
intent retains that exact object SHA, target and message. Permanent ref creation
starts only after the immutable intent artifact ID and ZIP digest are available.

The native existing GitHub tag/ref API creates only that exact authorized tag and
source; it never retargets or deletes a permanent tag. Its origin record binds the
actual object type/object/peeled target to the retained intent identity. The tag
annotation binds the original authorization, not an artifact digest or future
receipt. The ref must point to the exact detached object retained in the intent;
a copied annotation at another object SHA is not equivalent. Retain the
closed origin evidence before building. Neither record claims who created a tag
merely because its name matches; the prior intent authorizes exactly one target.

A tag existing before initial intent admission is a conflict. After interruption,
rerun all jobs of the original workflow at the same source with a later attempt. Authenticate
the original pre-tag intent and exact authorized tag/annotation, then record a new
completion context for that same run and source. Reuse the original intent
artifact instead of inventing new authorization. A failed-jobs-only rerun is not
a recovery path: it can reuse an earlier origin while new producer attempts need
a new completion context. Missing intent or a different
target fails closed. A new-main dispatch cannot rebuild that old transaction;
without retained signed evidence it directs the operator to rerun the original.

The same coordinator then builds the exact tagged commit, not whatever main
points to later. It uses the shared product/bin/target and companion inventory,
read-only qualified leaf builds, independent complete-union assembly, isolated
signer and native immutable publisher. Do not rely on a second workflow firing
when a token creates the tag. Source and workflow authority remain separately
bound even though they identify the same approved manual release transaction.
A later main advance cannot rewrite the tag or turn stable into rolling.

The fixed release-plz preparation phase is a qualified version-management tool
boundary. It is not an application producer, arbitrary command or an exception
allowing credentials into repository tasks/builds. Native tag, signer and
publisher jobs stay source-free. No repository-supplied release-plz config or
pre/post-release hook is execution authority.

Version-PR updates do not require a privileged regeneration hook. Generated
workflows carry static package/manifest ownership and inventory policy, not a
Cargo base-version literal. Native admission resolves the version as data from
that exact source, and signer/publisher independently verify the same binding.
Changes to paths, inventory or version ownership still require regeneration.

## Permanent publication and recovery

Repository immutable releases stay enabled. The native publisher keeps the
existing independent Administration-read immutability preflight, exact source
CI/tag-origin checks, protected human-approval environment and no-bypass policy.
No preview token, mutable release or rolling journal can satisfy stable admission.

After the exact tagged source is built, the isolated signer verifies the full
shared inventory and signs the canonical stable `PackageReleaseReceipt`, all
required archives and companions. Native publication creates a matching draft,
uploads the exact closed signed inventory, verifies it, then publishes once and
requires the API to report the final release as immutable. Permanent `v<P>` tags
and assets are never moved, replaced or deleted. An existing release can only be
reconciled as the same exact authenticated original publication, never adopted
because its version string matches.

Recovery reauthenticates the original pre-tag intent and tag-origin record,
original run attempts and, once signing occurred, retained signed artifact and
receipt/signatures. The exact authorized tag-only transaction may finish origin
admission before the first build; signed recovery never rebuilds. Matching draft
states can finish missing work; an already published exact immutable release can
continue Homebrew promotion. Unknown origin, altered asset IDs/digests, different
receipt bytes or a conflicting tag fails closed. Recovery does not rebuild or
relabel the original producer/signer identities. The existing bounded publication
and cleanup clocks are retained.

## Stable Homebrew and qualification

The signed stable channel selects only
`distribution.homebrew_tap.destinations.stable.formula`. It uses the same captured
base, credential-free updater, independent literal-byte validation, isolated W/O
and exact-head C/R/check/feedback merge boundaries as preview. Stable URLs name
the permanent `v<P>` release; a preview receipt or mutable URL cannot substitute.
The stable formula must already implement its intended prebuilt installation
behavior before unattended metadata updates are enabled. Enabling a Cask or
changing executable Ruby is a separately reviewed source change.

Acceptance includes manual-only triggers, default no-registry publication,
native exact-source tag origin before the first build, exact-tag builds after main
advances, complete shared inventory and signatures, empty-release prevention,
protected human approval, every interruption/retry state, both-channel formula
separation and actual prebuilt installation. See the
[channel acceptance matrix](native-release-channel-acceptance.md). A local test
or a generated workflow is not evidence of a real stable release.

Release-plz's documented `git_only`, tag selection, optional release-PR and
`git_release_enable = false` behavior is the intended existing mechanism:
[configuration](https://release-plz.dev/docs/config),
[single tag](https://release-plz.dev/docs/extra/single-tag), and
[release output](https://release-plz.dev/docs/usage/release).
