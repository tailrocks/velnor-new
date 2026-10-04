//! Exact official root compiler manifest; source audit never grants native installation.
//!
//! Downloaded official versioned manifest and its SHA256 sidecar 2026-10-03.
//! Rustup native Linux component-install/full-tree qualification is absent.

use super::{OfficialRootRustManifest, RustComponentArtifact};

pub(super) const COMPONENTS: &[RustComponentArtifact] = &[
    RustComponentArtifact {
        component: "rustc",
        xz_url: "https://static.rust-lang.org/dist/2026-09-03/rustc-1.98.1-x86_64-unknown-linux-gnu.tar.xz",
        xz_sha256: "e974f036b28565f37c0f3bd92ddefa809bee16c04f9dcf07b9ed96e05aaaf7c4",
        gzip_url: "https://static.rust-lang.org/dist/2026-09-03/rustc-1.98.1-x86_64-unknown-linux-gnu.tar.gz",
        gzip_sha256: "a6e35741daaac7978e7f485b564a783d13b6740a1ecf3e80c2e71696ca5cabb2",
    },
    RustComponentArtifact {
        component: "cargo",
        xz_url: "https://static.rust-lang.org/dist/2026-09-03/cargo-1.98.1-x86_64-unknown-linux-gnu.tar.xz",
        xz_sha256: "ea1de9f9e23107d97ee2b41a72c552f34064a593da503789218387aee59f3ba4",
        gzip_url: "https://static.rust-lang.org/dist/2026-09-03/cargo-1.98.1-x86_64-unknown-linux-gnu.tar.gz",
        gzip_sha256: "3f1215b2a3b88c7aaa008b561bd4f39d6c6672fa7821e562ab6ba1a6d6f37f61",
    },
    RustComponentArtifact {
        component: "rust-std",
        xz_url: "https://static.rust-lang.org/dist/2026-09-03/rust-std-1.98.1-x86_64-unknown-linux-gnu.tar.xz",
        xz_sha256: "fa3ff450172a16c026944030230c5069947af93c728d9179971d44e5e0cfb561",
        gzip_url: "https://static.rust-lang.org/dist/2026-09-03/rust-std-1.98.1-x86_64-unknown-linux-gnu.tar.gz",
        gzip_sha256: "eddab0358cbd12aeb897716aab00d1db7b59696e85b9ac4982e72259a9a976b1",
    },
    RustComponentArtifact {
        component: "clippy-preview",
        xz_url: "https://static.rust-lang.org/dist/2026-09-03/clippy-1.98.1-x86_64-unknown-linux-gnu.tar.xz",
        xz_sha256: "e167f333be24e1d5eea56ea563c7def0aa0bd613f5ce3445976c93b0288799d1",
        gzip_url: "https://static.rust-lang.org/dist/2026-09-03/clippy-1.98.1-x86_64-unknown-linux-gnu.tar.gz",
        gzip_sha256: "81ee497320fc54bab54f27692513808663a120496c27f4c02f60f10645dbc0cc",
    },
    RustComponentArtifact {
        component: "rustfmt-preview",
        xz_url: "https://static.rust-lang.org/dist/2026-09-03/rustfmt-1.98.1-x86_64-unknown-linux-gnu.tar.xz",
        xz_sha256: "b29a1addcbf2aa8f5785075605700e63cbe131b08c8334b50c09cfca8bbc51dc",
        gzip_url: "https://static.rust-lang.org/dist/2026-09-03/rustfmt-1.98.1-x86_64-unknown-linux-gnu.tar.gz",
        gzip_sha256: "16db0d6bb3535cfa84c009cffb161aac32d48f6e0648507a1f3368323cac3ec7",
    },
];

pub(super) const MANIFEST: OfficialRootRustManifest = OfficialRootRustManifest {
    version: "1.98.1",
    target: "x86_64-unknown-linux-gnu",
    manifest_url: "https://static.rust-lang.org/dist/channel-rust-1.98.1.toml",
    manifest_sha256: "a7c8774a5fd8441c997d94c029776cbc5eb111e9d72ab5d256fa69866644347e",
    release_date: "2026-09-03",
    rust_source_repository: "https://github.com/rust-lang/rust",
    rust_source_commit: "48a229ceaefd4985c50990b14116b6d856af0985",
    rust_source_tree: "22cb8c2037692834918e9320e63a6f87bb8d92ef",
    cargo_source_repository: "https://github.com/rust-lang/cargo",
    cargo_source_commit: "797e8a9bca276c1c9f9f738d2a20f484fa4eea9d",
    cargo_source_tree: "4007da55f36ad45f760b98668fc17807030ef22f",
    components: COMPONENTS,
};
