// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{
    error::Error,
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Command,
};

use super::fixture;

mod cargo_1_99_1 {
    pub(super) const COLD_NO_MATCHING_PACKAGE: &str = "error: no matching package named `itoa` found\nlocation searched: crates.io index\nrequired by package `probe v0.1.0 (/work/probe)`\nnote: offline mode (via `--offline`) can sometimes cause surprising resolution failures\nhelp: if this error is too confusing you may wish to retry without `--offline`\n";

    pub(super) const COLD_HTTP_REQUEST: &str = "error: failed to download `itoa v1.0.15`\n\nCaused by:\n  attempting to make an HTTP request, but --offline was specified\n";

    pub(super) const COLD_GIT_CHECKOUT: &str = "error: failed to get `gitdep` as a dependency of package `gitws v0.1.0 (/work/gitws)`\n\nCaused by:\n  failed to load source for dependency `gitdep`\n\nCaused by:\n  unable to update file:///work/gitdep#eb9bf045\n\nCaused by:\n  can't checkout from 'file:///work/gitdep': you are in the offline mode (--offline)\n";

    pub(super) const INVALID_PACKAGE_NAME_ECHO: &str = "error: invalid character `-` in package name: `--offline was specified`, the first character must be a Unicode XID start character (most letters or `_`)\n --> inv/Cargo.toml:2:8\n  |\n2 | name = \"--offline was specified\"\n  |        ^^^^^^^^^^^^^^^^^^^^^^^^^\n";

    pub(super) const TRANSIENT_CONNECT: &str = "    Updating crates.io index\nerror: failed to get `itoa` as a dependency of package `probe v0.1.0 (/work/probe)`\n\nCaused by:\n  failed to load source for dependency `itoa`\n\nCaused by:\n  unable to update registry `crates-io`\n\nCaused by:\n  download of config.json failed\n\nCaused by:\n  [7] Couldn't connect to server (Failed to connect to 127.0.0.1 port 9 after 0 ms: Couldn't connect to server)\n";

    pub(super) const MALFORMED_TOML: &str =
        "error: unclosed table, expected `]`\n --> Cargo.toml:5:5\n  |\n5 | [lib\n  |     ^\n";

    pub(super) const MALFORMED_MANIFEST: &str = "error: failed to parse manifest at `/work/probe/Cargo.toml`\n\nCaused by:\n  feature `x` includes `y` which is neither a dependency nor another feature\n\n  help: a feature with a similar name exists: `x`\n";

    pub(super) const MALFORMED_MANIFEST_NETWORK_PATH: &str = "error: failed to parse manifest at `/work/network/Cargo.toml`\n\nCaused by:\n  feature `timeout` includes `connection` which is neither a dependency nor another feature\n";

    pub(super) const MALFORMED_AFTER_WARNING: &str = "warning: unused manifest key: package.foo\nerror: unclosed table, expected `]`\n --> Cargo.toml:5:5\n";
}

const HOSTILE_REPO_DIRS: [&str; 2] = ["evil;touch PWNED;#", "evil';touch PWNED;#"];

const TRANSIENT_TIMEOUT: &str = "error: operation timed out\n";
const TRANSIENT_UNREACHABLE: &str = "error: network unreachable\n";
const TRANSIENT_AFTER_WARNING: &str =
    "warning: unused manifest key: package.foo\nerror: operation timed out\n";

#[test]
fn cold_registry_generate_names_the_warm_command() -> Result<(), Box<dyn Error>> {
    for stderr in [
        cargo_1_99_1::COLD_NO_MATCHING_PACKAGE,
        cargo_1_99_1::COLD_HTTP_REQUEST,
        cargo_1_99_1::COLD_GIT_CHECKOUT,
    ] {
        let fixture = fixture::Fixture::new()?;
        fixture.fail_qualification(stderr)?;
        let output = fixture.root.join("generated");
        let generated = fixture.generate_unguarded(&output)?;
        let text = fixture::stderr(&generated);
        assert_eq!(generated.status.code(), Some(1), "{text}");
        assert!(
            text.contains(&format!(
                "velnor-actions: cold_registry: Cargo.toml: dependency not in the local registry; run `cargo fetch --locked --manifest-path '{}'` (or the dependency does not exist)",
                fixture.repo.join("Cargo.toml").display()
            )),
            "{text}"
        );
        assert!(!text.contains("preparation_incomplete"), "{text}");
        assert!(!output.exists());
    }
    Ok(())
}

#[test]
fn cold_registry_warm_command_runs_from_a_nested_directory() -> Result<(), Box<dyn Error>> {
    let fixture = fixture::Fixture::new()?;
    fixture.fail_qualification(cargo_1_99_1::COLD_HTTP_REQUEST)?;
    let nested = fixture.repo.join("src");
    let argv = run_warm_command(&fixture, &nested)?;
    assert_eq!(
        argv,
        [
            "fetch".to_owned(),
            "--locked".to_owned(),
            "--manifest-path".to_owned(),
            path_text(&fixture.repo.join("Cargo.toml"))?,
        ]
    );
    assert!(Path::new(&argv[3]).is_file());
    Ok(())
}

#[test]
fn cold_registry_warm_command_is_one_shell_word_for_a_hostile_path() -> Result<(), Box<dyn Error>> {
    for repo_dir in HOSTILE_REPO_DIRS {
        let fixture = fixture::Fixture::with_repo_dir(repo_dir)?;
        fixture.fail_qualification(cargo_1_99_1::COLD_GIT_CHECKOUT)?;
        let argv = run_warm_command(&fixture, &fixture.repo)?;
        assert_eq!(argv.len(), 4, "{argv:?}");
        assert_eq!(argv[3], path_text(&fixture.repo.join("Cargo.toml"))?);
        assert!(Path::new(&argv[3]).is_file());
        assert!(!fixture.root.join("work/PWNED").exists());
    }
    Ok(())
}

fn path_text(path: &Path) -> Result<String, Box<dyn Error>> {
    Ok(path.to_str().ok_or("non-UTF-8 path")?.to_owned())
}

fn run_warm_command(
    fixture: &fixture::Fixture,
    generate_from: &Path,
) -> Result<Vec<String>, Box<dyn Error>> {
    let output = fixture.root.join("generated");
    let generated = fixture.generate_unguarded_from(generate_from, &output)?;
    let text = fixture::stderr(&generated);
    assert_eq!(generated.status.code(), Some(1), "{text}");
    let (_, after) = text.split_once("run `").ok_or(text.as_str())?;
    let (command, _) = after
        .split_once("` (or the dependency does not exist)")
        .ok_or(text.as_str())?;

    let bin: PathBuf = fixture.root.join("warm-bin");
    let work = fixture.root.join("work");
    fs::create_dir_all(&bin)?;
    fs::create_dir_all(&work)?;
    let cargo = bin.join("cargo");
    fs::write(&cargo, "#!/bin/sh\nprintf '%s\\n' \"$@\" > argv.txt\n")?;
    fs::set_permissions(&cargo, fs::Permissions::from_mode(0o700))?;
    let path = std::env::join_paths([bin, PathBuf::from("/usr/bin"), PathBuf::from("/bin")])?;
    let status = Command::new("/bin/sh")
        .args(["-c", command])
        .current_dir(&work)
        .env_clear()
        .env("PATH", path)
        .status()?;
    assert!(status.success(), "{command}");
    Ok(fs::read_to_string(work.join("argv.txt"))?
        .lines()
        .map(str::to_owned)
        .collect())
}

#[test]
fn transient_metadata_failure_is_not_reported_as_cold_registry() -> Result<(), Box<dyn Error>> {
    for (stderr, cause) in [
        (TRANSIENT_TIMEOUT, "operation timed out"),
        (TRANSIENT_UNREACHABLE, "network unreachable"),
        (
            cargo_1_99_1::TRANSIENT_CONNECT,
            "failed to get `itoa` as a dependency of package",
        ),
        (TRANSIENT_AFTER_WARNING, "operation timed out"),
    ] {
        let fixture = fixture::Fixture::new()?;
        fixture.fail_qualification(stderr)?;
        let output = fixture.root.join("generated");
        let generated = fixture.generate_unguarded(&output)?;
        let text = fixture::stderr(&generated);
        assert_eq!(generated.status.code(), Some(1), "{text}");
        assert!(
            text.contains(&format!(
                "preparation_incomplete: Cargo.toml: metadata_transient:error: {cause}"
            )),
            "{text}"
        );
        assert!(!text.contains("Updating crates.io index"), "{text}");
        assert!(!text.contains("unused manifest key"), "{text}");
        assert!(!text.contains("cold_registry"), "{text}");
        assert!(!text.contains("cargo fetch"), "{text}");
        assert!(!output.exists());
    }
    Ok(())
}

#[test]
fn malformed_manifest_failure_is_not_reported_as_cold_registry() -> Result<(), Box<dyn Error>> {
    for (stderr, diagnostic) in [
        (cargo_1_99_1::MALFORMED_TOML, "unclosed table, expected `]`"),
        (
            cargo_1_99_1::INVALID_PACKAGE_NAME_ECHO,
            "metadata_failed:error: invalid character `-` in package name",
        ),
        (
            cargo_1_99_1::MALFORMED_MANIFEST,
            "failed to parse manifest at",
        ),
        (
            cargo_1_99_1::MALFORMED_MANIFEST_NETWORK_PATH,
            "failed to parse manifest at `/work/network/Cargo.toml`",
        ),
        (
            cargo_1_99_1::MALFORMED_AFTER_WARNING,
            "metadata_failed:error: unclosed table, expected `]`",
        ),
    ] {
        let fixture = fixture::Fixture::new()?;
        fixture.fail_qualification(stderr)?;
        let output = fixture.root.join("generated");
        let generated = fixture.generate_unguarded(&output)?;
        let text = fixture::stderr(&generated);
        assert_eq!(generated.status.code(), Some(1), "{text}");
        assert!(text.contains(diagnostic), "{text}");
        assert!(!text.contains("cold_registry"), "{text}");
        assert!(!text.contains("metadata_offline"), "{text}");
        assert!(!text.contains("metadata_transient"), "{text}");
        assert!(!output.exists());
    }
    Ok(())
}
