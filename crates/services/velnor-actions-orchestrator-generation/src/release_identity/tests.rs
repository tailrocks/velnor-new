use super::*;

#[test]
fn origin_shapes_normalize_to_owner_repo() {
    for (url, owner_repo) in [
        ("https://github.com/acme/widgets.git", "acme/widgets"),
        ("https://github.com/acme/widgets", "acme/widgets"),
        ("https://github.com/acme/widgets/", "acme/widgets"),
        ("git@github.com:acme/widgets.git", "acme/widgets"),
        ("ssh://git@github.com/acme/widgets.git", "acme/widgets"),
        ("ssh://git@GITHUB.COM/acme/widgets.git", "acme/widgets"),
        ("Acme/Widgets", "Acme/Widgets"),
    ] {
        assert_eq!(normalize_origin(url).as_deref(), Some(owner_repo), "{url}");
    }
}

#[test]
fn foreign_or_malformed_origins_yield_none() {
    for url in [
        "https://evil.example/acme/widgets",
        "git@evil.example:acme/widgets.git",
        "https://github.com/acme",
        "https://github.com/acme/widgets/extra",
        "https://github.com/acme/",
        "git@github.com:acme",
        "not a url",
        "",
    ] {
        assert_eq!(normalize_origin(url), None, "{url}");
    }
}

#[test]
fn workspace_slugs_stay_lock_shaped() {
    assert_eq!(workspace_slug("Cargo.toml"), "root");
    assert_eq!(workspace_slug("crates/foo/Cargo.toml"), "crates-foo");
    assert_eq!(workspace_slug("My_WS.v2/Cargo.toml"), "my_ws-v2");
    let long = format!("{}/Cargo.toml", "d".repeat(80));
    let slug = workspace_slug(&long);
    assert_eq!(slug.len(), 64);
    assert!(slug.bytes().all(|b| b == b'd'));
}

#[test]
fn plan_ids_derive_from_the_source_sha() {
    let sha = "0123456789abcdef0123456789abcdef01234567";
    assert_eq!(plan_id_for_source(sha), "release-0123456789ab");
}
