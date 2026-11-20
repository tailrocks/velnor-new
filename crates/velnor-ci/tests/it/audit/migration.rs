// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

mod targets;

use std::fs;
use std::process::Output;

use velnor_test_support::real_cargo;

use super::{audit, command, own_units, workspace_below_the_root};
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
        let mut command = command(env!("CARGO_BIN_EXE_velnor-ci"));
        command.args(["inputs", "build", "--", "cargo"]);
        command
    } else {
        command(real_cargo())
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
    let built = command(env!("CARGO_BIN_EXE_velnor-ci")).current_dir(repo.root().join("rust")).args(["inputs", "build", "--", "sh", "-c", "printf run >> target/runs.txt; mkdir -p target/generated; printf 'cargo:rerun-if-changed=../target/generated\\n' > target/debug/build/lib-f000000000000008/output; touch -m -t 206501010000 target/debug/build/lib-f000000000000008/output"]).output().unwrap_or_else(|e| unreachable!("{e}"));
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

/// The cargo-nextest that the repository's mise.toml pins must be installed in the
/// mise data directory. Without it the `cargo-nextest` shim fails with an opaque
/// "not a valid shim" error, so the test names the pin and the fix instead.
fn pinned_nextest_problem(data_dir: Option<&std::path::Path>) -> Option<String> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut probe = std::process::Command::new("mise");
    probe
        .args(["which", "cargo-nextest"])
        .current_dir(&root)
        .env("MISE_GLOBAL_CONFIG_FILE", root.join("mise.toml"))
        .env("MISE_TRUSTED_CONFIG_PATHS", &root);
    if let Some(data_dir) = data_dir {
        probe.env("MISE_DATA_DIR", data_dir);
    }
    let output = probe
        .output()
        .unwrap_or_else(|e| unreachable!("cannot run mise: {e}"));
    (!output.status.success()).then(|| {
        "cargo-nextest pinned in mise.toml is not installed in the mise data directory: \
         run `mise install` in the repository root, then rerun the test"
            .to_owned()
    })
}

#[test]
fn nextest_discovery_names_the_missing_pin_before_running() {
    let data_dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("nextest-discovery-empty-mise-data");
    fs::create_dir_all(&data_dir).unwrap_or_else(|e| unreachable!("{e}"));
    let problem = pinned_nextest_problem(Some(&data_dir));
    fs::remove_dir_all(&data_dir).unwrap_or_else(|e| unreachable!("{e}"));
    let problem = problem.expect("an empty mise data directory has no pinned cargo-nextest");
    assert!(problem.contains("mise.toml"), "{problem}");
    assert!(problem.contains("`mise install`"), "{problem}");
}

#[test]
fn nextest_discovery_compiles_first_and_executes_the_tests_once() {
    if let Some(problem) = pinned_nextest_problem(None) {
        panic!("{problem}");
    }
    let repo = cargo_project(InputDirectory::Generated);
    repo.write("rust/src/lib.rs", r#"#[test]
fn records_execution() {
    use std::io::Write;
    let file = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/tests-ran.txt");
    std::fs::OpenOptions::new().create(true).append(true).open(file).unwrap().write_all(b"run").unwrap();
}
"#);
    let pinned = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../mise.toml");
    let built = command(env!("CARGO_BIN_EXE_velnor-ci"))
        .env_remove("NEXTEST_PROFILE")
        .env_remove("MISE_NO_CONFIG")
        .env("MISE_GLOBAL_CONFIG_FILE", &pinned)
        .env("MISE_TRUSTED_CONFIG_PATHS", repo.root())
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
        command(env!("CARGO_BIN_EXE_velnor-ci"))
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

fn compiler_discovery_with(prefix: &[&str]) {
    let repo = cargo_project(InputDirectory::Generated);
    let built = command(env!("CARGO_BIN_EXE_velnor-ci"))
        .current_dir(repo.root().join("rust"))
        .args(["inputs", "build", "--", "cargo"])
        .args(prefix)
        .args(["build", "--workspace", "--all-targets", "--offline"])
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(built.status.success(), "{prefix:?}: {built:?}");
    assert_eq!(runs(&repo), 2, "{prefix:?}");
    assert_fresh(&repo, "");
}

#[test]
fn a_cargo_toolchain_override_keeps_compiler_discovery_enabled() {
    compiler_discovery_with(&["+1.99.1"]);
}

#[test]
fn cargo_verbosity_keeps_compiler_discovery_enabled() {
    compiler_discovery_with(&["-v"]);
}

#[test]
fn a_cargo_color_option_keeps_compiler_discovery_enabled() {
    compiler_discovery_with(&["--color", "never"]);
}

#[test]
fn a_cargo_config_option_keeps_compiler_discovery_enabled() {
    compiler_discovery_with(&["--config", "build.jobs=1"]);
}
