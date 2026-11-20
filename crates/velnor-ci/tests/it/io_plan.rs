// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs::{self};
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
    let missing = stale_plan_command(&repo)
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
fn a_velnor_gen_owned_workflow_selects_nothing_while_rust_ci_and_a_hand_written_workflow_do_not() {
    const HEADER: &str =
        "# Generated by velnor-gen. Change .velnor/workflows.toml and run `velnor-gen generate`.\n";
    let sealed = velnor_render::seal(HEADER, "name: Generated\n");
    let repo = plan_repo("generated-workflows");
    repo.write_all(&[
        (".github/workflows/release.yml", &sealed),
        (".github/workflows/rust-ci.yml", &sealed),
        (".github/workflows/lint.yml", "name: Lint\n"),
    ]);
    let release = plan_with_bases(&repo, &[".github/workflows/release.yml"], &[]);
    assert_eq!(
        selected(&release),
        "Cargo.toml: none\ntools/Cargo.toml: none\n"
    );
    let deleted = plan_with_bases(
        &repo,
        &[".github/workflows/gone.yml"],
        &[(".github/workflows/gone.yml", &sealed)],
    );
    assert_eq!(
        selected(&deleted),
        "Cargo.toml: none\ntools/Cargo.toml: none\n"
    );
    let rust_ci = plan_with_bases(&repo, &[".github/workflows/rust-ci.yml"], &[]);
    assert_eq!(
        selected(&rust_ci),
        "Cargo.toml: all:generated-workflow\ntools/Cargo.toml: all:generated-workflow\n"
    );
    let hand_written = plan_with_bases(&repo, &[".github/workflows/lint.yml"], &[]);
    assert!(
        selected(&hand_written).starts_with(
            "Cargo.toml: all:unknown-path:.github/workflows/lint.yml\ntools/Cargo.toml: all:unknown-path:.github/workflows/lint.yml\n"
        ),
        "{hand_written:?}"
    );
}
