//! GAP-F: cargo-metadata DAG assertions plus the forbidden-token table.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use serde_json::Value as Json;

use crate::impl_common::TestResult;

/// Workspace root (two levels above this crate).
fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

/// Allowed intra-workspace edges per member package.
fn expected_internal(dir: &str) -> Vec<&str> {
    match dir {
        "crates/core/velnor-actions-contract" => vec![],
        "crates/services/velnor-actions-orchestrator" => vec![
            "velnor-actions-actionlint",
            "velnor-actions-contract",
            "velnor-actions-mise",
            "velnor-actions-rust",
            "velnor-actions-rust-core",
            "velnor-actions-tofu",
            "velnor-actions-tofu-core",
            "velnor-actions-workflow-renderer",
        ],
        "crates/adapters/velnor-actions-tofu" => {
            vec!["velnor-actions-contract", "velnor-actions-tofu-core"]
        }
        "crates/adapters/velnor-actions-rust" => {
            vec!["velnor-actions-contract", "velnor-actions-rust-core"]
        }
        "crates/apps/velnor-actions-cli" => vec!["velnor-actions-orchestrator"],
        _ => vec!["velnor-actions-contract"],
    }
}

/// Member directories in dependency-table order.
fn members() -> Vec<&'static str> {
    vec![
        "crates/adapters/velnor-actions-actionlint",
        "crates/apps/velnor-actions-cli",
        "crates/core/velnor-actions-contract",
        "crates/adapters/velnor-actions-mise",
        "crates/services/velnor-actions-orchestrator",
        "crates/adapters/velnor-actions-rust",
        "crates/adapters/velnor-actions-rust-core",
        "crates/adapters/velnor-actions-tofu",
        "crates/adapters/velnor-actions-tofu-core",
        "crates/services/velnor-actions-workflow-renderer",
    ]
}

#[test]
fn crate_boundaries_match_architecture_dependency_direction() -> TestResult {
    let output = std::process::Command::new("cargo")
        .args(["metadata", "--format-version", "1", "--manifest-path"])
        .arg(repo_root().join("Cargo.toml"))
        .current_dir(repo_root())
        .output()?;
    assert!(output.status.success(), "cargo metadata runs");
    let metadata: Json = serde_json::from_slice(&output.stdout)?;
    let mut id_to_name: BTreeMap<String, String> = BTreeMap::new();
    for package in metadata["packages"].as_array().ok_or("packages")? {
        id_to_name.insert(
            package["id"].as_str().ok_or("id")?.to_owned(),
            package["name"].as_str().ok_or("name")?.to_owned(),
        );
    }
    let mut edges: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for node in metadata["resolve"]["nodes"].as_array().ok_or("nodes")? {
        let id = node["id"].as_str().ok_or("node id")?;
        let Some(name) = id_to_name.get(id) else {
            continue;
        };
        if !name.starts_with("velnor-actions-") {
            continue;
        }
        let mut deps = BTreeSet::new();
        for dep in node["deps"].as_array().ok_or("deps")? {
            let pkg = dep["pkg"].as_str().ok_or("dep pkg")?;
            // Normal edges only: the architecture diagram governs
            // product dependencies; dev/build edges are test-only and
            // ride outside the diagram (the CLI gate scans the
            // `[dependencies]` section alone for the same reason).
            let normal = dep["dep_kinds"]
                .as_array()
                .is_some_and(|kinds| kinds.iter().any(|kind| kind["kind"].is_null()));
            if !normal {
                continue;
            }
            if let Some(dep_name) = id_to_name.get(pkg)
                && dep_name.starts_with("velnor-actions-")
            {
                deps.insert(dep_name.clone());
            }
        }
        edges.insert(name.clone(), deps);
    }
    for dir in members() {
        let name = dir.rsplit('/').next().ok_or("member")?;
        let found: BTreeSet<String> = edges.get(name).cloned().unwrap_or_default();
        let want: BTreeSet<String> = expected_internal(dir)
            .into_iter()
            .map(str::to_owned)
            .collect();
        assert_eq!(found, want, "{dir} edges drift");
    }
    assert!(
        edges
            .get("velnor-actions-contract")
            .is_some_and(BTreeSet::is_empty)
    );
    assert!(!has_cycle(&edges), "workspace DAG is acyclic");
    for (name, deps) in &edges {
        assert!(!deps.contains(name), "{name} self-depends");
    }
    Ok(())
}

/// True when the member graph holds a dependency cycle.
fn has_cycle(edges: &BTreeMap<String, BTreeSet<String>>) -> bool {
    let mut visiting = BTreeSet::new();
    let mut done = BTreeSet::new();
    for name in edges.keys() {
        if visit(name, edges, &mut visiting, &mut done) {
            return true;
        }
    }
    false
}

/// Depth-first cycle probe from one member.
fn visit(
    name: &str,
    edges: &BTreeMap<String, BTreeSet<String>>,
    visiting: &mut BTreeSet<String>,
    done: &mut BTreeSet<String>,
) -> bool {
    if !visiting.insert(name.to_owned()) {
        return true;
    }
    if done.contains(name) {
        visiting.remove(name);
        return false;
    }
    let mut found = false;
    if let Some(deps) = edges.get(name) {
        for dep in deps {
            if visit(dep, edges, visiting, done) {
                found = true;
                break;
            }
        }
    }
    visiting.remove(name);
    done.insert(name.to_owned());
    found
}

/// EXACT forbidden-token table for orchestrator sources (gaps F).
///
/// Any edit here is a deliberate policy change: process-spawn paths
/// must never appear in orchestrator code, raw or concat-built.
const FORBIDDEN_TOKENS: &[&str] = &[
    "tokio::process::Command",
    "std::process::Command",
    "Command::new",
    "CommandExt",
    ".spawn(",
    ".output(",
];

/// Evasion fixtures plus the token each must trip.
const EVASION_CASES: &[(&str, &str)] = &[
    ("tokio.rs", "tokio::process::Command"),
    ("concat.rs", "tokio::process::Command"),
    ("ext.rs", "CommandExt"),
    ("shell.rs", "sh -c"),
];

/// Normalize concat-built strings: drop quotes, whitespace, and `+`.
fn normalize(source: &str) -> String {
    source
        .chars()
        .filter(|ch| !matches!(ch, '"' | '\'' | '+' | ' ' | '\t' | '\n' | '\r'))
        .collect()
}

/// Tokens flagged in `source` after normalization.
fn flagged(source: &str) -> Vec<&'static str> {
    let flat = normalize(source);
    let mut hits = Vec::new();
    for token in FORBIDDEN_TOKENS {
        if flat.contains(token) {
            hits.push(*token);
        }
    }
    if source.contains("sh -c") || flat.contains("sh-c") {
        hits.push("sh -c");
    }
    hits
}

#[test]
fn forbidden_token_table_is_exact() {
    assert_eq!(
        FORBIDDEN_TOKENS,
        &[
            "tokio::process::Command",
            "std::process::Command",
            "Command::new",
            "CommandExt",
            ".spawn(",
            ".output(",
        ]
    );
}

#[test]
fn evasion_fixtures_are_all_flagged() -> TestResult {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/f2-evasion");
    let mut files = Vec::new();
    for entry in std::fs::read_dir(&dir)? {
        let path = entry?.path();
        if path.extension().is_some_and(|ext| ext == "rs") {
            files.push(path);
        }
    }
    assert_eq!(files.len(), EVASION_CASES.len(), "no stray fixtures");
    for (name, want) in EVASION_CASES {
        let source = std::fs::read_to_string(dir.join(name))?;
        let hits = flagged(&source);
        assert!(hits.contains(want), "{name} must trip {want}: {hits:?}");
    }
    Ok(())
}

/// Strip a trailing `//` comment, ignoring `//` inside string literals.
fn strip_line_comment(line: &str) -> &str {
    let bytes = line.as_bytes();
    let mut quoted = false;
    let mut escape = false;
    let mut index = 0;
    while index + 1 < bytes.len() {
        let byte = bytes[index];
        if escape {
            escape = false;
        } else if byte == b'\\' && quoted {
            escape = true;
        } else if byte == b'"' {
            quoted = !quoted;
        } else if byte == b'/' && bytes[index + 1] == b'/' && !quoted {
            return line[..index].trim_end();
        }
        index += 1;
    }
    line
}

#[test]
fn orchestrator_src_passes_forbidden_table() -> TestResult {
    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut checked = 0;
    let mut stack = vec![src];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir)? {
            let path = entry?.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if path.extension().is_some_and(|ext| ext != "rs") {
                continue;
            }
            let text = std::fs::read_to_string(&path)?;
            let code: String = text
                .lines()
                .map(strip_line_comment)
                .collect::<Vec<_>>()
                .join("\n");
            let hits = flagged(&code);
            let shell = hits.contains(&"sh -c");
            let spawn = hits.iter().any(|hit| *hit != "sh -c");
            assert!(!spawn, "spawn token in {}: {hits:?}", path.display());
            if shell {
                let name = path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default();
                assert_eq!(
                    name, "workflow_jobs.rs",
                    "sh -c confined to the fixed template"
                );
                assert!(code.contains("TASK_RUN_ENV"), "env-only template");
            }
            checked += 1;
        }
    }
    assert!(checked > 0, "sources scanned");
    Ok(())
}
