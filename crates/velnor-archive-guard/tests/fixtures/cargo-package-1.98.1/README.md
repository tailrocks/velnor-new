# Cargo package archive fixture

`velnor-archive-guard-fixture-0.0.0.crate` was produced with Cargo 1.98.1
from the adjacent `Cargo.toml`, `Cargo.lock`, and `src/lib.rs` in a temporary
directory outside a Git checkout:

```sh
cargo generate-lockfile --manifest-path Cargo.toml
cargo package --locked --no-verify --allow-dirty --manifest-path Cargo.toml
```

The archived fixture is 759 bytes and has SHA-256
`51053c31976efc0c26aa6d35b450b7dfa878cdde79cd10eea1992abd7b5104a3`.
This records the provenance of this test input; it does not claim that every
Cargo version reproduces identical archive bytes.
