// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use velnor_test_support::{is_mirror_wrapper, real_cargo};

const PLZ: &str = "[workspace]\ngit_tag_name = \"{{ package }}-v{{ version }}\"\ngit_release_draft = true\nrelease_always = false\nsemver_check = false\npublish_no_verify = true\n";

struct Checkout(PathBuf);

impl Checkout {
    fn new(name: &str) -> Self {
        let dir = crate::scratch_path("velnor-ci-manifests", name);
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
        self.check_with(files, toolchain, &[])
    }

    fn check_with(
        &self,
        files: &[&str],
        toolchain: Option<&str>,
        extra: &[&str],
    ) -> (Option<i32>, String) {
        let mut command = Command::new(env!("CARGO_BIN_EXE_velnor-ci"));
        command
            .args(["release", "check-config"])
            .args(files)
            .arg("--manifests")
            .args(extra)
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

    fn run_released(&self, files: &[&str]) -> (Option<i32>, String) {
        let toolchain = std::env::var("RUSTUP_TOOLCHAIN").unwrap_or_else(|_| "stable".to_owned());
        self.check_with(files, Some(&toolchain), &["--released-versions"])
    }
}

impl Drop for Checkout {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).ok();
    }
}

/// The search path the stub hands to the real cargo: the job's own, minus every folder whose
/// `cargo` is the crates-mirror wrapper, so no lookup by name can reach the wrapper.
fn exec_path() -> std::ffi::OsString {
    let path = std::env::var_os("PATH").unwrap_or_default();
    let kept = std::env::split_paths(&path).filter(|dir| !is_mirror_wrapper(&dir.join("cargo")));
    std::env::join_paths(kept).unwrap_or_else(|e| unreachable!("{e}"))
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
    let checkout = Checkout::new("excluded ' quote");
    let marker = checkout.0.join("wrapper-ran");
    checkout.write("shadow/cargo", "#!/bin/sh\nexit 99\n");
    let search = std::env::join_paths(std::iter::once(checkout.0.join("shadow")).chain(
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()),
    ))
    .unwrap_or_else(|e| unreachable!("{e}"));
    // Neither the `CARGO` variable nor the cargo path compiled into this test, as is: a CI
    // job that configured the crates mirror has `CARGO` set to the mirror wrapper while this
    // test is compiled and run, so both name the wrapper, which would forward back into this
    // stub forever (each round prepends to PATH until exec fails with E2BIG).
    let cargo = real_cargo();
    let stub = checkout.0.join("bin/cargo");
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
            "bin/cargo",
            "#!/bin/sh\nif [ \"${RUSTC_WRAPPER+x}\" = x ]; then touch \"$VELNOR_TEST_WRAPPER_MARKER\"; fi\nPATH=\"$VELNOR_TEST_EXEC_PATH\"\nexport PATH\nexec \"$VELNOR_TEST_REAL_CARGO\" \"$@\"\n",
        );
    fs::set_permissions(&stub, fs::Permissions::from_mode(0o755))
        .unwrap_or_else(|e| unreachable!("{e}"));
    let path = std::env::join_paths(
        std::iter::once(checkout.0.join("bin")).chain(std::env::split_paths(&search)),
    )
    .unwrap_or_else(|e| unreachable!("{e}"));
    let toolchain = std::env::var("RUSTUP_TOOLCHAIN").unwrap_or_else(|_| "stable".to_owned());
    let output = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .args(["release", "check-config", "release-plz.toml", "--manifests"])
        .current_dir(&checkout.0)
        .env("RUSTUP_TOOLCHAIN", toolchain)
        .env("RUSTC_WRAPPER", "must-be-removed")
        .env("VELNOR_TEST_WRAPPER_MARKER", &marker)
        .env("VELNOR_TEST_REAL_CARGO", cargo)
        .env("VELNOR_TEST_EXEC_PATH", exec_path())
        .env("PATH", path)
        .env_remove("CARGO")
        .env_remove("VELNOR_CARGO_CONFIG")
        .env_remove("VELNOR_CARGO_WRAPPER")
        .env_remove("VELNOR_CARGO_REAL")
        .env_remove("VELNOR_CARGO_RUSTUP")
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!marker.exists(), "RUSTC_WRAPPER reached cargo metadata");
}

#[test]
fn a_cargo_config_hazard_is_refused_before_cargo_metadata_runs() {
    let checkout = Checkout::new("hazard-first");
    let marker = checkout.0.join("cargo-ran");
    let stub = checkout.0.join("bin/cargo");
    checkout
        .package("", "app", "")
        .write("release-plz.toml", PLZ)
        .write(
            "bin/cargo",
            &format!("#!/bin/sh\ntouch {}\nexit 1\n", marker.display()),
        )
        .write(".cargo/config.toml", "[build]\nrustc-wrapper = \"true\"\n");
    fs::set_permissions(&stub, fs::Permissions::from_mode(0o755))
        .unwrap_or_else(|e| unreachable!("{e}"));
    let path = std::env::join_paths(std::iter::once(checkout.0.join("bin")).chain(
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()),
    ))
    .unwrap_or_else(|e| unreachable!("{e}"));
    let output = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .args(["release", "check-config", "release-plz.toml", "--manifests"])
        .current_dir(&checkout.0)
        .env("RUSTUP_TOOLCHAIN", "stable")
        .env("PATH", path)
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(1), "{stderr}");
    assert!(stderr.contains(".cargo/config.toml"), "{stderr}");
    assert!(!marker.exists(), "cargo metadata ran before the refusal");
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

#[test]
fn a_root_whose_members_all_opt_out_of_crates_io_may_set_a_proxy() {
    let checkout = Checkout::new("proxy-opted-out");
    checkout
        .write("Cargo.toml", "[workspace]\nmembers = [\"app\"]\n")
        .package("app", "app", "")
        .write(
            "release-plz.toml",
            &format!("{PLZ}\n[[package]]\nname = \"app\"\ngit_only = true\n"),
        )
        .write(
            ".cargo/config.toml",
            "[http]\nproxy = \"http://proxy.example:8080\"\n",
        );
    accepted(&checkout, &["release-plz.toml"]);
    checkout
        .write("Cargo.toml", "[workspace]\nmembers = [\"app\", \"lib\"]\n")
        .package("lib", "lib", "");
    refused(&checkout, &["release-plz.toml"], "proxy");
}

#[test]
fn a_released_version_that_ends_a_tag_ref_component_in_lock_is_refused_only_where_it_will_be_tagged()
 {
    let checkout = Checkout::new("lock-version");
    let version = |version: &str| {
        checkout
            .write("Cargo.toml", "[workspace]\nmembers = [\"app\"]\n")
            .write(
                "app/Cargo.toml",
                &format!(
                    "[package]\nname = \"app\"\nversion = \"{version}\"\nedition = \"2021\"\n"
                ),
            )
            .write("app/src/lib.rs", "");
    };
    checkout.write("release-plz.toml", PLZ);
    version("1.2.3-alpha.lock");
    accepted(&checkout, &["release-plz.toml"]);
    let (code, stderr) = checkout.run_released(&["release-plz.toml"]);
    assert_eq!(code, Some(1), "{stderr}");
    assert!(stderr.contains("app-v1.2.3-alpha.lock"), "{stderr}");
    version("1.2.3-alpha");
    let (code, stderr) = checkout.run_released(&["release-plz.toml"]);
    assert_eq!(code, Some(0), "{stderr}");
    checkout.write(
        "release-plz.toml",
        &PLZ.replace(
            "{{ package }}-v{{ version }}",
            "v{{ version }}-{{ package }}",
        ),
    );
    version("1.2.3-alpha.lock");
    let (code, stderr) = checkout.run_released(&["release-plz.toml"]);
    assert_eq!(code, Some(0), "{stderr}");
}
