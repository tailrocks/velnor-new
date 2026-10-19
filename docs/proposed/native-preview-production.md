# Native preview production protocol

This specification refines [native-preview-contract.md](native-preview-contract.md).
The complete static `PackageReleasePlan` and public release receipt are unchanged.
Source implementation and focused tests do not establish hosted capacity or live
publication. Official release, consumer adoption and two real upgrades remain gates.

## Literal execution graph

A validated complete preview plan determines sorted product/target pairs, bounded
by the existing archive limit of 256. Each pair receives the fixed indexed job ID
`preview-payload-000` through `preview-payload-255`. Jackin has six such jobs.
Each builds every declared executable for that pair. Empty or reduced plans,
partial bin inventories, arbitrary selectors and dynamically selected matrices
are not supported.

`preview-assemble` requires successful eligibility and every literal leaf. It
receives exact `toJSON(needs)` data: eligibility outputs must match the admitted
source/CI tuple and each expected leaf must report success with exactly
`artifact_id` and `artifact_digest`. IDs are distinct positive canonical decimal
integers; digests are mandatory 64-character lowercase SHA-256 values. Extra,
missing, duplicate, failed or stale-attempt inputs block assembly.

Leaf jobs have Contents read permission. Assembly additionally has Actions read
permission for fixed artifact and run reads. Neither source job has an approval
environment, OIDC, release writes, tap writes or signing authority. The service
job display name equals the fixed job ID, permitting exact live-role admission.
All concrete jobs remain bounded by the existing five-minute maximum.

## Complete production request

Canonical `velnor.preview-production.v1` contains exactly `schema`, `plan`,
`plan_sha256`, `context`, and `stage`. The SHA-256 covers canonical bytes of the
entire unchanged static plan, including all products, bins and companions.

Context contains repository/head repository numeric IDs, full source SHA and
ref, workflow authority SHA, current run ID and attempt, successful upstream CI
run ID and attempt, full-history commit count and deterministic version.
Structural validation is not remote authentication. Source jobs independently
re-derive the plan and history from the exact checkout. The source-free signer
compares fixed workflow/event/eligibility data with authenticated current GitHub
run, repository, source, latest CI and job-role observations.

`stage` is a closed tagged object: `payload` contains one admitted `pair`;
`assemble` contains the exact sorted complete `inputs` array. Each input has
`pair`, immutable numeric `artifact_id`, and unprefixed mandatory `sha256`.
No caller may fabricate a reduced plan to invoke the complete verifier.

## Source task handoff

The declared qualified Mise task receives one additional fixed variable:
`VELNOR_PREVIEW_REQUEST_FILE`, an owned absolute path to
`$RUNNER_TEMP/velnor/preview-task-request.json`. Its canonical
`velnor.preview-task.v1` object contains `schema`, the full production `request`,
and `inputs`. Each input row contains `pair`, `name`, `sha256`, `size` and an
admitted absolute `path`. Payload inputs are empty. Assembly has precisely one
archive and one SBOM row per pair. Runtime retains original file identities and
rehashes input/request bytes after source execution; identical replacement
files are still rejected. Existing qualified tool, SDK, version and resource
boundaries remain mandatory. Tasks receive no arbitrary environment overlay.

Payload output uses `velnor.package-payload-output.v1`, with the existing output
row fields, exact source/ref/version, precisely one archive and matching SBOM,
and no companion rows. It is independently verified against the selected pair
of the full plan; the complete-output verifier does not accept this schema.
Assembly output uses the complete `velnor.package-output.v1` schema. Assembly
must preserve admitted archive/SBOM bytes and generate only configured opaque
companions plus the full output manifest. Velnor verifies the entire full plan
and compares every resulting leaf with its admitted original.

## Artifact and signer admission

Each leaf upload contains exactly its archive, SBOM, and native
`preview-payload.json` (`velnor.preview-payload.v1`). Native metadata binds the
full request, typed qualified producer evidence, and exact asset sizes/digests.
It is written last after successful verification. Artifact names are
`velnor-preview-payload-r{run_id}-a{run_attempt}-{product_id}-{target}`.

Assembly uploads all complete raw assets, `package-output.json`, and native
`preview-aggregate.json` (`velnor.preview-aggregate.v1`). The aggregate binds its
full request, own producer proof, every exact leaf proof and full asset union.
Only new owned raw upload files and pending native metadata receive exact 0644
mode through retained no-follow descriptors, with identity/byte checks before
publication. The pinned upload action preserves that mode in raw service ZIPs.
Its name is `velnor-preview-aggregate-r{run_id}-a{run_attempt}`. The upload emits
its immutable service ID and ZIP digest to the source-free signer.

Names are consistency checks, never download authority. Native read-only
transport authenticates the current run and exact attempt, source/head
repository identities, workflow and push-CI authority, current main, latest
successful Required CI and one active fixed consumer job. It admits bounded
complete artifact/job pagination, downloads only the literal immutable ID,
requires API digest equality, and rechecks downloaded size/digest. There is no
artifact timestamp heuristic and no failed-only attempt reuse. Assembly
reauthenticates after source execution and all leaf byte checks.

Closed ZIP admission validates headers before selecting metadata, rejecting
hidden duplicate names, local/central disagreement, overlaps, paths, directories,
nonregular entries, encryption and bounded inventory/size violations. Extraction
uses pinned destination identity, exclusive descriptor-relative writes, streamed
CRC/size/SHA verification and cleanup of only files created by that operation.
One intersecting absolute deadline covers parsing and extraction; recovery and
promotion retain their own schema/signature authority around this shared layer.

The signer performs `admit-preview-aggregate-v1` before unchanged full signing.
It extracts native metadata separately from the exact raw input tree, admits no
extra-file exception, verifies the complete union, then creates signatures and
public receipt. No partial metadata is a public receipt or cryptographic proof.
The obsolete serial producer artifact and `preview-proof.json` path are removed.
