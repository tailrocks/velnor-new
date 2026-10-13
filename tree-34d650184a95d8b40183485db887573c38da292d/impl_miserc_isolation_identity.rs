use std::path::{Path, PathBuf};
use std::process::Command;

use super::{Fixture, output_text, run_raw};

pub(super) const PINNED_MISE: &str = "2026.10.2";
const FIX_SOURCE_REV: &str = "dfe74a90b41603625ee6aabecb42f14a1f5eb0f6";
const FIX_SOURCE_VERSION: &str = "2026.10.1";
const BINARY_ENV: &str = "VELNOR_MISE_REGRESSION_BINARY";
const BINARY_SHA_ENV: &str = "VELNOR_MISE_REGRESSION_SHA256";
const SOURCE_REV_ENV: &str = "VELNOR_MISE_REGRESSION_SOURCE_REV";

pub(super) struct MiseBinary {
    pub(super) path: PathBuf,
    pub(super) version: String,
    pub(super) known_broken_release: bool,
}

struct BinaryIdentity {
    version: &'static str,
    known_broken_release: bool,
    source_rev: Option<&'static str>,
}

impl MiseBinary {
    pub(super) fn select() -> Result<Self, String> {
        let explicit_binary = std::env::var_os(BINARY_ENV).is_some();
        let path = match std::env::var_os(BINARY_ENV) {
            Some(path) => PathBuf::from(path),
            None => find_on_path("mise")?,
        }
        .canonicalize()
        .map_err(|err| err.to_string())?;
        if !path.is_file() {
            return Err(format!("mise binary is not a file: {}", path.display()));
        }

        // Bind digest, platform, and expected release before the first exec.
        let sha256 = sha256_file(&path)?;
        let identity = identify_binary(explicit_binary, &sha256)?;
        let fixture = Fixture::new(None)?;
        let version_output = run_raw(&path, &fixture, &fixture.root, &["--version"], &[])?;
        if !version_output.status.success() {
            return Err(format!(
                "mise --version failed: {}",
                output_text(&version_output)
            ));
        }
        let version_text = String::from_utf8_lossy(&version_output.stdout);
        let version = version_text
            .split_whitespace()
            .next()
            .ok_or_else(|| "mise --version returned no release identifier".to_owned())?;
        if version != identity.version {
            return Err(format!(
                "mise version disagrees with verified binary identity: {version} != {}",
                identity.version
            ));
        }
        match identity.source_rev {
            Some(rev) => eprintln!("mise source={rev} version={version} sha256={sha256}"),
            None => eprintln!("mise official_release={version} sha256={sha256}"),
        }
        Ok(Self {
            path,
            version: version.to_owned(),
            known_broken_release: identity.known_broken_release,
        })
    }
}

fn identify_binary(explicit_binary: bool, sha256: &str) -> Result<BinaryIdentity, String> {
    if let Some(rev) = std::env::var(SOURCE_REV_ENV).ok() {
        if !explicit_binary || rev != FIX_SOURCE_REV {
            return Err(format!("unexpected fixed-source identity: {rev}"));
        }
        let expected = std::env::var(BINARY_SHA_ENV)
            .map_err(|_| format!("{BINARY_SHA_ENV} required for source binary"))?;
        if sha256 != expected {
            return Err(format!(
                "source binary digest mismatch: {sha256} != {expected}"
            ));
        }
        return Ok(BinaryIdentity {
            version: FIX_SOURCE_VERSION,
            known_broken_release: false,
            source_rev: Some(FIX_SOURCE_REV),
        });
    }

    let identity = official_identity(sha256).ok_or_else(|| {
        format!(
            "unqualified official mise digest/platform: {sha256} {}/{}",
            std::env::consts::OS,
            std::env::consts::ARCH
        )
    })?;
    if !explicit_binary && identity.version != PINNED_MISE {
        return Err(format!(
            "PATH mise {} does not match pin {PINNED_MISE}",
            identity.version
        ));
    }
    Ok(identity)
}

fn official_identity(sha256: &str) -> Option<BinaryIdentity> {
    let (version, known_broken_release) =
        match (std::env::consts::OS, std::env::consts::ARCH, sha256) {
            (
                "macos",
                "aarch64",
                "66d49acecca413c8b334922584982a4907a10588912829873d6c55d0c6d42612",
            ) => ("2026.10.2", false),
            (
                "linux",
                "aarch64",
                "8d230a5a23ac559ea24ec65280867f437000b37473d967a9bb954837592490a8",
            ) => ("2026.10.2", false),
            (
                "linux",
                "x86_64",
                "8f5f6660336f572830e33cd9b378d3131e529a0d4c4f0c553776be90a1ba302a",
            ) => ("2026.10.2", false),
            (
                "macos",
                "aarch64",
                "484c135bd4329975d608d3f77e26c2ece5d2f5590f18ca71f44440294f8cfa6f",
            ) => ("2026.9.18", true),
            (
                "linux",
                "x86_64",
                "d24fe0bf7e613824ad99f7b8dac3f2b381a37b9f75f84dd250855217095a8de4",
            ) => ("2026.9.18", true),
            _ => return None,
        };
    Some(BinaryIdentity {
        version,
        known_broken_release,
        source_rev: None,
    })
}

fn find_on_path(program: &str) -> Result<PathBuf, String> {
    std::env::split_paths(&std::env::var_os("PATH").ok_or_else(|| "PATH is unset".to_owned())?)
        .map(|dir| dir.join(program))
        .find(|candidate| candidate.is_file())
        .ok_or_else(|| format!("{program} not found on PATH"))
}

fn sha256_file(path: &Path) -> Result<String, String> {
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
            .ok_or_else(|| format!("invalid SHA-256 output from {program}: {line}"))?;
        return Ok(digest.to_ascii_lowercase());
    }
    Err("neither shasum nor sha256sum could hash the mise executable".to_owned())
}
