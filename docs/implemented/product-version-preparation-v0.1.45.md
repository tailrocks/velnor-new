# Product version preparation for v0.1.45

The immutable v0.1.44 helper is published. Native preview source admission succeeds, but its exported admission file retains private permissions under the generated restrictive umask. The artifact consumer correctly requires the native upload contract’s exact regular-file mode.

Version 0.1.45 prepares the producer fix using the existing secure upload helper. Private policy and plan files retain their private writer, and the strict ZIP inventory, file type, mode, size, and digest checks remain unchanged.

The root workspace version, 24 exact internal path requirements, and 14 workspace lock records advance from 0.1.44 to 0.1.45. External dependency records, the separately versioned runner workspace, tool pins, and historical release manifests remain unchanged. Native workflows, parity fixtures, and OpenTofu goldens must be regenerated from this source.

This is source preparation only. It does not publish, tag, retag, or claim a v0.1.45 release or helper artifact. Consumers must adopt only a published and verified helper and accepted manifest.
