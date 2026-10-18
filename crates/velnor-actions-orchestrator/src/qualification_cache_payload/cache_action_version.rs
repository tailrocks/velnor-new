//! Version input used by pinned `actions/cache` on Unix runners.

use crate::cache_action_stage::CacheActionCompression;
use sha2::{Digest, Sha256};
use std::path::{Component, Path};

const VERSION_SALT: &str = "1.0";
const MAX_ACTION_PATH_BYTES: usize = 4_096;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CompressionMethod {
    Gzip,
    ZstdWithoutLong,
}

impl CompressionMethod {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Gzip => "gzip",
            Self::ZstdWithoutLong => "zstd-without-long",
        }
    }
}

/// Give the pinned `@actions/cache` 6.1.0 version for one exact Unix path
/// input. The action hashes the original `paths` array, compression, and salt.
pub(super) fn version_for_action_path(
    action_path: &Path,
    compression: CompressionMethod,
) -> Result<String, &'static str> {
    let path = clean_unix_path(action_path)?;
    let preimage = format!("{path}|{}|{VERSION_SALT}", compression.as_str());
    Ok(lower_hex(&Sha256::digest(preimage.as_bytes())))
}

/// Give the pinned cache version for the exact method observed by the tar adapter.
pub(super) fn version_for_action_compression(
    action_path: &Path,
    compression: CacheActionCompression,
) -> Result<String, &'static str> {
    let path = clean_unix_path(action_path)?;
    let preimage = format!("{path}|{}|{VERSION_SALT}", compression.as_str());
    Ok(lower_hex(&Sha256::digest(preimage.as_bytes())))
}

/// Check one backend version against the exact method observed for this restore.
pub(super) fn version_agrees_with_action_compression(
    action_path: &Path,
    compression: CacheActionCompression,
    version: &str,
) -> Result<bool, &'static str> {
    Ok(version_for_action_compression(action_path, compression)? == version)
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

/// Return true when an API cache version agrees with the pinned Unix action
/// for this exact one-path input.
pub(super) fn version_agrees_with_action_path(
    action_path: &Path,
    version: &str,
) -> Result<bool, &'static str> {
    let gzip = version_for_action_path(action_path, CompressionMethod::Gzip)?;
    let zstd = version_for_action_path(action_path, CompressionMethod::ZstdWithoutLong)?;
    Ok(version == gzip || version == zstd)
}

fn clean_unix_path(path: &Path) -> Result<&str, &'static str> {
    let text = path.to_str().ok_or("cache_action_path_invalid")?;
    if text.len() > MAX_ACTION_PATH_BYTES
        || !text.starts_with('/')
        || text == "/"
        || text.bytes().any(|byte| byte.is_ascii_control())
        || text.contains('|')
        || text.split('/').skip(1).any(str::is_empty)
        || path.components().any(|component| {
            matches!(
                component,
                Component::CurDir | Component::ParentDir | Component::Prefix(_)
            )
        })
    {
        return Err("cache_action_path_invalid");
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::{
        CompressionMethod, version_agrees_with_action_compression, version_for_action_compression,
        version_for_action_path,
    };
    use crate::cache_action_stage::CacheActionCompression;
    use std::path::Path;

    const STAGE: &str =
        "/home/runner/_work/_temp/velnor/normal-cache-v3/mise_tools/0123456789abcdef/payload";

    #[test]
    fn pinned_action_version_uses_path_compression_and_salt() {
        assert_eq!(
            version_for_action_path(Path::new(STAGE), CompressionMethod::Gzip),
            Ok("d861d3196eba70bc05d9b3674f5e08d79b3791bfe105859713b70db3fc65b589".to_owned())
        );
        assert_eq!(
            version_for_action_path(Path::new(STAGE), CompressionMethod::ZstdWithoutLong),
            Ok("dd928ceb89dc405d95cab9889df73867015c4d6bdc1243d0bd3e560a6871c137".to_owned())
        );
    }

    #[test]
    fn path_or_compression_change_selects_a_different_version() {
        let gzip = version_for_action_path(Path::new(STAGE), CompressionMethod::Gzip)
            .expect("checked gzip version");
        let changed_path = version_for_action_path(
            Path::new("/home/runner/_work/_temp/velnor/other/payload"),
            CompressionMethod::Gzip,
        )
        .expect("checked changed path version");
        let changed_compression =
            version_for_action_path(Path::new(STAGE), CompressionMethod::ZstdWithoutLong)
                .expect("checked zstd version");
        assert_ne!(gzip, changed_path);
        assert_ne!(gzip, changed_compression);
    }

    #[test]
    fn exact_observed_sdk_method_selects_one_pinned_cache_version() {
        let path = Path::new(STAGE);
        let gzip = version_for_action_compression(path, CacheActionCompression::Gzip)
            .expect("gzip version");
        let zstd_without_long =
            version_for_action_compression(path, CacheActionCompression::ZstdWithoutLong)
                .expect("zstd without long version");
        let zstd = version_for_action_compression(path, CacheActionCompression::Zstd)
            .expect("long-range zstd version");
        assert_eq!(
            gzip,
            "d861d3196eba70bc05d9b3674f5e08d79b3791bfe105859713b70db3fc65b589"
        );
        assert_eq!(
            zstd_without_long,
            "dd928ceb89dc405d95cab9889df73867015c4d6bdc1243d0bd3e560a6871c137"
        );
        assert_ne!(gzip, zstd);
        assert_ne!(zstd_without_long, zstd);
        assert!(
            version_agrees_with_action_compression(path, CacheActionCompression::Zstd, &zstd)
                .expect("compare observed version")
        );
        assert!(
            !version_agrees_with_action_compression(path, CacheActionCompression::Gzip, &zstd)
                .expect("reject mismatched compression")
        );
    }

    #[test]
    fn action_path_must_be_one_clean_absolute_unix_path() {
        for path in [
            "relative/payload",
            "/tmp/../payload",
            "/tmp/line\nbreak",
            "/tmp/cache|payload",
        ] {
            assert_eq!(
                version_for_action_path(Path::new(path), CompressionMethod::Gzip),
                Err("cache_action_path_invalid")
            );
        }
    }
}
