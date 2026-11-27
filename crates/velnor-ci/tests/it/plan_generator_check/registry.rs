// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::oracle::{digest, lock_and_audit};
use super::{FLAT, PINNED, decision, merge, revision, world_over};
use crate::io::Repo;

fn macro_manifest(name: &str) -> String {
    format!(
        "[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[lib]\nproc-macro = true\n"
    )
}

const MACRO_LIBRARY: &str = "use proc_macro::TokenStream;\n#[proc_macro]\npub fn baked(_: TokenStream) -> TokenStream {\n    let dir = std::env::var(\"CARGO_MANIFEST_DIR\").unwrap();\n    let text = std::fs::read_to_string(format!(\"{dir}/../../docs/goals.md\")).unwrap();\n    format!(\"{text:?}\").parse().unwrap()\n}\n";

fn hex(text: &str) -> String {
    velnor_rust::sha256_hex(text.as_bytes())
}

const EVIL: &str = "sparse+https://evil.example/index/";
const CRATES_IO: &str = "registry+https://github.com/rust-lang/crates.io-index";

fn cargo_home(name: &str) -> Repo {
    vendored_home(name, "[source.crates-io]\nreplace-with = \"vend\"\n")
}

fn evil_home(name: &str) -> Repo {
    vendored_home(
        name,
        &format!(
            "[registries.evil]\nindex = \"{EVIL}\"\n\n[source.evil]\nregistry = \"{EVIL}\"\nreplace-with = \"vend\"\n"
        ),
    )
}

fn vendored_home(name: &str, redirect: &str) -> Repo {
    let home = Repo::empty(name);
    home.write_unstaged(
        "config.toml",
        &format!(
            "{redirect}\n[source.vend]\ndirectory = \"{}\"\n",
            home.root().join("vendor").display()
        ),
    );
    vendor_macro(&home, "ext");
    vendor_macro(&home, "dep");
    home
}

fn vendor_macro(home: &Repo, name: &str) {
    let manifest = macro_manifest(name);
    home.write_unstaged(&format!("vendor/{name}/Cargo.toml"), &manifest);
    home.write_unstaged(&format!("vendor/{name}/src/lib.rs"), MACRO_LIBRARY);
    home.write_unstaged(
        &format!("vendor/{name}/.cargo-checksum.json"),
        &format!(
            "{{\"files\":{{\"Cargo.toml\":\"{}\",\"src/lib.rs\":\"{}\"}},\"package\":\"{}\"}}",
            hex(&manifest),
            hex(MACRO_LIBRARY),
            hex(name)
        ),
    );
}

fn depend_on_the_macro(repo: &Repo) {
    depend_on(repo, "\"0.1\"");
}

fn depend_on(repo: &Repo, requirement: &str) {
    repo.write_unstaged(
        "crates/engine/Cargo.toml",
        &format!(
            "{}\n[dependencies]\next = {requirement}\n",
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

fn list_by_name_and(repo: &Repo, source: &str) {
    repo.write_unstaged(
        ".velnor/generator-externals.lock",
        &format!("ext 0.1.0\next 0.1.0 {source}\n"),
    );
}

#[test]
fn a_package_from_another_registry_never_counts_as_listed() {
    let home = evil_home("gc-evil-home");
    let repo = world_over("gc-evil", FLAT, PINNED, |repo| {
        depend_on(repo, "{ version = \"0.1\", registry = \"evil\" }");
        repo.write_unstaged(
            ".cargo/config.toml",
            &format!("[registries.evil]\nindex = \"{EVIL}\"\n"),
        );
        lock_and_audit(repo, Some(home.root()));
        list_by_name_and(repo, EVIL);
    });
    merge(&repo, |repo| repo.write("docs/goals.md", "goals\nmore\n"));
    assert_eq!(decision(&repo), "run");
}

#[test]
fn a_replaced_package_never_counts_as_listed() {
    let home = cargo_home("gc-replace-home");
    let repo = world_over("gc-replace", FLAT, PINNED, |repo| {
        depend_on_the_macro(repo);
        repo.write_unstaged(
            "Cargo.toml",
            "[workspace]\nmembers = [\"crates/velnor-gen\", \"crates/engine\", \"crates/other\", \"crates/ext\"]\nresolver = \"3\"\n\n[replace]\n\"ext:0.1.0\" = { path = \"crates/ext\" }\n",
        );
        repo.write_unstaged(
            "crates/ext/Cargo.toml",
            "[package]\nname = \"ext\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\ndep = \"0.1\"\n",
        );
        repo.write_unstaged("crates/ext/src/lib.rs", "pub use dep::baked;\n");
        lock_and_audit(repo, Some(home.root()));
        list_by_name_and(repo, CRATES_IO);
    });
    merge(&repo, |repo| repo.write("docs/goals.md", "goals\nmore\n"));
    assert_eq!(decision(&repo), "run");
}
