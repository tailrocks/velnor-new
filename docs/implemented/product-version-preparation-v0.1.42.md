# Product version preparation for v0.1.42

This prepares the v0.1.42 product release. It carries the Playwright system
dependency follow-ups found by the first consumer run of v0.1.41
(ChainArgos/platform PR #57): `install-deps` output is streamed live through
the bounded, escaped progress observer, one run gets 8 minutes and a failed or
stalled run is retried once (#361), and a deadline whose process-group cleanup
failed is terminal instead of retried (#362). Its exact source identity is the
final merged main commit; this record does not predict that SHA.

The root workspace version and exact internal path requirements advance from
0.1.41 to 0.1.42. The lockfile update is limited to the fourteen root workspace
packages; external package records, checksums, and dependency graphs remain
unchanged. The source-built 0.1.42 CLI regenerated the root `.github` tree in
place, the OpenTofu goldens were captured with
`scripts/capture-opentofu-goldens.sh capture` and the CLI parity goldens with
`VELNOR_UPDATE_GOLDENS=1` on the `parity_` tests. No managed `.github` file was
hand edited. The separately versioned runner workspace, tool pins, runner/image
graph, and historical release manifests remain unchanged.

This is version preparation, not publication. Exact-source hosted CI, protected
release publication, the accepted generator artifact, immutable release
manifests, and public signature/provenance verification remain separate
requirements.
