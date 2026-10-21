# Product version preparation for v0.1.32

This prepares the workspace and generated release graph for 0.1.32, which
carries `[stacks.gradle]` with per-project Gradle jobs (#255) and
`[[publish.maven]]` release-asset Maven bundles (#261). It does not change the
release protocol or weaken source, attempt, role, signature or
protected-environment admission.

The workspace packages advance together: the workspace version and the exact
internal path requirements are updated, and the root lockfile changes only the
local package versions. The independently versioned runner, external
dependencies, historical release manifests, consumer pins and project tool
configuration remain unchanged.

The source-built 0.1.32 CLI regenerated the root `.github` tree in place. The
OpenTofu goldens were captured and checked with `scripts/capture-opentofu-goldens.sh`
(all cases match) and the CLI parity goldens with `VELNOR_UPDATE_GOLDENS=1` on
the `parity_` tests. No managed `.github` file was hand edited.

Publication still requires reviewed default-branch source, exact-source full CI,
all three native compiler and qualification paths, attestations and the existing
protected `generator-release` approval.
