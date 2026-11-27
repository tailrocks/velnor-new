// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;
use std::path::Path;

use velnor_model::RepoPath;
use velnor_rust::{detect, is_plain_source};

use crate::facts::{TestResult, lib, repo};

const READ: [&str; 11] = [
    "pub const A: &str = include_str!(\"../x.md\");",
    "pub const A: &[u8] = include_bytes!(\"../x.bin\");",
    "include!(\"gen.rs\");",
    "pub const A: &str = core::include_str!(\"x\");",
    "pub const A: &str = include_str ! ( \"x\" );",
    "pub const A: &str = include_str!(concat!(env!(\"OUT_DIR\"), \"/x\"));",
    "pub const A: &[u8] = include_bytes![ \"x\" ];",
    "include!{ \"x.rs\" }",
    "macro_rules! m { () => { include_str!(\"x\") }; }",
    "pub const A: &str = std::include_str!(\"/abs\");",
    "pub const A: &str = include_str!(\"../../../../../outside\");",
];
const PLAIN: [&str; 4] = [
    "pub fn f() {}",
    "#[path = \"other.rs\"]\nmod other;",
    "mod child;\npub fn f() { child::g() }",
    "pub fn f() -> &'static str { \"text\" }",
];

fn reads_of(source: &str) -> Result<bool, Box<dyn Error>> {
    let view = repo(
        vec![("plain/Cargo.toml", vec![lib("plain", "plain/Cargo.toml")?])],
        &[("plain/src/lib.rs", source)],
    )?;
    Ok(!detect(&view)?.compile_inputs.is_empty())
}

fn plain(source: &str) -> Result<bool, Box<dyn Error>> {
    Ok(is_plain_source(
        &RepoPath::new("plain/src/lib.rs")?,
        source.as_bytes(),
    ))
}

fn sources(directory: &Path, found: &mut Vec<(String, String)>) -> TestResult {
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        let kind = entry.file_type()?;
        if kind.is_dir() && path.file_name().is_some_and(|name| name != "target") {
            sources(&path, found)?;
        } else if kind.is_file()
            && path.extension().is_some_and(|ext| ext == "rs")
            && let Ok(text) = fs::read_to_string(&path)
        {
            found.push((path.display().to_string(), text));
        }
    }
    Ok(())
}

#[test]
fn a_source_whose_scan_returns_a_read_is_never_plain() -> TestResult {
    for source in READ {
        assert!(reads_of(source)?, "the scan misses a read: {source}");
        assert!(!plain(source)?, "a source with a read is plain: {source}");
    }
    for source in PLAIN {
        assert!(!reads_of(source)?, "{source}");
        assert!(plain(source)?, "{source}");
    }
    Ok(())
}

#[test]
fn every_regular_rust_file_of_the_repository_that_is_plain_has_no_read() -> TestResult {
    let mut found = Vec::new();
    sources(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join(".."),
        &mut found,
    )?;
    let source = RepoPath::new("plain/src/lib.rs")?;
    found.sort();
    let plain: Vec<(String, String)> = found
        .into_iter()
        .filter(|(_, text)| is_plain_source(&source, text.as_bytes()))
        .step_by(4)
        .collect();
    assert!(plain.len() > 200, "{}", plain.len());
    let files: Vec<(String, &str)> = plain
        .iter()
        .enumerate()
        .map(|(index, (_, text))| (format!("plain/src/f{index}.rs"), text.as_str()))
        .collect();
    let refs: Vec<(&str, &str)> = files
        .iter()
        .map(|(name, text)| (name.as_str(), *text))
        .collect();
    let view = repo(
        vec![("plain/Cargo.toml", vec![lib("plain", "plain/Cargo.toml")?])],
        &refs,
    )?;
    let reads = detect(&view)?.compile_inputs;
    assert!(reads.is_empty(), "{reads:?}");
    Ok(())
}
