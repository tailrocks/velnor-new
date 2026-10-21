# Product version preparation for v0.1.33

This prepares workspace metadata and generated outputs for a proposed 0.1.33
release. It carries the v0.1.32 Gradle and Maven release features forward and
includes the native artifact ZIP metadata correction. This document records
preparation only: the complete local and hosted verification, reviewed main
merge, exact-source release admission, and protected release approval remain
required. No v0.1.33 release or tag is claimed here.

All 14 workspace packages now use 0.1.33. The 22 exact internal path
requirements and the 14 local package records in the root `Cargo.lock` were
updated together. The independently versioned runner, nested runner lockfile,
external dependency versions and edges, historical release manifests,
consumer pins, and tool configuration were not changed. Cargo metadata before
and after the version update reports the same 128 external packages and
dependency edges (SHA-256 `6331ea8d0af80f9d5d650abd6dfc2af27fe478d70b08c084f4464f0b993e4842`).

The freshly built 0.1.33 CLI regenerated the root `.github` tree. Two
independent `generate --output-dir` runs produced byte-identical trees, each
matching the checked-in generated tree. The OpenTofu golden capture and check
matched all five cases. The CLI parity golden update and follow-up check each
passed all four selected tests.

Native artifact metadata parsing uses declared archive offsets instead of
scanning compressed payloads. A fixed footer allowance and bounded per-entry
work cover maximum ZIP comments without increasing payload-dependent work.
Existing integrity, inventory, size, and deadline checks remain enforced.

Release admission requires the complete root workspace and nested runner formatting,
Clippy, Nextest, config, dependency-policy, and freshness gates, followed by
the exact-source main CI and protected `generator-release` admission described
in the product release workflow. Crates.io publication remains disabled by
the current release configuration.
