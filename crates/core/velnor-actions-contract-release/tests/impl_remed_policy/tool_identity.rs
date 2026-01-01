use super::*;
#[test]
fn ver_tool_identity_accepts_canonical_immutable_sources() {
    for source in [
        "https://www.python.org/downloads/release/python-3147/",
        "https://www.python.org/downloads/release/python-3147",
        "https://github.com/python/cpython/tree/v3.14.7",
        "https://github.com/tailrocks/velnor-new/releases/download/v0.1.0/velnor-actions-0.1.0-x86_64-unknown-linux-gnu",
    ] {
        let tool = ToolIdentity {
            name: "rust".to_owned(),
            version: "1.98.1".to_owned(),
            source: source.to_owned(),
            platforms: vec!["linux-x64".to_owned()],
            digest: "ab".repeat(32),
        };
        assert_eq!(tool.validate("catalog"), Ok(()), "{source}");
    }
}

#[test]
fn ver_tool_identity_accepts_every_current_mise_catalog_source() {
    // These are the nine source forms emitted by the checked-in catalog at
    // version 1.2.3. Keep this compatibility contract visible here when the
    // public ToolIdentity URL grammar is tightened.
    for source in [
        "https://static.rust-lang.org/dist/channel-rust-stable.toml",
        "https://github.com/jdx/mr-boxington/releases/tag/v1.2.3",
        "https://github.com/cli/cli/releases/tag/v1.2.3",
        "https://github.com/rhysd/actionlint/releases/tag/v1.2.3",
        "https://github.com/koalaman/shellcheck/releases/tag/v1.2.3",
        "https://github.com/zizmorcore/zizmor/releases/tag/v1.2.3",
        "https://github.com/nextest-rs/nextest/releases/tag/cargo-nextest-1.2.3",
        "https://github.com/opentofu/opentofu/releases/tag/v1.2.3",
        "https://crates.io/api/v1/crates/release-plz/1.2.3",
    ] {
        let tool = ToolIdentity {
            name: "catalog-tool".to_owned(),
            version: "1.2.3".to_owned(),
            source: source.to_owned(),
            platforms: vec!["linux-x64".to_owned()],
            digest: "ab".repeat(32),
        };
        assert_eq!(tool.validate("catalog"), Ok(()), "{source}");
    }
}

#[test]
fn ver_tool_identity_rejects_ambiguous_or_floating_sources() {
    let valid = ToolIdentity {
        name: "rust".to_owned(),
        version: "1.98.1".to_owned(),
        source: "https://static.rust-lang.org/dist/channel-rust-stable.toml".to_owned(),
        platforms: vec!["linux-x64".to_owned()],
        digest: "ab".repeat(32),
    };
    for source in [
        "http://www.python.org/downloads/release/python-3147/",
        "https://www.python.org/downloads/release/python-3147//",
        "https://www.python.org/downloads//release/python-3147/",
        "https://www.python.org/downloads/../release/python-3147/",
        "https://www.python.org/downloads/./release/python-3147/",
        "https://www.python.org/downloads/%2e%2e/release/python-3147/",
        "https://www.python.org/downloads/release/python-3147/?next=x",
        "https://www.python.org/downloads/release/python-3147/#files",
        "https://www.python.org/downloads/release/latest/",
        "https://www.python.org/downloads/release/LATEST/",
        "https://user@www.python.org/downloads/release/python-3147/",
        "https://www.python.org:443/downloads/release/python-3147/",
        "https://www..python.org/downloads/release/python-3147/",
        "https://www.python.org/downloads\\release/python-3147/",
        "https://www.python.org/downloads/release/python-3147/\n",
        "https://www.python.org/",
    ] {
        let mut tool = valid.clone();
        tool.source = source.to_owned();
        assert!(tool.validate("catalog").is_err(), "{source:?}");
    }
}

#[test]
fn ver_tool_identity_enforces_dns_label_and_host_length_boundaries() {
    let valid = ToolIdentity {
        name: "rust".to_owned(),
        version: "1.98.1".to_owned(),
        source: String::new(),
        platforms: vec!["linux-x64".to_owned()],
        digest: "ab".repeat(32),
    };
    let mut tool = valid.clone();
    tool.source = format!("https://{}.example/path/v1.2.3", "a".repeat(63));
    assert_eq!(tool.validate("catalog"), Ok(()));

    tool.source = format!("https://{}.example/path/v1.2.3", "a".repeat(64));
    assert!(tool.validate("catalog").is_err());

    let host_253 = [
        "a".repeat(63),
        "b".repeat(63),
        "c".repeat(63),
        "d".repeat(61),
    ]
    .join(".");
    tool.source = format!("https://{host_253}/path/v1.2.3");
    assert_eq!(tool.validate("catalog"), Ok(()));

    let host_254 = [
        "a".repeat(63),
        "b".repeat(63),
        "c".repeat(63),
        "d".repeat(62),
    ]
    .join(".");
    tool.source = format!("https://{host_254}/path/v1.2.3");
    assert!(tool.validate("catalog").is_err());
}
