# Product version preparation for v0.1.42

The official v0.1.41 release is published, but that helper does not include the
GitHub evidence fetch and import operations added in this source line. The
existing helper-manifest identity check must reject a v0.1.41 helper for these
new operations; consumers must not switch until a compatible v0.1.42 helper
manifest and source identity have been verified.

The root workspace version, exact internal path requirements, and corresponding
workspace lock records advance from 0.1.41 to 0.1.42. External package records,
checksums, dependency features, separately versioned runner workspace, tool
pins, image graph, and historical release manifests remain unchanged. Generated
workflows, parity fixtures, and OpenTofu goldens must be regenerated from the
merged source as part of completing this version preparation.

This is source preparation only. It does not publish, tag, retag, or claim a
v0.1.42 release or helper artifact.
