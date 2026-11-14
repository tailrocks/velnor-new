// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs::{self};
use std::process::{Command, Output};

use crate::io::Repo;

fn plan_repo(name: &str) -> Repo {
    let repo = Repo::new(name);
    repo.write("mise.toml", "[tools]\nrust = \"1.99.1\"\n");
    repo.write(
        "Cargo.toml",
        "[workspace]\nmembers = [\"a\", \"b\"]\nexclude = [\"tools\"]\nresolver = \"3\"\n",
    );
    repo.write("Cargo.lock", "version = 4\n");
    for member in ["a", "b"] {
        repo.write(
            &format!("{member}/Cargo.toml"),
            &format!("[package]\nname = \"{member}\"\nedition = \"2024\"\n"),
        );
        repo.write(&format!("{member}/src/lib.rs"), "");
    }
    repo.write(
        "tools/Cargo.toml",
        "[package]\nname = \"t\"\nedition = \"2024\"\n\n[dependencies]\na = { path = \"../a\" }\n",
    );
    repo.write("tools/Cargo.lock", "version = 4\n");
    repo.write("tools/src/main.rs", "fn main() {}\n");
    repo
}

pub(crate) fn plan_command(repo: &Repo) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_velnor-ci"));
    command
        .current_dir(repo.root())
        .env_remove("GITHUB_OUTPUT")
        .env_remove("GITHUB_EVENT_NAME");
    command
}

pub(crate) fn selected(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| !line.starts_with("  left out: "))
        .flat_map(|line| [line, "\n"])
        .collect()
}

#[test]
fn plan_appends_the_outputs_of_every_root_to_github_output_and_explains_on_request() {
    let repo = plan_repo("plan");
    let outputs = repo.root().join("github-output");
    fs::write(&outputs, "earlier=kept\n").unwrap_or_else(|e| unreachable!("{e}"));
    let output = plan_command(&repo)
        .current_dir(repo.root().join("b"))
        .args(["plan", "--explain", "--changed", "a/src/lib.rs"])
        .env("GITHUB_OUTPUT", &outputs)
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));

    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        selected(&output),
        "Cargo.toml: some\n  a changed:a/src/lib.rs\ntools/Cargo.toml: all:most-packages:1\n  t depends-on:a\n"
    );
    assert_eq!(
        fs::read_to_string(&outputs).unwrap_or_else(|e| unreachable!("{e}")),
        "earlier=kept\nrust_root_mode=some\nrust_root_packages=a\nrust_root_test_filter=package(a)\nrust_root_doctests=skip\nrust_root_docs=skip\nrust_tools_mode=all\nrust_tools_packages=\nrust_tools_test_filter=all()\nrust_tools_doctests=skip\nrust_tools_docs=skip\n"
    );
}

fn doc_flags(repo: &Repo, event: Option<&str>) -> String {
    let outputs = repo.root().join("doc-flags");
    fs::write(&outputs, "").unwrap_or_else(|e| unreachable!("{e}"));
    let mut command = plan_command(repo);
    command
        .args(["plan", "--changed", "rust-toolchain.toml"])
        .env("GITHUB_OUTPUT", &outputs);
    if let Some(event) = event {
        command.env("GITHUB_EVENT_NAME", event);
    }
    let output = command.output().unwrap_or_else(|e| unreachable!("{e}"));
    assert!(output.status.success(), "{output:?}");
    fs::read_to_string(&outputs)
        .unwrap_or_else(|e| unreachable!("{e}"))
        .lines()
        .filter(|line| line.contains("_doc") || line.contains("_mode"))
        .flat_map(|line| [line, " "])
        .collect()
}

#[test]
fn plan_reads_each_roots_sources_to_decide_the_doctest_and_documentation_steps() {
    let repo = plan_repo("doc-flags");
    assert_eq!(
        doc_flags(&repo, None),
        "rust_root_mode=all rust_root_doctests=skip rust_root_docs=skip rust_tools_mode=all rust_tools_doctests=skip rust_tools_docs=skip "
    );
    repo.write(
        "b/src/lib.rs",
        "/// ```\n/// assert!(true);\n/// ```\npub fn f() {}\n",
    );
    repo.write("tools/src/main.rs", "//! The tool.\nfn main() {}\n");
    assert_eq!(
        doc_flags(&repo, Some("pull_request")),
        "rust_root_mode=all rust_root_doctests=run rust_root_docs=run rust_tools_mode=all rust_tools_doctests=skip rust_tools_docs=run "
    );
    repo.write("b/src/lib.rs", "pub fn f() {}\n");
    repo.write("tools/src/main.rs", "fn main() {}\n");
    assert_eq!(
        doc_flags(&repo, Some("schedule")),
        "rust_root_mode=all rust_root_doctests=run rust_root_docs=run rust_tools_mode=all rust_tools_doctests=skip rust_tools_docs=run "
    );
    let outputs = repo.root().join("scheduled");
    let output = plan_command(&repo)
        .arg("plan")
        .env("GITHUB_OUTPUT", &outputs)
        .env("GITHUB_EVENT_NAME", "schedule")
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(output.status.success(), "{output:?}");
    let written = fs::read_to_string(&outputs).unwrap_or_else(|e| unreachable!("{e}"));
    assert!(
        written.contains("rust_root_mode=all\n") && written.contains("rust_tools_mode=all\n"),
        "{written}"
    );
    repo.link("b/src/lib.rs", "b/src/linked.rs");
    assert_eq!(
        doc_flags(&repo, None),
        "rust_root_mode=all rust_root_doctests=run rust_root_docs=run rust_tools_mode=all rust_tools_doctests=skip rust_tools_docs=skip "
    );
}

fn explain(repo: &Repo, changed: &str) -> String {
    explain_all(repo, &[changed])
}

fn explain_all(repo: &Repo, changed: &[&str]) -> String {
    let output = plan_command(repo)
        .args(["plan", "--explain", "--changed"])
        .args(changed)
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(output.status.success(), "{output:?}");
    selected(&output)
}

#[test]
fn with_an_empty_config_a_root_readme_selects_nothing_and_a_toolchain_file_selects_everything() {
    let repo = plan_repo("classes");
    repo.write(".velnor/rust-ci.toml", "");
    repo.write("README.md", "# readme\n");
    assert_eq!(
        explain(&repo, "README.md"),
        "Cargo.toml: none\ntools/Cargo.toml: none\n"
    );
    assert_eq!(
        explain(&repo, "rust-toolchain.toml"),
        "Cargo.toml: all:toolchain-file:rust-toolchain.toml\ntools/Cargo.toml: all:toolchain-file:rust-toolchain.toml\n"
    );
    let unknown = explain(&repo, "scripts/gen.sh");
    assert!(
        unknown.starts_with(
            "Cargo.toml: all:unknown-path:scripts/gen.sh\ntools/Cargo.toml: all:unknown-path:scripts/gen.sh\n"
        ),
        "{unknown}"
    );
    assert_eq!(unknown.matches("[[inputs]]").count(), 1, "{unknown}");
    let masked = explain_all(
        &repo,
        &[".cargo/config.toml", "scripts/gen.sh", "tools/gen.sh"],
    );
    assert!(
        masked.contains("unknown path: scripts/gen.sh\n"),
        "{masked}"
    );
    assert!(masked.contains("unknown path: tools/gen.sh\n"), "{masked}");
    assert!(unknown.contains("non_inputs"), "{unknown}");
    assert!(unknown.contains("\"scripts/gen.sh\","), "{unknown}");
    assert!(unknown.contains("[[inputs]]"), "{unknown}");
}

fn plan_lock(fnv: &str) -> String {
    format!(
        "version = 4\n\n[[package]]\nname = \"a\"\nversion = \"0.0.0\"\n\n[[package]]\nname = \"b\"\nversion = \"0.0.0\"\ndependencies = [\"fnv\"]\n\n[[package]]\nname = \"fnv\"\nversion = \"{fnv}\"\nsource = \"registry+https://github.com/rust-lang/crates.io-index\"\nchecksum = \"sum-{fnv}\"\n"
    )
}

fn plan_with_bases(repo: &Repo, changed: &[&str], bases: &[(&str, &str)]) -> Output {
    let mut command = plan_command(repo);
    command
        .args(["plan", "--explain", "--changed"])
        .args(changed);
    for (path, text) in bases {
        let file = repo.root().join(format!("base-{}", path.replace('/', "-")));
        fs::write(&file, text).unwrap_or_else(|e| unreachable!("{e}"));
        command
            .arg("--base-file")
            .arg(format!("{path}={}", file.display()));
    }
    command.output().unwrap_or_else(|e| unreachable!("{e}"))
}

#[test]
fn plan_reads_the_old_text_of_a_lockfile_and_selects_only_the_members_whose_dependencies_changed() {
    let repo = plan_repo("lock");
    repo.write("Cargo.lock", &plan_lock("1.0.6"));
    let output = plan_with_bases(
        &repo,
        &["Cargo.lock"],
        &[("Cargo.lock", &plan_lock("1.0.5"))],
    );
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        selected(&output),
        "Cargo.toml: some\n  b lock:fnv\ntools/Cargo.toml: none\n"
    );
    let without_base = plan_with_bases(&repo, &["Cargo.lock"], &[]);
    assert_eq!(
        selected(&without_base),
        "Cargo.toml: all:workspace-manifest\ntools/Cargo.toml: all:workspace-manifest\n"
    );
    let missing = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .current_dir(repo.root())
        .args(["plan", "--changed", "Cargo.lock", "--base-file"])
        .arg("Cargo.lock=/nonexistent/old-lock")
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(!missing.status.success());
    assert!(
        String::from_utf8_lossy(&missing.stderr).contains("cannot read /nonexistent/old-lock"),
        "{missing:?}"
    );
}

#[test]
fn plan_leaves_a_root_manifest_that_only_changed_dependency_versions_to_the_lockfile() {
    let repo = plan_repo("manifest");
    let manifest = |version: &str| {
        format!(
            "[workspace]\nmembers = [\"a\", \"b\"]\nexclude = [\"tools\"]\nresolver = \"3\"\n\n[workspace.dependencies]\nfnv = \"{version}\"\n"
        )
    };
    repo.write("Cargo.toml", &manifest("1.0.6"));
    repo.write("Cargo.lock", &plan_lock("1.0.5"));
    let bases = [
        ("Cargo.toml", manifest("1.0.5")),
        ("Cargo.lock", plan_lock("1.0.5")),
    ];
    let bases: Vec<(&str, &str)> = bases.iter().map(|(p, t)| (*p, t.as_str())).collect();
    let output = plan_with_bases(&repo, &["Cargo.toml", "Cargo.lock"], &bases);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        selected(&output),
        "Cargo.toml: none\ntools/Cargo.toml: none\n"
    );
    let alone = plan_with_bases(&repo, &["Cargo.toml"], &bases[..1]);
    assert_eq!(
        selected(&alone),
        "Cargo.toml: all:workspace-manifest\ntools/Cargo.toml: all:workspace-manifest\n"
    );
    repo.write(
        "Cargo.toml",
        &manifest("1.0.6").replace("resolver = \"3\"", "resolver = \"2\""),
    );
    let output = plan_with_bases(&repo, &["Cargo.toml", "Cargo.lock"], &bases);
    assert_eq!(
        selected(&output),
        "Cargo.toml: all:workspace-manifest\ntools/Cargo.toml: all:workspace-manifest\n"
    );
}
