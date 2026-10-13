# Rust test-registration compiler evidence

The test-registration audit associates each compiled Cargo test target with its
own rustc dep-info. It reads that artifact's Makefile prerequisites and raw
`# env-dep:OUT_DIR=` record together. A direct `include!(concat!(env!("OUT_DIR"),
"..."))` source is admitted only when the recorded directory joined with the
safe relative `.rs` suffix names a regular, non-symlink file that is an exact
prerequisite of the same artifact. This supports compiler-cache output outside
Cargo's target directory without accepting same-basename or cross-artifact
sources.

The audit still uses dep-info as corroboration rather than general source
authority: it follows Rust modules, source-level includes, and the supported
invoked macro forms, and reports unregistered test-bearing Rust files.
