// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fmt::Write as _;
use std::io::Write;

use sha2::{Digest, Sha256};

use crate::resolve_world::REPO;

pub(crate) fn zip_with(name: &str, bytes: &[u8]) -> Vec<u8> {
    zip_of(name, bytes)
}

pub(crate) fn digest_b3(bytes: &[u8]) -> String {
    format!("b3-{}", blake3::hash(bytes).to_hex())
}

pub(crate) fn sha256(bytes: &[u8]) -> String {
    let mut text = String::from("sha256:");
    for byte in Sha256::digest(bytes) {
        write!(text, "{byte:02x}").unwrap_or_else(|e| unreachable!("{e}"));
    }
    text
}

pub(crate) fn zip_of(name: &str, bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    {
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(&mut out));
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        zip.start_file(name, options)
            .unwrap_or_else(|e| unreachable!("{e}"));
        zip.write_all(bytes).unwrap_or_else(|e| unreachable!("{e}"));
        zip.finish().unwrap_or_else(|e| unreachable!("{e}"));
    }
    out
}

pub(crate) fn run_route(id: u64) -> String {
    format!("repos/{REPO}/actions/runs/{id}/attempts/1")
}

pub(crate) fn list_route(id: u64) -> String {
    format!("repos/{REPO}/actions/runs/{id}/artifacts?per_page=100")
}

pub(crate) fn artifact_route(id: u64) -> String {
    format!("repos/{REPO}/actions/artifacts/{}", 5000 + id)
}

pub(crate) fn zip_route(id: u64) -> String {
    format!("{}/zip", artifact_route(id))
}

pub(crate) fn repo_route() -> String {
    format!("repos/{REPO}")
}

pub(crate) fn branch_route() -> String {
    format!("repos/{REPO}/branches/main")
}
