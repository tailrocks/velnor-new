# Product version preparation for v0.1.54

The v0.1.53 helper is already published and immutable. This source line prepares the latest native Homebrew adoption and private candidate-install changes for the next helper version; consumers must continue to use only a published release with its verified manifest and attestation.

The workspace version and all 25 exact internal path requirements across the 10 member manifests advance from 0.1.53 to 0.1.54, including the orchestrator's normal and development requirements on the contract crate. Root-workspace lock resolution and generated artifacts are captured separately from this source edit. External dependency versions, the separately versioned runner workspace, tool pins, historical release manifests, and explicit consumer overrides remain unchanged. Workflows, parity fixtures, and OpenTofu goldens must be regenerated using the 0.1.54 CLI.

This is source preparation only. It does not publish, tag, retag, or claim a v0.1.54 release or helper artifact.
