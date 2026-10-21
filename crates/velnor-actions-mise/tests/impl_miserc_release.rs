//! Digest-verified discovery and provisioning of the qualified mise release.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

pub(super) const PINNED_BINARY_ENV: &str = "VELNOR_MISE_REGRESSION_BINARY";
pub(super) const PINNED_MISE: &str = "2026.10.17";

fn output_text(output: &Output) -> String {
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    text
}

pub(super) fn path_mise_is_pinned(path: &Path) -> bool {
    Command::new(path)
        .arg("--version")
        .env("MISE_NO_CONFIG", "1")
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| {
            String::from_utf8_lossy(&output.stdout)
                .split_whitespace()
                .next()
                .map(str::to_owned)
        })
        .is_some_and(|version| version == PINNED_MISE)
}

/// Download the qualified official release into a digest-checked temp cache.
pub(super) fn provision_official_release() -> Result<PathBuf, String> {
    let platform = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => "linux-x64",
        ("macos", "aarch64") => "macos-arm64",
        (os, arch) => return Err(format!("The Mise platform is not qualified: {os}/{arch}.")),
    };
    let expected = official_release_digest(PINNED_MISE)?;
    let cache = std::env::temp_dir().join(format!("velnor-mise-official-{PINNED_MISE}"));
    let binary = cache.join("mise/bin/mise");
    if binary.is_file() && sha256_file(&binary).is_ok_and(|digest| digest == expected) {
        return Ok(binary);
    }
    let staging = cache.with_extension(format!("staging-{}", std::process::id()));
    std::fs::remove_dir_all(&staging).ok();
    std::fs::create_dir_all(&staging).map_err(|err| err.to_string())?;
    let archive = staging.join("mise.tar.gz");
    let url = format!(
        "https://github.com/jdx/mise/releases/download/v{PINNED_MISE}/mise-v{PINNED_MISE}-{platform}.tar.gz"
    );
    let fetched = Command::new("curl")
        .args([
            "--fail",
            "--silent",
            "--show-error",
            "--location",
            "--output",
        ])
        .arg(&archive)
        .arg(&url)
        .output()
        .map_err(|err| format!("curl could not fetch the pinned Mise release. Error: {err}."))?;
    if !fetched.status.success() {
        return Err(format!(
            "The Mise executable on PATH is not version {PINNED_MISE}. Download from {url} failed. Error: {}. Set {PINNED_BINARY_ENV} to an official {PINNED_MISE} executable.",
            output_text(&fetched)
        ));
    }
    let unpacked = Command::new("tar")
        .arg("-xzf")
        .arg(&archive)
        .arg("-C")
        .arg(&staging)
        .output()
        .map_err(|err| err.to_string())?;
    if !unpacked.status.success() {
        return Err(format!("tar failed. Error: {}.", output_text(&unpacked)));
    }
    let staged = staging.join("mise/bin/mise");
    let actual = sha256_file(&staged)?;
    if actual != expected {
        return Err(format!(
            "The downloaded Mise digest does not match: {actual} != {expected}."
        ));
    }
    std::fs::remove_dir_all(&cache).ok();
    match std::fs::rename(&staging, &cache) {
        Ok(()) => Ok(binary),
        // A concurrent test process published the same verified release.
        Err(_) if binary.is_file() => {
            std::fs::remove_dir_all(&staging).ok();
            Ok(binary)
        }
        Err(err) => Err(err.to_string()),
    }
}

pub(super) fn find_on_path(program: &str) -> Result<PathBuf, String> {
    std::env::split_paths(&std::env::var_os("PATH").ok_or_else(|| "PATH is not set.".to_owned())?)
        .map(|dir| dir.join(program))
        .find(|candidate| candidate.is_file())
        .ok_or_else(|| format!("The program {program} was not found on PATH."))
}

pub(super) fn official_release_digest(version: &str) -> Result<&'static str, String> {
    match (version, std::env::consts::OS, std::env::consts::ARCH) {
        ("2026.10.17", "linux", "x86_64") => {
            Ok("fe6dd17e68e09682b31d2e7bb7bb755d22636ac1af5d94621e300f2ca9e9fe8a")
        }
        ("2026.10.17", "macos", "aarch64") => {
            Ok("9e27709db5eb1e900d83149418d3067309f2a5004d4994d94246ab658ba4a3ce")
        }
        _ => Err(format!(
            "The Mise release and platform are not qualified: {version} {}/{}.",
            std::env::consts::OS,
            std::env::consts::ARCH
        )),
    }
}

pub(super) fn sha256_file(path: &Path) -> Result<String, String> {
    for (program, args) in [("shasum", vec!["-a", "256"]), ("sha256sum", vec![])] {
        let output = match Command::new(program).args(args).arg(path).output() {
            Ok(output) if output.status.success() => output,
            _ => continue,
        };
        let line = String::from_utf8_lossy(&output.stdout);
        let digest = line
            .split_whitespace()
            .next()
            .filter(|text| text.len() == 64 && text.chars().all(|ch| ch.is_ascii_hexdigit()))
            .ok_or_else(|| format!("The SHA-256 output from {program} is invalid: {line}."))?;
        return Ok(digest.to_ascii_lowercase());
    }
    Err("Neither shasum nor sha256sum could hash the Mise executable.".to_owned())
}
