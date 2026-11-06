# Source preview SDK contract

This contract defines a separately approved SDK bootstrap channel for the Velnor
repository. The stable generator lock and consumer release manifests retain the
[bootstrap and release contract](bootstrap-and-release-contract.md).
The candidate cannot plan its own build, qualification, publication, or adoption.

## Publication authority

A protected `main` push builds and qualifies one Linux x86-64 SDK with the fixed
pinned build command. Qualification downloads that build artifact by ID and
executes those exact bytes against the committed generation oracle. Qualification
does not rebuild the SDK or derive expected results from the candidate.

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

The canonical tag annotation binds the source, package version, producer run and
attempt, artifact ID, archive digest, and successful immutability precheck time.
The tag authority verifies the authenticated push actor and rerun actor, the
annotated tag object, both rulesets, and the complete annotation. The precheck
must precede tag creation by at most one minute and be no older than 15 minutes
when tag admission runs. An expired annotation fails closed. Reruns do not
extend its lifetime or permit moving the tag.

The tag workflow retrieves the successful main producer artifact by ID.
The archive must match its authenticated GitHub digest before bounded extraction.
The artifact contains exactly one regular SDK executable. The tag workflow
retains those same bytes as an immutable artifact in the tag run.

Source-free jobs create a draft, attest the canonical receipt and SDK together,
and publish through the protected `generator-release` environment. These jobs
perform no repository checkout or candidate execution. They verify both signed
subjects, certificate and run identities, the tag, draft identity, and exact
asset hashes. The publisher rechecks source ancestry and the tag before
publication, then requires an immutable release with the complete asset set.

The workflow token cannot query the administration-only immutability endpoint.
The trusted owner precheck supplies that authority. Repository administrators
remain trusted to preserve the setting throughout publication. The workflow's
final immutable-result check does not independently prove the earlier setting.
[GitHub immutability API](https://docs.github.com/en/rest/repos/repos#check-if-immutable-releases-are-enabled-for-a-repository).

A retry can resume an owned draft only when its existing assets have the exact
expected names and hashes. Conflicting assets and published releases cannot be
overwritten or deleted. The channel does not create or move stable version tags.

## Receipt and release assets

The prerelease tag is `sdk-preview-<full-source-sha>`. The SDK version is its actual
numeric package version. The version does not identify the publication by itself.
The source SHA, release ID, proof runs, and asset hashes identify the publication.

The release contains these assets:

- `SDK_PREVIEW_RECEIPT.json`
- `SDK_PREVIEW_RECEIPT.json.sigstore.jsonl`
- `velnor-actions-x86_64-unknown-linux-gnu`

The schema-1 receipt records the repository, package version, source SHA, tag,
release ID, signer run, CI run, qualification run, and payload identity. The
payload identity contains its target, name, size, and SHA-256 digest. The receipt
uses sorted JSON object keys, compact separators, and no final newline.
The Sigstore bundle binds both the receipt and payload subjects.

The lock records the complete receipt identity and the hashes of the receipt and
bundle. A lock entry cannot supply a different verifier, trusted root, repository,
workflow, or executable hash policy.

## Staged approval and activation

The optional `[planner-preview-staged]` and `[planner-preview]` sections are
separate from the stable generator records in `.velnor/generator.lock`.
A staged entry does not select a preview SDK for ordinary CI.

Staging approval uses the verifier from an independently authenticated, already
merged protected commit. The verifier checks the immutable release, both signed
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

Tests of parser rules, API policy, or historical signed fixtures do not establish
that a new preview was published successfully. Adoption requires a real signed
publication and successful verification of that publication before staging.
