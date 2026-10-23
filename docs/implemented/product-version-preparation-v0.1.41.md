# Product version preparation for v0.1.41

This prepares the v0.1.41 product release with the credential-cleared tool
launch correction for native preview publishing. The correction keeps the
credential boundary closed while preserving the system environment needed to
launch the pinned Mise executable. Its exact source identity is the final
merged main commit; this record does not predict that SHA.

The root workspace version and exact internal path requirements advance from
0.1.40 to 0.1.41. The lockfile update is limited to the fourteen root workspace
packages; external package records, checksums, and dependency graphs remain
unchanged. Repository-generated workflows, parity fixtures, and OpenTofu
goldens must be regenerated and verified with the repository tools as part of
the completed version-preparation change. The separately versioned runner
workspace, tool pins, runner/image graph, and historical release manifests
remain unchanged.

This is version preparation, not publication. Exact-source hosted CI, protected
release publication, the accepted generator artifact, immutable release
manifests, and public signature/provenance verification remain separate
requirements. Jackin and its Homebrew tap adopt v0.1.41 only after its actual
published binary and source identity are verified.
