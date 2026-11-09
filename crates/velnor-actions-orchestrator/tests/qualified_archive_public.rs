// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::json;
use sha2::{Digest, Sha256};
use std::{error::Error, fs};

#[test]
fn qualification_rejects_duplicate_zip_paths_before_installation() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let root = fs::canonicalize(temp.path())?;
    let platform = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => "linux_x64",
        ("macos", "aarch64") => "macos_arm64",
        ("macos", "x86_64") => "macos_x64",
        _ => return Ok(()),
    };
    for (duplicate, bytes, expected_sha, expected_len) in [
        (
            false,
            include_bytes!("fixtures/duplicate_zip/tool-single.zip").as_slice(),
            "9c8c806d9bcc0d95c7763d5f02c24126c08353178b6c8de0f8e5d62537718815",
            146,
        ),
        (
            true,
            include_bytes!("fixtures/duplicate_zip/tool-duplicate.zip").as_slice(),
            "e84533ea32bcd7d4f1bd7513e062f49f7c1c60d3e0c30c32c3dcfea359dcf5ec",
            270,
        ),
    ] {
        let label = if duplicate { "duplicate" } else { "single" };
        let archive = root.join(format!("{label}.zip"));
        assert_eq!(bytes.len(), expected_len);
        fs::write(&archive, bytes)?;
        let digest: String = Sha256::digest(bytes)
            .iter()
            .flat_map(|byte| {
                const HEX: &[u8; 16] = b"0123456789abcdef";
                [
                    char::from(HEX[usize::from(byte >> 4)]),
                    char::from(HEX[usize::from(byte & 15)]),
                ]
            })
            .collect();
        assert_eq!(digest, expected_sha);
        let url = "https://github.com/example/tool/releases/download/v1.2.3/tool.zip";
        let request = json!({"schema_version":1,"platform":platform,"timeout_seconds":30,"tools":[{"recipe":{"id":"tool","backend":{"kind":"aqua","package":"example/tool"},"version":"1.2.3","options":{"kind":"default"},"depends_on":[],"platforms":[{"platform":platform,"artifacts":[{"url":url}],"dependency_artifacts":[],"executables":[{"name":"tool","path":"bin/tool","probe":{"kind":"version","expected":"tool 1.2.3"}}]}]},"archives":[{"path":archive,"url":url,"sha256":digest}]}]});
        let request_path = root.join(format!("{label}.json"));
        fs::write(&request_path, serde_json::to_vec(&request)?)?;
        let work = root.join(format!("{label}-work"));
        let output = root.join(format!("{label}-candidate.json"));
        let result = velnor_actions_orchestrator::qualify_tools(&request_path, &work, &output);
        if duplicate {
            assert!(result.is_err(), "duplicate archive accepted");
            assert!(!output.exists());
            assert!(!work.join("tools/tool/unpacked/0").exists());
        } else {
            result?;
            assert!(output.is_file());
        }
    }
    Ok(())
}
