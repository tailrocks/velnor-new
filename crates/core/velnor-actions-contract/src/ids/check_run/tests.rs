use super::CheckRunId;

#[test]
fn parses_a_scoped_canonical_public_check_run_url() {
    let id = CheckRunId::parse_api_url(
        "https://api.github.com/repos/ChainArgos/java-monorepo/check-runs/9021",
        "chainargos",
        "JAVA-MONOREPO",
    )
    .expect("canonical URL is valid");
    assert_eq!(id.get().get(), 9021);
}

#[test]
fn rejects_invalid_scope_even_when_it_matches_the_url() {
    let invalid = [
        ("", "r", "https://api.github.com/repos//r/check-runs/9"),
        (".", "r", "https://api.github.com/repos/./r/check-runs/9"),
        ("..", "r", "https://api.github.com/repos/../r/check-runs/9"),
        (
            "space owner",
            "r",
            "https://api.github.com/repos/space owner/r/check-runs/9",
        ),
        (
            "owner",
            "r?query",
            "https://api.github.com/repos/owner/r?query/check-runs/9",
        ),
    ];
    for (owner, repository, url) in invalid {
        assert!(CheckRunId::parse_api_url(url, owner, repository).is_err());
    }
}

#[test]
fn rejects_noncanonical_url_and_numeric_id_forms() {
    let invalid = [
        "http://api.github.com/repos/owner/r/check-runs/9",
        "https://api.github.com.evil/repos/owner/r/check-runs/9",
        "https://api.github.com/repos/other/r/check-runs/9",
        "https://api.github.com/repos/owner/r/check-runs/0",
        "https://api.github.com/repos/owner/r/check-runs/09",
        "https://api.github.com/repos/owner/r/check-runs/9/",
        "https://api.github.com/repos/owner/r/check-runs/9?x=1",
        "https://api.github.com/repos/owner/r/check-runs/9223372036854775808",
    ];
    for url in invalid {
        assert!(CheckRunId::parse_api_url(url, "owner", "r").is_err());
    }
}

#[test]
fn rejects_invalid_expected_scope_segments() {
    let url = "https://api.github.com/repos/owner/r/check-runs/9";
    let invalid = [
        ("", "r"),
        (".", "r"),
        ("..", "r"),
        ("owner", "r/other"),
        ("owner", "r?query"),
    ];
    for (owner, repository) in invalid {
        assert!(CheckRunId::parse_api_url(url, owner, repository).is_err());
    }
    let long_owner = "a".repeat(101);
    assert!(CheckRunId::parse_api_url(url, &long_owner, "r").is_err());
}
