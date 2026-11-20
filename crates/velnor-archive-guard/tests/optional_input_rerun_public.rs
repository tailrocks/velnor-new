// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const CARGO_TOML: &str = "[package]\nname = \"probe\"\nversion = \"0.0.0\"\nedition = \"2021\"\nbuild = \"build.rs\"\n\n[dependencies]\n\n[build-dependencies]\nrustix = { version = \"=1.1.6\", default-features = false, features = [\"fs\", \"process\", \"std\"] }\nsha2 = { version = \"=0.11.1\", default-features = false }\n";
const MAIN: &str = "fn main() { println!(\"{}\", env!(\"VELNOR_ARCHIVE_GUARD_FINGERPRINT\")); }\n";
const MANIFEST: &str = "file Cargo.toml\ntree crates/probe/src\noptional .cargo/config.toml\noptional rust-toolchain\n";

struct Probe {
    root: PathBuf,
    target: PathBuf,
}

impl Probe {
    fn new(base: &Path, in_root_target: bool) -> Result<Self, Box<dyn Error>> {
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
        let target = if in_root_target {
            root.join("target")
        } else {
            base.join("target")
        };
        fs::copy(own.join("../../Cargo.lock"), root.join("Cargo.lock"))?;
        Ok(Self { root, target })
    }

    fn build(&self) -> Result<String, Box<dyn Error>> {
        let output = Command::new(env!("CARGO"))
            .args(["build", "--offline", "-v", "--manifest-path"])
            .arg(self.root.join("crates/probe/Cargo.toml"))
            .env("CARGO_TARGET_DIR", &self.target)
            .output()?;
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        assert!(output.status.success(), "cargo build failed: {stderr}");
        Ok(stderr)
    }

    fn embedded(&self) -> Result<String, Box<dyn Error>> {
        let output = Command::new(self.target.join("debug/probe")).output()?;
        Ok(String::from_utf8(output.stdout)?.trim().to_owned())
    }

    fn expected(&self) -> Result<String, Box<dyn Error>> {
        Ok(velnor_archive_guard::source_fingerprint(&self.root)?)
    }
}

#[test]
fn optional_input_created_after_the_first_build_reruns_the_build_script()
-> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let probe = Probe::new(&temp.path().canonicalize()?, false)?;
    probe.build()?;
    assert_eq!(probe.embedded()?, probe.expected()?);

    fs::create_dir(probe.root.join(".cargo"))?;
    fs::write(probe.root.join(".cargo/config.toml"), "")?;
    probe.build()?;
    assert_eq!(
        probe.embedded()?,
        probe.expected()?,
        "creating .cargo/config.toml left a stale embedded fingerprint"
    );

    fs::write(probe.root.join("rust-toolchain"), "1.99.1\n")?;
    probe.build()?;
    assert_eq!(
        probe.embedded()?,
        probe.expected()?,
        "creating rust-toolchain left a stale embedded fingerprint"
    );
    Ok(())
}

#[test]
fn absent_optional_inputs_leave_an_unchanged_crate_fresh() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let probe = Probe::new(&temp.path().canonicalize()?, true)?;
    probe.build()?;
    let second = probe.build()?;
    assert!(
        !second.contains("Compiling probe") && !second.contains("Running `"),
        "an unchanged crate rebuilt: {second}"
    );
    Ok(())
}
