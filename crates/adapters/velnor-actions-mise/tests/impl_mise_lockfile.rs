//! `mise.lock` install-verification audit cases (G2).
use velnor_actions_mise::ToolCatalog;
use velnor_actions_mise::catalog::PinnedTool;
use velnor_actions_mise::toolfiles::lockfile::{
    InstallCoverage, InstallSubject, audit_install_coverage, mise_platform_for_target,
    parse_mise_lockfile, subject_for_install_spec,
};

fn catalog() -> ToolCatalog {
    ToolCatalog::pinned()
}

/// Audit subject for one catalog tool's emitted spec.
fn subject(tool: PinnedTool, catalog: &ToolCatalog) -> Result<InstallSubject, String> {
    let spec = catalog.tool_spec(tool);
    subject_for_install_spec(&spec, catalog).ok_or_else(|| "emitted spec resolves".to_owned())
}

fn checksum(hex: &str) -> String {
    format!("sha256:{hex}")
}

/// v3 lock with one fully checksummed tool plus a checksum-less rust.
fn lock_text() -> String {
    format!(
        "# @generated\n\nlockfile_version = 3\n\n[[tools.actionlint]]\nversion = \"1.7.12\"\nbackend = \"aqua:rhysd/actionlint\"\nspecifiers = [\"1.7.12\"]\n\n[tools.actionlint.\"platforms.linux-x64\"]\nchecksum = \"{}\"\nurl = \"https://example.invalid/a\"\n\n[tools.actionlint.\"platforms.macos-arm64\"]\nchecksum = \"{}\"\nurl = \"https://example.invalid/b\"\n\n[[tools.rust]]\nversion = \"1.98.1\"\nbackend = \"core:rust\"\nspecifiers = [\"1.98.1\"]\n",
        checksum(&"a".repeat(64)),
        checksum(&"b".repeat(64)),
    )
}

#[test]
fn parses_v3_entries_with_platform_checksums() -> Result<(), String> {
    let lock = parse_mise_lockfile(&lock_text())?;
    let actionlint = lock.tools.get("actionlint").ok_or("actionlint entry")?;
    assert_eq!(actionlint.version, "1.7.12");
    assert_eq!(
        actionlint.checksums.get("linux-x64").map(String::as_str),
        Some(checksum(&"a".repeat(64)).as_str())
    );
    assert_eq!(actionlint.checksums.len(), 2);
    let rust = lock.tools.get("rust").ok_or("rust entry")?;
    assert_eq!(rust.version, "1.98.1");
    assert!(rust.checksums.is_empty());
    Ok(())
}

#[test]
fn parses_quoted_backend_qualified_keys() -> Result<(), String> {
    let text = format!(
        "[[tools.\"aqua:nextest-rs/nextest/cargo-nextest\"]]\nversion = \"0.9.146\"\n\n[tools.\"aqua:nextest-rs/nextest/cargo-nextest\".\"platforms.linux-x64\"]\nchecksum = \"{}\"\n",
        checksum(&"c".repeat(64))
    );
    let lock = parse_mise_lockfile(&text)?;
    let entry = lock
        .tools
        .get("aqua:nextest-rs/nextest/cargo-nextest")
        .ok_or("qualified entry")?;
    assert_eq!(entry.version, "0.9.146");
    assert!(entry.checksums.contains_key("linux-x64"));
    Ok(())
}

#[test]
fn structural_garbage_fails_the_parse() {
    for bad in [
        "[[[not toml",
        "[[tools.x]]\nversion = nope\n",
        "[[tools.x]]\nversion = \"1\"\n[tools.x.\"platforms.linux-x64\"\n",
        "[[tools.x]]\nversion = \"1\"\nversion = \"2\"\n",
        "[tools.orphan.\"platforms.linux-x64\"]\nchecksum = \"sha256:aa\"\n",
    ] {
        assert!(
            parse_mise_lockfile(bad).is_err(),
            "must reject structural garbage: {bad:?}"
        );
    }
    assert!(parse_mise_lockfile("").is_ok());
    assert!(parse_mise_lockfile("# only a comment\n").is_ok());
}

#[test]
fn coverage_matrix_names_every_state() -> Result<(), String> {
    let lock = parse_mise_lockfile(&lock_text())?;
    let catalog = catalog();
    let subjects = [
        subject(PinnedTool::Actionlint, &catalog)?,
        subject(PinnedTool::Rust, &catalog)?,
        subject(PinnedTool::Zizmor, &catalog)?,
    ];
    let verdicts = audit_install_coverage(&lock, &subjects, "linux-x64");
    assert_eq!(verdicts[0], InstallCoverage::Verified);
    assert_eq!(verdicts[1], InstallCoverage::NoChecksums);
    assert_eq!(verdicts[2], InstallCoverage::MissingEntry);
    assert!(!verdicts[0].is_blocking());
    assert!(!verdicts[1].is_blocking());
    assert!(!verdicts[2].is_blocking());
    let drifted = lock_text().replace("version = \"1.7.12\"", "version = \"1.7.11\"");
    let lock = parse_mise_lockfile(&drifted)?;
    let verdicts = audit_install_coverage(
        &lock,
        &[subject(PinnedTool::Actionlint, &catalog)?],
        "linux-x64",
    );
    assert_eq!(
        verdicts[0],
        InstallCoverage::VersionDrift {
            locked: "1.7.11".to_owned()
        }
    );
    assert!(!verdicts[0].is_blocking());
    Ok(())
}

#[test]
fn platform_hole_and_corrupt_shape_block() -> Result<(), String> {
    let catalog = catalog();
    let linux_block: Vec<String> = lock_text()
        .lines()
        .skip_while(|line| !line.contains("platforms.linux-x64"))
        .take(3)
        .map(str::to_owned)
        .collect();
    assert_eq!(linux_block.len(), 3);
    let holed = linux_block
        .iter()
        .fold(lock_text(), |text, line| text.replacen(line, "", 1));
    let lock = parse_mise_lockfile(&holed)?;
    let subjects = [subject(PinnedTool::Actionlint, &catalog)?];
    let verdicts = audit_install_coverage(&lock, &subjects, "linux-x64");
    assert_eq!(
        verdicts[0],
        InstallCoverage::MissingPlatform {
            locked_platforms: vec!["macos-arm64".to_owned()]
        }
    );
    assert!(verdicts[0].is_blocking());
    for bad in ["xyz", "", "sha256:short", "sha256:GGGG", "no-scheme-here"] {
        let tampered = lock_text().replace(&checksum(&"a".repeat(64)), bad);
        let lock = parse_mise_lockfile(&tampered)?;
        let verdicts = audit_install_coverage(&lock, &subjects, "linux-x64");
        assert_eq!(
            verdicts[0],
            InstallCoverage::CorruptChecksum {
                observed: bad.to_owned()
            },
            "shape {bad:?} must block"
        );
        assert!(verdicts[0].is_blocking());
    }
    Ok(())
}

#[test]
fn uppercase_checksums_verify() -> Result<(), String> {
    let catalog = catalog();
    let subjects = [subject(PinnedTool::Actionlint, &catalog)?];
    let upper = lock_text().replace(&"a".repeat(64), &"A".repeat(64));
    let lock = parse_mise_lockfile(&upper)?;
    let verdicts = audit_install_coverage(&lock, &subjects, "linux-x64");
    assert_eq!(verdicts[0], InstallCoverage::Verified);
    let mixed = "aBcDeF0123456789".repeat(4);
    assert_eq!(mixed.len(), 64);
    let tampered = lock_text().replace(&"a".repeat(64), &mixed);
    let lock = parse_mise_lockfile(&tampered)?;
    let verdicts = audit_install_coverage(&lock, &subjects, "linux-x64");
    assert_eq!(verdicts[0], InstallCoverage::Verified);
    Ok(())
}

#[test]
fn drifted_spec_version_does_not_resolve() {
    let catalog = catalog();
    for spec in ["rust@1.97.0", "actionlint@1.7.11", "rust@latest", "rust@"] {
        assert_eq!(
            subject_for_install_spec(spec, &catalog),
            None,
            "{spec} must not resolve"
        );
    }
}

#[test]
fn platform_map_covers_supported_targets() {
    assert_eq!(
        mise_platform_for_target("x86_64-unknown-linux-gnu"),
        Some("linux-x64")
    );
    assert_eq!(
        mise_platform_for_target("aarch64-apple-darwin"),
        Some("macos-arm64")
    );
    assert_eq!(
        mise_platform_for_target("x86_64-apple-darwin"),
        Some("macos-x64")
    );
    assert_eq!(mise_platform_for_target("wasm32-unknown-unknown"), None);
    assert_eq!(mise_platform_for_target(""), None);
}

#[test]
fn every_catalog_spec_resolves_and_foreign_does_not() {
    let catalog = catalog();
    for tool in PinnedTool::ALL {
        let spec = catalog.tool_spec(tool);
        let subject = subject_for_install_spec(&spec, &catalog);
        assert!(subject.is_some(), "{spec} must resolve");
        let subject = subject.expect("resolved");
        assert_eq!(
            subject.expected_version,
            catalog.version(tool),
            "{spec} pins the catalog version"
        );
        let key = spec.split_once('@').map_or("", |(head, _)| head);
        assert_eq!(subject.lock_key, key, "{spec} keeps its spec key");
    }
    for foreign in [
        "cargo-deny@0.20.2",
        "node@20.0.0",
        "not-a-spec",
        "",
        "@1.0.0",
    ] {
        assert_eq!(
            subject_for_install_spec(foreign, &catalog),
            None,
            "{foreign} must not resolve"
        );
    }
}

/// A decoy entry under the non-addressed key never affects the verdict.
///
/// Nextest's emitted spec addresses the backend-qualified key, so a
/// fully checksummed `nextest` entry is invisible to that install:
/// no entry under the addressed key means `MissingEntry`, never a
/// borrowed `Verified`.
#[test]
fn decoy_entry_under_alias_key_does_not_verify() -> Result<(), String> {
    let catalog = catalog();
    let version = catalog.version(PinnedTool::Nextest);
    let text = format!(
        "[[tools.nextest]]\nversion = \"{version}\"\n\n[tools.nextest.\"platforms.linux-x64\"]\nchecksum = \"{}\"\n",
        checksum(&"d".repeat(64))
    );
    let lock = parse_mise_lockfile(&text)?;
    let subject = subject(PinnedTool::Nextest, &catalog)?;
    assert_ne!(
        subject.lock_key, "nextest",
        "emitted spec must address the qualified key"
    );
    let verdicts = audit_install_coverage(&lock, &[subject], "linux-x64");
    assert_eq!(verdicts[0], InstallCoverage::MissingEntry);
    Ok(())
}
