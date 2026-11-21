// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::process::Command;

use crate::io::Repo;

use super::fixture::{TestResult, setup};

/// The CI shape: the job's `cargo-nextest` runs the tests and never passes through the wrapper, so
/// the test processes only see what the job exported. What the wrapper leaves on disk and in the
/// environment of the cargo it launches is how `velnor-test-support` finds the real cargo.
#[test]
fn the_forward_wrapper_exports_and_records_the_real_cargo_it_launches() -> TestResult {
    let repo = Repo::empty("crates-mirror-real-export");
    let root = repo.root();
    repo.write_unstaged("ca.pem", "not used by this fixture");
    let output = setup(root, "sparse+https://mirror.example/index/");
    assert!(output.status.success(), "{output:?}");
    let wrapper = fs::canonicalize(root.join("velnor-cargo/cargo"))?;
    let config = fs::read_to_string(root.join("env"))?
        .lines()
        .find_map(|line| line.strip_prefix("VELNOR_CARGO_CONFIG=").map(str::to_owned))
        .ok_or("exported Cargo config")?;
    repo.write_unstaged(
        "real/cargo",
        "#!/bin/sh\nprintf 'real=%s\\n' \"$VELNOR_CARGO_REAL\" > \"$PROBE_OUT\"\n",
    );
    fs::set_permissions(root.join("real/cargo"), fs::Permissions::from_mode(0o755))?;
    let real = fs::canonicalize(root.join("real/cargo"))?;

    let output = Command::new(&wrapper)
        .arg("probe")
        .current_dir(root)
        .env("PROBE_OUT", root.join("probe.txt"))
        .env("VELNOR_CARGO_CONFIG", config)
        .env("VELNOR_CARGO_WRAPPER", &wrapper)
        .env("CARGO_HOME", root.join("home"))
        .env(
            "PATH",
            std::env::join_paths([root.join("real"), "/usr/bin".into(), "/bin".into()])?,
        )
        .env_remove("VELNOR_CARGO_REAL")
        .env_remove("VELNOR_CARGO_RUSTUP")
        .env_remove("VELNOR_CARGO_DEPTH")
        .env_remove("__MISE_ACTIVE_WRAPPERS")
        .output()?;
    assert!(output.status.success(), "{output:?}");

    let seen = fs::read_to_string(root.join("probe.txt"))?;
    assert_eq!(seen, format!("real={}\n", real.display()));
    let recorded = fs::metadata(wrapper.with_file_name("velnor-cargo-real"))?;
    let real = fs::metadata(real)?;
    assert_eq!((recorded.dev(), recorded.ino()), (real.dev(), real.ino()));
    Ok(())
}
