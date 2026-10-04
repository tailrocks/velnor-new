# Useful tool/source snapshots

The neutral contract owns closed payload domains, canonical roots and observer
bindings. Mise compiles the observer; the orchestrator supplies its source-bound
records; the renderer admits their exact bindings and producer sequence. Pure
trusted tool producers restore before Mise bootstrap installation, observe the
restored payload before setup, then observe again after executable preparation
and verification. Isolated Cargo, npm and Bun source producers observe their
source payload after restore and after source verification, before save,
publication lookup and terminal report. Plan consumes Cargo source snapshots;
source publication belongs to the isolated producer. PR readers perform no
snapshot hashing or writes.

Restore uses a unique lookup key plus the constrained compatibility prefix,
never an existing immutable primary that can starve newer repaired state. Useful
changes produce a content digest plus run/attempt snapshot suffix. Unchanged or
empty payloads skip compression/upload. The run suffix is used only after a
measured useful delta, including a repair that recreates the same original bytes
while the original immutable entry is damaged.

The Mise owner compiles one universal source-bound observer for all ten closed
cache domains. Its isolated host Python engine records opaque paths, file
contents, modes, symlink targets, hardlink topology, extended attributes, ACLs
and flags. It ignores access/modification timestamps, never edits them,
never hashes archived symlink targets as file contents, and suppresses tool exports when
a remaining link target is missing or outside the selected payload closure. Filesystem
observation assumes a quiescent producer payload. Snapshot errors suppress saves
and preserve the actual validation result. Bookkeeping lives outside all payloads.
Root/phase/output/restore bindings are typed and validated; changing either the
script or its roots fails rendering. Cargo credentials and reconstructed registry
trees belong outside the source subset. Cargo, npm and Bun producer roles admit
the observer over their exact independently qualified archive payload. Compiled
domain support alone does not establish integration: Gradle and OpenTofu source
roles currently admit no snapshot observations. Bun observes every member,
including package fixtures named `.git` or `.tmp`; source qualification keeps
authenticated dependency transport outside that archive. Npm observes only
`content-v2` and its fixed `public-proof-v1.json` sibling. Its authorization-bearing
index remains outside both archive and inventory. Proof-only repairs therefore
produce useful snapshot deltas even when content is unchanged.

This observer decides useful deltas; its digest does not authenticate restored
executables or their publisher. The same inventory authority supplies integrity
checks. Before observation records metadata; after observation rejects any
extended attributes, ACLs or flags before authorizing export. The pinned BSD tar
can export private attributes, so recording them alone grants no archive privacy.
Fresh macOS files here retain `com.apple.provenance`, which strict rejection
correctly leaves unavailable for export. A supported suppression adapter requires
independent transport qualification before this policy can change.
Linux flags are read with the supported read-only filesystem ioctl, without
following links. Actual Docker fixtures expose nonzero flags on ordinary files;
strict rejection also leaves those exports unavailable. Historical Linux success
under the earlier engine without flags does not qualify the current policy.
Source links suppress export in both phases;
producer verification must independently reject or reset restored links before
using the payload because bookkeeping failure remains advisory.
Every invocation starts with `available=false`; only completed observation and
bookkeeping emit `available=true`. Unavailable observations clear save
authorization. Terminal receipts must require this flag and valid bound digests,
because the observer deliberately exits successfully after export suppression.

Behavioral tests execute the actual emitted script for truly fresh temporary
roots, unchanged fresh payloads, same-byte proxy repair, a third unchanged
payload, source fill, empty snapshots, damaged/escaped links, and changed root
or script contracts. These are local regression proofs, not hosted-runner
performance qualification.

The prior shell implementation batched SHA and stat work. Its preserved samples in
`ci-performance-snapshot-evidence/observer-local.json` include the script digest,
raw output and wall time for the recorded historical script revision. A sample
of 500 files/125 MiB and a sample of 2,000 tiny
files measure observer overhead only. Each has one Darwin observation; no
percentile, Linux transfer cost, real tool closure or cross-run hosted reuse
claim follows from them. Hosted T01–T03/T18/T20 evidence remains required.

`ci-performance-snapshot-evidence/common-inventory-local.json` preserves actual
isolated Python, opaque inventory, metadata canary and generated universal
observer proofs. It explicitly records macOS export unavailability and the
local filesystem limitation for invalid UTF-8 filenames. Neither limitation is
counted as green CI or hosted performance verification.

`ci-performance-snapshot-evidence/current-wrapper-local.json` supersedes earlier
wrapper evidence for current-source behavior. It records the exact 393-line
wrapper reconstructed from current Rust factory formatting and the closed domain
registry; the Rust factory itself was not executed. Populated before observation
succeeds, while strict after observation remains unavailable on this host.
Empty-root success proves terminal output ordering only. These local proofs
establish neither populated strict-after success nor hosted performance.
