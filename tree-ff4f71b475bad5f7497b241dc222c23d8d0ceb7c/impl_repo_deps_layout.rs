//! Test registration, fixture coverage, and repository size policy.

use std::error::Error;

use super::physical_lines;
use crate::impl_repo_policy::{MEMBERS, manifest, read, repo_root, test_markers, tree_files};

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
