//! dbt-fusion artifact admission.
//!
//! Only `https://public.cdn.getdbt.com/fs/cli/fs-v<version>-<triple>.tar.gz`
//! is admitted, for the declared exact version and the three platforms Mise
//! locks. The checksum must come from the project's `mise.lock`; a lock row
//! without one is a finding, never a skipped check.

use crate::DbtError;
use crate::version::FusionVersion;

/// The only host dbt-fusion archives may come from.
pub const FUSION_HOST: &str = "public.cdn.getdbt.com";

/// Platforms with a published Fusion archive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum FusionPlatform {
    /// Linux x86-64 (`linux-x64`).
    LinuxX64,
    /// Linux arm64 (`linux-arm64`).
    LinuxArm64,
    /// macOS arm64 (`macos-arm64`).
    MacosArm64,
}

impl FusionPlatform {
    /// Every admitted platform.
    pub const ALL: [Self; 3] = [Self::LinuxX64, Self::LinuxArm64, Self::MacosArm64];

    /// Mise platform key.
    #[must_use]
    pub const fn mise_key(self) -> &'static str {
        match self {
            Self::LinuxX64 => "linux-x64",
            Self::LinuxArm64 => "linux-arm64",
            Self::MacosArm64 => "macos-arm64",
        }
    }

    const fn triple(self) -> &'static str {
        match self {
            Self::LinuxX64 => "x86_64-unknown-linux-gnu",
            Self::LinuxArm64 => "aarch64-unknown-linux-gnu",
            Self::MacosArm64 => "aarch64-apple-darwin",
        }
    }
}

/// The admitted archive URL of `version` for `platform`.
#[must_use]
pub fn fusion_url(version: FusionVersion, platform: FusionPlatform) -> String {
    format!(
        "https://{FUSION_HOST}/fs/cli/fs-v{version}-{}.tar.gz",
        platform.triple()
    )
}

/// Check the `dbt-fusion` rows of a `mise.lock` against the declared version.
///
/// For each platform the row must exist, its `url` must equal [`fusion_url`],
/// and it must carry a `checksum`. Findings are sorted.
#[must_use]
pub fn check_mise_lock(lock_text: &str, version: FusionVersion) -> Vec<DbtError> {
    let Ok(lock) = lock_text.parse::<toml::Table>() else {
        return vec![DbtError::bare("dbt_fusion_lock_unparsed")];
    };
    let row = lock
        .get("tools")
        .and_then(|tools| tools.get("dbt-fusion"))
        .and_then(toml::Value::as_array)
        .and_then(|rows| {
            rows.iter().find(|row| {
                row.get("version").and_then(toml::Value::as_str) == Some(&version.to_string())
            })
        });
    let Some(row) = row else {
        return vec![DbtError::bare("dbt_fusion_lock_missing")];
    };
    let mut findings = Vec::new();
    for platform in FusionPlatform::ALL {
        let key = platform.mise_key();
        let Some(entry) = row.get(format!("platforms.{key}")) else {
            findings.push(DbtError::new("dbt_fusion_lock_platform_missing", key));
            continue;
        };
        if entry.get("url").and_then(toml::Value::as_str) != Some(&fusion_url(version, platform)) {
            findings.push(DbtError::new("dbt_fusion_url_untrusted", key));
        }
        if entry
            .get("checksum")
            .and_then(toml::Value::as_str)
            .is_none()
        {
            findings.push(DbtError::new("dbt_fusion_checksum_missing", key));
        }
    }
    findings.sort();
    findings
}
