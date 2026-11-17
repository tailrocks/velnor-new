// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::time::{Duration, SystemTime};

use crate::delta::success;
use crate::delta_source::helper_at;
use crate::io::Repo;

struct Fixture {
    repo: Repo,
    directory: PathBuf,
    identity: String,
}

impl Fixture {
    fn new(name: &str, nested: bool) -> Self {
        let repo = Repo::new(name);
        let directory = if nested {
            repo.root().join("nested")
        } else {
            repo.root().to_owned()
        };
        let prefix = if nested { "nested/" } else { "" };
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
        let mut fixture = Self {
            repo,
            directory,
            identity: String::new(),
        };
        success(&fixture.cargo(&["generate-lockfile", "--offline"]));
        velnor_gen::io::write_metadata(fixture.repo.root()).unwrap_or_else(|e| unreachable!("{e}"));
        fixture.repo.git(&["add", "--all"]);
        let (output, values) = fixture.helper(&[
            "delta",
            "prepare",
            "--target-dir",
            "target",
            "--staging-dir",
            "delta",
        ]);
        success(&output);
        values
            .lines()
            .find_map(|line| line.strip_prefix("source_identity="))
            .unwrap_or_else(|| unreachable!("{values}"))
            .clone_into(&mut fixture.identity);
        fixture.delta("apply");
        let output = fixture.build();
        success(&output);
        fixture
            .repo
            .write_unstaged("velnor-run.log", &String::from_utf8_lossy(&output.stderr));
        fixture
    }

    fn helper(&self, args: &[&str]) -> (Output, String) {
        helper_at(&self.repo, args, Some(&self.identity), &self.directory)
    }

    fn delta(&self, verb: &str) {
        let (output, values) = self.helper(&[
            "delta",
            verb,
            "--target-dir",
            "target",
            "--staging-dir",
            "delta",
            "--package",
            "app",
        ]);
        success(&output);
        if verb == "stage" {
            assert_eq!(values, "save=true\n");
        }
    }

    fn cargo(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO"))
            .args(args)
            .current_dir(&self.directory)
            .env("CARGO_TARGET_DIR", self.directory.join("target"))
            .env_remove("RUSTC_WRAPPER")
            .env_remove("RUSTC_WORKSPACE_WRAPPER")
            .output()
            .unwrap_or_else(|e| unreachable!("{e}"))
    }

    fn build(&self) -> Output {
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

    fn change(&self, path: &str, contents: &str) {
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

    fn restore(&self) {
        fs::remove_dir_all(self.directory.join("target")).unwrap_or_else(|e| unreachable!("{e}"));
        self.delta("apply");
        success(&self.helper(&["inputs", "restore"]).0);
    }
}

fn assert_units(output: &Output, compiled: &[&str], fresh: &[&str]) {
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
fn changed_member_rebuilds_its_dependents_and_preserves_unrelated_units() {
    let fixture = Fixture::new("delta-selective-members", false);
    fixture.incomplete_watchsets(false);
    fixture.change("app/src/lib.rs", "pub fn value() -> u32 { 3 }\n");
    fixture.restore();
    assert_units(
        &fixture.build(),
        &["app", "peer"],
        &["spare", "local", "stable"],
    );
}

#[test]
fn changed_local_dependency_rebuilds_itself_and_its_workspace_dependents() {
    let fixture = Fixture::new("delta-selective-local", false);
    fixture.incomplete_watchsets(false);
    fixture.change("local/src/lib.rs", "pub fn value() -> u32 { 4 }\n");
    fixture.restore();
    assert_units(
        &fixture.build(),
        &["local", "app", "peer"],
        &["spare", "stable"],
    );
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
