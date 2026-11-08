// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#![cfg(target_os = "linux")]

use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant},
};

const ARCHIVE: &str = "itoa-1.0.18.crate";
const REGISTRY: &str = "registry/cache/index.crates.io-1949cf8c6b5b557f";

struct Fixture {
    temp: tempfile::TempDir,
    workspace: PathBuf,
    home: PathBuf,
    source: PathBuf,
}

impl Fixture {
    fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let workspace = temp.path().join("workspace");
        let lock_dir =
            workspace.join("crates/velnor-actions-cli/tests/fixtures/native-image-qualification");
        fs::create_dir_all(&lock_dir)?;
        fs::copy(
            root().join(
                "crates/velnor-actions-cli/tests/fixtures/native-image-qualification/Cargo.lock",
            ),
            lock_dir.join("Cargo.lock"),
        )?;
        let source = temp.path().join("velnor/cargo").join(REGISTRY);
        let home = temp.path().join("velnor-native-image-qualification/cargo");
        fs::create_dir_all(&source)?;
        fs::create_dir_all(&home)?;
        Ok(Self {
            temp,
            workspace,
            home,
            source,
        })
    }

    fn seed(&self) -> Result<(), Box<dyn std::error::Error>> {
        fs::copy(
            root()
                .join("crates/velnor-actions-cli/tests/fixtures/native-cargo-cache")
                .join(ARCHIVE),
            self.source.join(ARCHIVE),
        )?;
        Ok(())
    }

    fn command(&self) -> Command {
        self.command_for("fixture", &self.home)
    }

    fn command_for(&self, mode: &str, home: &Path) -> Command {
        let mut command = Command::new("/usr/bin/python3");
        command
            .args(["-I", "-S"])
            .arg(root().join("scripts/native-image-cargo-cache.py"))
            .arg(mode)
            .env_clear()
            .env("GITHUB_WORKSPACE", &self.workspace)
            .env("RUNNER_TEMP", self.temp.path())
            .env("CARGO_HOME", home);
        command
    }

    fn run(&self) -> Result<velnor_actions_mise::BoundedCommandOutput, Box<dyn std::error::Error>> {
        let output = velnor_actions_mise::run_command_bounded(
            &mut self.command(),
            64 * 1024,
            Duration::from_secs(5),
            Duration::from_secs(1),
        );
        if output.termination != velnor_actions_mise::BoundedCommandTermination::Completed {
            return Err("archive helper did not complete".into());
        }
        assert!(
            output.status == velnor_actions_mise::BoundedCommandStatus::Exited(0),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        Ok(output)
    }

    fn target(&self) -> PathBuf {
        self.home.join(REGISTRY).join(ARCHIVE)
    }

    fn lock(&self) -> PathBuf {
        self.workspace
            .join("crates/velnor-actions-cli/tests/fixtures/native-image-qualification/Cargo.lock")
    }
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn verified_archive_only_is_copied() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    fixture.seed()?;
    let parent = fixture
        .source
        .parent()
        .ok_or("cache parent")?
        .parent()
        .ok_or("registry parent")?;
    let source_home = parent.parent().ok_or("source home")?;
    fs::write(source_home.join("config.toml"), "poison")?;
    fs::write(source_home.join("credentials.toml"), "poison")?;
    fs::create_dir_all(parent.join("index"))?;
    fs::write(parent.join("index/poison"), "poison")?;
    fs::create_dir_all(parent.join("src"))?;
    fs::write(parent.join("src/poison"), "poison")?;
    fs::create_dir_all(source_home.join("git"))?;
    fs::write(source_home.join("git/poison"), "poison")?;
    let output = fixture.run()?;
    assert!(String::from_utf8(output.stdout)?.contains("copied=1 bytes=15935"));
    assert_eq!(
        fs::read(fixture.target())?,
        fs::read(fixture.source.join(ARCHIVE))?
    );
    assert!(!fixture.home.join("registry/index").exists());
    assert!(!fixture.home.join("registry/src").exists());
    assert!(!fixture.home.join("config.toml").exists());
    assert!(!fixture.home.join("credentials.toml").exists());
    assert!(!fixture.home.join("git").exists());
    Ok(())
}

#[test]
fn partial_workspace_cache_reuses_only_verified_archives() -> Result<(), Box<dyn std::error::Error>>
{
    let fixture = Fixture::new()?;
    fixture.seed()?;
    fs::copy(
        root().join("Cargo.lock"),
        fixture.workspace.join("Cargo.lock"),
    )?;
    fs::write(fixture.source.join("adler2-2.0.1.crate"), b"corrupt")?;
    let home = fixture
        .temp
        .path()
        .join("velnor-qualification-runtime/cargo");
    fs::create_dir_all(&home)?;
    let output = velnor_actions_mise::run_command_bounded(
        &mut fixture.command_for("workspace", &home),
        64 * 1024,
        Duration::from_secs(5),
        Duration::from_secs(1),
    );
    assert_eq!(
        output.status,
        velnor_actions_mise::BoundedCommandStatus::Exited(0)
    );
    assert!(String::from_utf8(output.stdout)?.contains("copied=1 bytes=15935"));
    let target = home.join(REGISTRY);
    assert_eq!(
        fs::read(target.join(ARCHIVE))?,
        fs::read(fixture.source.join(ARCHIVE))?
    );
    assert!(!target.join("adler2-2.0.1.crate").exists());
    assert_eq!(fs::read_dir(target)?.count(), 1);
    Ok(())
}

#[test]
fn noncanonical_sources_never_import_registry_archives() -> Result<(), Box<dyn std::error::Error>> {
    let canonical = "registry+https://github.com/rust-lang/crates.io-index";
    for source in [
        "git+https://example.invalid/repo#1234",
        "registry+https://example.invalid/index",
    ] {
        let fixture = Fixture::new()?;
        fixture.seed()?;
        let lock = fixture.lock();
        let content = fs::read_to_string(&lock)?;
        assert_eq!(content.matches(canonical).count(), 1);
        fs::write(lock, content.replace(canonical, source))?;
        let output = fixture.run()?;
        assert!(String::from_utf8(output.stdout)?.contains("copied=0 bytes=0"));
        assert!(!fixture.target().exists());
    }
    Ok(())
}

#[test]
fn absent_corrupt_and_unsafe_archives_fall_back() -> Result<(), Box<dyn std::error::Error>> {
    for case in [
        "missing", "corrupt", "symlink", "hardlink", "fifo", "oversize",
    ] {
        let fixture = Fixture::new()?;
        let source = fixture.source.join(ARCHIVE);
        match case {
            "missing" => (),
            "corrupt" => {
                fixture.seed()?;
                fs::write(&source, b"corrupt")?;
            }
            "symlink" => std::os::unix::fs::symlink("missing", &source)?,
            "hardlink" => {
                fixture.seed()?;
                fs::hard_link(&source, fixture.source.join("other"))?;
            }
            "fifo" => assert!(Command::new("mkfifo").arg(&source).status()?.success()),
            "oversize" => {
                fs::File::create(&source)?.set_len(128 * 1024 * 1024 + 1)?;
            }
            _ => unreachable!(),
        }
        let output = fixture.run()?;
        assert!(
            String::from_utf8(output.stdout)?.contains("copied=0 bytes=0"),
            "{case}"
        );
        assert!(!fixture.target().exists(), "{case}");
        let entries = fixture.home.join(REGISTRY);
        if entries.exists() {
            assert_eq!(fs::read_dir(entries)?.count(), 0, "{case}");
        }
    }
    Ok(())
}

#[test]
fn malformed_and_fifo_lock_fall_back_without_blocking() -> Result<(), Box<dyn std::error::Error>> {
    for case in ["malformed", "duplicate", "fifo"] {
        let fixture = Fixture::new()?;
        fixture.seed()?;
        let lock = fixture.lock();
        match case {
            "malformed" => fs::write(&lock, "not toml [")?,
            "duplicate" => {
                let contents = fs::read_to_string(&lock)?;
                let package = contents
                    .split("[[package]]")
                    .nth(1)
                    .ok_or("missing package")?;
                fs::write(&lock, format!("{contents}\n[[package]]{package}"))?;
            }
            "fifo" => {
                fs::remove_file(&lock)?;
                assert!(Command::new("mkfifo").arg(&lock).status()?.success());
            }
            _ => unreachable!(),
        }
        let mut child = fixture.command().spawn()?;
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(status) = child.try_wait()? {
                assert!(status.success(), "{case}");
                break;
            }
            if Instant::now() >= deadline {
                child.kill()?;
                child.wait()?;
                panic!("lock blocked: {case}");
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(!fixture.target().exists(), "{case}");
    }
    Ok(())
}

#[test]
fn unsafe_cache_paths_do_not_publish_archives() -> Result<(), Box<dyn std::error::Error>> {
    let source_fixture = Fixture::new()?;
    source_fixture.seed()?;
    let cache = source_fixture.source.parent().ok_or("cache parent")?;
    let moved = cache.with_extension("moved");
    fs::rename(cache, &moved)?;
    std::os::unix::fs::symlink(&moved, cache)?;
    let output = source_fixture.run()?;
    assert!(String::from_utf8(output.stdout)?.contains("copied=0 bytes=0"));
    assert!(!source_fixture.target().exists());

    let target_fixture = Fixture::new()?;
    target_fixture.seed()?;
    std::os::unix::fs::symlink(&target_fixture.source, target_fixture.home.join("registry"))?;
    let output = target_fixture.command().output()?;
    assert!(!output.status.success());
    assert!(!target_fixture.target().exists());
    Ok(())
}

#[test]
fn existing_staging_file_is_never_removed() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    fixture.seed()?;
    let target = fixture.home.join(REGISTRY);
    fs::create_dir_all(&target)?;
    let stage = target.join(format!(".{ARCHIVE}.part"));
    fs::write(&stage, b"existing")?;
    let output = fixture.run()?;
    assert!(String::from_utf8(output.stdout)?.contains("copied=0 bytes=0"));
    assert_eq!(fs::read(stage)?, b"existing");
    assert!(!fixture.target().exists());
    Ok(())
}
