// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;
use std::path::Path;
use std::process::Command;

use velnor_actions_contract::WorkflowPolicy;
use velnor_actions_orchestrator::{prepare, render_staged_tree};

use super::tools_producers_donors::prepared_for;
use super::workflow_configuration_fixture;

const PLATFORM: &str = "linux-x64";

pub(super) struct HttpSpec {
    pub(super) name: String,
    version: String,
    url: String,
    sha256: String,
}

impl HttpSpec {
    pub(super) fn row(&self) -> String {
        format!(
            "http-{} {} {PLATFORM} {} {}",
            self.name, self.version, self.url, self.sha256
        )
    }
}

fn http_specs(text: &str) -> Vec<HttpSpec> {
    let mut specs = Vec::new();
    let mut rest = text;
    while let Some(at) = rest.find("http:") {
        rest = &rest[at + "http:".len()..];
        let Some((name, tail)) = rest.split_once("[url=") else {
            break;
        };
        let Some((url, tail)) = tail.split_once(",checksum=sha256:") else {
            break;
        };
        let Some((sha256, tail)) = tail.split_once("]@") else {
            break;
        };
        let version: String = tail
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+'))
            .collect();
        if !name.contains(|c: char| !(c.is_ascii_alphanumeric() || c == '-')) {
            specs.push(HttpSpec {
                name: name.to_owned(),
                version,
                url: url.to_owned(),
                sha256: sha256.to_owned(),
            });
        }
    }
    specs
}

const LINK_CHECK_CONFIG: &str = "schema = 1\n[workflow]\nname = \"CI\"\ndefault_branch = \"testmain\"\n[workflow.verify]\njobs = [\"link-check\"]\n";

pub(super) fn rendered_http_specs() -> Result<Vec<HttpSpec>, Box<dyn Error>> {
    let (_repo, repository) = prepared_for(WorkflowPolicy::VelnorRepositoryV1)?;
    let consumer = workflow_configuration_fixture::make_repo(LINK_CHECK_CONFIG)?;
    let trees = [
        render_staged_tree(&repository)?,
        render_staged_tree(&prepare(consumer.path())?)?,
    ];
    let mut specs: Vec<HttpSpec> = trees
        .iter()
        .flat_map(|tree| &tree.files)
        .filter(|file| file.path.starts_with(".github/workflows/"))
        .flat_map(|file| http_specs(&file.bytes))
        .collect();
    specs.sort_by_key(HttpSpec::row);
    specs.dedup_by_key(|spec| spec.row());
    Ok(specs)
}

pub(super) const MACHETE_BINARY: &[u8] = b"fixture cargo-machete binary\n";
pub(super) const MACHETE_DIR: &str = "http-cargo-machete";
pub(super) const MACHETE_VERSION: &str = "0.9.2";

fn sha256_hex(path: &Path) -> Result<String, Box<dyn Error>> {
    let output = Command::new("sha256sum").arg(path).output().or_else(|_| {
        Command::new("shasum")
            .args(["-a", "256"])
            .arg(path)
            .output()
    })?;
    let text = String::from_utf8(output.stdout)?;
    Ok(text
        .split_whitespace()
        .next()
        .ok_or("no digest")?
        .to_owned())
}

pub(super) fn machete_archive(scratch: &Path) -> Result<(Vec<u8>, String), Box<dyn Error>> {
    let tree = scratch.join("tree");
    fs::create_dir_all(tree.join("cargo-machete-v0.9.2"))?;
    fs::write(
        tree.join("cargo-machete-v0.9.2/cargo-machete"),
        MACHETE_BINARY,
    )?;
    let archive = scratch.join("machete.tar.gz");
    let status = Command::new("tar")
        .arg("-czf")
        .arg(&archive)
        .arg("-C")
        .arg(&tree)
        .arg("cargo-machete-v0.9.2")
        .status()?;
    assert!(status.success(), "tar failed");
    Ok((fs::read(&archive)?, sha256_hex(&archive)?))
}

pub(super) fn pins_with_fixture_sha(pins: &str, sha256: &str) -> String {
    pins.lines()
        .map(|row| {
            let mut fields: Vec<&str> = row.split(' ').collect();
            if fields.len() == 5 && fields[3].contains("/cargo-machete/") {
                fields[4] = sha256;
            }
            fields.join(" ")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub(super) fn assert_urls_carry_pinned_version(rows: &[&str]) -> Result<(), Box<dyn Error>> {
    for row in rows {
        let fields: Vec<&str> = row.split(' ').collect();
        let [name, version, _, url, _] = fields[..] else {
            return Err(format!("malformed pin row: {row}").into());
        };
        assert!(
            url.contains(&format!("/v{version}/")) && url.contains(&format!("v{version}")),
            "{name} pins version {version} but its url is {url}"
        );
    }
    Ok(())
}
