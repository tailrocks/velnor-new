# Native release channels and Homebrew contract

**Status:** the human owner's 2026-10-20 rolling-preview and manual-stable
requirements replace immutable preview publication. This is the implementation
contract, not evidence that either new channel has passed runtime qualification.
The existing producer, receipt, signature, publication and Homebrew boundaries
remain the foundation. No parallel release system or compatibility parser exists.

## Shared configuration and public types

`RustBinaryReleaseConfig` owns `manifest_path`, `version_package`, `version_env`,
`producer`, `products`, `companions` and `evidence` once. `version_package` selects
one discovered Cargo package among the declared products as the base-version
authority. Both channels consume
the same complete product/target/bin and companion inventory. There is no old
root `enabled`, `package` or `bin` fallback and no preview-owned inventory copy.
Unknown fields are rejected. All channel combinations default to disabled.

```toml
[stacks.rust.binary_release]
manifest_path = "Cargo.toml"
version_package = "jackin"
version_env = "JACKIN_VERSION_OVERRIDE"

[stacks.rust.binary_release.preview]
enabled = true
versioning = "package-preview-commit-count-sha7"
environment = "preview-publish"
release_id = 328385904

[stacks.rust.binary_release.stable]
enabled = true
environment = "release"

[stacks.rust.binary_release.producer]
task = "release:package"
source = { mise_config = "mise.toml", working_directory = "." }
runner = "xcode-27-arm64"
inputs = ["Cargo.lock", "Cargo.toml", "mise.toml", "scripts/release-package.sh"]
tools = ["rust", "cargo-zigbuild", "zig"]
timeout_minutes = 20
build_jobs = 3
output_directory = "dist/release"
manifest = "dist/release/package-output.json"

[stacks.rust.binary_release.evidence]
profile = "signed-sbom-v1"

[[stacks.rust.binary_release.products]]
id = "jackin"
package = "jackin"
bins = ["jackin", "jackin-role", "jackin-usage-broker"]
targets = ["aarch64-apple-darwin", "aarch64-unknown-linux-gnu", "x86_64-apple-darwin", "x86_64-unknown-linux-gnu"]
archive = "tar.gz"

[[stacks.rust.binary_release.products]]
id = "jackin-capsule"
package = "jackin-capsule"
bins = ["jackin-capsule"]
targets = ["aarch64-unknown-linux-gnu", "x86_64-unknown-linux-gnu"]
archive = "tar.gz"

[[stacks.rust.binary_release.companions]]
id = "capsule-manifest"
path = "capsule-manifest.json"
asset_name = "capsule-manifest.json"
```

The example release ID is Jackin's observed existing mutable preview release,
not a portable default. Every enabled rolling channel must explicitly bind its
own qualified existing release. The example producer closure is abbreviated;
real configuration must declare the complete tracked task, tool and system-tool
closure, including its observed Xcode tuple when applicable.

Public channel types are `RustBinaryPreviewConfig` and `RustBinaryStableConfig`.
Shared producer, product, archive, companion and evidence types are named
`RustBinaryReleaseProducer`, `RustBinaryReleaseProduct`, `RustBinaryReleaseArchive`,
`RustBinaryReleaseCompanion`, `RustBinaryReleaseEvidence` and
`RustBinaryReleaseEvidenceProfile`. Old preview-only aliases are removed.

`PackageReleaseChannel`, `PackageReleasePlan` and `PackageReleaseReceipt` remain
shared typed owners. Preview means rolling publication; stable means permanent
version publication. Channel selection is a closed enum, not an arbitrary tag,
URL, event, permission, command or evidence override. The trusted task request
carries the selected channel and literal version; a producer cannot choose them.
A consumer may choose its declared preview Cargo profile from that typed request.
Stable continues to use the consumer's declared stable release profile.

Producer native-operation budgets accept one through thirty minutes, default five,
and retain one original operation clock including cleanup. Each payload and assembly
job adds a fixed five-minute lifecycle allowance outside that operation budget:
one, five and thirty-minute operations therefore receive six, ten and thirty-five
minute job limits. This bounded allowance covers setup, evidence retention and
job cleanup; it is not a guarantee that every setup or cold build fits. Updater budgets remain one
through five minutes. Signer, publisher, acquisition and control jobs remain
five minutes. General crate-job policy is independent and is not changed here.

## Source and version authority

Preview is automatically selected only by completion of the exact source's
full default-branch push CI. The actual source SHA, workflow authority SHA,
repository, default branch and original CI run attempt must agree. The latest
CI attempt, `Required`, every source-declared execution obligation and all
required job results must be successful before publication. Recheck current
main and these facts before every mutation and final commit. A newer main makes
an unfinished older preview ineligible; retained evidence is not new authority.

Workflow identity is admitted active once per SDK invocation, with exact workflow
ID, canonical path and display name. GitHub documents that
[disabling a workflow](https://docs.github.com/en/actions/how-tos/manage-workflow-runs/disable-and-enable-workflows)
prevents new triggers and reruns but does not cancel existing runs. Disabling is
therefore not a cancellation authority for an already admitted producing run.
Its current attempt and required running/completed state remain fresh checks.
Successful completed CI-attempt evidence, including the exact-source `Required`
job, may be reused within that invocation under its full repository, source,
branch, workflow, run and attempt identity. Failed or incomplete evidence is
never cached as success. Every latest-CI admission still reads the latest source
CI listing and current run attempt, status and conclusion; this cache never
substitutes historical success for a newer or unsuccessful current attempt.
Artifact expiry after immutable bytes are admitted cannot change those held
bytes, but may prevent a future recovery invocation from reacquiring them. The
terminal publication verifier deliberately refreshes the retained artifact pointer
rather than treating invocation-local admission as permanent availability.

For numeric base version `P`, complete-history count `N` and source SHA `S`, the
preview version is `P-preview.N+S[0..7]`. Its only public tag is literal `preview`.
There are no versioned preview tags or preview releases per commit. Reruns retain
source/count identity and must not silently substitute different signed bytes.

Stable starts only through the manual release-plz flow in
[binary-release-contract.md](binary-release-contract.md). Release-plz prepares
versions and creates the single permanent `v<P>` tag before the binary build.
The native coordinator builds that exact tagged commit; a later main advance
does not retarget or invalidate an otherwise authorized permanent stable source.
The source remains reachable from the protected default branch and has its own
successful exact-source full CI. No preview candidate or environment approval
can authorize stable publication.

Existing image and package publication triggers, attested main-digest promotion,
private-source fetch custody and permanent release-asset bundles retain their
contracts. This preview exception does not widen those mechanisms.

## Qualified existing rolling release

Repository-wide immutable releases stay enabled. GitHub documents that releases
published before enabling immutability remain mutable unless republished.
Rolling admission requires the configured positive release ID, same repository,
exact tag `preview`, `draft=false`, `prerelease=true` and `immutable=false`.
An absent or immutable target is a prerequisite failure, not permission to create
another release or toggle a repository/organization setting. Never delete,
recreate, redraft or republish the rolling release.

Resolve the actual Git ref rather than trusting release `target_commitish`,
which can remain stale for an existing tag. Preview advancement uses only a
normal `force=false` fast-forward update after proving the previous committed
source is an ancestor. Never delete or force-move a tag. Keep existing tag
non-fast-forward/deletion protection; do not add a bypass or exception unless a
separately reviewed actual provider restriction proves one necessary.

This one-time existing-mutable-release prerequisite is real. GitHub has no
per-new-release opt-out demonstrated under globally enabled immutability.
A fresh repository without such a release cannot claim rolling readiness from
configuration or mocks alone.

## Complete signed generation and publication transaction

Producer and assembly still emit one complete, bounded typed inventory. Each
product/target has one leaf with its human product/target name, followed by
complete-union assembly. No unselected product, undeclared companion, arbitrary
path or proof-disable option is admitted. The six Jackin executable archives,
archive checksums, SBOMs, capsule manifest, detached bundles, aggregate checksum
list and canonical receipt remain mandatory where declared by the shared plan.
Desktop companions retain their existing task and signing/notarization custody;
a new Cask or executable formula change is not implied by this contract.

Canonical `velnor.package-release.v2` receipt bytes bind channel, repository slug
and numeric ID, source/ref/event and original CI attempt, workflow authority and
original producer/signer run attempt, version, tag, complete products and members,
companion inventory, asset sizes/digests, evidence profile and signer identity.
Unknown/duplicate fields and noncanonical JSON fail. Preview uses `workflow_run`
and `refs/heads/main`; stable records its manual coordinator and exact stable
source/tag provenance. Stable has no preview commit-count identity. Both channels
use their complete declared archives and companions; stable is no longer the
old one-bin Apple-only raw payload path. Receipt and detached bundle are excluded
from receipt self-digests; signatures authenticate those exact receipt bytes.

Every required archive and companion, and the receipt, has its required detached
signature and GitHub SLSA attestation with exact source, workflow, run/attempt,
issuer and hosted-runner identity. SBOM entries come from actual compiled units
and bind their archive bytes; workspace presence is not compilation evidence.
No field claiming successful verification replaces actual cryptographic checks.

Rolling publication reuses the original authenticated retained signed artifact.
A native bounded journal binds the fixed release ID, original source/run/receipt,
complete logical inventory, generation-specific remote names, uploaded asset IDs
and exact sizes/digests. Staging names are derived from trusted generation
identity, never accepted from arbitrary producer URLs. Upload and verify all new
generation bytes before treating them as committed. Commit the authenticated
generation receipt/pointer last, with the source tag and complete inventory
admitted again. Only then may native Homebrew promotion consume the generation.

The executor proposes transitions; it does not grant write authority. The
backend consumes each private observation once, including on failure, and checks
mutable candidate authority immediately before every actual write. Terminal
verification reads all actual asset bytes from the same configured numeric
release ID rather than enumerating historical releases. One absolute deadline
covers initial cryptographic admission through terminal byte verification.

After interruption or an ambiguous API response, reobserve asset IDs and
size/digest/state before acting. Resume only the same authenticated original
signed candidate. Delete only exact journal-owned obsolete/staging asset IDs
whose identity still matches; never adopt or clean arbitrary unknown assets.
A stale journal cannot overwrite a newer committed generation, roll back its
tag, or cause an old candidate to replace newer formula metadata. Unknown state
fails closed for inspected reconciliation; elapsed time grants no authority.
Stable recovery preserves its permanent tag, matching draft and original signed
candidate, uploads only missing exact assets, and never overwrites a publication.

GitHub's public asset API has no atomic multi-asset swap. The existing live
release can temporarily list old and staged assets or have replacement gaps.
This contract promises no mixed-generation *acceptance*, not an instantaneous
physical listing swap or uninterrupted historical rolling URLs. Readers verify
one committed signed generation, its complete closed inventory and digests, and
reread generation/tag before and after acquisition. A change or partial state
rejects; it is not a reason to accept whichever subset downloaded successfully.

## Credential and execution boundaries

Reuse source-free authenticated artifact acquisition before repository code.
Its process exits and emits a digest bound to the exact request and immutable
input closure. The credential-free producer/updater rejects ambient credentials,
reopens and pins all admitted inputs, and only then loads source or runs its
qualified named Mise task. No later authenticated step runs in that source job.

Build jobs have no OIDC, release-write or tap credentials. Independent signers
have attestation/OIDC authority but no repository task or build execution.
Publisher and recovery jobs consume authenticated signed bytes without producer,
updater or application execution. The native stable tag phase is the separate
source-free exact-SHA authority described by the stable contract. Release-plz
only prepares version PRs; neither phase permits arbitrary repository tasks
with a writer token.

Tap updater runs without write credentials. W is the target-scoped formula PR
writer; O independently reads protection; C emits exact-head completion; R is
the genuinely authenticated reviewer. Their numeric identities and permissions
remain distinct. The source-only `RELEASE_STATUS_READ_TOKEN` role explicitly
requires Administration-read, Actions-read and Metadata-read in the source
repository, without Contents or write grants. Its native GET allowlist contains
only that repository's identity, main protection/rules, global immutability and
the three fixed release/preview-publish/tap-update environment settings and branch
policies. It never reads artifacts, workflow runs or repository contents and
never substitutes a writer or Tap identity. This refined read role requires
external permission qualification; an old Administration-only token is not
assumed sufficient. GitHub’s [branch endpoint permissions](https://docs.github.com/en/rest/branches/branches?apiVersion=2026-03-10#get-a-branch)
require Contents-read for private resources, with an explicit public-resource
exception. The no-Contents reader must therefore qualify its branch reads on
the actual public source repository; private access is not inferred and does
not trigger automatic permission expansion or credential fallback. No writer
bypass, admin write, copied credential or producer-cache authority is introduced
by channel support.

## Native Homebrew destinations and continuation

`HomebrewTapConfig` retains one repository, updater and shared W/O/C/R policy.
It removes the old singleton `source` and `formula` fields and the
`HomebrewTapSource` enum. `HomebrewTapDestinations` has only optional `preview`
and `stable` entries, each a `HomebrewTapDestination { formula }`. At least one
is configured when enabled; each references an enabled native channel. Formula
paths must be distinct, safe regular files and present in the captured updater
input closure. There is no fallback to a preview formula for stable receipts.

```toml
[distribution.homebrew_tap]
enabled = true
repository = "jackin-project/homebrew-tap"
environment = "tap-update"
app_id_variable = "HOMEBREW_TAP_APP_ID"
app_private_key_secret = "HOMEBREW_TAP_APP_PRIVATE_KEY"
merge = "squash-after-gates"
# Existing qualified updater and distinct O/C/R bindings remain mandatory.

[distribution.homebrew_tap.destinations.preview]
formula = "Formula/jackin-preview.rb"

[distribution.homebrew_tap.destinations.stable]
formula = "Formula/jackin.rb"
```

The signed receipt's channel alone selects the destination and channel-distinct
deterministic PR branch. Capture tap main and the selected formula, run the
qualified updater, and independently derive exact canonical metadata-only bytes
without evaluating Ruby. Every other byte and path, mode, alias, dependency,
install/test statement and executable expression remains unchanged. A stable
formula that needs new executable installation logic requires an explicitly
reviewed ordinary source change before unattended metadata promotion is enabled.
No optional Cask is smuggled into a one-formula patch.

Both channels retain exact-head Required/DCO, complete feedback dispositions,
independent C/R completion, source/receipt reauthentication and server-enforced
strict PR-only merge protection. Preview admission rereads the latest committed
generation immediately before the head-matched merge and reobserves it afterwards.
GitHub cannot atomically predicate a tap-repository merge on a source-repository
generation; a concurrent newer commit is a detectable supersession requiring
reconciliation, not a claim that the server enforced cross-repository latestness.
Stable compares permanent version/source and never downgrades the stable formula.

Retained native continuation independently selects verified committed candidates
for each enabled channel, then performs the existing four isolated selection,
preparation, writer and one-shot merge roles. It never rebuilds, signs, publishes,
recovers publication, dispatches another workflow or borrows a current-run artifact.
Its queue is distinct from publication so periodic pending runs cannot discard a
pending newer source release. Pending checks/review stay pending for a later
bounded invocation; no long polling, asynchronous replacement-head merge or
provider-completion waiver is introduced.

Exact-head evidence and protection details remain in
[native-homebrew-merge-contract.md](native-homebrew-merge-contract.md).
The [channel acceptance matrix](native-release-channel-acceptance.md) defines
observable proof before release and consumer activation.
