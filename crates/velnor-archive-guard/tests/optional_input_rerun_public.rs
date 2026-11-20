// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::process::Command;

const CARGO_TOML: &str = "[package]\nname = \"probe\"\nversion = \"0.0.0\"\nedition = \"2021\"\nbuild = \"build.rs\"\n\n[dependencies]\n\n[build-dependencies]\nrustix = { version = \"=1.1.6\", default-features = false, features = [\"fs\", \"process\", \"std\"] }\nsha2 = { version = \"=0.11.1\", default-features = false }\n";
const MAIN: &str = "mod stale_inputs;\nfn main() {\n    if let Err(error) = stale_inputs::verify_unwatched_inputs(env!(\"VELNOR_ARCHIVE_GUARD_ROOT\"), env!(\"VELNOR_ARCHIVE_GUARD_UNWATCHED\")) {\n        eprintln!(\"{error}\");\n        std::process::exit(1);\n    }\n    println!(\"{}\", env!(\"VELNOR_ARCHIVE_GUARD_FINGERPRINT\"));\n}\n";
const MANIFEST: &str = "file Cargo.toml\ntree crates/probe/src\noptional .cargo/config.toml\noptional rust-toolchain\n";

#[derive(Clone, Copy)]
enum Layout {
    OutsideRoot,
    InRoot,
    NestedInRoot,
    SymlinkedIntoRoot,
    SymlinkResolved,
    AliasedAncestor,
}

enum Consumed {
    Fingerprint(String),
    Refused(String),
}

struct Probe {
    root: PathBuf,
    target: PathBuf,
}

impl Probe {
    fn new(base: &Path, layout: Layout) -> Result<Self, Box<dyn Error>> {
        let root = base.join("root");
        let krate = root.join("crates/probe");
        fs::create_dir_all(krate.join("src"))?;
        fs::create_dir_all(root.join("scripts"))?;
        fs::create_dir_all(krate.join("build_support"))?;
        let own = Path::new(env!("CARGO_MANIFEST_DIR"));
        for file in [
            "build.rs",
            "build_support/archive_guard_inputs.rs",
            "build_support/archive_guard_inputs_filesystem.rs",
            "build_support/archive_guard_inputs_filesystem_tests.rs",
            "build_support/archive_guard_watch.rs",
            "src/stale_inputs.rs",
        ] {
            fs::copy(own.join(file), krate.join(file))?;
        }
        fs::write(krate.join("Cargo.toml"), CARGO_TOML)?;
        fs::write(krate.join("src/main.rs"), MAIN)?;
        fs::write(
            root.join("Cargo.toml"),
            "[workspace]\nmembers = [\"crates/probe\"]\nresolver = \"2\"\n",
        )?;
        fs::write(root.join("scripts/archive-guard-inputs.txt"), MANIFEST)?;
        let real = base.join("real-target");
        let target = match layout {
            Layout::OutsideRoot => base.join("target"),
            Layout::InRoot => root.join("target"),
            Layout::NestedInRoot => root.join("target/velnor-roots/probe-slug"),
            Layout::AliasedAncestor => {
                let alias = base.join("alias");
                symlink(base, &alias)?;
                alias.join("root/target")
            }
            Layout::SymlinkedIntoRoot | Layout::SymlinkResolved => {
                fs::create_dir(&real)?;
                symlink(&real, root.join("target"))?;
                if matches!(layout, Layout::SymlinkedIntoRoot) {
                    root.join("target")
                } else {
                    real
                }
            }
        };
        fs::copy(own.join("../../Cargo.lock"), root.join("Cargo.lock"))?;
        Ok(Self { root, target })
    }

    fn build(&self) -> Result<String, Box<dyn Error>> {
        let output = Command::new(velnor_test_support::real_cargo())
            .args(["build", "--offline", "-v", "--manifest-path"])
            .arg(self.root.join("crates/probe/Cargo.toml"))
            .env("CARGO_TARGET_DIR", &self.target)
            .output()?;
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        assert!(output.status.success(), "cargo build failed: {stderr}");
        Ok(stderr)
    }

    fn consume(&self) -> Result<Consumed, Box<dyn Error>> {
        let output = Command::new(self.target.join("debug/probe")).output()?;
        if output.status.success() {
            Ok(Consumed::Fingerprint(
                String::from_utf8(output.stdout)?.trim().to_owned(),
            ))
        } else {
            Ok(Consumed::Refused(String::from_utf8(output.stderr)?))
        }
    }

    fn embedded(&self) -> Result<String, Box<dyn Error>> {
        match self.consume()? {
            Consumed::Fingerprint(value) => Ok(value),
            Consumed::Refused(message) => Err(message.into()),
        }
    }

    fn assert_not_stale(
        &self,
        created: &str,
        appeared: &[&str],
        may_refuse: bool,
    ) -> Result<(), Box<dyn Error>> {
        match self.consume()? {
            Consumed::Fingerprint(value) => assert_eq!(
                value,
                self.expected()?,
                "creating {created} left a stale embedded fingerprint"
            ),
            Consumed::Refused(message) => assert!(
                may_refuse && appeared.iter().any(|path| message.contains(path)),
                "creating {created}: unexpected refusal: {message}"
            ),
        }
        Ok(())
    }

    fn expected(&self) -> Result<String, Box<dyn Error>> {
        Ok(velnor_archive_guard::source_fingerprint(&self.root)?)
    }
}

fn assert_created_inputs_are_noticed(layout: Layout) -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let probe = Probe::new(&temp.path().canonicalize()?, layout)?;
    let may_refuse = !matches!(layout, Layout::OutsideRoot);
    probe.build()?;
    assert_eq!(probe.embedded()?, probe.expected()?);

    fs::create_dir(probe.root.join(".cargo"))?;
    fs::write(probe.root.join(".cargo/config.toml"), "")?;
    probe.build()?;
    probe.assert_not_stale(".cargo/config.toml", &[".cargo/config.toml"], may_refuse)?;

    fs::write(probe.root.join("rust-toolchain"), "1.99.1\n")?;
    probe.build()?;
    probe.assert_not_stale(
        "rust-toolchain",
        &[".cargo/config.toml", "rust-toolchain"],
        may_refuse,
    )?;
    Ok(())
}

fn assert_unchanged_crate_is_fresh(layout: Layout) -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let probe = Probe::new(&temp.path().canonicalize()?, layout)?;
    probe.build()?;
    let second = probe.build()?;
    assert!(
        !second.contains("Compiling probe") && !second.contains("Running `"),
        "an unchanged crate rebuilt: {second}"
    );
    Ok(())
}

#[test]
fn optional_input_created_with_the_target_outside_the_root_is_noticed() -> Result<(), Box<dyn Error>>
{
    assert_created_inputs_are_noticed(Layout::OutsideRoot)
}

#[test]
fn optional_root_input_created_with_the_target_in_the_root_is_noticed() -> Result<(), Box<dyn Error>>
{
    assert_created_inputs_are_noticed(Layout::InRoot)
}

#[test]
fn optional_root_input_created_with_a_nested_root_target_is_noticed() -> Result<(), Box<dyn Error>>
{
    assert_created_inputs_are_noticed(Layout::NestedInRoot)
}

#[test]
fn optional_root_input_created_with_a_symlinked_target_is_noticed() -> Result<(), Box<dyn Error>> {
    assert_created_inputs_are_noticed(Layout::SymlinkedIntoRoot)
}

#[test]
fn optional_root_input_created_with_a_resolved_symlink_target_is_noticed()
-> Result<(), Box<dyn Error>> {
    assert_created_inputs_are_noticed(Layout::SymlinkResolved)
}

#[test]
fn absent_optional_inputs_leave_an_unchanged_crate_fresh_outside_the_root()
-> Result<(), Box<dyn Error>> {
    assert_unchanged_crate_is_fresh(Layout::OutsideRoot)
}

#[test]
fn absent_optional_inputs_leave_an_unchanged_crate_fresh_in_the_root() -> Result<(), Box<dyn Error>>
{
    assert_unchanged_crate_is_fresh(Layout::InRoot)
}

#[test]
fn absent_optional_inputs_leave_an_unchanged_crate_fresh_with_a_nested_root_target()
-> Result<(), Box<dyn Error>> {
    assert_unchanged_crate_is_fresh(Layout::NestedInRoot)
}

#[test]
fn absent_optional_inputs_leave_an_unchanged_crate_fresh_with_a_symlinked_target()
-> Result<(), Box<dyn Error>> {
    assert_unchanged_crate_is_fresh(Layout::SymlinkedIntoRoot)
}

#[test]
fn absent_optional_inputs_leave_an_unchanged_crate_fresh_with_a_resolved_symlink_target()
-> Result<(), Box<dyn Error>> {
    assert_unchanged_crate_is_fresh(Layout::SymlinkResolved)
}

#[test]
fn optional_root_input_created_with_a_target_through_a_symlinked_ancestor_is_noticed()
-> Result<(), Box<dyn Error>> {
    assert_created_inputs_are_noticed(Layout::AliasedAncestor)
}

#[test]
fn absent_optional_inputs_leave_an_unchanged_crate_fresh_with_a_target_through_a_symlinked_ancestor()
-> Result<(), Box<dyn Error>> {
    assert_unchanged_crate_is_fresh(Layout::AliasedAncestor)
}
