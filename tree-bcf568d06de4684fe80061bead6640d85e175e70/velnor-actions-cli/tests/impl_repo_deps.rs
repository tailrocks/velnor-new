//! Repo-shape policy continued: dependencies, tests, sizes, CLI structure.

use std::collections::BTreeMap;
use std::error::Error;

use crate::impl_repo_policy::{
    MEMBERS, dep_key, dep_lines, dep_referenced, manifest, read, repo_root, test_markers,
    tree_files,
};

/// Intra-workspace edges allowed per member package.
fn expected_internal(dir: &str) -> Vec<&str> {
    match dir {
        "crates/velnor-actions-contract" => vec![],
        "crates/velnor-actions-orchestrator" => vec![
            "velnor-actions-actionlint",
            "velnor-actions-contract",
            "velnor-actions-mise",
            "velnor-actions-rust",
            "velnor-actions-workflow-renderer",
        ],
        "crates/velnor-actions-cli" => {
            vec!["velnor-actions-contract", "velnor-actions-orchestrator"]
        }
        _ => vec!["velnor-actions-contract"],
    }
}

#[test]
fn dependency_edges_match_ownership_table() -> Result<(), Box<dyn Error>> {
    for (dir, _) in MEMBERS {
        let body = manifest(dir)?;
        let mut in_deps = false;
        let mut found = Vec::new();
        for line in body.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with('[') {
                in_deps = trimmed == "[dependencies]";
            } else if in_deps && trimmed.contains("velnor-actions-") {
                found.push(dep_key(trimmed).to_owned());
            }
        }
        found.sort();
        let mut want: Vec<String> = expected_internal(dir)
            .into_iter()
            .map(str::to_owned)
            .collect();
        want.sort();
        assert_eq!(found, want, "{dir} edges drift");
    }
    Ok(())
}

#[test]
fn external_deps_allowlisted_used_and_narrow() -> Result<(), Box<dyn Error>> {
    let allowed = [
        "serde",
        "serde_json",
        "toml",
        "cargo_metadata",
        "globset",
        "blake3",
        "clap",
        "thiserror",
        "anyhow",
        "tracing",
        "tempfile",
    ];
    for (dir, _) in MEMBERS {
        let body = manifest(dir)?;
        assert!(!body.contains("tokio"), "{dir} must not use tokio");
        for line in dep_lines(&body) {
            let key = dep_key(line);
            if key.starts_with("velnor-actions") {
                continue;
            }
            assert!(allowed.contains(&key), "{dir} uses {key}");
            if let Some(index) = line.find("features") {
                let quoted: Vec<&str> = line[index..].split('"').collect();
                for feature in quoted.into_iter().skip(1).step_by(2) {
                    assert_eq!(feature, "derive", "{dir}/{key} feature {feature}");
                }
            }
            assert!(dep_referenced(dir, key)?, "{dir} never uses {key}");
        }
    }
    Ok(())
}

#[test]
fn dep_versions_exact_and_consistent() -> Result<(), Box<dyn Error>> {
    let mut seen: BTreeMap<String, String> = BTreeMap::new();
    for (dir, _) in MEMBERS {
        for line in dep_lines(&manifest(dir)?) {
            if !line.contains("version") {
                continue;
            }
            let version = line.split('"').nth(1).ok_or("version shape")?;
            assert!(version.starts_with('='), "{dir}: {line} must pin exactly");
            let key = dep_key(line).to_owned();
            if let Some(prior) = seen.insert(key.clone(), version.to_owned()) {
                assert_eq!(prior, version, "{key} version diverges");
            }
        }
    }
    assert!(!seen.is_empty());
    Ok(())
}

#[test]
fn no_custom_linter_modules() -> Result<(), Box<dyn Error>> {
    for (dir, _) in MEMBERS {
        for path in tree_files(&format!("{dir}/src"), "rs")? {
            let name = path
                .file_name()
                .and_then(|stem| stem.to_str())
                .unwrap_or("");
            if !dir.ends_with("actionlint") {
                assert!(!name.contains("linter"), "{}", path.display());
            }
            assert!(
                !std::fs::read_to_string(&path)?.contains("mod linter"),
                "{}",
                path.display()
            );
        }
    }
    Ok(())
}

#[test]
fn one_test_entry_per_crate() -> Result<(), Box<dyn Error>> {
    for (dir, _) in MEMBERS {
        let body = manifest(dir)?;
        assert!(body.contains("autotests = false"), "{dir}");
        assert!(body.contains("name = \"velnor_"), "{dir}");
        let entries = body.matches("[[test]]").count();
        assert!((1..=2).contains(&entries), "{dir} has {entries} entries");
        assert!(
            entries < test_markers(dir)?,
            "{dir} nears one-binary-per-case"
        );
    }
    Ok(())
}

#[test]
fn every_crate_has_registered_tests() -> Result<(), Box<dyn Error>> {
    for (dir, _) in MEMBERS {
        assert!(test_markers(dir)? >= 1, "{dir} has no tests");
        for line in manifest(dir)?.lines() {
            if line.trim().starts_with("path = ") {
                let path = line.split('"').nth(1).ok_or("test path")?;
                assert!(repo_root().join(dir).join(path).is_file(), "{dir}/{path}");
            }
        }
    }
    Ok(())
}

#[test]
fn cli_tests_assert_through_binary_only() -> Result<(), Box<dyn Error>> {
    // Built at runtime so this very file does not trip its own scan.
    let stem = ["velnor", "actions"].join("_");
    let import = format!("use {stem}");
    let path_use = format!("{stem}::");
    for path in tree_files("crates/velnor-actions-cli/tests", "rs")? {
        let body = std::fs::read_to_string(&path)?;
        assert!(
            !body.contains(&import),
            "{} imports implementation",
            path.display()
        );
        assert!(
            !body.contains(&path_use),
            "{} uses implementation",
            path.display()
        );
    }
    Ok(())
}

/// Physical lines: newline count, matching `wc -l`.
pub(crate) fn physical_lines(body: &str) -> usize {
    body.bytes().filter(|byte| *byte == b'\n').count()
}

#[test]
fn size_limits_hold() -> Result<(), Box<dyn Error>> {
    let mut over = Vec::new();
    for (dir, _) in MEMBERS {
        for area in ["src", "tests"] {
            for path in tree_files(&format!("{dir}/{area}"), "rs")? {
                let lines = physical_lines(&std::fs::read_to_string(&path)?);
                if lines > 400 {
                    over.push(format!("{} ({lines})", path.display()));
                }
                let name = path
                    .file_name()
                    .and_then(|stem| stem.to_str())
                    .unwrap_or("");
                if (name == "lib.rs" || name == "main.rs") && lines > 150 {
                    over.push(format!("{} lib/main ({lines})", path.display()));
                }
            }
        }
    }
    assert!(over.is_empty(), "over 400 lines: {}", over.join(", "));
    assert!(read("clippy.toml")?.contains("too-many-lines-threshold = 80"));
    assert!(read("Cargo.toml")?.contains("too_many_lines = \"deny\""));
    let mut docs = tree_files("docs", "md")?;
    docs.extend(tree_files(".velnor", "toml")?);
    docs.extend(tree_files(".velnor", "json")?);
    for path in docs {
        let lines = physical_lines(&std::fs::read_to_string(&path)?);
        assert!(lines <= 400, "{} has {lines} lines", path.display());
    }
    Ok(())
}

#[test]
fn lockfile_committed_and_locked_used() -> Result<(), Box<dyn Error>> {
    assert!(!read("Cargo.lock")?.trim().is_empty());
    let tracked = std::process::Command::new("git")
        .arg("ls-files")
        .arg("--error-unmatch")
        .arg("Cargo.lock")
        .current_dir(repo_root())
        .output()?;
    assert!(tracked.status.success(), "Cargo.lock not committed");
    for file in [
        "crates/velnor-actions-mise/src/requests.rs",
        "crates/velnor-actions-orchestrator/src/vectors.rs",
        ".github/workflows/velnor.yml",
    ] {
        assert!(read(file)?.contains("--locked"), "{file} misses --locked");
    }
    Ok(())
}

#[test]
fn cli_invokes_no_tools_directly() -> Result<(), Box<dyn Error>> {
    let banned = [
        "process::Command",
        "Command::new",
        ".spawn(",
        ".status()",
        ".output()",
        "cargo",
        "mise",
        "mbx",
        "nextest",
        "rustup",
        "\"gh\"",
        "shell",
        "Shell",
        "sh -c",
    ];
    for path in tree_files("crates/velnor-actions-cli/src", "rs")? {
        let body = std::fs::read_to_string(&path)?;
        for token in banned {
            assert!(!body.contains(token), "{} leaks {token}", path.display());
        }
        for (index, _) in body.match_indices("std::process::") {
            let rest = &body[index + "std::process::".len()..];
            assert!(
                rest.starts_with("ExitCode"),
                "{} uses {rest:?}",
                path.display()
            );
        }
    }
    Ok(())
}

#[test]
fn init_reads_no_tool_files() -> Result<(), Box<dyn Error>> {
    let body = read("crates/velnor-actions-orchestrator/src/init.rs")?;
    for token in [
        "read_to_string",
        "File::open",
        ".read(",
        "read_dir",
        "read_link",
        "io::Read",
    ] {
        assert!(!body.contains(token), "init.rs inspects via {token}");
    }
    Ok(())
}

#[test]
fn parse_tests_live_outside_src() -> Result<(), Box<dyn Error>> {
    for path in tree_files("crates/velnor-actions-cli/src", "rs")? {
        assert!(
            !std::fs::read_to_string(&path)?.contains("#[test]"),
            "{}",
            path.display()
        );
    }
    assert!(read("crates/velnor-actions-cli/tests/impl_cli_args.rs")?.contains("#[test]"));
    Ok(())
}
