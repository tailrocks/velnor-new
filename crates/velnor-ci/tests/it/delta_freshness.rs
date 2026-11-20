// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::time::{Duration, SystemTime};
use velnor_test_support::real_cargo;

use crate::delta::success;
use crate::delta_source::{helper_at, strip_mirror_overrides};
use crate::io::Repo;

pub(super) struct Fixture {
    pub(super) repo: Repo,
    pub(super) directory: PathBuf,
    identity: String,
}

impl Fixture {
    pub(super) fn new(name: &str, nested: bool) -> Self {
        let mut fixture = Self::unbuilt(name, nested);
        fixture.build_baseline(nested);
        fixture
    }

    pub(super) fn unbuilt(name: &str, nested: bool) -> Self {
        let repo = Repo::new(name);
        let directory = if nested {
            repo.root().join("nested")
        } else {
            repo.root().to_owned()
        };
        let prefix = if nested { "nested/" } else { "" };
        if nested {
            repo.write(
                "Cargo.toml",
                "[workspace]\nmembers=['outer']\nexclude=['nested']\nresolver='3'\n",
            );
            repo.write(
                "outer/Cargo.toml",
                "[package]\nname='outer'\nversion='0.1.0'\nedition='2024'\n",
            );
            repo.write("outer/src/lib.rs", "pub fn value() -> u32 { 1 }\n");
        }
        repo.write("mise.toml", "[tools]\n");
        repo.write(
            ".velnor/rust-ci.toml",
            &format!("exclude=['{prefix}local','{prefix}stable']\n"),
        );
        repo.write(".cargo/config.toml", "[build]\njobs=2\n");
        repo.write(&format!("{prefix}Cargo.toml"), "[workspace]\nmembers=['app','peer','spare']\nexclude=['local','stable']\nresolver='3'\n");
        for name in ["app", "peer", "spare", "local", "stable"] {
            let dependencies = match name {
                "app" => "[dependencies]\nlocal={path='../local'}\nstable={path='../stable'}\n",
                "peer" => "[dependencies]\napp={path='../app'}\n",
                _ => "",
            };
            repo.write(
                &format!("{prefix}{name}/Cargo.toml"),
                &format!(
                    "[package]\nname='{name}'\nversion='0.1.0'\nedition='2024'\n{dependencies}"
                ),
            );
            let body = match name {
                "app" => "local::value() + ::stable::value()",
                "peer" => "app::value()",
                _ => "1",
            };
            repo.write(
                &format!("{prefix}{name}/src/lib.rs"),
                &format!("mod stable;\npub fn value() -> u32 {{ stable::touch(); {body} }}\n"),
            );
            repo.write(
                &format!("{prefix}{name}/src/stable.rs"),
                "pub fn touch() {}\n",
            );
        }
        Self {
            repo,
            directory,
            identity: String::new(),
        }
    }

    pub(super) fn build_baseline(&mut self, nested: bool) {
        let lock = ["generate-lockfile", "--offline"];
        success(&self.cargo(&lock));
        if nested {
            success(&self.cargo_at(&lock, self.repo.root()));
        }
        velnor_gen::io::write_metadata(self.repo.root()).unwrap_or_else(|e| unreachable!("{e}"));
        self.repo.git(&["add", "--all"]);
        let checkout = velnor_gen::io::read_recorded_checkout(self.repo.root())
            .unwrap_or_else(|e| unreachable!("{e}"));
        let facts = velnor_rust::detect(&checkout.view).unwrap_or_else(|e| unreachable!("{e}"));
        assert_eq!(facts.rust_roots.len(), if nested { 2 } else { 1 });
        let (output, values) = self.delta_result("prepare");
        success(&output);
        values
            .lines()
            .find_map(|line| line.strip_prefix("source_identity="))
            .unwrap_or_else(|| unreachable!("{values}"))
            .clone_into(&mut self.identity);
        self.delta("apply");
        let output = self.build();
        success(&output);
        self.repo
            .write_unstaged("velnor-run.log", &String::from_utf8_lossy(&output.stderr));
    }

    pub(super) fn helper(&self, args: &[&str]) -> (Output, String) {
        helper_at(&self.repo, args, Some(&self.identity), &self.directory)
    }

    pub(super) fn delta(&self, verb: &str) {
        let (output, values) = self.delta_result(verb);
        success(&output);
        if verb == "stage" {
            assert_eq!(values, "save=true\n");
        }
        println!("{}", String::from_utf8_lossy(&output.stdout));
    }

    fn delta_result(&self, verb: &str) -> (Output, String) {
        let nested = self.directory != self.repo.root();
        helper_at(
            &self.repo,
            &[
                "delta",
                verb,
                "--target-dir",
                if nested { "nested/target" } else { "target" },
                "--staging-dir",
                if nested { "nested/delta" } else { "delta" },
                "--package",
                "app",
            ],
            Some(&self.identity),
            self.repo.root(),
        )
    }

    fn cargo(&self, args: &[&str]) -> Output {
        self.cargo_at(args, &self.directory)
    }

    fn cargo_at(&self, args: &[&str], directory: &std::path::Path) -> Output {
        let mut command = Command::new(real_cargo());
        command
            .args(args)
            .current_dir(directory)
            .env("CARGO_TARGET_DIR", directory.join("target"))
            .env_remove("RUSTC_WRAPPER")
            .env_remove("RUSTC_WORKSPACE_WRAPPER");
        strip_mirror_overrides(&mut command);
        command
            .output()
            .unwrap_or_else(|e| unreachable!("{e}"))
    }

    pub(super) fn build(&self) -> Output {
        self.cargo(&[
            "build",
            "--workspace",
            "--offline",
            "--verbose",
            "--color",
            "never",
        ])
    }

    fn incomplete_watchsets(&self, empty: bool) {
        let entries = fs::read_dir(self.directory.join("target/debug/.fingerprint"))
            .unwrap_or_else(|e| unreachable!("{e}"));
        let mut count = 0;
        for entry in entries {
            let entry = entry.unwrap_or_else(|e| unreachable!("{e}"));
            for file in fs::read_dir(entry.path()).unwrap_or_else(|e| unreachable!("{e}")) {
                let file = file.unwrap_or_else(|e| unreachable!("{e}"));
                if !file.file_name().to_string_lossy().starts_with("dep-") {
                    continue;
                }
                let bytes = if empty {
                    vec![1, 0, 0, 0, 255, 1, 0, 0, 0, 0, 0, 0, 0, 0]
                } else {
                    let mut bytes = vec![1, 0, 0, 0, 255, 1, 1, 0, 0, 0, 0, 13, 0, 0, 0];
                    bytes.extend_from_slice(b"src/stable.rs");
                    bytes.extend_from_slice(&[0, 0, 0, 0, 0]);
                    bytes
                };
                fs::write(file.path(), bytes).unwrap_or_else(|e| unreachable!("{e}"));
                count += 1;
            }
        }
        assert!(count >= 5);
        success(&self.helper(&["inputs", "save"]).0);
        self.delta("stage");
    }

    pub(super) fn change(&self, path: &str, contents: &str) {
        let path = self.directory.join(path);
        fs::write(&path, contents).unwrap_or_else(|e| unreachable!("{e}"));
        fs::File::options()
            .write(true)
            .open(path)
            .and_then(|file| {
                file.set_times(
                    fs::FileTimes::new()
                        .set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(1)),
                )
            })
            .unwrap_or_else(|e| unreachable!("{e}"));
    }

    pub(super) fn restore(&self) {
        fs::remove_dir_all(self.directory.join("target")).unwrap_or_else(|e| unreachable!("{e}"));
        self.delta("apply");
        success(&self.helper(&["inputs", "restore"]).0);
    }
}

pub(super) fn assert_units(output: &Output, compiled: &[&str], fresh: &[&str]) {
    success(output);
    let log = String::from_utf8_lossy(&output.stderr);
    for name in compiled {
        assert!(log.contains(&format!("Compiling {name} ")), "{log}");
    }
    for name in fresh {
        assert!(log.contains(&format!("Fresh {name} ")), "{log}");
    }
}

fn changed_source_is_checked(empty: bool) {
    let fixture = Fixture::new("delta-watchsets", false);
    fixture.incomplete_watchsets(empty);
    fixture.restore();
    assert_units(
        &fixture.build(),
        &[],
        &["app", "peer", "spare", "local", "stable"],
    );
    fixture
        .repo
        .write_unstaged("velnor-run.log", "Compiling app v0.1.0\n");
    success(&fixture.helper(&["inputs", "save"]).0);
    fixture.delta("stage");
    fixture.change(
        "app/src/lib.rs",
        "compile_error!(\"current source rejected\");\n",
    );
    fixture.restore();
    let output = fixture.build();
    fs::remove_dir_all(fixture.directory.join("target")).unwrap_or_else(|e| unreachable!("{e}"));
    let cold = fixture.build();
    assert!(!cold.status.success(), "{cold:?}");
    assert!(String::from_utf8_lossy(&cold.stderr).contains("current source rejected"));
    assert!(
        !output.status.success(),
        "changed source was reused: {output:?}"
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("current source rejected"));
}

#[test]
fn changed_source_is_checked_despite_empty_cached_watchsets() {
    changed_source_is_checked(true);
}

#[test]
fn changed_source_is_checked_despite_nonempty_incomplete_cached_watchsets() {
    changed_source_is_checked(false);
}

#[test]
fn changed_member_rechecks_every_cached_package() {
    let fixture = Fixture::new("delta-selective-members", false);
    fixture.incomplete_watchsets(false);
    fixture.change("app/src/lib.rs", "pub fn value() -> u32 { 3 }\n");
    fixture.restore();
    assert_units(
        &fixture.build(),
        &["app", "peer", "spare", "local", "stable"],
        &[],
    );
}

#[test]
fn changed_local_dependency_rechecks_every_cached_package() {
    let fixture = Fixture::new("delta-selective-local", false);
    fixture.incomplete_watchsets(false);
    fixture.change("local/src/lib.rs", "pub fn value() -> u32 { 4 }\n");
    fixture.restore();
    assert_units(
        &fixture.build(),
        &["app", "peer", "spare", "local", "stable"],
        &[],
    );
}

#[test]
fn repository_root_apply_rechecks_the_nested_target_workspace() {
    let fixture = Fixture::new("delta-nested-target", true);
    fixture.incomplete_watchsets(false);
    fixture.change(
        "app/src/lib.rs",
        "compile_error!(\"nested current source rejected\");\n",
    );
    fixture.restore();
    let output = fixture.build();
    fs::remove_dir_all(fixture.directory.join("target")).unwrap_or_else(|e| unreachable!("{e}"));
    let cold = fixture.build();
    assert!(!cold.status.success(), "{cold:?}");
    assert!(String::from_utf8_lossy(&cold.stderr).contains("nested current source rejected"));
    assert!(
        !output.status.success(),
        "nested source was reused: {output:?}"
    );
    let log = String::from_utf8_lossy(&output.stderr);
    assert!(log.contains("nested current source rejected"), "{log}");
}

#[test]
fn nested_workspace_parent_configuration_cannot_keep_old_fingerprints() {
    let fixture = Fixture::new("delta-parent-configuration", true);
    fixture.incomplete_watchsets(false);
    fixture.change("../.cargo/config.toml", "[build]\njobs=1\n");
    fixture.restore();
    assert_units(
        &fixture.build(),
        &["app", "peer", "spare", "local", "stable"],
        &[],
    );
}

fn shared_local_source_is_checked(custom_target: bool) {
    let mut fixture = Fixture::unbuilt("delta-custom-local-target", false);
    if custom_target {
        fixture.repo.write(
            "local/Cargo.toml",
            "[package]\nname='local'\nversion='0.1.0'\nedition='2024'\n[lib]\npath='../app/src/shared.rs'\n",
        );
    } else {
        fixture.repo.write(
            "local/src/lib.rs",
            "#[path=\"../../app/src/shared.rs\"]\nmod shared;\npub fn value() -> u32 { shared::value() }\n",
        );
    }
    fixture
        .repo
        .write("app/src/shared.rs", "pub fn value() -> u32 { 1 }\n");
    fixture.build_baseline(false);
    let artifact = fixture.directory.join("target/debug/libspare.rlib");
    let compiled = fs::read(&artifact).unwrap_or_else(|e| unreachable!("{e}"));
    fixture.incomplete_watchsets(false);
    fixture.restore();
    assert_units(
        &fixture.build(),
        &[],
        &["app", "peer", "spare", "local", "stable"],
    );
    fixture
        .repo
        .write_unstaged("velnor-run.log", "Compiling app v0.1.0\n");
    success(&fixture.helper(&["inputs", "save"]).0);
    fixture.delta("stage");
    fixture.change(
        "app/src/shared.rs",
        "compile_error!(\"custom local source rejected\");\n",
    );
    fixture.restore();
    assert_eq!(
        fs::read(&artifact).unwrap_or_else(|e| unreachable!("{e}")),
        compiled
    );
    let restored = fixture.build();
    fs::remove_dir_all(fixture.directory.join("target")).unwrap_or_else(|e| unreachable!("{e}"));
    let cold = fixture.build();
    assert!(!cold.status.success(), "{cold:?}");
    assert!(String::from_utf8_lossy(&cold.stderr).contains("custom local source rejected"));
    assert!(
        !restored.status.success(),
        "custom local source was reused: {restored:?}"
    );
    assert!(String::from_utf8_lossy(&restored.stderr).contains("custom local source rejected"));
}

#[test]
fn custom_local_target_inside_a_member_cannot_reuse_rejected_source() {
    shared_local_source_is_checked(true);
}

#[test]
fn local_module_inside_a_member_cannot_reuse_rejected_source() {
    shared_local_source_is_checked(false);
}
