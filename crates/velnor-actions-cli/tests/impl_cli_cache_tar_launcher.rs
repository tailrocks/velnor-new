//! Binary tests for installing the trusted tar aliases.

#![cfg(unix)]

use std::error::Error;
use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::PathBuf;
use std::process::{Command, Output};

use crate::impl_cli_tmp::{cleanup, code, fresh_tempdir};
use sha2::{Digest, Sha256};

const OP: &str = "install-cache-tar-aliases-v1";

struct Fixture {
    root: PathBuf,
    helper: PathBuf,
    digest: String,
}

impl Fixture {
    fn new(prefix: &str) -> Result<Self, Box<dyn Error>> {
        let root = fresh_tempdir(prefix)?;
        let helper_dir = root.join("velnor/bin");
        fs::create_dir_all(&helper_dir)?;
        let helper = helper_dir.join("velnor-actions-0.1.0");
        fs::copy(env!("CARGO_BIN_EXE_velnor-actions"), &helper)?;
        fs::set_permissions(&helper, fs::Permissions::from_mode(0o755))?;
        let digest = lower_hex(&Sha256::digest(fs::read(&helper)?));
        Ok(Self {
            root,
            helper,
            digest,
        })
    }

    fn install(&self) -> Result<Output, Box<dyn Error>> {
        let mut command = Command::new(&self.helper);
        command
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("RUNNER_TEMP", &self.root)
            .env("GITHUB_ACTIONS", "true")
            .env("GITHUB_RUN_ID", "700")
            .env("GITHUB_RUN_ATTEMPT", "1")
            .env("VELNOR_INTERNAL_OP", OP)
            .env("VELNOR_CACHE_TAR_VERSION", "0.1.0")
            .env("VELNOR_CACHE_TAR_SHA256", &self.digest);
        Ok(command.output()?)
    }

    fn cleanup(&self) {
        cleanup(&self.root);
    }
}

fn lower_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    output
}

#[test]
fn installed_aliases_execute_the_verified_cli_and_share_its_inode() -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new("cache-tar-launcher")?;
    let installed = fixture.install()?;
    assert_eq!(code(&installed), 0, "{:?}", installed.stderr);

    for alias in ["tar", "gtar"] {
        let output = Command::new(fixture.root.join("velnor/bin").join(alias))
            .arg("--version")
            .env_clear()
            .output()?;
        assert_eq!(code(&output), 0, "alias={alias}: {:?}", output.stderr);
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            "tar (GNU tar) velnor-cache-adapter-v1\n"
        );
    }

    let source = fs::metadata(&fixture.helper)?;
    for alias in ["tar", "gtar"] {
        let metadata = fs::metadata(fixture.root.join("velnor/bin").join(alias))?;
        assert_eq!(metadata.dev(), source.dev());
        assert_eq!(metadata.ino(), source.ino());
    }
    fixture.cleanup();
    Ok(())
}

#[test]
fn foreign_alias_collision_is_preserved_and_partial_link_is_removed() -> Result<(), Box<dyn Error>>
{
    let fixture = Fixture::new("cache-tar-collision")?;
    let foreign = fixture.root.join("velnor/bin/gtar");
    fs::write(&foreign, b"foreign entry")?;
    let before = fs::read(&foreign)?;

    let installed = fixture.install()?;
    assert_ne!(code(&installed), 0);
    assert_eq!(fs::read(&foreign)?, before);
    assert!(!fixture.root.join("velnor/bin/tar").exists());
    assert_eq!(fs::metadata(&fixture.helper)?.nlink(), 1);
    fixture.cleanup();
    Ok(())
}

#[test]
fn digest_mismatch_creates_no_aliases() -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new("cache-tar-digest")?;
    let mut command = Command::new(&fixture.helper);
    let output = command
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("RUNNER_TEMP", &fixture.root)
        .env("GITHUB_ACTIONS", "true")
        .env("GITHUB_RUN_ID", "700")
        .env("GITHUB_RUN_ATTEMPT", "1")
        .env("VELNOR_INTERNAL_OP", OP)
        .env("VELNOR_CACHE_TAR_VERSION", "0.1.0")
        .env("VELNOR_CACHE_TAR_SHA256", "0".repeat(64))
        .output()?;

    assert_ne!(code(&output), 0);
    assert!(!fixture.root.join("velnor/bin/tar").exists());
    assert!(!fixture.root.join("velnor/bin/gtar").exists());
    fixture.cleanup();
    Ok(())
}
