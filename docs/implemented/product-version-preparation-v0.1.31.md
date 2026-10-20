# Product version preparation for v0.1.31

This prepares the Velnor workspace and its generated release graph for 0.1.31,
the release carrying the private-sources consumer fixes: the generated
actionlint configuration declares the private-sources App id variable, and the
private fetch extracts bundles to `<workspace>/.velnor/deps/<id>/`. The
workspace version, exact internal path requirements, and workspace lockfile
advance together. The independently versioned runner workspace and historical
release manifests remain unchanged.

The source-built 0.1.31 CLI generated the root workflow tree in place and a
separate fresh preview with identical bytes. The OpenTofu goldens were
recaptured with `scripts/capture-opentofu-goldens.sh capture` and re-checked
(`ALL GOLDENS MATCH`), and the CLI parity goldens were refreshed through
`VELNOR_UPDATE_GOLDENS=1`.

This is source preparation, not publication or hosted qualification. Publication
requires reviewed default-branch source, exact-source CI, all three native build
and qualification paths, attestation verification, and the existing protected
`generator-release` approval. Consumers adopt only the verified official release
manifest and executable, then regenerate through that exact executable.
