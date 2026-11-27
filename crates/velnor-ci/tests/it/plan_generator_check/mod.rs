// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::fs;
use std::process::{Command, Output};

use crate::io::Repo;
use crate::io_plan::plan_command;
use crate::plan_api::event;

mod merges;
mod oracle;
mod registry;
mod render;

pub(super) const FLAT: &str = "";
pub(super) const NESTED: &str = "tools/gen/";
pub(super) const PINNED: &str = "0.1.117";
pub(super) const CONFIG: &str = "full_run = \"off\"\n\n[[lint]]\nkind = 'generated-check'\nfrom_source = true\ngate_on_plan = true\n";
pub(super) const MISE: &str =
    include_str!("../../../../../generator/samples/feature-lane/view/mise.toml");
const MISE_LOCK: &str =
    include_str!("../../../../../generator/samples/feature-lane/view/mise.lock");
const HELPER: &str = include_str!("../../../../../.velnor/rust-ci.lock");

pub(super) fn git(repo: &Repo, args: &[&str]) {
    let mut full = vec![
        "-c",
        "user.name=lab",
        "-c",
        "user.email=lab@example.com",
        "-c",
        "commit.gpgsign=false",
    ];
    full.extend_from_slice(args);
    repo.git(&full);
}

pub(super) fn helper_lock(version: &str) -> String {
    let rest = HELPER.split_once('\n').map_or("", |(_, rest)| rest);
    format!("version = \"{version}\"\n{rest}")
}

pub(super) fn manifest(name: &str, dependency: Option<&str>) -> String {
    let dependencies = dependency.map_or_else(String::new, |name| {
        format!("\n[dependencies]\n{name} = {{ path = \"../{name}\" }}\n")
    });
    format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2024\"\n{dependencies}")
}

const LOCK: &str = "version = 4\n\n[[package]]\nname = \"engine\"\nversion = \"0.1.0\"\n\n[[package]]\nname = \"other\"\nversion = \"0.1.0\"\n\n[[package]]\nname = \"velnor-gen\"\nversion = \"0.1.0\"\ndependencies = [\n \"engine\",\n]\n";

pub(super) fn files(prefix: &str, pin: &str) -> Vec<(String, String)> {
    let inputs = format!(
        "docs/note.md\tengine\t{prefix}crates/engine/Cargo.toml\nshared/borrowed.rs\tengine\t{prefix}crates/engine/Cargo.toml\n"
    );
    let own = |path: &str| format!("{prefix}{path}");
    let up = "../".repeat(3 + prefix.matches('/').count());
    vec![
        (".velnor/rust-ci.toml".into(), CONFIG.into()),
        (".velnor/rust-ci.lock".into(), helper_lock(pin)),
        (".velnor/rust-inputs.lock".into(), inputs),
        ("mise.toml".into(), "[tools]\nrust = \"1.99.1\"\n".into()),
        ("mise.lock".into(), MISE_LOCK.into()),
        (
            own("Cargo.toml"),
            "[workspace]\nmembers = [\"crates/velnor-gen\", \"crates/engine\", \"crates/other\"]\nresolver = \"3\"\n".into(),
        ),
        (own("Cargo.lock"), LOCK.into()),
        (own("crates/velnor-gen/Cargo.toml"), manifest("velnor-gen", Some("engine"))),
        (own("crates/velnor-gen/src/main.rs"), "fn main() { println!(\"{}\", engine::note()); }\n".into()),
        (own("crates/engine/Cargo.toml"), manifest("engine", None)),
        (
            own("crates/engine/src/lib.rs"),
            format!("pub fn note() -> &'static str {{ include_str!(\"{up}docs/note.md\") }}\n#[path = \"{up}shared/borrowed.rs\"]\nmod borrowed;\npub fn n() -> u32 {{ borrowed::n() }}\n"),
        ),
        (own("crates/other/Cargo.toml"), manifest("other", None)),
        (own("crates/other/src/lib.rs"), "pub fn o() -> u32 { 1 }\n".into()),
        ("docs/note.md".into(), "note\n".into()),
        ("docs/goals.md".into(), "goals\n".into()),
        ("shared/borrowed.rs".into(), "pub fn n() -> u32 { 1 }\n".into()),
        ("README.md".into(), "readme\n".into()),
        (own("crates/other/src/old.rs"), "pub fn old() {}\n".into()),
    ]
}

pub(super) fn world(name: &str, prefix: &str, pin: &str) -> Repo {
    world_over(name, prefix, pin, |_| ())
}

pub(super) fn world_over(name: &str, prefix: &str, pin: &str, base: impl FnOnce(&Repo)) -> Repo {
    let repo = Repo::new(name);
    for (path, text) in files(prefix, pin) {
        repo.write_unstaged(&path, &text);
    }
    base(&repo);
    repo.git(&["add", "--all"]);
    repo.commit();
    repo.git(&["branch", "-M", "main"]);
    repo
}

pub(super) fn merge(repo: &Repo, edit: impl FnOnce(&Repo)) {
    repo.git(&["checkout", "--quiet", "-b", "topic"]);
    edit(repo);
    repo.git(&["add", "--all"]);
    repo.commit();
    repo.git(&["checkout", "--quiet", "main"]);
    repo.write("docs/later.md", "later\n");
    repo.commit();
    git(
        repo,
        &["merge", "--quiet", "--no-ff", "--message", "merge", "topic"],
    );
}

pub(super) fn remove(repo: &Repo, path: &str) {
    fs::remove_file(repo.root().join(path)).unwrap_or_else(|e| unreachable!("{e}"));
}

pub(super) fn revision(repo: &Repo, name: &str) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo.root())
        .args(["rev-parse", name])
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

pub(super) fn plan(repo: &Repo) -> (Output, BTreeMap<String, String>) {
    let outputs = repo.root().join("github-output");
    fs::write(&outputs, "").unwrap_or_else(|e| unreachable!("{e}"));
    let output = plan_command(repo)
        .args(["plan", "--explain"])
        .env("GITHUB_EVENT_NAME", "pull_request")
        .env("GITHUB_EVENT_PATH", event(repo, "main"))
        .env("GITHUB_SHA", revision(repo, "HEAD"))
        .env("GITHUB_OUTPUT", &outputs)
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(output.status.success(), "{output:?}");
    let written = fs::read_to_string(&outputs).unwrap_or_else(|e| unreachable!("{e}"));
    let values = written
        .lines()
        .filter_map(|line| line.split_once('='))
        .map(|(key, value)| (key.to_owned(), value.to_owned()))
        .collect();
    (output, values)
}

pub(super) fn decision(repo: &Repo) -> String {
    decided(repo).0
}

pub(super) fn decided(repo: &Repo) -> (String, String) {
    let (output, values) = plan(repo);
    let decision = values
        .get("lint_generated_check")
        .cloned()
        .unwrap_or_else(|| "absent".to_owned());
    let explanation = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    (decision, explanation)
}
