# Native preview production protocol

This specification refines [native-preview-contract.md](native-preview-contract.md).
The complete static `PackageReleasePlan` and public release receipt are unchanged.
Source implementation and focused tests do not establish hosted capacity or live
publication. Official release, consumer adoption and two real upgrades remain gates.

## Bounded source matrix and complete artifact union

A validated complete preview plan determines sorted product/target pairs,
bounded by a preview-specific limit of 128. The separate full release archive
limit remains 256. Eligibility emits the matrix from the complete A-bound plan;
one fixed `preview-payload` matrix job builds each pair's complete executable
inventory. There are no generated literal-leaf job IDs and no reduced or
caller-selected matrix. Empty plans, partial bin inventories and arbitrary
selectors are rejected.

`preview-assemble` requires successful eligibility and the matrix job. Native
admission enumerates the complete authenticated GitHub jobs and artifacts API
union for the exact current run and attempt, matches every expected pair to
exactly one successful matrix child and immutable artifact ID, and checks each
uploaded archive digest against the API digest. Missing, extra, duplicate,
failed, stale-attempt or mismatched artifacts block assembly. Names are
consistency checks, never download authority. Assembly downloads only admitted
numeric IDs, verifies their bytes, and builds the complete plan union with
configured companions. The matrix and artifact union derive from A's full plan;
a leaf cannot supply a smaller plan.

Matrix producer jobs have Contents read permission. Assembly additionally has
Actions read permission for fixed artifact and run reads. Neither source job has
an approval environment, OIDC, release writes, tap writes or signing authority.
Generated roles use fixed machine IDs and exact live job observations for
admission. Jobs and operations remain bounded by their declared limits.

## Complete production request

Canonical `velnor.release-production.v1` contains exactly `schema`, complete
`plan`, `policy_sha256`, `plan_sha256`, `context`, and `stage`. Hashes bind the
canonical complete policy and plan from A, including all products, bins and
companions. Context contains repository/head repository IDs, source A and ref,
workflow authority B, current run/attempt, successful exact-A CI run/attempt,
full-history count and deterministic version. Structural validation is not
remote authentication. Source tasks independently rederive policy, task closure
and history from the admitted A checkout. Source-free signer and publisher
admission authenticates B's current run, workflow, certificate and job roles,
the A<=B<=H ancestry, and latest exact-A CI evidence.

`stage` is a closed tagged object: `payload` contains one admitted `pair`;
`assemble` contains the exact sorted complete `inputs` array. Each input has
`pair`, immutable numeric `artifact_id`, and unprefixed mandatory `sha256`.
No caller may fabricate a reduced plan to invoke the complete verifier.

## Source task handoff

The declared qualified Mise task receives one additional fixed variable:
`VELNOR_RELEASE_TASK_REQUEST_FILE`, an owned absolute path to
`$RUNNER_TEMP/velnor/release-task-request.json`. Its canonical
`velnor.release-task.v1` object contains `schema`, the full production `request`,
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
transport authenticates the current B run and exact attempt, source/head
repository identities, workflow and exact-A push-CI authority, the A<=B<=H
ancestry, latest successful exact-A `Required` CI and one active fixed consumer
job. It admits bounded
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
