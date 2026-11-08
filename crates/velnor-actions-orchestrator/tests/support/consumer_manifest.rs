// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{error::Error, fmt::Write as _, fs, path::Path};

use sha2::{Digest, Sha256};

pub(crate) fn write(root: &Path, executable: &Path) -> Result<(), Box<dyn Error>> {
    let mut digest = String::new();
    for byte in Sha256::digest(fs::read(executable)?) {
        write!(digest, "{byte:02x}")?;
    }
    let host = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => "x86_64-unknown-linux-gnu",
        ("macos", "aarch64") => "aarch64-apple-darwin",
        ("macos", "x86_64") => "x86_64-apple-darwin",
        _ => return Err("unsupported test host".into()),
    };
    let version = env!("CARGO_PKG_VERSION");
    let targets = velnor_actions_contract::SUPPORTED_TARGETS.iter().map(|target| {
        serde_json::json!({
            "target": target,
            "artifact": format!("https://github.com/tailrocks/velnor-new/releases/download/v{version}/velnor-actions-{version}-{target}"),
            "sha256": if *target == host { digest.as_str() } else { "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa" }
        })
    }).collect::<Vec<_>>();
    let manifest = serde_json::json!({"schema":1,"version":version,"repository":"tailrocks/velnor-new","commit":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","targets":targets});
    fs::write(
        root.join(".velnor/release-manifest.json"),
        manifest.to_string(),
    )?;
    Ok(())
}
