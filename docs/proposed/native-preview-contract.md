# Native release channels and Homebrew contract

**Status:** immutable per-source preview and manual-stable channels. This is the
implementation contract, not evidence that either channel has passed runtime
qualification.
The existing producer, receipt, signature, publication and Homebrew boundaries
remain the foundation. No parallel release system or compatibility parser exists.

## Shared configuration and public types

`RustBinaryReleaseConfig` owns `manifest_path`, `version_package`, `version_env`,
`producer`, `publication_timeout_minutes`, `products`, `companions` and `evidence` once. `version_package` selects
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
publication_timeout_minutes = 15

[stacks.rust.binary_release.preview]
enabled = true
versioning = "package-preview-commit-count-sha7"
environment = "preview-publish"

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

The example producer closure is abbreviated;
real configuration must declare the complete tracked task, tool and system-tool
closure, including its observed Xcode tuple when applicable.

Public channel types are `RustBinaryPreviewConfig` and `RustBinaryStableConfig`.
Shared producer, product, archive, companion and evidence types are named
`RustBinaryReleaseProducer`, `RustBinaryReleaseProduct`, `RustBinaryReleaseArchive`,
`RustBinaryReleaseCompanion`, `RustBinaryReleaseEvidence` and
`RustBinaryReleaseEvidenceProfile`. Old preview-only aliases are removed.

`PackageReleaseChannel`, `PackageReleasePlan` and `PackageReleaseReceipt` remain
shared typed owners. Preview means one immutable release per source commit;
stable means permanent version publication. Channel selection is a closed enum, not an arbitrary tag,
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
through five minutes. `publication_timeout_minutes` defaults to five and accepts
whole minutes from one through 333, without clamping. It remains the native
publication deadline and continues to bound preview source eligibility. Preview
eligibility uses this value for both automatic publication and manual recovery
because its two admission phases can independently acquire and verify the
original signed artifact. Stable eligibility, signer and other acquisition/control
jobs retain their existing five-minute defaults.

Publisher and original-evidence recovery job limits add the caps on their actual
pre-operation steps, the configured native deadline, and one minute for completion
and cleanup. An uncapped preparation step receives the normal five-minute step
default, while the existing six-minute `Acquire Velnor` cap is preserved. With
the current steps, a 15-minute operation receives a 42-minute publisher job and
a 32-minute recovery job; their defaults are 32 and 22 minutes. The maximum
333-minute operation fits the publisher's 360-minute job limit with the current
preparation caps. Rendering recomputes the total from the actual steps and rejects
a combined budget above 360 minutes if those steps or caps change.

The native operation still uses one absolute deadline, including retained
signed-directory custody and final readback. It never refreshes per asset.
Initial admission retains one five-minute aggregate scope; nested archive scopes
cannot refresh it. Each HTTP request remains bounded to 90 seconds. A shorter
enclosing deadline still wins. The setting does not change producer budgets,
credentials, validation, or publication authority. The example explicitly selects
15 minutes; it does not change the default. General crate-job policy is
independent and is not changed here.

## Source and version authority

Preview admission keeps source and workflow authority separate. Let A be the
full source SHA selected from successful push CI, B the full SHA of the active
preview workflow definition, and H the current protected `main` tip. Authenticate
the ancestry relation `A <= B <= H`, along with the repository, branch, workflow,
run and attempt identities. A does not have to equal B or H. The latest
successful CI attempt for exact source A, its `Required` job, every source-
declared execution obligation and all required job results remain mandatory,
including during recovery. A newer CI attempt for A must be checked; historical
success cannot stand in for the latest exact-A attempt. Every remote mutation
rechecks the current ancestry and the relevant current run/attempt authority.

Source selection is two-phase. Before checkout, a credential-free admission
step authenticates the CI source A and workflow authority B from GitHub facts;
it performs no source execution. A is then checked out with complete history in
a credential-free context. From A, native code reads canonical configuration,
projects the package/product inventory with the bounded pure manifest
projection, and discovers the declared task and its exact input closure with
`discover_preview_producer`. Explicit qualified-tool declarations and producer
policy come from A. B supplies workflow custody and trusted bootstrap tools;
source-controlled tools are only admitted through A's declared qualification.
The resulting canonical admission record binds A's policy, plan, task closure,
tool declarations and execution tuple before any declared task runs.

For manual recovery, both source-selection and finalization operations receive
the same original producer run ID and attempt from the closed native operation
bindings. Each phase independently authenticates that retained signed candidate.
The original producer identity is distinct from the source CI run and the
current recovery caller; neither can substitute for it after checkout.

The producer executes the admitted A task, while the active run, workflow,
signer certificate, run attempt and job authority are bound to B. The signed
release receipt records source A and workflow authority B separately. The
version and immutable tag remain A-derived: `<base>-preview.<full-history-count>+
<A-sha7>` and `preview-<full-A-sha>`. A later protected tip H may be newer than
A; it does not revoke an otherwise admitted ancestor release. Homebrew promotion
has a narrower rule: it proceeds only if A is still the current protected
`main` tip at the tap mutation boundary. Otherwise the result is a typed clean
skip, leaving the immutable release intact.

Workflow identity is admitted active once per SDK invocation, with exact workflow
ID, canonical path and display name. GitHub documents that
[disabling a workflow](https://docs.github.com/en/actions/how-tos/manage-workflow-runs/disable-and-enable-workflows)
prevents new triggers and reruns but does not cancel existing runs. Disabling is
therefore not a cancellation authority for an already admitted producing run.
Its current attempt and required running/completed state remain fresh checks.
Successful completed CI-attempt evidence, including the exact-A `Required`
job, may be reused within that invocation under its full repository, source,
branch, workflow, run and attempt identity. Failed or incomplete evidence is
never cached as success. Every latest exact-A CI admission still reads the
latest matching source-CI listing and current run attempt, status and
conclusion; this cache never substitutes historical success for a newer or
unsuccessful current attempt.
Artifact expiry after immutable bytes are admitted cannot change those held
bytes, but may prevent a future recovery invocation from reacquiring them. The
terminal publication verifier reauthenticates the held original closure and
retained signed bytes, including their signature and exact source, workflow run
and attempt identity. It then revalidates the published tag and complete asset
inventory against that authenticated candidate. It does not refresh the original
workflow artifact pointer; a new recovery invocation must independently reacquire
and authenticate the original artifact before it can resume publication.

For numeric base version `P`, complete-history count `N` and source SHA `S`, the
preview version remains `P-preview.N+S[0..7]`. Its immutable tag is
`preview-<full lowercase S>`. Each source SHA owns a separate prerelease; reruns
retain source/count identity and must not silently substitute different signed
bytes. Neither an existing preview nor its assets are replaced or republished.

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

## Immutable per-source preview release

Repository-wide immutable releases stay enabled. Every preview tag names its
full source SHA and is created once with an authenticated recovery envelope that
binds the source SHA, canonical receipt digest and complete release inventory.
The publisher creates the tag without force, then reobserves and authenticates
it before creating the prerelease draft. An existing tag without the exact
recovery envelope, a tag for another source or an existing release with a
different receipt is a conflict. Never delete, recreate, redraft, move or
republish a preview tag or release.

An unfinished draft may resume only from the original retained signed artifact
and exact authenticated receipt. Upload only missing assets whose expected
names, sizes and digests match the receipt. Publish once after complete asset
verification. A published preview must be immutable and match the exact receipt
and complete asset set; otherwise fail closed. A newer protected tip H does
not revoke an unfinished source A when authenticated `A <= B <= H`, exact-A CI
remains successful, and original evidence is reacquired. It never authorizes
changing that candidate's tag or bytes.

## Complete signed generation and publication transaction

Producer and assembly still emit one complete, bounded typed inventory. Each
product/target is one entry in a bounded source matrix, followed by complete
API-union assembly. No unselected product, undeclared companion, arbitrary
path or proof-disable option is admitted. The six Jackin executable archives,
archive checksums, SBOMs, capsule manifest, detached bundles, aggregate checksum
list and canonical receipt remain mandatory where declared by the shared plan.
Desktop companions retain their existing task and signing/notarization custody;
a new Cask or executable formula change is not implied by this contract.

Canonical `velnor.package-release.v2` receipt bytes bind channel, repository slug
and numeric ID, source A/ref/event and original CI attempt, workflow authority B and
original producer/signer run attempt, version, tag, complete products and members,
companion inventory, asset sizes/digests, evidence profile and signer identity.
Unknown/duplicate fields and noncanonical JSON fail. Preview uses `workflow_run`
and `refs/heads/main`; stable records its manual coordinator and exact stable
source/tag provenance. Stable has no preview commit-count identity. Both channels
use their complete declared archives and companions; stable is no longer the
old one-bin Apple-only raw payload path. Receipt and detached bundle are excluded
from receipt self-digests; signatures authenticate those exact receipt bytes.

Every required archive and companion, and the receipt, has its required detached
signature and GitHub SLSA attestation with exact source A, workflow authority B,
run/attempt, issuer and hosted-runner identity. SBOM entries come from actual compiled units
and bind their archive bytes; workspace presence is not compilation evidence.
No field claiming successful verification replaces actual cryptographic checks.

Preview publication reuses the original authenticated retained signed artifact.
A native bounded journal binds the immutable source tag, original source/run/receipt,
complete logical inventory, uploaded asset IDs and exact sizes/digests. Remote
names come from the trusted receipt, never arbitrary producer URLs. Upload and
verify the complete asset set before publishing the draft. Only then may native
Homebrew promotion consume that immutable source generation, and only while A
is still current protected `main`; otherwise promotion is a typed clean skip.

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
A stale journal cannot overwrite a release, roll back a tag, or cause an old
candidate to replace newer formula metadata. Unknown state fails closed for
inspected reconciliation; elapsed time grants no authority.
Stable recovery preserves its permanent tag, matching draft and original signed
candidate, uploads only missing exact assets, and never overwrites a publication.

Preview releases are immutable after publication, so readers verify one signed
receipt, its complete closed inventory and digests, and the source-specific tag
before acquisition. A partial draft is not a published release and cannot be
accepted as a complete generation.

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

Main protection is the union of the effective `rules/branches/main` read and the
classic `branches/main/protection` read. A branch protected only by rulesets has
no classic source: with Administration read, GitHub answers the classic route
with `404 {"message":"Branch not protected"}` (the generic `404 Not Found` is
treated the same). Only those exact 404 answers mean "no classic source", and the
rules read must then supply every required control on its own. Every other
status or message, including 401 and 403, fails closed with
`package_publish_settings_read_failed`. The `preview-publish` and `tap-update`
environments must have no protection rule other than the single `branch_policy`
rule GitHub reports for their custom deployment branch policy.

## Native Homebrew destinations and continuation

The tap supports two exclusive formula-preparation routes. The native route
uses `distribution.homebrew_formula` and no updater; it derives the formula
from the typed specification and requires the committed source-release policy
during generation. The custom route keeps a source-qualified updater and no
native formula specification; it remains available to existing adopters and
does not require that native source-policy file. Both routes keep one tap
repository and the shared W/O/C/R policy. Configuring both routes, or neither
on an enabled tap, is rejected.

`HomebrewTapConfig` removes the old singleton `source` and `formula` fields and
the `HomebrewTapSource` enum. `HomebrewTapDestinations` has only optional
`preview` and `stable` entries, each a `HomebrewTapDestination { formula }`. At
least one is configured when enabled; each references an enabled channel.
Formula paths must be distinct and safe. On the custom route, each destination
must also be present in the captured updater input closure. There is no fallback
to a preview formula for stable receipts.

```toml
[distribution.homebrew_tap]
enabled = true
repository = "jackin-project/homebrew-tap"
environment = "tap-update"
app_id_variable = "HOMEBREW_TAP_APP_ID"
app_private_key_secret = "HOMEBREW_TAP_APP_PRIVATE_KEY"
merge = "squash-after-gates"
# This example selects the native route; omit `updater` and use the typed spec below.

[distribution.homebrew_tap.destinations.preview]
formula = "Formula/jackin-preview.rb"

[distribution.homebrew_tap.destinations.stable]
formula = "Formula/jackin.rb"
```

The custom route instead retains its qualified `updater` binding, including
task source, runner, bounded timeout, tool list, and complete input closure,
and omits `[distribution.homebrew_formula]`. Its destination formula must be
inside that closure. This keeps older source-qualified updater configurations
valid while preventing an ambiguous native/custom combination.

The signed receipt's channel alone selects the destination and channel-distinct
deterministic PR branch. Capture tap main and the selected formula, run the
qualified updater on the custom route, and independently validate exact release
literals and monotonicity without evaluating Ruby. On the native route, derive
the canonical metadata-only formula from the typed specification. Both routes
preserve every other byte and path, mode, alias, dependency, install/test
statement, and executable expression. A stable formula that needs new
executable installation logic requires an explicitly reviewed ordinary source
change before unattended metadata promotion is enabled. No optional Cask is
smuggled into a one-formula patch.

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
