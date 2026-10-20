# Product version preparation for v0.1.31

This prepares the workspace and generated release graph for 0.1.31, including
the native release fix that authenticates an executing job independently of the
workflow's aggregate queue status. It does not change the release protocol or
weaken source, attempt, role, signature or protected-environment admission.

The thirteen workspace packages advance together. Twenty-one exact internal
path requirements across nine member manifests and the workspace version are
updated; the root lockfile changes only those thirteen local package versions.
The independently versioned runner, external dependencies, historical release
manifests, consumer pins and project tool configuration remain unchanged.

The source-built 0.1.31 CLI generated the root tree and two fresh previews with
identical bytes. Existing CLI parity and OpenTofu capture/check procedures own
the current goldens. The sixteen release-family snapshots are derived from the
existing routing fixture through that CLI and retain their existing compiled
version normalization. The tag and release returned authenticated 404 responses
during preparation; synthetic fixture manifests are not publication evidence.

Publication still requires reviewed default-branch source, exact-source full CI,
all three native compiler and qualification paths, attestations and the existing
protected `generator-release` approval. Consumers adopt only the independently
verified official release manifest and executable, then regenerate through it.
