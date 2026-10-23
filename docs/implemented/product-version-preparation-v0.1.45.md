# Product version preparation for v0.1.45

The official v0.1.44 helper is published, but it cannot emit and revalidate the
canonical verifier action definitions and calls required by this source line.
Generated consumers need a compatible v0.1.45 helper before those definitions
can be regenerated and checked from source.

The root workspace version, 24 exact internal path requirements, and 14
workspace lock records advance from 0.1.44 to 0.1.45. External package records,
checksums, dependency features, the separately versioned runner workspace, tool
pins, historical release manifests, and explicit consumer overrides remain
unchanged. Generated workflows, parity fixtures, and OpenTofu goldens must be
regenerated from this source as part of completing the version preparation.

This is source preparation only. It does not publish, tag, retag, or claim a
v0.1.45 release or helper artifact.
