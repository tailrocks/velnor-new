# Velnor V1 Rust dependency policy

**Status:** Proposed specification, companion to the [Rust quality
contract](rust-quality-contract.md) §7. Landed implementation records live in
`docs/implemented/` and must change in the same implementation pull request.

The words **MUST**, **MUST NOT**, **SHOULD**, and **MAY** are normative.

Dependencies MUST have a concrete responsibility and narrow features. The
initial set MAY include `serde`, `serde_json`, `toml`, `cargo_metadata`,
`globset`, `blake3`, `clap` (`derive`), `thiserror`, `anyhow`, and `tracing`.
Development dependencies MAY include `tempfile`, `proptest`, `insta`,
`proc-macro2`, and `syn` for source-level test-registration checks. Use only
the `full` and `visit` `syn` features needed to inspect complete Rust syntax
and module/test attribute declarations.

The test-registration check starts from Cargo compiler artifacts whose
profiles actually enable `cfg(test)`, then follows Rust modules, direct literal
`include!` sources, direct `include!(concat!(env!("OUT_DIR"), "..."))` sources
with a safe relative `.rs` suffix, and invoked `macro_rules!` includes that
have one empty matcher and empty calls in the same source module. An `OUT_DIR`
source is accepted only when that exact artifact's rustc dep-info records one
absolute `OUT_DIR` env-dependency and the exact `OUT_DIR`-joined source is a
regular, non-symlink file listed among the same artifact's prerequisites. The
compiler-recorded directory may be outside Cargo's target directory, such as
when a compiler cache relocates build output. Missing, malformed, duplicate,
or mismatched evidence fails closed; metadata-only targets cannot claim
generated `OUT_DIR` sources. A macro call is associated only with an earlier
definition in Rust source order. Other invoked macros that contain `include!`
fail closed. A metadata-declared test target
skipped by Cargo because its `required-features` are disabled is still registered: its
declared source and statically reachable module/include closure are inspected,
but this does not claim that the target was compiled. For compiled targets,
the source closure uses test-profile artifacts and corroborating rustc
dep-info. Dep-info alone is not source authority because it also includes
`include_str!` and `include_bytes!` data. Conditional source edges remain
possible even when the host dep-info omits them. The check scans package Rust
sources for test attributes, including test items emitted by invoked macro
bodies, and reports test-bearing files outside those closures. It excludes
only Cargo metadata's exact target directory and the repository's known Git
metadata path, never a directory merely because of its name. The check
evaluates test-profile conditions (`cfg(test)`, `cfg(not(test))`, `cfg_attr`,
and constant `cfg`) and conservatively treats target, feature, and custom cfg
predicates as possible. Its registration result means syntactically reachable
under the evaluated test-profile conditions, not that every target-specific
branch was compiled on the host running the check.

Tokio requires a demonstrated asynchronous-I/O need. Prefer the standard
library for one-use facilities. Shared versions belong in
`[workspace.dependencies]`; members MUST opt in explicitly.

`deny.toml` MUST reject yanked/unsound advisories, unknown registries, moving
Git sources, and wildcard dependency versions. It MUST define the reviewed
license allowlist. `cargo deny --locked check` and `cargo machete` MUST run in
CI. Duplicate versions MAY warn initially and MUST be reviewed. Cargo-vet MAY
be added when dependency audit provenance justifies it.
