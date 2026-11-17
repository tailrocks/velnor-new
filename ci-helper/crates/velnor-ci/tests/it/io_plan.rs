// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs::{self};
use std::os::unix::fs::PermissionsExt;
use std::process::{Command, Output};

use crate::io::Repo;

pub(crate) fn plan_repo(name: &str) -> Repo {
    let repo = Repo::new(name);
    repo.write_all(&[
        ("mise.toml", "[tools]\nrust = \"1.99.1\"\n"),
        (
            "Cargo.toml",
            "[workspace]\nmembers = [\"a\", \"b\"]\nexclude = [\"tools\"]\nresolver = \"3\"\n",
        ),
        ("Cargo.lock", "version = 4\n"),
        ("a/Cargo.toml", "[package]\nname = \"a\"\nedition = \"2024\"\n"),
        ("a/src/lib.rs", ""),
        ("b/Cargo.toml", "[package]\nname = \"b\"\nedition = \"2024\"\n"),
        ("b/src/lib.rs", ""),
        (
            "tools/Cargo.toml",
            "[package]\nname = \"t\"\nedition = \"2024\"\n\n[dependencies]\na = { path = \"../a\" }\n",
        ),
        ("tools/Cargo.lock", "version = 4\n"),
        ("tools/src/main.rs", "fn main() {}\n"),
    ]);
    repo
}

static METADATA: std::sync::Mutex<()> = std::sync::Mutex::new(());

pub(crate) fn plan_command(repo: &Repo) -> Command {
    let writing = METADATA
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    velnor_gen::io::write_metadata(repo.root()).unwrap_or_else(|e| unreachable!("{e}"));
    drop(writing);
    stale_plan_command(repo)
}

pub(crate) fn stale_plan_command(repo: &Repo) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_velnor-ci"));
    command.current_dir(repo.root());
    for variable in [
        "GITHUB_OUTPUT",
        "GITHUB_STEP_SUMMARY",
        "GITHUB_EVENT_NAME",
        "GITHUB_EVENT_PATH",
        "GITHUB_REPOSITORY",
        "GITHUB_TOKEN",
        "GITHUB_API_URL",
    ] {
        command.env_remove(variable);
    }
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
        "earlier=kept\nrust_root_mode=some\nrust_root_packages=a\nrust_root_test_filter=package(a)\nrust_root_package_args=-p a\nrust_root_doctests=skip\nrust_root_doctests_known=false\nrust_root_docs=skip\nrust_tools_mode=all\nrust_tools_packages=\nrust_tools_test_filter=all()\nrust_tools_package_args=--workspace\nrust_tools_doctests=skip\nrust_tools_doctests_known=false\nrust_tools_docs=run\nrust_deny=skip\nrust_machete=skip\nrust_root_tests=run\nrust_tools_tests=run\n"
    );
}

fn doc_flags(repo: &Repo, event: Option<&str>) -> String {
    doc_flags_of(repo, event, "rust-toolchain.toml")
}

fn doc_flags_of(repo: &Repo, event: Option<&str>, changed: &str) -> String {
    let outputs = repo
        .root()
        .join(format!("doc-flags-{}", event.unwrap_or("none")));
    fs::write(&outputs, "").unwrap_or_else(|e| unreachable!("{e}"));
    let mut command = plan_command(repo);
    command
        .args(["plan", "--changed", changed])
        .env("GITHUB_OUTPUT", &outputs);
    if let Some(event) = event {
        command.env("GITHUB_EVENT_NAME", event);
    }
    let output = command.output().unwrap_or_else(|e| unreachable!("{e}"));
    assert!(output.status.success(), "{output:?}");
    fs::read_to_string(&outputs)
        .unwrap_or_else(|e| unreachable!("{e}"))
        .lines()
        .filter(|line| {
            (line.contains("_doc") && !line.contains("_known")) || line.contains("_mode")
        })
        .flat_map(|line| [line, " "])
        .collect()
}

const SCAN_ALL: &str = "rust_root_mode=all rust_root_doctests=run rust_root_docs=run rust_tools_mode=all rust_tools_doctests=skip rust_tools_docs=run ";
const DOCS_ALL: &str = "rust_root_mode=all rust_root_doctests=run rust_root_docs=run rust_tools_mode=all rust_tools_doctests=skip rust_tools_docs=run ";

#[test]
fn plan_reads_each_roots_sources_to_decide_the_doctest_and_documentation_steps() {
    let plain = plan_repo("doc-flags");
    let minimal = plain.fork("doc-flags-minimal");
    minimal.write_unstaged(
        "tools/Cargo.toml",
        "[package]\nname = \"t\"\nedition = \"2024\"\n",
    );
    let documented = plain.fork("doc-flags-documented");
    documented.write_unstaged(
        "tools/Cargo.toml",
        "[package]\nname = \"t\"\nedition = \"2024\"\n\n[dependencies]\na = { path = \"../a\" }\n",
    );
    documented.write_unstaged(
        "b/src/lib.rs",
        "/// ```\n/// assert!(true);\n/// ```\npub fn f() {}\n",
    );
    documented.write_unstaged("tools/src/main.rs", "//! The tool.\nfn main() {}\n");
    let linked = plain.fork("doc-flags-linked");
    linked.link("b/src/lib.rs", "b/src/linked.rs");
    let outputs = plain.root().join("scheduled");
    let (untouched, pull_request, scheduled, bare, linked_flags, minimal_flags) =
        std::thread::scope(|scope| {
            let pull_request = scope.spawn(|| doc_flags(&documented, Some("pull_request")));
            let linked_flags = scope.spawn(|| doc_flags(&linked, None));
            let minimal_flags = scope.spawn(|| doc_flags(&minimal, None));
            let scheduled = scope.spawn(|| doc_flags(&plain, Some("schedule")));
            let bare = scope.spawn(|| {
                plan_command(&plain)
                    .arg("plan")
                    .env("GITHUB_OUTPUT", &outputs)
                    .env("GITHUB_EVENT_NAME", "schedule")
                    .output()
                    .unwrap_or_else(|e| unreachable!("{e}"))
            });
            let untouched = doc_flags(&plain, None);
            (
                untouched,
                pull_request.join().unwrap_or_else(|_| unreachable!()),
                scheduled.join().unwrap_or_else(|_| unreachable!()),
                bare.join().unwrap_or_else(|_| unreachable!()),
                linked_flags.join().unwrap_or_else(|_| unreachable!()),
                minimal_flags.join().unwrap_or_else(|_| unreachable!()),
            )
        });
    assert_eq!(
        untouched,
        "rust_root_mode=all rust_root_doctests=skip rust_root_docs=skip rust_tools_mode=all rust_tools_doctests=skip rust_tools_docs=run "
    );
    assert_eq!(pull_request, DOCS_ALL);
    assert_eq!(scheduled, DOCS_ALL);
    assert!(bare.status.success(), "{bare:?}");
    let written = fs::read_to_string(&outputs).unwrap_or_else(|e| unreachable!("{e}"));
    assert!(
        written.contains("rust_root_mode=all\n") && written.contains("rust_tools_mode=all\n"),
        "{written}"
    );
    assert_eq!(linked_flags, SCAN_ALL);
    assert_eq!(
        minimal_flags,
        "rust_root_mode=all rust_root_doctests=skip rust_root_docs=skip rust_tools_mode=all rust_tools_doctests=skip rust_tools_docs=skip "
    );
}

#[test]
fn plan_reads_no_file_when_a_package_reads_the_whole_repository() {
    let repo = plan_repo("doc-flags-whole");
    repo.write(".velnor/rust-inputs.lock", ".\ta\n");
    repo.write("logo.bin", "x");
    let guarded = repo.root().join("logo.bin");
    fs::set_permissions(&guarded, fs::Permissions::from_mode(0o000))
        .unwrap_or_else(|e| unreachable!("{e}"));
    let flags = doc_flags(&repo, None);
    fs::set_permissions(&guarded, fs::Permissions::from_mode(0o644))
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(
        flags,
        "rust_root_mode=all rust_root_doctests=run rust_root_docs=run rust_tools_mode=all rust_tools_doctests=skip rust_tools_docs=run "
    );
}

#[test]
fn plan_treats_a_source_over_the_size_bound_as_possibly_holding_docs() {
    let repo = plan_repo("doc-flags-large");
    repo.write("a/src/lib.rs", &"// pad\n".repeat(200_000));
    assert_eq!(
        doc_flags(&repo, None),
        "rust_root_mode=all rust_root_doctests=run rust_root_docs=run rust_tools_mode=all rust_tools_doctests=skip rust_tools_docs=run "
    );
}

#[test]
fn plan_treats_sources_over_the_total_size_bound_as_possibly_holding_docs() {
    let repo = plan_repo("doc-flags-total");
    for n in 0..10 {
        repo.write(&format!("a/src/m{n}.rs"), &"// pad\n".repeat(140_000));
    }
    assert_eq!(
        doc_flags(&repo, None),
        "rust_root_mode=all rust_root_doctests=run rust_root_docs=run rust_tools_mode=all rust_tools_doctests=skip rust_tools_docs=run "
    );
}

#[test]
fn plan_reads_no_source_of_a_root_that_selects_nothing() {
    let repo = plan_repo("doc-flags-unselected");
    assert_eq!(
        doc_flags_of(&repo, None, "tools/src/main.rs"),
        "rust_root_mode=none rust_root_doctests=run rust_root_docs=run rust_tools_mode=all rust_tools_doctests=skip rust_tools_docs=run "
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
    repo.write_all(&[(".velnor/rust-ci.toml", ""), ("README.md", "# readme\n")]);
    let (readme, toolchain, unknown, masked) = std::thread::scope(|scope| {
        let readme = scope.spawn(|| explain(&repo, "README.md"));
        let toolchain = scope.spawn(|| explain(&repo, "rust-toolchain.toml"));
        let masked = scope.spawn(|| {
            explain_all(
                &repo,
                &[".cargo/config.toml", "scripts/gen.sh", "tools/gen.sh"],
            )
        });
        let unknown = explain(&repo, "scripts/gen.sh");
        let join = |handle: std::thread::ScopedJoinHandle<'_, String>| {
            handle.join().unwrap_or_else(|_| unreachable!())
        };
        (join(readme), join(toolchain), unknown, join(masked))
    });
    assert_eq!(readme, "Cargo.toml: none\ntools/Cargo.toml: none\n");
    assert_eq!(
        toolchain,
        "Cargo.toml: all:toolchain-file:rust-toolchain.toml\ntools/Cargo.toml: all:toolchain-file:rust-toolchain.toml\n"
    );
    assert!(
        unknown.starts_with(
            "Cargo.toml: all:unknown-path:scripts/gen.sh\ntools/Cargo.toml: all:unknown-path:scripts/gen.sh\n"
        ),
        "{unknown}"
    );
    assert_eq!(unknown.matches("[[inputs]]").count(), 1, "{unknown}");
    assert!(
        masked.contains("unknown path: scripts/gen.sh\n"),
        "{masked}"
    );
    assert!(masked.contains("unknown path: tools/gen.sh\n"), "{masked}");
    assert!(unknown.contains("non_inputs"), "{unknown}");
    assert!(unknown.contains("\"scripts/gen.sh\","), "{unknown}");
    assert!(unknown.contains("[[inputs]]"), "{unknown}");
}

pub(crate) fn plan_lock(fnv: &str) -> String {
    format!(
        "version = 4\n\n[[package]]\nname = \"a\"\nversion = \"0.0.0\"\n\n[[package]]\nname = \"b\"\nversion = \"0.0.0\"\ndependencies = [\"fnv\"]\n\n[[package]]\nname = \"fnv\"\nversion = \"{fnv}\"\nsource = \"registry+https://github.com/rust-lang/crates.io-index\"\nchecksum = \"sum-{fnv}\"\n"
    )
}

pub(crate) fn plan_with_bases(repo: &Repo, changed: &[&str], bases: &[(&str, &str)]) -> Output {
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

#[test]
fn plan_says_a_doctest_is_known_only_when_a_fenced_example_was_found() {
    let repo = plan_repo("doctests-known");
    let known = |repo: &Repo| {
        let outputs = repo.root().join("known");
        fs::write(&outputs, "").unwrap_or_else(|e| unreachable!("{e}"));
        let output = plan_command(repo)
            .args(["plan", "--changed", "rust-toolchain.toml"])
            .env("GITHUB_OUTPUT", &outputs)
            .output()
            .unwrap_or_else(|e| unreachable!("{e}"));
        assert!(output.status.success(), "{output:?}");
        fs::read_to_string(&outputs)
            .unwrap_or_else(|e| unreachable!("{e}"))
            .lines()
            .filter(|line| line.contains("_doctests"))
            .flat_map(|line| [line, " "])
            .collect::<String>()
    };
    repo.write("a/src/lib.rs", "/// Adds one.\npub fn f() {}\n");
    assert_eq!(
        known(&repo),
        "rust_root_doctests=skip rust_root_doctests_known=false rust_tools_doctests=skip rust_tools_doctests_known=false "
    );
    repo.write(
        "tools/Cargo.toml",
        "[package]\nname = \"t\"\nedition = \"2024\"\n\n[dependencies]\na = { path = \"../a\" }\nserde_derive = \"1\"\n",
    );
    repo.write(
        "a/src/lib.rs",
        "/// ```\n/// assert!(true);\n/// ```\npub fn f() {}\n",
    );
    assert!(
        known(&repo).contains("rust_root_doctests=run rust_root_doctests_known=true "),
        "{}",
        known(&repo)
    );
}
