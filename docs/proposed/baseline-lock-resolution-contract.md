# Baseline lock resolution

Status: approved source contract; activation requires native and SDK qualification.

## Purpose

`BaselineLockResolution` supplies a governing lock for an authenticated published
crate whose original archive has no governing lock. It is a separate resolution
purpose from the shipping graph. Current source and shipping metadata remain
locked.

This adopts an explicit locked semver policy. Reusing an archived lock or creating
a new baseline lock does not establish equality with cargo-semver-checks 0.50.0's
fresh placeholder resolution. Qualification must state that distinction.

## Admission

1. Authenticate the complete registry index response and checksum-bound archive.
   Select the baseline through native Cargo dependency and exact-query semantics;
   callers cannot choose a different published baseline by claiming a version.
2. Preserve the original archive and extracted source. Bind all file bytes,
   full ordinary permissions and directory paths, including empty directories and
   `.git` payloads. Resolution cannot rewrite the original tree.
3. Discover the workspace manifest and governing lock through native Cargo before
   any metadata or resolver launch. The absence must be an observed filesystem
   fact, not a caller boolean.
4. Admit this role only when that original governing lock is absent. An existing
   stale, invalid, unreadable or unsatisfied lock fails; this role cannot rescue it.
5. Require genuine source-qualified SDK tools and the native resolver capability.
   Paths, banners, digests, dictionaries and serialized receipts alone grant no
   execution authority.

Before native workspace or root resolution, validate the archive's normalized
manifest against its authenticated index identity through the native source
parser. Reject foreign registries, Git sources and source or workspace inheritance
outside the authenticated tree. Index dependency fields alone cannot authorize
the archive manifest as a new workspace root.

The source producer uses the independent cold SDK algorithm. Its private tool
capabilities verify the installed Cargo, rustc and rustdoc against authenticated
installation genesis and the complete canonical manifest. They do not depend on
a cache capsule and cannot be serialized as a grant to the native wrapper. A
native consumer must independently verify the same owned installation or use a
separately qualified capability boundary. Actual host and SDK purpose remain
strict; Mac diagnostics do not qualify a Linux source SDK.

The native parent captures `CargoResolutionContext` in its own process. Its fixed
internal producer frame carries `governingLockContext` with exactly `format: 1`,
`workspaceManifest`, `governingLockfile` and `lockfileBytesBase64`. Paths are
canonical absolute paths within the authenticated source namespace. Lock bytes
are nonempty, at most 16 MiB, encoded with canonical padded RFC 4648 Base64; the
whole frame is at most 32 MiB. This frame is an observation, never a serialized
tool or execution grant. The SDK verifies exact governing bytes before its first
Cargo launch and after every launch, including failures. Metadata's workspace
must match the captured native workspace. The native parent also rechecks its
original context after the fixed producer returns. There is no any-lock fallback.

## Frozen registry universe

The resolver receives exact authenticated index responses and checksum-bound
archives needed for native candidate selection, optional and development
dependencies, target conditions, features and backtracking. A receipt for only the
baseline crate or only the selected graph is insufficient.

Use a frozen local registry rather than an ordinary remote registry's offline
cache. Native remote offline lookup can omit uncached candidates and choose an
older version. Local registry lookup must retain every index candidate. Every
unknown or missing index/archive request fails, including requests on abandoned
backtracking branches. A successful resolution alone cannot assert completeness.

Use a fresh private Cargo home and extracted source cache. Existing extracted
local-registry sources can bypass native archive checksum validation. Independently
verify authenticated archive bytes and freeze all registry bytes, modes and paths
before and after the operation. No ambient registry, cache or network fallback is
permitted.

## Native operation

Use the source-qualified Cargo 797e8a9 native `ops::generate_lockfile` semantics,
including native all-feature and development-dependency handling. Repository and
ambient Cargo configuration, hooks, wrappers and foreign sources cannot affect
this role. Native tool information queries require qualified tools too; no crate
compilation or build script execution belongs to resolution.

Materialize a private derived source tree. Its source bytes and paths must match
the authenticated original. Only the separately bound governing lock may be
added. No manifest normalization, synthetic workspace, Git initialization or
cache marker exemption is permitted.

Bind the resulting lock to the original archive, complete frozen registry inputs,
native source and tool identities, workspace/governing-lock paths and effective
resolver context. Seal this derived graph before locked metadata or docs run.
Recheck original source, archive, registry and tools after failures as well as
success. A failed or incomplete resolution produces no qualified baseline.

## Execution containment

Activation requires the fixed qualified Linux read-only and seccomp boundary,
with its own closed source purpose. It cannot reuse a Cargo verification purpose
to admit other operations. The native owner constructs exact commands; callers
cannot supply arbitrary commands or namespace policy.

Cargo docs execute build scripts and procedural macros. Their compiler, tools,
original sources and registry inputs must be mounted read-only. Only designated
target/docs outputs are writable. The namespace must contain background children
and be completely torn down before comparison, report generation or qualified
evidence leaves it. Per-spawn tool hashes cannot substitute for this containment.
Both successful and failed execution require teardown and integrity checks.

Read-only source mounts and teardown do not establish compiler-output integrity.
Crate code must not be able to replace rustdoc JSON, metadata or qualified reports
while compilation runs. Documentation rights remain unavailable until the actual
protected compiler-output model is qualified.

Lock resolution admits no crate compilation, source hooks or repository wrappers.
Any native tool information queries still require qualified tool capabilities.

## Native executor attachment

The private preparation authority owns an admitted-source registry. Only trusted
snapshot and acquisition owners may issue its entries: current source, native
history, authenticated baseline, separately sealed derived baseline, and native
edited scratch. Each entry binds manifest identities, complete source bytes,
modes, layout, purpose and lifecycle. Native requested paths only identify an
existing entry; they cannot register source or confer rights.

The authority issues an immutable invocation lease from existing entry and
selected-manifest identities, closed operation purpose, and the borrowed actual
same-process native governing context when required. The SDK binds that selected
lease; its zero-argument operand accessor cannot substitute another snapshot.
Snapshot owners invalidate edited entries and exclusively reseal actual changes.
There is no public path-registration, arbitrary callback, or serialized authority
constructor. Cross-runtime transport still requires the qualified private native
and namespace attachment.

Cargo identity preserves actual bounded verbose-version stdout bytes. Historical
manifest reads remain native lockless observations. Full metadata requires the
captured governing lock. Workspace version mutation accepts already edited native
source and changes only its existing governing lock; no manifest edit plan belongs
to this operation. Comparison reads preserve native archive order and bind shared
source paths against the entire admitted snapshot.

The mutation lease also retains a genuine original native manifest-read witness
from the read-only original snapshot and tool lease, captured before release-plz
edits. Native code compares the edited package summaries against that witness
before compiler queries or mutation. External dependency requirements, kinds,
features and targets must remain bound; lock nodes, checksums and adjacency alone
cannot prove those descriptors. Nonempty unused patches fail closed. Serialized
metadata or caller-supplied witness hashes cannot issue the original witness.

## Required negative evidence

- Existing stale or invalid lock cannot enter the absent-lock role.
- Full index containing dependency 1.0 and 1.1 with only the 1.0 archive cannot
  silently resolve to 1.0 because 1.1 is uncached.
- Missing index or archive on a backtracking branch cannot disappear into a
  successful older solution.
- An existing extracted cache cannot bypass archive verification.
- Modified archive, source, modes, directory layout, registry or tools fail.
- Foreign registry, Git/path escape, repository configuration, wrapper or hook
  cannot influence resolution.
- Locked metadata/doc generation cannot mutate the sealed derived lock.

Mocked helper tests establish local invariants only. Independent native behavior
and actual SDK source qualification are required before source-factory activation.
