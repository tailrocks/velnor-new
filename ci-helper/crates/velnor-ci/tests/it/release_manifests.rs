// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const PLZ: &str = "[workspace]\ngit_tag_name = \"{{ package }}-v{{ version }}\"\ngit_release_draft = true\nrelease_always = false\nsemver_check = false\npublish_no_verify = true\n";

struct Checkout(PathBuf);

impl Checkout {
    fn new(name: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("velnor-ci-manifests-{}-{name}", std::process::id()));
        fs::remove_dir_all(&dir).ok();
        fs::create_dir_all(&dir).unwrap_or_else(|e| unreachable!("{e}"));
        Self(dir)
    }

    fn write(&self, path: &str, text: &str) -> &Self {
        let file = self.0.join(path);
        fs::create_dir_all(file.parent().unwrap_or(Path::new("")))
            .unwrap_or_else(|e| unreachable!("{e}"));
        fs::write(file, text).unwrap_or_else(|e| unreachable!("{e}"));
        self
    }

    fn package(&self, dir: &str, name: &str, extra: &str) -> &Self {
        let prefix = if dir.is_empty() {
            String::new()
        } else {
            format!("{dir}/")
        };
        self.write(
            &format!("{prefix}Cargo.toml"),
            &format!(
                "[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n{extra}"
            ),
        )
        .write(&format!("{prefix}src/lib.rs"), "")
    }

    fn check(&self, files: &[&str], toolchain: Option<&str>) -> (Option<i32>, String) {
        let mut command = Command::new(env!("CARGO_BIN_EXE_velnor-ci"));
        command
            .args(["release", "check-config"])
            .args(files)
            .arg("--manifests")
            .current_dir(&self.0)
            .env_remove("RUSTUP_TOOLCHAIN");
        if let Some(toolchain) = toolchain {
            command.env("RUSTUP_TOOLCHAIN", toolchain);
        }
        let output = command.output().unwrap_or_else(|e| unreachable!("{e}"));
        (
            output.status.code(),
            String::from_utf8_lossy(&output.stderr).into_owned(),
        )
    }

    fn run(&self, files: &[&str]) -> (Option<i32>, String) {
        let toolchain = std::env::var("RUSTUP_TOOLCHAIN").unwrap_or_else(|_| "stable".to_owned());
        self.check(files, Some(&toolchain))
    }
}

impl Drop for Checkout {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).ok();
    }
}

fn refused(checkout: &Checkout, files: &[&str], needle: &str) {
    let (code, stderr) = checkout.run(files);
    assert_eq!(code, Some(1), "{stderr}");
    assert!(stderr.contains(needle), "{stderr}");
}

fn accepted(checkout: &Checkout, files: &[&str]) {
    let (code, stderr) = checkout.run(files);
    assert_eq!(code, Some(0), "{stderr}");
}

#[test]
fn a_publish_list_inherited_from_a_workspace_above_the_release_root_is_refused() {
    let checkout = Checkout::new("ancestor");
    checkout
        .write(
            "Cargo.toml",
            "[workspace]\nmembers = [\"crates/app\"]\n[workspace.package]\npublish = [\"company\"]\n",
        )
        .package("crates/app", "app", "publish.workspace = true\n")
        .write("crates/app/release-plz.toml", PLZ);
    refused(
        &checkout,
        &["crates/app/release-plz.toml"],
        "crates/app/Cargo.toml: package.publish names registry company",
    );
    checkout.write(
        "Cargo.toml",
        "[workspace]\nmembers = [\"crates/app\"]\n[workspace.package]\npublish = false\n",
    );
    accepted(&checkout, &["crates/app/release-plz.toml"]);
}

#[test]
fn an_excluded_example_and_a_nested_workspace_are_not_packages_of_the_root() {
    let checkout = Checkout::new("excluded");
    let marker = checkout.0.join("wrapper-ran");
    let wrapper = checkout.0.join("wrapper.sh");
    checkout
        .write(
            "Cargo.toml",
            "[workspace]\nmembers = [\".\"]\nexclude = [\"examples/demo\"]\n[package]\nname = \"app\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .write("src/lib.rs", "")
        .package("examples/demo", "demo", "publish = [\"x\"]\n[workspace]\n")
        .package("nested", "inner", "publish = [\"x\"]\n[workspace]\n")
        .write("release-plz.toml", PLZ)
        .write(
            "wrapper.sh",
            &format!("#!/bin/sh\ntouch {}\nexec \"$@\"\n", marker.display()),
        );
    let toolchain = std::env::var("RUSTUP_TOOLCHAIN").unwrap_or_else(|_| "stable".to_owned());
    let output = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .args(["release", "check-config", "release-plz.toml", "--manifests"])
        .current_dir(&checkout.0)
        .env("RUSTUP_TOOLCHAIN", toolchain)
        .env("RUSTC_WRAPPER", &wrapper)
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!marker.exists());
}

#[test]
fn a_package_release_plz_does_not_publish_may_name_another_registry() {
    let checkout = Checkout::new("opted-out");
    checkout
        .write("Cargo.toml", "[workspace]\nmembers = [\"app\", \"tool\"]\n")
        .package("app", "app", "")
        .package("tool", "tool", "publish = [\"x\"]\n")
        .write(
            "release-plz.toml",
            &format!("{PLZ}\n[[package]]\nname = \"tool\"\npublish = false\n"),
        );
    accepted(&checkout, &["release-plz.toml"]);
    checkout.write("release-plz.toml", PLZ);
    refused(&checkout, &["release-plz.toml"], "names registry x");
}

#[test]
fn two_roots_that_release_the_same_inherited_package_are_refused() {
    let checkout = Checkout::new("twice");
    checkout
        .write("Cargo.toml", "[workspace]\nmembers = [\"a\", \"b\"]\n")
        .package("a", "pkg-a", "")
        .package("b", "pkg-b", "")
        .write(
            "a/release-plz.toml",
            &format!("{PLZ}pr_branch_prefix = \"a-\"\n")
                .replace("{{ package }}-v", "a-{{ package }}-v"),
        )
        .write(
            "b/release-plz.toml",
            &format!("{PLZ}pr_branch_prefix = \"b-\"\n")
                .replace("{{ package }}-v", "b-{{ package }}-v"),
        );
    refused(
        &checkout,
        &["a/release-plz.toml", "b/release-plz.toml"],
        "released from several roots",
    );
}

#[test]
fn a_toolchain_the_checkout_could_pick_is_refused() {
    let checkout = Checkout::new("toolchain");
    checkout
        .package("", "app", "")
        .write("release-plz.toml", PLZ);
    let (code, stderr) = checkout.check(&["release-plz.toml"], None);
    assert_eq!(code, Some(1), "{stderr}");
    assert!(stderr.contains("RUSTUP_TOOLCHAIN is not set"), "{stderr}");
}

#[test]
fn an_absolute_release_plz_path_is_checked_like_a_relative_one() {
    let checkout = Checkout::new("absolute");
    checkout
        .package("", "app", "publish = [\"company\"]\n")
        .write("release-plz.toml", PLZ);
    let absolute = checkout.0.join("release-plz.toml");
    let canonical = absolute
        .canonicalize()
        .unwrap_or_else(|e| unreachable!("{e}"));
    refused(
        &checkout,
        &[&canonical.display().to_string()],
        "names registry company",
    );
}

fn partition(checkout: &Checkout, hidden_a: &str, hidden_b: &str) {
    let plz = |prefix: &str, hidden: &str| {
        let extra = if hidden.is_empty() {
            String::new()
        } else {
            format!("\n[[package]]\nname = \"{hidden}\"\nrelease = false\n")
        };
        format!("{PLZ}pr_branch_prefix = \"{prefix}-\"\n{extra}")
            .replace("{{ package }}-v", &format!("{prefix}-{{{{ package }}}}-v"))
    };
    checkout
        .write("Cargo.toml", "[workspace]\nmembers = [\"a\", \"b\"]\n")
        .package("a", "pkg-a", "")
        .package("b", "pkg-b", "")
        .write("a/release-plz.toml", &plz("a", hidden_a))
        .write("b/release-plz.toml", &plz("b", hidden_b));
}

#[test]
fn roots_that_partition_a_shared_workspace_with_release_false_are_accepted() {
    let checkout = Checkout::new("partition");
    partition(&checkout, "pkg-b", "pkg-a");
    accepted(&checkout, &["a/release-plz.toml", "b/release-plz.toml"]);
    partition(&checkout, "pkg-b", "");
    refused(
        &checkout,
        &["a/release-plz.toml", "b/release-plz.toml"],
        "released from several roots",
    );
}
