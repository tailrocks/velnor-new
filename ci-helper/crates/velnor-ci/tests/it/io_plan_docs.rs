// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::os::unix::fs::PermissionsExt;

use crate::io::Repo;
use crate::io_plan::{plan_command, plan_repo};

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
