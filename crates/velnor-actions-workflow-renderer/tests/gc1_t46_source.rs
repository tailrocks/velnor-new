// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0F) as usize] as char);
    }
    out
}

pub(crate) fn json_string(value: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if (c as u32) < 0x20 => {
                let code = c as u32;
                out.push_str("\\u00");
                out.push(HEX[(code >> 4) as usize] as char);
                out.push(HEX[(code & 0x0F) as usize] as char);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

pub(crate) fn module_imports(source: &str) -> Vec<String> {
    let mut imports: Vec<String> = source
        .lines()
        .filter_map(|line| {
            line.strip_prefix("import ")
                .or_else(|| {
                    line.strip_prefix("from ")
                        .and_then(|rest| rest.split(' ').next())
                })
                .map(str::to_owned)
        })
        .collect();
    imports.sort();
    imports.dedup();
    imports
}

pub(crate) fn check_fixed_source(
    name: &str,
    source: &str,
    expected_imports: &[&str],
    extra_forbidden: &[&str],
) -> Result<(), Box<dyn Error>> {
    let mut forbidden = vec![
        "import candidate",
        "from candidate",
        "candidate-built",
        "candidate-supplied",
        "subprocess",
        "os.system",
        "os.exec",
        "os.popen",
        "os.spawn",
        "os.environ",
        "os.getenv",
        "shutil",
        "tempfile",
        "argparse",
        "tomllib",
        "check_output",
        "Popen",
        "eval(",
        "exec(",
        "compile(",
        "__import__",
        "GITHUB_",
        "read_text",
        "GH_TOKEN",
        "GH_HOST",
        "rev-parse",
    ];
    forbidden.extend(extra_forbidden);
    for blocked in forbidden {
        forbid(source, blocked, &format!("{name} fixed input"))?;
    }
    let imports = module_imports(source);
    let expected: Vec<String> = expected_imports
        .iter()
        .map(|item| (*item).to_owned())
        .collect();
    if imports != expected {
        return Err(fail(&format!(
            "{name} left its standard-library-only imports"
        )));
    }
    Ok(())
}

pub(crate) fn check_fixed_source_imports() -> Result<(), Box<dyn Error>> {
    check_fixed_source(
        IDENTITY_FILE,
        IDENTITY_SOURCE,
        &["json", "re"],
        &["socket", "urllib", "http.client", "ssl", "tarfile", "gzip"],
    )?;
    check_fixed_source(
        REGISTRY_FILE,
        REGISTRY_SOURCE,
        &["hashlib", "json", "re", "urllib.parse"],
        &[
            "socket",
            "http.client",
            "ssl",
            "tarfile",
            "gzip",
            "import os",
        ],
    )?;
    check_fixed_source(
        PUBLISH_FILE,
        PUBLISH_SOURCE,
        &["hashlib", "json", "re", "tarfile"],
        &[
            "socket",
            "urllib",
            "http.client",
            "ssl",
            "gzip",
            "import os",
        ],
    )?;
    for (name, source) in [
        (IDENTITY_FILE, IDENTITY_SOURCE),
        (REGISTRY_FILE, REGISTRY_SOURCE),
        (PUBLISH_FILE, PUBLISH_SOURCE),
    ] {
        for marker in ["AuthorityError", "raise AuthorityError"] {
            require(source, marker, &format!("{name} fixed input"))?;
        }
    }
    Ok(())
}

pub(crate) fn check_fixed_identity_markers() -> Result<(), Box<dyn Error>> {
    require(IDENTITY_SOURCE, "validate_identity", "identity fixed input")?;
    require(IDENTITY_SOURCE, "select_ci_run", "identity fixed input")?;
    require(
        IDENTITY_SOURCE,
        "validate_attestation_output",
        "identity fixed input",
    )?;
    require(
        IDENTITY_SOURCE,
        "validate_source_inputs",
        "identity fixed input",
    )?;
    Ok(())
}

pub(crate) fn check_fixed_registry_markers() -> Result<(), Box<dyn Error>> {
    require(REGISTRY_SOURCE, "registry_url", "registry fixed input")?;
    require(
        REGISTRY_SOURCE,
        "blob_download_location",
        "registry fixed input",
    )?;
    require(
        REGISTRY_SOURCE,
        "validate_upload_location",
        "registry fixed input",
    )?;
    require(REGISTRY_SOURCE, "upload_target", "registry fixed input")?;
    require(
        REGISTRY_SOURCE,
        "parse_auth_challenge",
        "registry fixed input",
    )?;
    require(
        REGISTRY_SOURCE,
        "parse_token_payload",
        "registry fixed input",
    )?;
    require(
        REGISTRY_SOURCE,
        "check_head_response",
        "registry fixed input",
    )?;
    require(
        REGISTRY_SOURCE,
        "check_get_response",
        "registry fixed input",
    )?;
    require(REGISTRY_SOURCE, "redirect_target", "registry fixed input")?;
    Ok(())
}

pub(crate) fn check_fixed_publish_markers() -> Result<(), Box<dyn Error>> {
    require(
        PUBLISH_SOURCE,
        "check_record_binding",
        "publish fixed input",
    )?;
    require(PUBLISH_SOURCE, "blob_descriptors", "publish fixed input")?;
    require(PUBLISH_SOURCE, "read_manifest_bytes", "publish fixed input")?;
    require(PUBLISH_SOURCE, "extract_blob", "publish fixed input")?;
    require(PUBLISH_SOURCE, "check_record_source", "publish fixed input")?;
    require(PUBLISH_SOURCE, "publish_image", "publish fixed input")?;
    require(
        PUBLISH_SOURCE,
        "check_publication_proof",
        "publish fixed input",
    )?;
    require(
        PUBLISH_SOURCE,
        "fetch_registry_token",
        "publish fixed input",
    )?;
    require(PUBLISH_SOURCE, "build_evidence", "publish fixed input")?;
    require(PUBLISH_SOURCE, "validate_evidence", "publish fixed input")?;
    Ok(())
}

pub(crate) fn check_sources_are_fixed() -> Result<(), Box<dyn Error>> {
    check_fixed_source_imports()?;
    check_fixed_identity_markers()?;
    check_fixed_registry_markers()?;
    check_fixed_publish_markers()?;
    Ok(())
}

pub(crate) struct Fixture {
    pub(crate) root: tempfile::TempDir,
    pub(crate) driver: std::path::PathBuf,
}
