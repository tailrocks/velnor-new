# Product version preparation for v0.1.26

This prepares the Velnor workspace and its generated release graph for 0.1.26.
The workspace version, exact internal path requirements, and workspace lockfile
advance together. The independently versioned runner workspace and historical
release manifests remain unchanged.

Native preview payload jobs display their literal product and target as
`Build <product> / <target>`. Internal job IDs, artifact identities, dependencies,
and authenticated assembly and signer names retain their existing contracts.

The source-built 0.1.26 CLI generated the root workflow tree and a separate fresh
preview with identical bytes. Existing OpenTofu capture/check and CLI parity
procedures own the current golden outputs. The version tag and release returned
authenticated 404 responses during preparation.

This is source preparation, not publication or hosted qualification. Publication
requires reviewed default-branch source, exact-source CI, all three native build
and qualification paths, attestation verification, and the existing protected
`generator-release` approval. Consumers adopt only the verified official release
manifest and executable, then regenerate through that exact executable.
