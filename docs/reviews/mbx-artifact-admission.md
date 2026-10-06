# MBX artifact admission boundary

Status: source factory draft only. No authenticated same-run or historical
artifact admission exists; no supported warm producer is claimed.

`mbx_admission_source.rs` independently captures the actual native MBX transport
registry result and regenerates all four source records, arguments and environments.
The registry currently has no qualified MBX transport publication. Captured
unsupported state emits false preparation, admission, verification and cache
availability outputs without credentials or payload creation.

## Why artifact names cannot authenticate an exporter

The GitHub artifact API exposes artifact ID, name, archive digest and workflow-run
repository/source metadata. Its documented artifact representation has neither
uploader job ID nor run attempt. A suffix containing the current attempt provides
a selection key; it is not service-authenticated uploader evidence.

An immutable workflow with one declared upload action does not remove this gap.
Repository tasks execute in the same job and may possess an artifact runtime
capability. They can upload an exact expected artifact name before the late native
export. The enabling condition is shared runtime publication authority across
repository execution and native export. Step names, successful job status and the
absence of another declared upload action do not prove that the late exporter
produced the downloaded archive.

Activation must therefore either independently authenticate complete protected
task source and explicitly adopt its trust assumption, or establish a separately
qualified export witness binding the exact archive digest under a capability that
repository tasks cannot exercise. Neither authority has been qualified here.

## Native verification and external origin authority

The frozen native implementation supports `mbx cache verify DIRECTORY --json`
before Cargo configuration, repository execution, import or hydration. Public
report schema 1 exposes `valid`, `actions`, `objects`, `files`, `bytes`,
`physical_digest`, `native_closure_digest` and `semantic_digest`.

It checks physical closure, native object/attachment references and receipt
evidence, then rechecks the physical digest. Native receipt-context shape alone
does not authenticate GitHub origin. Retained historical contexts need separately
verified original source records, run attempts and receipt certificates; current
export metadata must not relabel those original contexts.

## Activation gates

1. Actual qualified immutable MBX source/binary acquisition and digest verification
   outside transported payload roots, matching every descriptor owner field.
2. Independently qualified complete protected source or separately bound export
   witness, with the trust assumption recorded explicitly.
3. Bounded actions-read-only service queries establishing exact repository, run,
   attempt, successful producer and archive digest. API redirect downloads must
   not forward bearer credentials to object storage.
4. Independently authenticated retained historical origins and immutable source
   records; unavailable evidence remains cold.
5. Native data-only verification using the acquired qualified owner, before save.

Sources: [GitHub artifact REST documentation](https://docs.github.com/en/rest/actions/artifacts?apiVersion=2022-11-28),
[GitHub workflow-job REST documentation](https://docs.github.com/en/rest/actions/workflow-jobs?apiVersion=2022-11-28).
The inspected native checkpoint is
`/tmp/velnor-mbx-native-checkpoint-20261003-074739/source`,
`crates/mbx/src/cli/cache.rs`, `verify_directory`.
