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
profiles actually enable `cfg(test)`, then follows Rust modules and expanded
literal `include!` sources. It scans package Rust sources for test attributes,
including attributes emitted in macro bodies, and reports test-bearing files
outside those source closures. Rustc dep-info confirms that an `include!` was
expanded; dep-info entries alone MUST NOT register sources because they also
include `include_str!` and `include_bytes!` data. The check evaluates
test-profile conditions (`cfg(test)`, `cfg(not(test))`, and constant `cfg`)
and conservatively treats target, feature, and custom cfg predicates as
possible. Its registration result therefore means syntactically reachable in
a test-profile configuration, not that every target-specific branch was
compiled on the host running the check.

Tokio requires a demonstrated asynchronous-I/O need. Prefer the standard
library for one-use facilities. Shared versions belong in
`[workspace.dependencies]`; members MUST opt in explicitly.

`deny.toml` MUST reject yanked/unsound advisories, unknown registries, moving
Git sources, and wildcard dependency versions. It MUST define the reviewed
license allowlist. `cargo deny --locked check` and `cargo machete` MUST run in
CI. Duplicate versions MAY warn initially and MUST be reviewed. Cargo-vet MAY
be added when dependency audit provenance justifies it.
