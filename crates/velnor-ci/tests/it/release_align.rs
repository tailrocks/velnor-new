// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use crate::support::fresh_dir;

const LEAD_ROOT: &str = "[workspace]\nmembers = [\"crates/*\"]\nresolver = \"3\"\n";
const LEAD_CI: &str = "[package]\nname = \"velnor-ci\"\nversion = \"0.1.83\"\n\n[dependencies]\n# the generator moves with the group\nvelnor-gen = { version = \"=0.1.81\", path = \"../../../follower/crates/velnor-gen\" }\nvelnor-model = { version = \"=0.0.0\", path = \"../../../follower/crates/velnor-model\" }\n";
const LEAD_LOCK: &str = "version = 4\n\n[[package]]\nname = \"velnor-ci\"\nversion = \"0.1.83\"\n\n[[package]]\nname = \"velnor-gen\"\nversion = \"0.1.81\"\n\n[[package]]\nname = \"velnor-model\"\nversion = \"0.0.0\"\n";
const FOLLOWER_ROOT: &str = "[workspace]\nmembers = [\"crates/velnor-gen\", \"crates/velnor-model\"]\n\n[workspace.package]\nversion = \"0.0.0\"\n";
const GEN: &str = "[package]\nname = \"velnor-gen\"\nversion = \"0.1.81\" # moves with velnor-ci\n\n[dependencies]\nvelnor-model = { version = \"=0.0.0\", path = \"../velnor-model\" }\n";
const MODEL: &str = "[package]\nname = \"velnor-model\"\nversion.workspace = true\n";
const FOLLOWER_LOCK: &str = "version = 4\n\n[[package]]\nname = \"serde\"\nversion = \"0.1.81\"\nsource = \"registry+https://github.com/rust-lang/crates.io-index\"\n\n[[package]]\nname = \"velnor-gen\"\nversion = \"0.1.81\"\ndependencies = [\n \"serde\",\n]\n\n[[package]]\nname = \"velnor-model\"\nversion = \"0.0.0\"\n";

const FILES: [&str; 8] = [
    "lead/Cargo.toml",
    "lead/crates/velnor-ci/Cargo.toml",
    "lead/Cargo.lock",
    "follower/Cargo.toml",
    "follower/crates/velnor-gen/Cargo.toml",
    "follower/crates/velnor-model/Cargo.toml",
    "follower/Cargo.lock",
    "follower/crates/velnor-gen/src.txt",
];

struct Fixture(PathBuf);

impl Fixture {
    fn new(name: &str) -> Self {
        let fixture = Self(fresh_dir("velnor-ci-align", name));
        fixture.write("lead/Cargo.toml", LEAD_ROOT);
        fixture.write("lead/crates/velnor-ci/Cargo.toml", LEAD_CI);
        fixture.write("lead/Cargo.lock", LEAD_LOCK);
        fixture.write("follower/Cargo.toml", FOLLOWER_ROOT);
        fixture.write("follower/crates/velnor-gen/Cargo.toml", GEN);
        fixture.write("follower/crates/velnor-model/Cargo.toml", MODEL);
        fixture.write("follower/Cargo.lock", FOLLOWER_LOCK);
        fixture.write("follower/crates/velnor-gen/src.txt", "untouched");
        fixture
    }

    fn write(&self, name: &str, text: &str) {
        let path = self.0.join(name);
        fs::create_dir_all(path.parent().unwrap_or(Path::new(".")))
            .unwrap_or_else(|e| unreachable!("{e}"));
        fs::write(path, text).unwrap_or_else(|e| unreachable!("{e}"));
    }

    fn text(&self, name: &str) -> String {
        fs::read_to_string(self.0.join(name)).unwrap_or_else(|e| unreachable!("{e}"))
    }

    fn snapshot(&self) -> Vec<String> {
        FILES
            .iter()
            .map(|name| fs::read_to_string(self.0.join(name)).unwrap_or_default())
            .collect()
    }

    fn align(&self, members: &[&str]) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_velnor-ci"));
        command
            .args(["release", "align"])
            .args(["--lead", "lead/Cargo.toml", "--lead-package", "velnor-ci"])
            .args(["--follower", "follower/Cargo.toml"]);
        for member in members {
            command.args(["--member", member]);
        }
        command
            .current_dir(&self.0)
            .output()
            .unwrap_or_else(|e| unreachable!("{e}"))
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ignored = fs::remove_dir_all(&self.0);
    }
}

fn refused(fixture: &Fixture, members: &[&str], expected: &str) {
    let before = fixture.snapshot();
    let output = fixture.align(members);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(1), "{stderr}");
    assert!(stderr.contains(expected), "{expected}: {stderr}");
    assert_eq!(fixture.snapshot(), before, "a refusal must write nothing");
}

#[test]
fn the_follower_takes_the_lead_version_in_manifests_pins_and_both_locks() {
    let fixture = Fixture::new("aligns");
    let output = fixture.align(&["velnor-gen"]);
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert_eq!(
        fixture.text("follower/crates/velnor-gen/Cargo.toml"),
        GEN.replace("\"0.1.81\"", "\"0.1.83\"")
    );
    assert_eq!(
        fixture.text("lead/crates/velnor-ci/Cargo.toml"),
        LEAD_CI.replace("=0.1.81", "=0.1.83")
    );
    assert_eq!(
        fixture.text("follower/Cargo.lock"),
        FOLLOWER_LOCK.replace(
            "velnor-gen\"\nversion = \"0.1.81",
            "velnor-gen\"\nversion = \"0.1.83"
        )
    );
    assert_eq!(
        fixture.text("lead/Cargo.lock"),
        LEAD_LOCK.replace(
            "velnor-gen\"\nversion = \"0.1.81",
            "velnor-gen\"\nversion = \"0.1.83"
        )
    );
    assert_eq!(
        fixture.text("follower/crates/velnor-model/Cargo.toml"),
        MODEL
    );
    assert_eq!(fixture.text("follower/Cargo.toml"), FOLLOWER_ROOT);
}

#[test]
fn a_second_run_changes_nothing() {
    let fixture = Fixture::new("idempotent");
    assert_eq!(fixture.align(&["velnor-gen"]).status.code(), Some(0));
    let aligned = fixture.snapshot();
    let output = fixture.align(&["velnor-gen"]);
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert_eq!(fixture.snapshot(), aligned);
    assert!(String::from_utf8_lossy(&output.stdout).contains("already aligned at 0.1.83"));
}

#[test]
fn a_member_ahead_of_the_lead_is_refused() {
    let fixture = Fixture::new("ahead");
    fixture.write(
        "follower/crates/velnor-gen/Cargo.toml",
        &GEN.replace("0.1.81", "0.1.90"),
    );
    refused(&fixture, &["velnor-gen"], "ahead of velnor-ci");
}

#[test]
fn a_manifest_that_is_not_toml_is_refused() {
    let fixture = Fixture::new("unparsable");
    fixture.write(
        "follower/crates/velnor-model/Cargo.toml",
        "[package\nname = ",
    );
    refused(&fixture, &["velnor-gen"], "is not valid TOML");
}

#[test]
fn a_package_outside_the_follower_workspace_is_not_a_group_member() {
    let fixture = Fixture::new("stranger");
    refused(
        &fixture,
        &["velnor-ci"],
        "not a member of the release group",
    );
}

#[test]
fn a_member_that_inherits_its_version_is_refused() {
    let fixture = Fixture::new("inherited");
    refused(&fixture, &["velnor-model"], "inherited from the workspace");
}

#[test]
fn a_pin_that_is_not_exact_is_refused() {
    let fixture = Fixture::new("range");
    fixture.write(
        "lead/crates/velnor-ci/Cargo.toml",
        &LEAD_CI.replace("=0.1.81", "0.1"),
    );
    refused(&fixture, &["velnor-gen"], "not the exact pin");
}

#[test]
fn a_follower_without_a_lock_is_refused() {
    let fixture = Fixture::new("nolock");
    fs::remove_file(fixture.0.join("follower/Cargo.lock")).unwrap_or_else(|e| unreachable!("{e}"));
    let before = fixture.snapshot();
    let output = fixture.align(&["velnor-gen"]);
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(fixture.snapshot(), before);
}
