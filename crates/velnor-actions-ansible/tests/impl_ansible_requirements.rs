use velnor_actions_ansible::{check_core_agreement, parse_requirements};

const H1: &str = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const H2: &str = "sha256:fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210";

fn pinned() -> String {
    format!(
        "# compiled\nansible-core==2.21.4 \\\n    --hash={H1} \\\n    --hash={H2}\nPyYAML==6.0.2 --hash={H1}\nansible_lint==26.1.0 \\\n    --hash={H1}\n"
    )
}

fn codes(text: &str) -> Vec<String> {
    match parse_requirements(text) {
        Ok(_) => Vec::new(),
        Err(findings) => findings.iter().map(|f| f.code().to_owned()).collect(),
    }
}

#[test]
fn a_fully_hashed_file_parses_with_normalized_names() {
    let parsed = parse_requirements(&pinned()).expect("pinned");
    let names: Vec<&str> = parsed.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(names, ["ansible-core", "pyyaml", "ansible-lint"]);
    assert_eq!(parsed[0].hashes.len(), 2);
    assert_eq!(parsed[0].version, "2.21.4");
}

#[test]
fn the_consumer_style_unhashed_file_fails() {
    assert_eq!(
        codes("ansible-core==2.21.4\nansible-lint==26.1.0\n"),
        [
            "python_requirements_unhashed:ansible-core",
            "python_requirements_unhashed:ansible-lint",
        ]
    );
    assert_eq!(
        codes("ansible-core\n"),
        ["python_requirement_unpinned:ansible-core"]
    );
    assert_eq!(
        codes("ansible-core>=2.20\n"),
        ["python_requirement_unpinned:ansible-core>=2.20"]
    );
}

#[test]
fn options_urls_and_malformed_hashes_are_refused() {
    for text in [
        "--index-url https://evil.example/simple\n",
        "-r other.txt\n",
        "-e .\n",
        "pkg @ https://x/y.whl\n",
        "https://x/y.whl\n",
    ] {
        assert!(
            codes(text)
                .iter()
                .any(|c| c.starts_with("python_requirement_option_forbidden")),
            "{text}"
        );
    }
    assert_eq!(
        codes("a==1.0 --hash=sha256:abc\n"),
        [
            "python_requirement_hash_malformed:a",
            "python_requirements_unhashed:a"
        ]
    );
    assert!(
        codes(&format!("a==1.0 --hash={H1}\nA==1.0 --hash={H1}\n"))
            .contains(&"python_requirement_duplicate:a".to_owned())
    );
}

#[test]
fn ansible_core_must_match_mise() {
    let parsed = parse_requirements(&pinned()).expect("pinned");
    assert!(check_core_agreement(&parsed, "2.21.4").is_ok());
    let error = check_core_agreement(&parsed, "2.21.3").expect_err("disagree");
    assert_eq!(
        error.code(),
        "conflicting_project_tool_versions:ansible-core"
    );
    let error = check_core_agreement(&[], "2.21.4").expect_err("absent");
    assert_eq!(error.code(), "python_requirement_missing:ansible-core");
}
