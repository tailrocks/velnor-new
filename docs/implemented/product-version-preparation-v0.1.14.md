# Product version preparation for v0.1.14

This change prepares the Velnor source tree for v0.1.14. It does not publish
the release. The root workspace, path requirements, lockfile, generator asset
catalog, generated workflows, and active release snapshots select 0.1.14.
Historical release manifests and signed evidence remain unchanged. The
independently versioned runner workspace remains separately versioned.

The v0.1.14 tag and release endpoints both returned authenticated 404 responses
during read-only preflight. No occupied tag was reused. The generated workflow
tree and active Rust snapshots were regenerated from the 0.1.14 source. The
OpenTofu goldens remain checked against the candidate CLI; their capture and
check commands are `scripts/capture-opentofu-goldens.sh capture` and
`scripts/capture-opentofu-goldens.sh check`.

The update includes qualified Rust 1.99.1, MBX 1.24.1, Mise 2026.10.13,
updated tool and action pins, immutable image references, and native
`xcode-27` / Xcode 27.1 support. The Intel-target generator build runs on the
ARM macOS runner and uses the pinned MBX macOS-arm64 artifact to cross-compile;
native Intel qualification consumes the prebuilt candidate and does not install
or invoke MBX. MBX 1.24.1 has no macOS-x64 artifact. Rust's `core:rust` Mise
entry records its version only, so it is not treated as an explicit artifact
receipt; its distribution is separately qualified by the Rust toolchain and
release checks.

Publication still requires the reviewed source to merge, exact-main CI to pass,
and the canonical product-release workflow to complete all three build,
qualification, signature, immutable-publication, and manifest-verification
jobs. The existing `generator-release` human review environment remains in
force. Local image builds and test results are not hosted release evidence.
