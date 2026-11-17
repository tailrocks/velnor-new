// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::process::{Command, Output};

use super::{audit, own_units, workspace_below_the_root};
use crate::io::Repo;

#[derive(Clone, Copy)]
enum InputDirectory {
    Shared,
    Generated,
    OutDir,
    Repository,
}

fn cargo_project(input: InputDirectory) -> Repo {
    let repo = Repo::new("audit-build-migration");
    repo.write("mise.toml", "[tools]\nrust = \"1.99.1\"\n");
    repo.write(
        "rust/Cargo.toml",
        "[package]\nname = \"app\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    );
    repo.write("rust/src/lib.rs", "pub fn value() -> u8 { 1 }\n");
    repo.write(
        "rust/Cargo.lock",
        "version = 4\n\n[[package]]\nname = \"app\"\nversion = \"0.1.0\"\n",
    );
    repo.write("shared/value.txt", "input");
    let directory = match input {
        InputDirectory::Shared => "root.join(\"../shared\")",
        InputDirectory::Generated => "root.join(\"target/generated\")",
        InputDirectory::Repository => "root.join(\"..\")",
        InputDirectory::OutDir => {
            "std::path::PathBuf::from(std::env::var_os(\"OUT_DIR\").unwrap())"
        }
    };
    repo.write(
        "rust/build.rs",
        &format!(
            r#"fn main() {{
    let root = std::path::PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let counter = root.join("target/runs.txt");
    let count = std::fs::read_to_string(&counter).unwrap_or_default().parse::<u32>().unwrap_or(0);
    std::fs::write(counter, (count + 1).to_string()).unwrap();
    let input = {directory};
    std::fs::create_dir_all(&input).unwrap();
    if !input.join("value.txt").exists() {{ std::fs::write(input.join("value.txt"), "input").unwrap(); }}
    println!("cargo:rerun-if-changed={{}}", input.display());
}}
"#
        ),
    );
    repo
}

fn build(repo: &Repo, wrapped: bool) -> Output {
    let mut command = if wrapped {
        let mut command = Command::new(env!("CARGO_BIN_EXE_velnor-ci"));
        command.args(["inputs", "build", "--", "cargo"]);
        command
    } else {
        Command::new("cargo")
    };
    command
        .current_dir(repo.root().join("rust"))
        .args(["build", "--workspace", "--all-targets", "--offline"])
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"))
}

fn runs(repo: &Repo) -> u32 {
    fs::read_to_string(repo.root().join("rust/target/runs.txt"))
        .unwrap_or_else(|e| unreachable!("{e}"))
        .parse()
        .unwrap_or_else(|e| unreachable!("{e}"))
}

fn assert_fresh(repo: &Repo, expected: &str) {
    let written = audit(repo, &["--write"]);
    assert!(written.status.success(), "{written:?}");
    assert_eq!(
        fs::read_to_string(repo.root().join(".velnor/rust-inputs.lock"))
            .unwrap_or_else(|e| unreachable!("{e}")),
        expected
    );
}

#[test]
fn cached_cargo_outputs_without_inventory_are_rebuilt_before_becoming_trusted() {
    let repo = cargo_project(InputDirectory::Shared);
    let plain = build(&repo, false);
    assert!(plain.status.success(), "{plain:?}");
    assert_eq!(runs(&repo), 1);
    let refused = audit(&repo, &["--write"]);
    assert_eq!(refused.status.code(), Some(1), "{refused:?}");
    assert!(!repo.root().join(".velnor/rust-inputs.lock").exists());
    let rebuilt = build(&repo, true);
    assert!(rebuilt.status.success(), "{rebuilt:?}");
    assert_eq!(runs(&repo), 2);
    assert_fresh(&repo, "shared\tapp\n");
    let unchanged = build(&repo, true);
    assert!(unchanged.status.success(), "{unchanged:?}");
    assert_eq!(runs(&repo), 2);
    assert_fresh(&repo, "shared\tapp\n");
}

#[test]
fn a_clean_cargo_build_discovers_generated_directory_inputs_before_a_verified_rebuild() {
    let repo = cargo_project(InputDirectory::Generated);
    let built = build(&repo, true);
    assert!(built.status.success(), "{built:?}");
    assert_eq!(runs(&repo), 2);
    assert_fresh(&repo, "");
}

#[test]
fn an_arbitrary_command_that_discovers_a_directory_is_executed_once() {
    let repo = workspace_below_the_root();
    own_units(&repo, "rust/target/debug", &[]);
    let built = Command::new(env!("CARGO_BIN_EXE_velnor-ci")).current_dir(repo.root().join("rust")).args(["inputs", "build", "--", "sh", "-c", "printf run >> target/runs.txt; mkdir -p target/generated; printf 'cargo:rerun-if-changed=../target/generated\\n' > target/debug/build/lib-f000000000000008/output; touch -m -t 206501010000 target/debug/build/lib-f000000000000008/output"]).output().unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(
        fs::read_to_string(repo.root().join("rust/target/runs.txt"))
            .unwrap_or_else(|e| unreachable!("{e}")),
        "run"
    );
    assert!(built.status.success(), "{built:?}");
    let refused = audit(&repo, &["--write"]);
    assert_eq!(refused.status.code(), Some(1), "{refused:?}");
    assert!(!repo.root().join(".velnor/rust-inputs.lock").exists());
}

#[test]
fn declared_out_dir_inputs_survive_compiler_discovery_invalidation() {
    let repo = cargo_project(InputDirectory::OutDir);
    let built = build(&repo, true);
    assert!(built.status.success(), "{built:?}");
    assert_eq!(runs(&repo), 2);
    assert_fresh(&repo, "");
    let unchanged = build(&repo, true);
    assert!(unchanged.status.success(), "{unchanged:?}");
    assert_eq!(runs(&repo), 2);
    assert_fresh(&repo, "");
}

#[test]
fn nextest_discovery_compiles_first_and_executes_the_tests_once() {
    let repo = cargo_project(InputDirectory::Generated);
    repo.write("rust/src/lib.rs", r#"#[test]
fn records_execution() {
    use std::io::Write;
    let file = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/tests-ran.txt");
    std::fs::OpenOptions::new().create(true).append(true).open(file).unwrap().write_all(b"run").unwrap();
}
"#);
    let built = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .env_remove("NEXTEST_PROFILE")
        .current_dir(repo.root().join("rust"))
        .args([
            "inputs",
            "build",
            "--",
            "cargo",
            "nextest",
            "run",
            "--workspace",
            "--offline",
            "--no-tests",
            "fail",
        ])
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(built.status.success(), "{built:?}");
    assert_eq!(runs(&repo), 2);
    assert_eq!(
        fs::read_to_string(repo.root().join("rust/target/tests-ran.txt"))
            .unwrap_or_else(|e| unreachable!("{e}")),
        "run"
    );
    assert_fresh(&repo, "");
}

fn package_output(repo: &Repo, package: &str) -> std::path::PathBuf {
    fs::read_dir(repo.root().join("rust/target/debug/build"))
        .unwrap_or_else(|e| unreachable!("{e}"))
        .map(|entry| entry.unwrap_or_else(|e| unreachable!("{e}")))
        .find(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with(&format!("{package}-"))
                && entry.path().join("output").is_file()
        })
        .unwrap_or_else(|| unreachable!("missing {package} output"))
        .path()
        .join("output")
}

#[test]
fn a_partial_build_never_invalidates_an_unselected_packages_output() {
    let repo = cargo_project(InputDirectory::Shared);
    repo.write("rust/Cargo.toml", "[package]\nname = \"app\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[workspace]\nmembers = [\"lib\"]\nresolver = \"3\"\n");
    repo.write(
        "rust/lib/Cargo.toml",
        "[package]\nname = \"lib\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    );
    repo.write("rust/lib/src/lib.rs", "");
    repo.write(
        "rust/lib/build.rs",
        r#"fn main() {
    let root = std::path::PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let counter = root.join("../target/lib-runs.txt");
    let count = std::fs::read_to_string(&counter).unwrap_or_default().parse::<u32>().unwrap_or(0);
    std::fs::write(counter, (count + 1).to_string()).unwrap();
    println!("cargo:rerun-if-changed=../../shared");
}
"#,
    );
    repo.write("rust/Cargo.lock", "version = 4\n\n[[package]]\nname = \"app\"\nversion = \"0.1.0\"\n\n[[package]]\nname = \"lib\"\nversion = \"0.1.0\"\n");
    let plain = build(&repo, false);
    assert!(plain.status.success(), "{plain:?}");
    let app = package_output(&repo, "app");
    let lib = package_output(&repo, "lib");
    let identity = |path: &std::path::Path| {
        (
            fs::read(path).unwrap_or_else(|e| unreachable!("{e}")),
            fs::metadata(path)
                .and_then(|metadata| metadata.modified())
                .unwrap_or_else(|e| unreachable!("{e}")),
        )
    };
    let before_app = identity(&app);
    let before_lib = identity(&lib);
    let partial = || {
        Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
            .current_dir(repo.root().join("rust"))
            .args([
                "inputs",
                "build",
                "--",
                "cargo",
                "build",
                "-p",
                "app",
                "--all-targets",
                "--offline",
            ])
            .output()
            .unwrap_or_else(|e| unreachable!("{e}"))
    };
    let refused = partial();
    assert_eq!(refused.status.code(), Some(1), "{refused:?}");
    assert!(
        String::from_utf8_lossy(&refused.stderr)
            .contains("outside this command's explicit selection"),
        "{refused:?}"
    );
    assert_eq!(identity(&app), before_app);
    assert_eq!(identity(&lib), before_lib);
    assert_eq!(runs(&repo), 1);
    let lib_counter = repo.root().join("rust/target/lib-runs.txt");
    assert_eq!(
        fs::read_to_string(&lib_counter).unwrap_or_else(|e| unreachable!("{e}")),
        "1"
    );
    let prepared = build(&repo, true);
    assert!(prepared.status.success(), "{prepared:?}");
    assert_eq!(runs(&repo), 2);
    let trusted_lib = identity(&lib);
    fs::remove_file(app.with_extension("velnor-inputs.json"))
        .unwrap_or_else(|e| unreachable!("{e}"));
    let rebuilt = partial();
    assert!(rebuilt.status.success(), "{rebuilt:?}");
    assert_eq!(runs(&repo), 3);
    assert_eq!(identity(&lib), trusted_lib);
    assert_eq!(
        fs::read_to_string(&lib_counter).unwrap_or_else(|e| unreachable!("{e}")),
        "2"
    );
    assert_fresh(&repo, "shared\tapp\nshared\tlib\n");
}

#[test]
fn cargo_toolchain_and_global_options_keep_compiler_discovery_enabled() {
    for prefix in [
        vec!["+1.99.1"],
        vec!["-v"],
        vec!["--color", "never"],
        vec!["--config", "build.jobs=1"],
    ] {
        let repo = cargo_project(InputDirectory::Generated);
        let built = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
            .current_dir(repo.root().join("rust"))
            .args(["inputs", "build", "--", "cargo"])
            .args(&prefix)
            .args(["build", "--workspace", "--all-targets", "--offline"])
            .output()
            .unwrap_or_else(|e| unreachable!("{e}"));
        assert!(built.status.success(), "{prefix:?}: {built:?}");
        assert_eq!(runs(&repo), 2, "{prefix:?}");
        assert_fresh(&repo, "");
    }
}

#[test]
fn a_cross_target_created_by_cargo_at_the_source_root_does_not_hide_source_changes() {
    use std::time::{Duration, UNIX_EPOCH};

    let repo = cargo_project(InputDirectory::Repository);
    repo.write(".velnor/prepare", "");
    repo.write_unstaged("rust/target/runs.txt", "0");
    let version = Command::new("rustc")
        .arg("-vV")
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(version.status.success(), "{version:?}");
    let version = String::from_utf8(version.stdout).unwrap_or_else(|e| unreachable!("{e}"));
    let host = version
        .lines()
        .find_map(|line| line.strip_prefix("host: "))
        .unwrap_or_else(|| unreachable!("{version}"));
    let built = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .current_dir(repo.root().join("rust"))
        .env("CARGO_TARGET_DIR", repo.root())
        .args([
            "inputs",
            "build",
            "--",
            "cargo",
            "build",
            "--workspace",
            "--all-targets",
            "--offline",
            "--target",
            host,
        ])
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(built.status.success(), "{built:?}");
    let artifact = format!("{host}/debug/new-artifact");
    repo.write_unstaged(&artifact, "generated");
    repo.set_mtime(&artifact, UNIX_EPOCH + Duration::from_secs(4_000_000_000));
    let check = || {
        Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
            .current_dir(repo.root())
            .env("CARGO_TARGET_DIR", repo.root())
            .args(["inputs", "audit", "--write"])
            .output()
            .unwrap_or_else(|e| unreachable!("{e}"))
    };
    let written = check();
    assert!(written.status.success(), "{written:?}");
    let lock = repo.root().join(".velnor/rust-inputs.lock");
    assert_eq!(
        fs::read_to_string(&lock).unwrap_or_else(|e| unreachable!("{e}")),
        ".\tapp\n"
    );
    fs::remove_file(&lock).unwrap_or_else(|e| unreachable!("{e}"));
    repo.set_mtime(
        "shared/value.txt",
        UNIX_EPOCH + Duration::from_secs(4_000_000_000),
    );
    let stale = check();
    assert_eq!(stale.status.code(), Some(1), "{stale:?}");
    assert!(!lock.exists());
}
