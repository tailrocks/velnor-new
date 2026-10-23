# Product version preparation for v0.1.43

This prepares v0.1.43 with a bounded recursive Git-tree response budget for
native package source admission. Jackin main source
`35ff5eef7bf3348d1b63070017dc7d1543408477` has a complete recursive tree
response of 1,732,801 bytes, which exceeds the ordinary 1 MiB JSON budget.
Only the recursive tree read receives an explicit 8 MiB budget. Ordinary
reads retain their 1 MiB budget, and tree identity, truncation, entry-count,
path, file-mode, and source-custody validation remain mandatory.

Transport fixtures now pass serialized bytes through the same bounded JSON
decoder as real responses. Regression coverage must prove that a valid tree
above 1 MiB is rejected by the default read and accepted by source admission,
and that a valid tree above 8 MiB is rejected before tree validation.

The workspace version and exact internal path requirements advance from
0.1.42 to 0.1.43. The lockfile changes only fourteen workspace package versions;
external package records, checksums, and dependency graphs remain unchanged.
Generated workflows, parity fixtures, and OpenTofu goldens are regenerated
with the repository tools. The separately versioned runner workspace, tool
pins, and historical release manifests remain unchanged.

This record describes version preparation, not successful publication.
Exact-source hosted CI, protected publication, accepted release artifacts,
immutable manifests, and public signature/provenance verification are separate
requirements. Consumers adopt v0.1.43 only after its official binary and
source identity are verified.
