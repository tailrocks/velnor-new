// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::sha256::sha256_hex;
use crate::toml_file::table;

/// Digest lines for Cargo configuration read from outside the checkout.
///
/// Every file is hashed by content under its absolute path. A file that is
/// not UTF-8 TOML, or that uses `include`, is refused: the key cannot cover
/// what it pulls in.
pub fn external_config_profile(files: &[(String, Vec<u8>)]) -> Result<String, String> {
    let mut out = String::new();
    for (path, bytes) in files {
        let parsed = table(bytes).map_err(|_| {
            format!("{path} is not valid UTF-8 TOML: the base cache key cannot cover it")
        })?;
        if parsed.contains_key("include") {
            return Err(format!(
                "{path} uses include, which is not supported: the base cache key cannot cover the included files"
            ));
        }
        out = format!("{out}{path}\n{}\n", sha256_hex(bytes));
    }
    Ok(out)
}
