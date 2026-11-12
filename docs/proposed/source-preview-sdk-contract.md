# Source preview SDK contract

This contract defines a separately approved SDK bootstrap channel for the Velnor
repository. The stable generator lock and consumer release manifests retain the
[bootstrap and release contract](bootstrap-and-release-contract.md).
The candidate cannot plan its own build, qualification, publication, or adoption.

## Publication authority

A protected `main` push builds one Linux x86-64 SDK and one Darwin arm64 SDK
with fixed pinned build commands. Qualification downloads each original build
artifact by ID and executes those exact bytes on the matching architecture
against the committed generation oracle. Qualification does not rebuild either
SDK or derive expected results from the candidate.

An annotated `sdk-preview-<full-source-sha>` tag authorizes publication. The tag
points directly to the qualified source commit, which remains reachable from
protected `main`. The tag workflow definition SHA equals the source SHA.
Reusable stable release calls retain their existing authority checks.

The source requires a successful push CI run with a successful Required verdict.
It also requires a successful source-qualification dispatch on the same commit.
Each proof identifies the workflow path, workflow reference, definition SHA, run
ID, and attempt. A successful workflow conclusion does not replace the checks
of its required jobs.

The fixed owner account creates SDK preview tags. Repository rules restrict tag
creation to that account and prohibit updates and deletion. Before creating a
tag, the owner verifies repository release immutability through GitHub's
administration API. The owner credential remains local.

The owner runs the tag helper from an independently reviewed, protected revision
and supplies that same revision as its verifier authority. The candidate must be
a later commit descended from the authority. The helper loads verifier modules
only from that authority; candidate files are data and never execute with the
owner credential. Its arguments are `create <source-sha> <pinned-gh-path>
<trusted-verifier-sha>`.

The canonical tag annotation binds the source, package version, and each target's
producer run, attempt, artifact ID, and archive digest. It also binds the
successful immutability precheck time.
The tag authority verifies the authenticated push actor and rerun actor, the
annotated tag object, both rulesets, and the complete annotation. The precheck
must precede tag creation by at most one minute and be no older than 15 minutes
at the original tag workflow run's GitHub-recorded creation time. This window
binds the owner precheck to the original tag event, including when execution is
queued. A retry retains that original timestamp; it cannot refresh the precheck
or move the tag. Repository administrators remain trusted to preserve immutable
releases throughout admission and publication; an old precheck does not prove
the current administration setting.

The tag workflow retrieves both successful main producer artifacts by ID.
Each archive must match its authenticated GitHub digest before bounded
extraction and contain exactly its target's regular SDK executable. ELF x86-64
and Mach-O arm64 validation reject swapped or unsupported payloads. The tag
workflow retains each original executable as an immutable artifact in the tag
run. It preserves the separate original and retained artifact identities.

Source-free jobs create a draft, attest the canonical receipt and both SDK executables together,
and publish through the protected `generator-source-preview` environment. It
requires the owner reviewer and admits only tags matching `sdk-preview-*` through
a tag-specific deployment policy. The stable `generator-release` environment
retains its separate protection. These jobs
perform no repository checkout or candidate execution. They verify all three signed
subjects, certificate and run identities, the tag, draft identity, and exact
asset hashes. The publisher rechecks source ancestry and the tag before
publication, then requires an immutable release with the complete asset set.

The source-free publication jobs run in a separately reviewed reusable workflow
referenced by a full commit SHA. The caller workflow identifies the candidate
source and tag; the reusable job context identifies the publication authority.
These identities are distinct. Adoption requires a signature from the protected
authority pin and independently checks the candidate source identity. Before
an independent authority has been merged and pinned, tag publication is disabled.
Protected main review, owner tag approval, and protected environment review remain
controls on other repository writes; the authority pin governs which signatures
can authorize adoption.

The workflow token cannot query the administration-only immutability endpoint.
The trusted owner precheck supplies that authority. Repository administrators
remain trusted to preserve the setting throughout publication. The workflow's
final immutable-result check does not independently prove the earlier setting.
[GitHub immutability API](https://docs.github.com/en/rest/repos/repos#check-if-immutable-releases-are-enabled-for-a-repository).

A retry can resume an owned draft only when its existing assets have the exact
expected names and hashes. Conflicting assets and published releases cannot be
overwritten or deleted. The channel does not create or move stable version tags.

The draft marker binds the original source and payload artifact IDs and the
annotated producer proof. The draft job searches the complete bounded release
listing before creating a release and verifies retained pairs with the pinned
GitHub CLI. The read-only attestation job does not query unpublished drafts.
A retry recovers those original artifacts
and an exact retained receipt and bundle together, and reuses a verified pair
without signing again. An empty draft without a successful signed pair may use
the current attestation attempt. A receipt already uploaded to a partial draft
pins the pair; missing or conflicting evidence fails closed. A published release
is accepted only after verification of its exact immutable assets and signature.


## Receipt and release assets

The prerelease tag is `sdk-preview-<full-source-sha>`. The SDK version is its actual
numeric package version. The version does not identify the publication by itself.
The source SHA, release ID, proof runs, and asset hashes identify the publication.

The release contains these assets:

- `SDK_PREVIEW_RECEIPT.json`
- `SDK_PREVIEW_RECEIPT.json.sigstore.jsonl`
- `velnor-actions-x86_64-unknown-linux-gnu`
- `velnor-actions-aarch64-apple-darwin`

The schema-3 receipt records the repository, package version, source SHA, tag,
release ID, source admission, draft preparation, signing, CI, qualification,
and retained source artifact identity. Its closed `targets` object contains
exactly `linux` and `darwin`. Each target binds its original producer run and
attempt, artifact ID, archive digest, retained candidate payload artifact ID,
qualification job ID, and payload target, name, size, and SHA-256 digest.
Original and retained artifacts remain distinct. Each target's qualification
job belongs to its authenticated producer run and attempt. The separate
source-qualification dispatch remains required. Missing, duplicate, extra, or
cross-assigned target identities fail closed.

Each workflow tuple contains its path, ref, source SHA, run ID, and exact attempt.
Source, draft, and signer share the tag run but may have different attempts,
ordered from source to signer. The receipt uses sorted JSON object keys, compact
separators, and no final newline. The Sigstore bundle binds the receipt and both
payload subjects. The immutable release contains exactly the four listed assets.

The lock records the complete receipt identity, the hashes of the receipt and
bundle, and the successful publisher's exact attempt and job ID. Publisher
identity is selected after publication and is excluded from the signed receipt.
Staging checks successful source, draft, and signer jobs at their exact attempts;
a failed aggregate tag attempt does not invalidate those successful roles.
It independently checks the pinned publisher job. CI and qualification still
require successful aggregate runs. A lock entry cannot supply a different verifier, trusted root, repository,
workflow, or executable hash policy.

## Staged approval and activation

The optional `[planner-preview-staged]` and `[planner-preview]` sections are
separate from the stable generator records in `.velnor/generator.lock`.
A staged entry does not select a preview SDK for ordinary CI.

Staging approval uses the verifier from an independently authenticated, already
merged protected commit. The verifier checks the immutable release, all three signed
subjects, exact run attempts, successful required jobs, and source ancestry.
Independent review binds that result to the staging pull request's current head
and base. The operator checks those identities again before merging.
Candidate checkout code and successful candidate-defined checks cannot supply
this approval.

Activation requires an exact match with the protected base's staged or active
entry. A candidate cannot introduce and approve a new controller in one change.
When the protected base has an active preview, candidate omission cannot force
a downgrade to the stable SDK. Unsupported targets and failed verification fail
closed.

## Runtime identity

`ControllerLocator::PinnedRelease` binds a stable version to its exact versioned
executable path. `ControllerLocator::ProtectedBaseDispatch` uses the fixed path
`$RUNNER_TEMP/velnor/bin/controller/binary`.
The dispatcher comes from protected workflow authority and selects the approved
controller before that controller parses candidate configuration.

The selected controller's actual version, source, and digest accompany planning
and task reports. Named checks, source qualification, and Required use the same
approved identity. Required checks each report against the identity used by Plan.
A neutral executable path alone does not establish this agreement.
A preview executable cannot be installed under a different stable version label.

State producer execution requires the generated producer execution descriptor
at the planned source commit. A preview controller rejects producer execution
when that descriptor is absent. Commit the descriptor before activating a
preview controller that requires it, and activate the same controller for Plan,
task execution, report writing, and Required.

Tests of parser rules, API policy, or historical signed fixtures do not establish
that a new preview was published successfully. Adoption requires a real signed
publication and successful verification of that publication before staging.

## Runtime SDK selection

Planner preview approval selects the CI controller only. It does not authorize
Mac qualification or native publication operations. Runtime selection requires
its own `[runtime-preview-staged]` entry, followed by activation of that exact
identity in `[runtime-preview]` from the protected base. Both sections live in
`.velnor/generator.lock`, separately from the planner sections and stable
generator records. A candidate cannot stage and approve itself.

Stable and preview runtime identities are distinct types. Stable selection
retains the stable release manifest and complete stable target inventory.
Preview selection binds the immutable preview release, source, independent
publication authority, receipt and bundle digests, complete paired target
inventory, and the selected host executable's exact digest. A receipt digest is
never interpreted as a stable release-manifest digest. The running executable
must match the selected authenticated target bytes.

Linux native signing and Darwin qualification consume their respective targets
from the same approved preview. SDK source, image source, and adoption commit
remain separate identities. A preview executable is not installed or described
as the stable SDK with the same numeric package version. Unsupported targets,
missing approval, or failed verification fail closed.
