# Cargo package archive fixture

`velnor-archive-guard-fixture-0.0.0.crate` is a 759-byte archive created with
Cargo 1.98.1 from the adjacent `Cargo.toml`, `Cargo.lock`, and `src/lib.rs`.
The archive was initially created in a temporary directory outside a Git
checkout. At that time, the lockfile was generated before packaging:

```sh
cargo generate-lockfile --manifest-path Cargo.toml
cargo package --locked --no-verify --allow-dirty --manifest-path Cargo.toml
```

That unlocked lockfile-generation command records historical provenance only;
do not use it to reproduce the fixture. For a future reproduction, copy only
`Cargo.toml`, the committed `Cargo.lock`, and `src/lib.rs` into a fresh
scratch directory, then use Cargo 1.98.1 with the committed lockfile:

```sh
cargo package --locked --offline --no-verify --allow-dirty --manifest-path Cargo.toml
shasum -a 256 target/package/velnor-archive-guard-fixture-0.0.0.crate
```

The checked-in archive's SHA-256 is
`51053c31976efc0c26aa6d35b450b7dfa878cdde79cd10eea1992abd7b5104a3`. This hash
identifies the stored test input; byte-for-byte reproduction has not been
claimed.
