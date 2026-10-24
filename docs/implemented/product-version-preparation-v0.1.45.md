# Product version preparation for v0.1.45

At the time this preparation began, the official v0.1.44 release was published
and no v0.1.45 helper artifact had yet been published or verified. The source
line prepared the root workspace and its internal path requirements for
v0.1.45; generated consumers kept their verified immutable helper pins until
a compatible helper and source identity were available.

The v0.1.45 source also corrected native preview source-admission upload
permissions with the existing secure upload helper. Private policy and plan
files kept their private writer, and the strict ZIP inventory, file type,
mode, size, and digest checks remained unchanged. The 14 root-workspace lock
records moved with the workspace version; external package records, checksums,
features, the separately versioned runner workspace, tool pins, and historical
release manifests stayed unchanged.

Generated workflows, parity fixtures, and OpenTofu goldens had to be regenerated
from the source before consumers could adopt the helper. This note records
source preparation at that time; consumers adopt a helper only after its
source identity and published artifact are verified.
