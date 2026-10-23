# Product version preparation for v0.1.46

The v0.1.45 source line on main includes the preview source-admission file-mode correction, but its generated consumer does not include the canonical verifier action contract integrated by this branch. The v0.1.45 product-release workflow was already launched from main before this verifier-action work was integrated. This source preparation advances to v0.1.46 so the canonical verifier definitions and typed calls can be generated without changing or replacing that v0.1.45 source or any release artifact.

The root workspace version, 24 exact internal path requirements, and 14 root-workspace lock records advance from 0.1.45 to 0.1.46. External dependency records and checksums, dependency features, the separately versioned runner workspace, tool pins, historical release manifests, and explicit consumer overrides remain unchanged. Generated workflows, parity fixtures, and OpenTofu goldens must be regenerated from this source.

This is source preparation only. It does not publish, tag, retag, or claim a v0.1.46 release or helper artifact.
