use super::*;

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

fn rust_sources_recursive(
    root: &std::path::Path,
) -> Result<Vec<PathBuf>, Box<dyn std::error::Error>> {
    let mut pending = vec![root.to_path_buf()];
    let mut sources = Vec::new();
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(directory)? {
            let entry = entry?;
            let kind = entry.file_type()?;
            let path = entry.path();
            if kind.is_dir() {
                pending.push(path);
            } else if kind.is_file() && path.extension().is_some_and(|ext| ext == "rs") {
                sources.push(path);
            }
        }
    }
    sources.sort();
    Ok(sources)
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

type SourceForbiddenHits = Vec<(PathBuf, Vec<&'static str>)>;

/// Return forbidden tokens from production sources, excluding unit-test-only
/// modules that are deliberately compiled only for tests and may spawn tools.
fn production_source_hits(
    root: &std::path::Path,
) -> Result<SourceForbiddenHits, Box<dyn std::error::Error>> {
    let mut hits_by_path = Vec::new();
    for path in rust_sources_recursive(root)? {
        let relative = path.strip_prefix(root)?;
        let file_is_test_module = path
            .file_name()
            .is_some_and(|name| name.to_string_lossy().ends_with("_tests.rs"));
        let is_in_test_directory = relative
            .components()
            .any(|component| matches!(component.as_os_str().to_str(), Some("test" | "tests")));
        // rust_manifest_step.rs registers this test child under #[cfg(test)].
        // Keep the production restrictions for other *_tests paths.
        let is_rust_manifest_test_child = relative.starts_with("rust_manifest_step_tests");
        if file_is_test_module || is_in_test_directory || is_rust_manifest_test_child {
            continue;
        }
        let text = std::fs::read_to_string(&path)?;
        let code: String = text
            .lines()
            .map(strip_line_comment)
            .collect::<Vec<_>>()
            .join("\n");
        let hits = flagged(&code);
        if !hits.is_empty() {
            hits_by_path.push((path, hits));
        }
    }
    Ok(hits_by_path)
}

#[test]
fn production_source_scan_excludes_test_only_modules_and_rejects_process_spawns() -> TestResult {
    let temp = tempfile::tempdir()?;
    let src = temp.path().join("src");
    std::fs::create_dir_all(src.join("nested/tests"))?;
    std::fs::create_dir_all(src.join("rust_manifest_step_tests"))?;
    std::fs::create_dir_all(src.join("fixture_tests"))?;
    std::fs::write(
        src.join("runtime.rs"),
        "let _ = std::process::Command::new(\"node\").output();",
    )?;
    std::fs::write(
        src.join("validator_tests.rs"),
        "let _ = std::process::Command::new(\"node\").output();",
    )?;
    std::fs::write(
        src.join("nested/tests/fixture.rs"),
        "let _ = std::process::Command::new(\"node\").output();",
    )?;
    std::fs::write(
        src.join("rust_manifest_step_tests/execution.rs"),
        "let _ = std::process::Command::new(\"node\").output();",
    )?;
    std::fs::write(
        src.join("fixture_tests/runtime.rs"),
        "let _ = std::process::Command::new(\"node\").output();",
    )?;

    let hits = production_source_hits(&src)?;
    assert_eq!(
        hits.len(),
        2,
        "The scanner must exclude only registered test modules."
    );
    assert!(hits.iter().any(|(path, _)| path == &src.join("runtime.rs")));
    assert!(
        hits.iter()
            .any(|(path, _)| path == &src.join("fixture_tests/runtime.rs"))
    );
    assert!(
        hits.iter()
            .all(|(_, tokens)| tokens.contains(&"Command::new"))
    );
    assert!(hits.iter().all(|(_, tokens)| tokens.contains(&".output(")));
    Ok(())
}

#[test]
fn orchestrator_src_passes_forbidden_table() -> TestResult {
    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    let hits_by_path = production_source_hits(&src)?;
    let checked = rust_sources_recursive(&src)?.len();
    for (path, hits) in hits_by_path {
        let shell = hits.contains(&"sh -c");
        let spawn = hits.iter().any(|hit| *hit != "sh -c");
        assert!(!spawn, "spawn token in {}: {hits:?}", path.display());
        if shell {
            let text = std::fs::read_to_string(&path)?;
            let code: String = text
                .lines()
                .map(strip_line_comment)
                .collect::<Vec<_>>()
                .join("\n");
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
    }
    assert!(checked > 0, "sources scanned");
    Ok(())
}
