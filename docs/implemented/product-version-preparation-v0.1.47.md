# Product version preparation for v0.1.47

The published v0.1.46 CLI generates the source-admission flow added to main. This branch also needs canonical verifier composite definitions and typed caller bindings. The v0.1.46 CLI does not emit or validate that verifier-action contract, so consumers need the v0.1.47 source line for generated workflows.

The root workspace version, 25 exact internal path requirements, and 14 root-workspace lock records advance from 0.1.46 to 0.1.47. The incoming feature's external package records, checksums, dependency features, separately versioned runner workspace, tool pins, historical release manifests, and explicit consumer overrides remain intact. Generated workflows, parity fixtures, and OpenTofu goldens must be regenerated from this source.

This is source preparation only. It does not publish, tag, retag, or claim a v0.1.47 release or helper artifact.
