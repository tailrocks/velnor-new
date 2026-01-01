# Velnor V1 Rust dependency policy

**Status:** Proposed specification, companion to the [Rust quality
contract](rust-quality-contract.md) §7. Landed implementation records live in
`docs/implemented/` and must change in the same implementation pull request.

The words **MUST**, **MUST NOT**, **SHOULD**, and **MAY** are normative.

Dependencies MUST have a concrete responsibility and narrow features. The
initial set MAY include `serde`, `serde_json`, `toml`, `cargo_metadata`,
`globset`, `blake3`, `clap` (`derive`), `thiserror`, `anyhow`, and `tracing`.
Development dependencies MAY include `tempfile`, `proptest`, and `insta`.
Tokio requires a demonstrated asynchronous-I/O need. Prefer the standard
library for one-use facilities. Shared versions belong in
`[workspace.dependencies]`; members MUST opt in explicitly.

`deny.toml` MUST reject yanked/unsound advisories, unknown registries, moving
Git sources, and wildcard dependency versions. It MUST define the reviewed
license allowlist. `cargo deny --locked check` and `cargo machete` MUST run in
CI. Duplicate versions MAY warn initially and MUST be reviewed. Cargo-vet MAY
be added when dependency audit provenance justifies it.
