# Product version preparation for v0.1.44

The official v0.1.43 helper is published, but its `PublishConfig` rejects
unknown fields. It cannot parse the new `[publish].provenance` configuration
used by this source line. Generated consumers must remain pinned to their
current helper until a compatible v0.1.44 helper and its source identity have
been published and verified.

The root workspace version, 24 exact internal path requirements, and 14
workspace lock records advance from 0.1.43 to 0.1.44. External package
records, checksums, dependency features, the separately versioned runner
workspace, tool pins, and historical release manifests remain unchanged.
Generated workflows, parity fixtures, and OpenTofu goldens must be
regenerated from this source as part of completing the version preparation.

This is source preparation only. It does not publish, tag, retag, or claim a
v0.1.44 release or helper artifact.
