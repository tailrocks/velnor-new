// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::Path;
use std::process::Command;

use super::{FLAT, PINNED, decided, git, merge, revision, world_over};
use crate::io::Repo;
use crate::toolchain::pinned_channel;

type Edit = fn(&Repo);

const VENDORED_LIB: &str = "pub fn foo() -> u32 { 1 }\n";
const CONFIG_TOML: &str = "[source.crates-io]\nreplace-with = \"vendored-sources\"\n\n[source.vendored-sources]\ndirectory = \"vendor\"\n";

fn hex(bytes: &[u8]) -> String {
    velnor_rust::sha256_hex(bytes)
}

fn vendor(repo: &Repo, library: &str) {
    let files = format!(
        "{{\"src/lib.rs\":\"{}\",\"Cargo.toml\":\"{}\"}}",
        hex(library.as_bytes()),
        hex(FOO_MANIFEST.as_bytes())
    );
    repo.write_unstaged("vendor/foo/Cargo.toml", FOO_MANIFEST);
    repo.write_unstaged("vendor/foo/src/lib.rs", library);
    repo.write_unstaged(
        "vendor/foo/.cargo-checksum.json",
        &format!("{{\"files\":{files},\"package\":\"{}\"}}", hex(b"foo")),
    );
}

const FOO_MANIFEST: &str = "[package]\nname = \"foo\"\nversion = \"1.0.0\"\nedition = \"2024\"\n";

const INHERITED: [&str; 5] = [
    "HOME",
    "CARGO_HOME",
    "RUSTUP_HOME",
    "RUSTUP_TOOLCHAIN",
    "TMPDIR",
];

pub(super) fn isolated(program: &str, home: Option<&Path>) -> Command {
    let mut command = Command::new(program);
    command.env_clear();
    for name in INHERITED {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
    if let Some(home) = home {
        command.env("CARGO_HOME", home);
    }
    command
}

pub(super) fn toolchain_path() -> String {
    let found = Command::new("rustup")
        .args(["which", "--toolchain", pinned_channel(), "cargo"])
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    let cargo = String::from_utf8_lossy(&found.stdout).trim().to_owned();
    let bin = std::path::Path::new(&cargo)
        .parent()
        .unwrap_or_else(|| unreachable!("{cargo}"));
    format!(
        "{}:{}",
        bin.display(),
        std::env::var("PATH").unwrap_or_default()
    )
}

fn cargo(repo: &Repo, home: Option<&Path>, args: &[&str]) {
    let output = isolated("cargo", home)
        .current_dir(repo.root())
        .env("PATH", toolchain_path())
        .args(args)
        .arg("--offline")
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(
        output.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn audited(repo: &Repo) {
    audit_world(repo, false);
}

fn audited_vendored(repo: &Repo) {
    audit_world(repo, true);
}

fn audit_world(repo: &Repo, vendored: bool) {
    let vendored_call = if vendored {
        repo.write_unstaged(".cargo/config.toml", CONFIG_TOML);
        repo.write_unstaged(
            "Cargo.toml",
            "[workspace]\nmembers = [\"crates/velnor-gen\", \"crates/engine\", \"crates/other\"]\nexclude = [\"vendor\"]\nresolver = \"3\"\n",
        );
        repo.write_unstaged(
            ".velnor/rust-ci.toml",
            &format!("exclude = [\"vendor/foo/Cargo.toml\"]\n{}", super::CONFIG),
        );
        vendor(repo, VENDORED_LIB);
        repo.write_unstaged(
            "crates/engine/Cargo.toml",
            &format!(
                "{}\n[dependencies]\nfoo = \"1\"\n",
                super::manifest("engine", None)
            ),
        );
        " + foo::foo()"
    } else {
        ""
    };
    repo.write_unstaged(
        "crates/engine/src/lib.rs",
        &format!("pub fn note() -> &'static str {{ include_str!(\"../../../docs/note.md\") }}\n#[path = \"../../../shared/borrowed.rs\"]\nmod borrowed;\n#[path = \"../../other/src/borrowed_from_other.rs\"]\nmod from_other;\npub fn n() -> u32 {{ borrowed::n() + from_other::m(){vendored_call} }}\n"),
    );
    repo.write_unstaged(
        "shared/borrowed.rs",
        "mod child;\npub fn n() -> u32 { child::c() }\n",
    );
    repo.write_unstaged("shared/child.rs", "pub fn c() -> u32 { 1 }\n");
    repo.write_unstaged(
        "crates/velnor-gen/src/main.rs",
        "fn main() { println!(\"{} {}\", engine::note(), engine::n()); }\n",
    );
    repo.write_unstaged(
        "crates/other/src/borrowed_from_other.rs",
        "pub fn m() -> u32 { 1 }\n",
    );
    lock_and_audit(repo, None);
}

pub(super) fn lock_and_audit(repo: &Repo, home: Option<&Path>) {
    repo.git(&["add", "--all"]);
    cargo(repo, home, &["generate-lockfile"]);
    repo.git(&["add", "--all"]);
    for command in [
        ["clippy", "--workspace", "--all-targets"],
        ["build", "--workspace", "--all-targets"],
    ] {
        let output = isolated(env!("CARGO_BIN_EXE_velnor-ci"), home)
            .current_dir(repo.root())
            .env("PATH", toolchain_path())
            .args(["inputs", "build", "--", "cargo"])
            .args(command)
            .arg("--offline")
            .output()
            .unwrap_or_else(|e| unreachable!("{e}"));
        assert!(
            output.status.success(),
            "{command:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let written = isolated(env!("CARGO_BIN_EXE_velnor-ci"), home)
        .current_dir(repo.root())
        .env("PATH", toolchain_path())
        .args(["inputs", "audit", "--write"])
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(written.status.success(), "{written:?}");
}

pub(super) fn digest(repo: &Repo, revision: &str, name: &str, home: Option<&Path>) -> String {
    git(repo, &["checkout", "--quiet", "--detach", revision]);
    let target = Repo::empty(name);
    let built = isolated("cargo", home)
        .current_dir(repo.root())
        .args(["build", "--offline", "-p", "velnor-gen"])
        .env("CARGO_TARGET_DIR", target.root())
        .env("CARGO_INCREMENTAL", "0")
        .env("CARGO_PROFILE_DEV_DEBUG", "0")
        .env("ZERO_AR_DATE", "1")
        .env("PATH", toolchain_path())
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    if !built.status.success() {
        return "build failed".to_owned();
    }
    let binary =
        fs::read(target.root().join("debug/velnor-gen")).unwrap_or_else(|e| unreachable!("{e}"));
    hex(&binary)
}

fn corpus() -> Vec<(&'static str, bool, Edit)> {
    vec![
        ("goals", false, |repo| {
            repo.write("docs/goals.md", "goals\nmore\n");
        }),
        ("readme", false, |repo| {
            repo.write("README.md", "readme\nmore\n");
        }),
        ("plain-rs", false, |repo| {
            repo.write("crates/other/src/lib.rs", "pub fn o() -> u32 { 7 }\n");
        }),
        ("engine", true, |repo| {
            let file = "crates/engine/src/lib.rs";
            let text = fs::read_to_string(repo.root().join(file)).unwrap_or_default();
            repo.write(
                file,
                &text.replace("from_other::m() }", "from_other::m() + 1 }"),
            );
        }),
        ("embedded", true, |repo| {
            repo.write("docs/note.md", "note\nmore\n");
        }),
        ("borrowed", true, |repo| {
            repo.write(
                "shared/borrowed.rs",
                "mod child;\npub fn n() -> u32 { child::c() + 1 }\n",
            );
        }),
        ("child", true, |repo| {
            repo.write("shared/child.rs", "pub fn c() -> u32 { 5 }\n");
        }),
        ("from-other", true, |repo| {
            repo.write(
                "crates/other/src/borrowed_from_other.rs",
                "pub fn m() -> u32 { 4 }\n",
            );
        }),
    ]
}

fn compare(repo: &Repo, name: &str) -> (String, String, String, String) {
    let (decided, why) = decided(repo);
    let merged = revision(repo, "HEAD");
    let before = digest(repo, "HEAD^1", &format!("gc-oracle-before-{name}"), None);
    let after = digest(repo, &merged, &format!("gc-oracle-after-{name}"), None);
    assert_ne!(before, "build failed", "{name}: the base builds");
    (decided, why, before, after)
}

#[test]
fn a_skip_implies_the_generator_artifact_is_fresh() {
    let mut skipped = Vec::new();
    for (name, changes, edit) in corpus() {
        let repo = world_over(&format!("gc-oracle-{name}"), FLAT, PINNED, audited);
        merge(&repo, edit);
        let (decided, why, before, after) = compare(&repo, name);
        if changes {
            assert_ne!(before, after, "{name}: the control changes the artifact");
        }
        if decided == "skip" {
            assert_eq!(
                before, after,
                "{name}: a skip must leave the artifact unchanged"
            );
            skipped.push(name);
        }
        assert!(changes || decided == "skip", "{name} should skip: {why}");
    }
    assert_eq!(skipped, ["goals", "readme", "plain-rs"]);
}

#[test]
fn an_edit_inside_a_vendored_source_never_skips_and_breaks_the_cold_build() {
    for (name, edit) in [
        (
            "vendored",
            (|repo: &Repo| repo.write("vendor/foo/src/lib.rs", "pub fn foo() -> u32 { 3 }\n"))
                as Edit,
        ),
        ("vendored-goals", |repo: &Repo| {
            repo.write("docs/goals.md", "goals\nmore\n");
        }),
    ] {
        let repo = world_over(&format!("gc-oracle-{name}"), FLAT, PINNED, audited_vendored);
        merge(&repo, edit);
        assert_eq!(decided(&repo).0, "run", "{name}");
    }
    let repo = world_over("gc-oracle-vendored-build", FLAT, PINNED, audited_vendored);
    merge(&repo, |repo| {
        repo.write("vendor/foo/src/lib.rs", "pub fn foo() -> u32 { 3 }\n");
    });
    let merged = revision(&repo, "HEAD");
    assert_ne!(
        digest(&repo, "HEAD^1", "gc-oracle-vendored-before", None),
        "build failed"
    );
    assert_eq!(
        digest(&repo, &merged, "gc-oracle-vendored-after", None),
        "build failed"
    );
}

#[test]
fn the_nested_cargo_inherits_only_the_named_variables() {
    let output = isolated("env", None)
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    let text = String::from_utf8_lossy(&output.stdout);
    let names: Vec<&str> = text
        .lines()
        .filter_map(|line| line.split_once('=').map(|(name, _)| name))
        .filter(|name| !INHERITED.contains(name))
        .collect();
    assert_eq!(names, Vec::<&str>::new());
}
