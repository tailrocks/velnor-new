//! Repo-shape policy continued: dependencies, tests, sizes, CLI structure.

#[path = "impl_repo_git_policy.rs"]
mod git_policy;

use std::collections::BTreeMap;
use std::error::Error;

#[path = "impl_repo_archive_deps.rs"]
pub(crate) mod archive_deps;
#[path = "impl_repo_size_limits.rs"]
mod size_limits;

use crate::impl_cli_tmp::git_fixture;

use crate::impl_repo_policy::{
    MEMBERS, dep_key, dep_lines, manifest, p11_toml, read, repo_root, test_markers, tree_files,
};

/// Intra-workspace edges allowed per member package.
fn expected_internal(dir: &str) -> Vec<&str> {
    match dir {
        "crates/velnor-actions-contract" | "crates/velnor-actions-freshness" => vec![],
        "crates/velnor-actions-orchestrator" => vec![
            "velnor-actions-actionlint",
            "velnor-actions-contract",
            "velnor-actions-mise",
            "velnor-actions-rust",
            "velnor-actions-tofu",
            "velnor-actions-workflow-renderer",
        ],
        "crates/velnor-actions-cli" => {
            vec!["velnor-actions-freshness", "velnor-actions-orchestrator"]
        }
        _ => vec!["velnor-actions-contract"],
    }
}

#[test]
fn dependency_edges_match_ownership_table() -> Result<(), Box<dyn Error>> {
    for (dir, _) in MEMBERS {
        let doc = p11_toml::parse(&manifest(dir)?)?;
        let mut found: Vec<String> = p11_toml::section(&doc, "dependencies")
            .map(|deps| {
                deps.pairs
                    .iter()
                    .filter(|pair| pair.0.starts_with("velnor-actions"))
                    .map(|pair| pair.0.clone())
                    .collect()
            })
            .unwrap_or_default();
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

#[test]
fn lockfile_committed_and_locked_used() -> Result<(), Box<dyn Error>> {
    assert_ne!(read("Cargo.lock")?.trim(), "");
    let tracked = git_fixture::command(&repo_root())?
        .arg("ls-files")
        .arg("--error-unmatch")
        .arg("Cargo.lock")
        .output()?;
    assert!(tracked.status.success(), "Cargo.lock not committed");
    for file in [
        "crates/velnor-actions-mise/src/requests.rs",
        "crates/velnor-actions-orchestrator/src/vectors.rs",
        ".github/workflows/ci.yml",
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
        // "mise" stays unbanned: repo-policy names a mise-version operation.
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

#[test]
fn test_entries_match_layout_and_stay_far_below_cases() -> Result<(), Box<dyn Error>> {
    for (dir, _) in MEMBERS {
        let body = manifest(dir)?;
        let entries = body.matches("[[test]]").count();
        let want = if dir.ends_with("orchestrator") { 2 } else { 1 };
        assert_eq!(entries, want, "{dir} entry drift");
        let mut in_test = false;
        for line in body.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with('[') {
                in_test = trimmed == "[[test]]";
            } else if in_test && trimmed.starts_with("path = ") {
                let path = trimmed.split('"').nth(1).ok_or("test path")?;
                let rest = path.strip_prefix("tests/").ok_or("test path")?;
                assert!(!rest.contains('/'), "{dir} nests test binary {path}");
            }
        }
        assert!(
            entries * 10 <= test_markers(dir)?,
            "{dir} nears one-binary-per-case"
        );
    }
    Ok(())
}

#[test]
fn fixtures_stay_independent_and_cover_failures() -> Result<(), Box<dyn Error>> {
    let tokens = [
        "fail",
        "invalid",
        "missing",
        "reject",
        "err",
        "denied",
        "forbidden",
        "empty",
        "boundary",
    ];
    let mut tempdir_files = 0;
    let mut failure_cases = 0;
    for (dir, _) in MEMBERS {
        for path in tree_files(&format!("{dir}/tests"), "rs")? {
            let body = std::fs::read_to_string(&path)?;
            if body.contains("TempDir") {
                tempdir_files += 1;
            }
            for line in body.lines() {
                let trimmed = line.trim_start();
                if trimmed.starts_with("fn ") && tokens.iter().any(|token| trimmed.contains(token))
                {
                    failure_cases += 1;
                }
            }
        }
    }
    assert!(tempdir_files >= 10, "only {tempdir_files} TempDir files");
    assert!(failure_cases >= 50, "only {failure_cases} failure cases");
    let helper = read("crates/velnor-actions-cli/tests/impl_cli_tmp.rs")?;
    for token in ["std::process::id()", "fetch_add", "create_dir_all"] {
        assert!(helper.contains(token), "fresh_tempdir loses {token}");
    }
    Ok(())
}
