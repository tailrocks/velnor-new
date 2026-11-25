// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::io::Repo;

use super::features::{app, library, repo_of, selects};
use super::unit;

fn only_a_bench_that_opts_out() -> Repo {
    let repo = repo_of(&[("benches/b.rs", "fn main() {}\n")]);
    app(
        &repo,
        "autobenches = false\n\n[[bench]]\nname = \"b\"\nharness = false\nbench = false\n",
    );
    repo
}

#[test]
fn all_targets_leaves_out_a_benchmark_that_opts_out() {
    for (cargo, expected) in [
        (&["build", "-p", "app", "--all-targets"][..], false),
        (&["build", "-p", "app", "--bench", "b"], true),
    ] {
        assert_eq!(
            selects(&only_a_bench_that_opts_out(), "app", cargo),
            expected,
            "{cargo:?}"
        );
    }
}

fn library_that_cannot_be_tested(doctest: bool) -> Repo {
    let repo = repo_of(&[("src/lib.rs", "")]);
    app(
        &repo,
        &format!("\n[lib]\ntest = false\ndoctest = {doctest}\n"),
    );
    repo
}

#[test]
fn a_default_test_run_skips_a_library_that_is_neither_tested_nor_doctested() {
    for (doctest, cargo, expected) in [
        (false, &["test", "-p", "app"][..], false),
        (true, &["test", "-p", "app"], true),
        (false, &["build", "-p", "app"], true),
    ] {
        assert_eq!(
            selects(&library_that_cannot_be_tested(doctest), "app", cargo),
            expected,
            "{doctest} {cargo:?}"
        );
    }
}

fn dev_dependency_of_a_doctested_library(doctest: bool) -> Repo {
    let repo = repo_of(&[("src/lib.rs", "")]);
    library(&repo, "vendor/devtool", "devtool", "");
    app(
        &repo,
        &format!(
            "\n[lib]\ntest = false\ndoctest = {doctest}\n\n[dev-dependencies]\ndevtool = {{ path = \"vendor/devtool\" }}\n"
        ),
    );
    repo
}

#[test]
fn a_doctest_compiles_the_dev_dependencies_of_a_library_that_is_not_tested() {
    for (doctest, cargo, expected) in [
        (true, &["test", "-p", "app"][..], true),
        (false, &["test", "-p", "app"], false),
        (true, &["build", "-p", "app"], false),
    ] {
        assert_eq!(
            selects(
                &dev_dependency_of_a_doctested_library(doctest),
                "devtool",
                cargo
            ),
            expected,
            "{doctest} {cargo:?}"
        );
    }
}

fn binary_requiring(feature: &str, features: &str) -> Repo {
    let repo = repo_of(&[("src/gated.rs", "fn main() {}\n")]);
    app(
        &repo,
        &format!(
            "autobins = false\n{features}\n[[bin]]\nname = \"gated\"\npath = \"src/gated.rs\"\nrequired-features = [\"{feature}\"]\n"
        ),
    );
    repo
}

#[test]
fn a_required_feature_that_the_package_does_not_declare_is_never_enabled() {
    let build = &["build", "-p", "app"][..];
    let all = &["build", "-p", "app", "--all-features"][..];
    for (feature, features, cargo, expected) in [
        ("default", "", build, false),
        ("default", "\n[features]\ndefault = []\n", build, true),
        ("ghost", "", all, false),
        ("ghost", "\n[features]\nghost = []\n", all, true),
    ] {
        assert_eq!(
            selects(&binary_requiring(feature, features), "app", cargo),
            expected,
            "{feature} {features:?} {cargo:?}"
        );
    }
}

fn suppressed_implicit_feature() -> Repo {
    let repo = repo_of(&[("src/lib.rs", "")]);
    library(&repo, "vendor/leaf", "leaf", "");
    app(
        &repo,
        "\n[features]\nbar = [\"dep:leaf\"]\n\n[dependencies]\nleaf = { path = \"vendor/leaf\", optional = true }\n",
    );
    repo
}

#[test]
fn a_feature_that_uses_dep_syntax_removes_the_implicit_feature_of_the_dependency() {
    for (cargo, expected) in [
        (&["build", "-p", "app", "--features", "leaf"][..], false),
        (&["build", "-p", "app", "--features", "bar"], true),
    ] {
        assert_eq!(
            selects(&suppressed_implicit_feature(), "leaf", cargo),
            expected,
            "{cargo:?}"
        );
    }
}

fn legacy_default_features() -> Repo {
    let repo = repo_of(&[("src/lib.rs", "")]);
    library(&repo, "vendor/inner", "inner", "");
    library(&repo, "vendor/leaf", "leaf", "");
    unit(
        &repo,
        "vendor/leaf/Cargo.toml",
        "[package]\nname = \"leaf\"\nedition = \"2021\"\n\n[features]\ndefault = [\"dep:inner\"]\n\n[dependencies]\ninner = { path = \"../inner\", optional = true }\n",
    );
    library(&repo, "vendor/mid", "mid", "");
    unit(
        &repo,
        "vendor/mid/Cargo.toml",
        "[package]\nname = \"mid\"\nedition = \"2021\"\n\n[dependencies]\nleaf = { path = \"../leaf\", default_features = false }\n",
    );
    app(&repo, "\n[dependencies]\nmid = { path = \"vendor/mid\" }\n");
    repo
}

#[test]
fn the_legacy_default_features_spelling_of_an_unowned_dependency_disables_the_defaults() {
    assert!(!selects(
        &legacy_default_features(),
        "inner",
        &["build", "-p", "app"]
    ));
}

fn platform_dependencies() -> Repo {
    let repo = repo_of(&[("src/lib.rs", "")]);
    library(&repo, "vendor/unix", "unixonly", "");
    library(&repo, "vendor/windows", "windowsonly", "");
    app(
        &repo,
        "\n[target.'cfg(unix)'.dependencies]\nunixonly = { path = \"vendor/unix\" }\n\n[target.'cfg(windows)'.dependencies]\nwindowsonly = { path = \"vendor/windows\" }\n",
    );
    unit(
        &repo,
        "rust-toolchain.toml",
        "[toolchain]\nchannel = \"1.99.1\"\ntargets = [\"x86_64-pc-windows-msvc\"]\n",
    );
    repo
}

const WINDOWS: &str = "[build]\ntarget = \"x86_64-pc-windows-msvc\"\n";
const LINUX: &str = "[build]\ntarget = \"x86_64-unknown-linux-gnu\"\n";

#[test]
fn a_command_line_config_decides_the_target_of_the_platform_dependencies() {
    for (package, expected) in [("windowsonly", true), ("unixonly", false)] {
        let repo = platform_dependencies();
        repo.git(&["add", "--all"]);
        let cargo = [
            "build",
            "-p",
            "app",
            "--config",
            "build.target=\"x86_64-pc-windows-msvc\"",
        ];
        assert_eq!(selects(&repo, package, &cargo), expected, "{package}");
    }
}

#[test]
fn the_extensionless_cargo_config_wins_over_the_one_with_an_extension() {
    for (package, expected) in [("windowsonly", true), ("unixonly", false)] {
        let repo = platform_dependencies();
        unit(&repo, ".cargo/config", WINDOWS);
        unit(&repo, ".cargo/config.toml", LINUX);
        repo.git(&["add", "--all"]);
        assert_eq!(
            selects(&repo, package, &["build", "-p", "app"]),
            expected,
            "{package}"
        );
    }
}
