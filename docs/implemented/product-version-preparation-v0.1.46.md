# Product version preparation for v0.1.46

The immutable v0.1.45 helper is published. Its source-admission upload satisfies the strict ZIP contract, but native preview payload acquisition cannot authenticate its matrix job because the generated acquisition step lacks the product and target selectors already supplied to the later execution step.

Version 0.1.46 prepares the renderer fix. Source acquisition must receive only the same public matrix identity at step scope. The runtime role validator, authenticated job-name checks, read-only permissions, source and run binding, and pre-checkout ordering remain unchanged. Acquisition must not reference its own admission output.

The root workspace version, 24 exact internal path requirements, and 14 workspace lock records advance from 0.1.45 to 0.1.46. External dependency records, the separately versioned runner workspace, and historical release manifests remain unchanged. The required freshness gate discovered Mise 2026.10.18, MR Boxington 1.27.0, and release-plz 0.4.0. Their catalog, bootstrap identities, checksums, current-pin fixtures, and policy evidence form one qualified update set. The separately authorized current-toolchain baseline updates MR Boxington in the project and local Mise configuration; native Mise lock generation preserves all unrelated tool records. Native workflows, parity fixtures, and OpenTofu goldens must be regenerated from this source.

This is source preparation only. It does not publish, tag, retag, or claim a v0.1.46 release or helper artifact. Consumers must adopt only a published and verified helper and accepted manifest.
