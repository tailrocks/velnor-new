//! Negative cases for the four enabled Alint rules (`.alint.yml`).
//!
//! Each test embeds one fixture from `fixtures/alint-negative/` and asserts
//! the mirrored rule predicate rejects it. The predicates mirror the rule
//! semantics in `.alint.yml`; real enforcement is the pinned `alint` binary
//! in its own CI job (rust-quality-contract §8).

/// Required repo-shape files, mirrored from the `required-files` rule.
const REQUIRED_FILES: [&str; 9] = [
    "Cargo.toml",
    "Cargo.lock",
    "clippy.toml",
    "deny.toml",
    "rustfmt.toml",
    "CODEOWNERS",
    ".alint.yml",
    ".config/nextest.toml",
    "AGENTS.md",
];

const REPO_FILE_LIST: &str =
    include_str!("../../../../fixtures/alint-negative/required-files/repo-file-list.txt");
const STRAY_RS: &str = include_str!("../../../../fixtures/alint-negative/crates-only/stray.rs");
const OVERSIZED_RS: &str =
    include_str!("../../../../fixtures/alint-negative/rust-max-lines/oversized.rs");
const BIG_LIB_RS: &str = include_str!("../../../../fixtures/alint-negative/lib-main-max-lines/lib.rs");

/// Required entries absent from `present` (mirrors `file_exists`).
fn missing_required(present: &[&str]) -> Vec<&'static str> {
    let mut missing = Vec::new();
    for name in REQUIRED_FILES {
        if !present.contains(&name) {
            missing.push(name);
        }
    }
    missing
}

/// True when `repo_path` matches `**/*.rs` outside `crates/**`.
fn violates_crates_only(repo_path: &str) -> bool {
    std::path::Path::new(repo_path)
        .extension()
        .is_some_and(|ext| ext == "rs")
        && !repo_path.starts_with("crates/")
}

/// Physical line count, matching `file_max_lines` accounting.
fn physical_lines(text: &str) -> usize {
    text.lines().count()
}

/// True when `text` exceeds an Alint `max_lines` limit.
fn exceeds_max_lines(text: &str, max_lines: usize) -> bool {
    physical_lines(text) > max_lines
}

/// Value of the `// key: value` header on the fixture's first line.
fn header_value<'fixture>(text: &'fixture str, key: &str) -> Option<&'fixture str> {
    let first = text.lines().next()?;
    let rest = first.strip_prefix("// ")?;
    let (name, value) = rest.split_once(": ")?;
    if name == key { Some(value) } else { None }
}

#[test]
fn required_files_fixture_reports_exactly_one_missing() {
    let owned: Vec<String> = REPO_FILE_LIST
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect();
    let present: Vec<&str> = owned.iter().map(String::as_str).collect();
    assert_eq!(missing_required(&present), vec!["clippy.toml"]);
    let complete: Vec<&str> = REQUIRED_FILES.to_vec();
    assert!(missing_required(&complete).is_empty());
}

#[test]
fn crates_only_fixture_path_is_rejected() {
    let repo_path = header_value(STRAY_RS, "repo-path").expect("fixture header");
    assert!(
        violates_crates_only(repo_path),
        "fixture path must be rejected: {repo_path}"
    );
    assert!(!violates_crates_only(
        "crates/core/velnor-actions-contract/src/lib.rs"
    ));
    assert!(!violates_crates_only("docs/notes.md"));
    assert!(violates_crates_only("crates-notes.rs"));
}

#[test]
fn rust_max_lines_fixture_exceeds_400() {
    let expected: usize = header_value(OVERSIZED_RS, "expected-lines")
        .expect("fixture header")
        .parse()
        .expect("line-count header is a number");
    assert_eq!(physical_lines(OVERSIZED_RS), expected);
    assert_eq!(expected, 401);
    assert!(exceeds_max_lines(OVERSIZED_RS, 400));
    assert!(!exceeds_max_lines(&"// filler\n".repeat(400), 400));
    assert!(exceeds_max_lines(&"// filler\n".repeat(401), 400));
}

#[test]
fn lib_main_max_lines_fixture_exceeds_150() {
    let expected: usize = header_value(BIG_LIB_RS, "expected-lines")
        .expect("fixture header")
        .parse()
        .expect("line-count header is a number");
    assert_eq!(physical_lines(BIG_LIB_RS), expected);
    assert_eq!(expected, 151);
    assert!(exceeds_max_lines(BIG_LIB_RS, 150));
    assert!(!exceeds_max_lines(&"// filler\n".repeat(150), 150));
    assert!(exceeds_max_lines(&"// filler\n".repeat(151), 150));
}
