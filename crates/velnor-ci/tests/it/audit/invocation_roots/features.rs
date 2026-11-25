// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::io::Repo;

use super::{WORKSPACE_TARGET, placed, unit, workspace_output};

const REFUSAL: &str = "outside this command's explicit selection";

fn repo_of(files: &[(&str, &str)]) -> Repo {
    let repo = Repo::new("audit-selection-features");
    unit(&repo, ".gitignore", "target/\ntarget-velnor/\n");
    unit(&repo, "mise.toml", "[tools]\nrust = \"1.99.1\"\n");
    unit(&repo, "Cargo.lock", "version = 4\n");
    for (path, text) in files {
        unit(&repo, path, text);
    }
    repo.git(&["add", "--all"]);
    repo
}

fn library(repo: &Repo, directory: &str, name: &str, extra: &str) {
    unit(
        repo,
        &format!("{directory}/Cargo.toml"),
        &format!("[package]\nname = \"{name}\"\nedition = \"2024\"\nbuild = \"build.rs\"\n{extra}"),
    );
    unit(repo, &format!("{directory}/src/lib.rs"), "");
    unit(repo, &format!("{directory}/build.rs"), "fn main() {}\n");
}

fn app(repo: &Repo, manifest: &str) {
    if repo.root().join("vendor").exists() {
        unit(repo, ".velnor/rust-ci.toml", "exclude = [\"vendor\"]\n");
    }
    unit(
        repo,
        "Cargo.toml",
        &format!("[package]\nname = \"app\"\nedition = \"2024\"\n{manifest}"),
    );
    repo.git(&["add", "--all"]);
}

fn selects(repo: &Repo, package: &str, cargo: &[&str]) -> bool {
    placed(repo, &format!("{WORKSPACE_TARGET}/debug"));
    unit(
        repo,
        &format!("{WORKSPACE_TARGET}/debug/build/{package}-f000000000000005/output"),
        "cargo:rerun-if-changed=src/lib.rs\n",
    );
    let mut command = vec!["cargo"];
    command.extend_from_slice(cargo);
    command.push("--no-such-flag");
    let stderr = workspace_output(repo, &command);
    let refused = stderr.contains(REFUSAL);
    assert!(
        refused || stderr.contains("build command failed"),
        "{cargo:?}: {stderr}"
    );
    !refused
}

fn gated_binary() -> Repo {
    let repo = repo_of(&[("src/gated.rs", "fn main() {}\n")]);
    app(
        &repo,
        "build = \"build.rs\"\nautobins = false\n\n[features]\ndefault = [\"on\"]\non = [\"extra\"]\nextra = []\nother = []\n\n[[bin]]\nname = \"gated\"\npath = \"src/gated.rs\"\nrequired-features = [\"extra\"]\n",
    );
    unit(&repo, "build.rs", "fn main() {}\n");
    repo.git(&["add", "--all"]);
    repo
}

#[test]
fn a_feature_gated_binary_is_compiled_when_the_requested_features_enable_what_it_requires() {
    for (cargo, expected) in [
        (&["build", "-p", "app"][..], true),
        (&["build", "-p", "app", "--no-default-features"], false),
        (
            &[
                "build",
                "-p",
                "app",
                "--no-default-features",
                "--features",
                "extra",
            ],
            true,
        ),
        (
            &[
                "build",
                "-p",
                "app",
                "--no-default-features",
                "--features",
                "on",
            ],
            true,
        ),
        (
            &[
                "build",
                "-p",
                "app",
                "--no-default-features",
                "-F",
                "app/extra",
            ],
            true,
        ),
        (
            &[
                "build",
                "-p",
                "app",
                "--no-default-features",
                "--features",
                "other",
            ],
            false,
        ),
        (
            &[
                "build",
                "-p",
                "app",
                "--no-default-features",
                "--all-features",
            ],
            true,
        ),
    ] {
        assert_eq!(
            selects(&gated_binary(), "app", cargo),
            expected,
            "{cargo:?}"
        );
    }
}

fn optional_helper() -> Repo {
    let repo = repo_of(&[("src/lib.rs", "")]);
    library(&repo, "vendor/helper", "helper", "");
    app(
        &repo,
        "build = \"build.rs\"\n\n[features]\ndefault = [\"dep:helper\"]\nsuite = [\"dep:helper\"]\nnone = []\n\n[dependencies]\nhelper = { path = \"vendor/helper\", optional = true }\n",
    );
    unit(&repo, "build.rs", "fn main() {}\n");
    repo.git(&["add", "--all"]);
    repo
}

#[test]
fn an_optional_dependency_enabled_through_the_feature_table_is_selected() {
    for (cargo, expected) in [
        (&["build", "-p", "app"][..], true),
        (&["build", "-p", "app", "--no-default-features"], false),
        (
            &[
                "build",
                "-p",
                "app",
                "--no-default-features",
                "--features",
                "suite",
            ],
            true,
        ),
        (
            &[
                "build",
                "-p",
                "app",
                "--no-default-features",
                "--features",
                "none",
            ],
            false,
        ),
    ] {
        assert_eq!(
            selects(&optional_helper(), "helper", cargo),
            expected,
            "{cargo:?}"
        );
    }
}

fn platform_dependencies() -> Repo {
    let repo = repo_of(&[("src/lib.rs", "")]);
    library(&repo, "vendor/unix", "unixonly", "");
    library(&repo, "vendor/windows", "windowsonly", "");
    app(
        &repo,
        "\n[target.'cfg(unix)'.dependencies]\nunixonly = { path = \"vendor/unix\" }\n\n[target.'cfg(windows)'.dependencies]\nwindowsonly = { path = \"vendor/windows\" }\n\n[target.'cfg(not(any(unix, windows)))'.dependencies]\n",
    );
    repo
}

#[test]
fn a_dependency_for_a_platform_is_selected_only_where_its_predicate_holds() {
    for (cargo, package, expected) in [
        (&["build", "-p", "app"][..], "unixonly", true),
        (&["build", "-p", "app"], "windowsonly", false),
        (
            &["build", "-p", "app", "--target", "x86_64-pc-windows-msvc"],
            "windowsonly",
            true,
        ),
        (
            &["build", "-p", "app", "--target=x86_64-pc-windows-msvc"],
            "unixonly",
            false,
        ),
    ] {
        assert_eq!(
            selects(&platform_dependencies(), package, cargo),
            expected,
            "{cargo:?} {package}"
        );
    }
}

fn opted_out_library(flag: &str) -> Repo {
    let repo = repo_of(&[("src/lib.rs", "")]);
    app(&repo, &format!("\n[lib]\n{flag} = false\n"));
    repo
}

#[test]
fn a_target_that_opts_out_of_benchmarks_or_documentation_is_not_built_by_them() {
    for (flag, cargo, expected) in [
        ("bench", &["build", "-p", "app", "--benches"][..], false),
        ("bench", &["bench", "-p", "app"], false),
        ("bench", &["build", "-p", "app", "--lib"], true),
        ("bench", &["build", "-p", "app"], true),
        ("doc", &["doc", "-p", "app"], false),
        ("doc", &["doc", "-p", "app", "--lib"], true),
        ("doc", &["build", "-p", "app"], true),
    ] {
        assert_eq!(
            selects(&opted_out_library(flag), "app", cargo),
            expected,
            "{flag} {cargo:?}"
        );
    }
}

fn renamed_dependency() -> Repo {
    let repo = repo_of(&[("src/lib.rs", ""), ("other/src/lib.rs", "")]);
    library(&repo, "vendor/leaf", "leaf", "");
    library(
        &repo,
        "vendor/mid",
        "mid",
        "\n[features]\nleaf = [\"dep:leaf\"]\n\n[dependencies]\nleaf = { path = \"../leaf\", optional = true }\n",
    );
    unit(
        &repo,
        "other/Cargo.toml",
        "[package]\nname = \"other\"\nedition = \"2024\"\n\n[dependencies]\nmid = { path = \"../vendor/mid\" }\n",
    );
    app(
        &repo,
        "build = \"build.rs\"\n\n[dependencies]\nhelper = { path = \"vendor/mid\", package = \"mid\", optional = true }\n\n[workspace]\nmembers = [\"other\"]\nexclude = [\"vendor\"]\n",
    );
    unit(&repo, "build.rs", "fn main() {}\n");
    repo.git(&["add", "--all"]);
    repo
}

#[test]
fn a_feature_forwarded_through_a_renamed_or_weak_dependency_reaches_its_target() {
    for (cargo, expected) in [
        (
            &["build", "-p", "app", "--features", "helper/leaf"][..],
            true,
        ),
        (&["build", "-p", "app", "--features", "helper"], false),
        (&["build", "-p", "other", "--features", "mid?/leaf"], true),
        (&["build", "-p", "other"], false),
        (&["build", "-p", "app", "--features", "helper?/leaf"], false),
    ] {
        assert_eq!(
            selects(&renamed_dependency(), "leaf", cargo),
            expected,
            "{cargo:?}"
        );
    }
}
