// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::oracle::{digest, lock_and_audit};
use super::{FLAT, PINNED, decision, merge, revision, world_over};
use crate::io::Repo;

const MACRO_MANIFEST: &str = "[package]\nname = \"ext\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[lib]\nproc-macro = true\n";
const MACRO_LIBRARY: &str = "use proc_macro::TokenStream;\n#[proc_macro]\npub fn baked(_: TokenStream) -> TokenStream {\n    let dir = std::env::var(\"CARGO_MANIFEST_DIR\").unwrap();\n    let text = std::fs::read_to_string(format!(\"{dir}/../../docs/goals.md\")).unwrap();\n    format!(\"{text:?}\").parse().unwrap()\n}\n";

fn hex(text: &str) -> String {
    velnor_rust::sha256_hex(text.as_bytes())
}

fn cargo_home(name: &str) -> Repo {
    let home = Repo::empty(name);
    let vendor = "vend";
    home.write_unstaged(
        "config.toml",
        &format!(
            "[source.crates-io]\nreplace-with = \"{vendor}\"\n\n[source.{vendor}]\ndirectory = \"{}\"\n",
            home.root().join("vendor").display()
        ),
    );
    home.write_unstaged("vendor/ext/Cargo.toml", MACRO_MANIFEST);
    home.write_unstaged("vendor/ext/src/lib.rs", MACRO_LIBRARY);
    home.write_unstaged(
        "vendor/ext/.cargo-checksum.json",
        &format!(
            "{{\"files\":{{\"Cargo.toml\":\"{}\",\"src/lib.rs\":\"{}\"}},\"package\":\"{}\"}}",
            hex(MACRO_MANIFEST),
            hex(MACRO_LIBRARY),
            hex("ext")
        ),
    );
    home
}

fn depend_on_the_macro(repo: &Repo) {
    repo.write_unstaged(
        "crates/engine/Cargo.toml",
        &format!(
            "{}\n[dependencies]\next = \"0.1\"\n",
            super::manifest("engine", None)
        ),
    );
    repo.write_unstaged(
        "crates/engine/src/lib.rs",
        "pub fn note() -> &'static str { include_str!(\"../../../docs/note.md\") }\npub fn baked() -> &'static str { ext::baked!() }\n",
    );
    repo.write_unstaged(
        "crates/velnor-gen/src/main.rs",
        "fn main() { println!(\"{} {}\", engine::note(), engine::baked()); }\n",
    );
}

#[test]
fn an_edit_a_registry_proc_macro_reads_never_skips_the_generated_check() {
    let home = cargo_home("gc-registry-home");
    let repo = world_over("gc-registry", FLAT, PINNED, |repo| {
        depend_on_the_macro(repo);
        lock_and_audit(repo, Some(home.root()));
    });
    merge(&repo, |repo| repo.write("docs/goals.md", "goals\nmore\n"));
    assert_eq!(decision(&repo), "run");
    let merged = revision(&repo, "HEAD");
    let before = digest(&repo, "HEAD^1", "gc-registry-before", Some(home.root()));
    let after = digest(&repo, &merged, "gc-registry-after", Some(home.root()));
    assert_ne!(before, "build failed");
    assert_ne!(before, after, "the proc-macro baked the edited file in");
}

fn add_a_local_twin(repo: &Repo) {
    repo.write_unstaged(
        "Cargo.toml",
        "[workspace]\nmembers = [\"crates/velnor-gen\", \"crates/engine\", \"crates/other\", \"crates/ext\"]\nresolver = \"3\"\n",
    );
    repo.write_unstaged(
        "crates/ext/Cargo.toml",
        "[package]\nname = \"ext\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    );
    repo.write_unstaged("crates/ext/src/lib.rs", "pub fn local() {}\n");
    repo.write_unstaged(
        "crates/velnor-gen/Cargo.toml",
        &format!(
            "{}ext = {{ path = \"../ext\" }}\n",
            super::manifest("velnor-gen", Some("engine"))
        ),
    );
}

#[test]
fn a_local_member_sharing_a_registry_proc_macros_name_and_version_does_not_hide_it() {
    let home = cargo_home("gc-twin-home");
    let repo = world_over("gc-twin", FLAT, PINNED, |repo| {
        depend_on_the_macro(repo);
        add_a_local_twin(repo);
        lock_and_audit(repo, Some(home.root()));
    });
    merge(&repo, |repo| repo.write("docs/goals.md", "goals\nmore\n"));
    assert_eq!(decision(&repo), "run");
}
