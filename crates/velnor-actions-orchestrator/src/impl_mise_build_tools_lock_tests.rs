use super::*;

#[test]
fn mise_lock_preserves_typed_v3_repository_ids_and_rejects_malformed_ids() {
    let value = toml::from_str(
        r#"
lockfile_version = 3

[tools]
hk = [
  { version = "2.5.0", backend = "packslip:github.com/jdx/hk", specifiers = ["2.5.0"], "platforms.macos-arm64" = { url = "https://example.test/hk.tar.gz", checksum = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", repository_ids = { repository = "922514152", owner = "jdx" } } }
]
"#,
    )
    .expect("valid TOML repository identity");
    let lock = parse_native_mise_lock(&value).expect("lock projection");
    let artifact = lock.tools["hk"][0].platforms.get("macos-arm64").unwrap();
    let ids = artifact.repository_ids.as_ref().unwrap();
    assert_eq!(ids.repository.as_deref(), Some("922514152"));
    assert_eq!(ids.owner.as_deref(), Some("jdx"));
    assert!(artifact.unsupported_fields.is_empty());

    for ids in [
        r#"repository_ids = "922514152""#,
        r#"repository_ids = { owner = "jdx" }"#,
        r"repository_ids = { repository = 922514152 }",
        r#"repository_ids = { repository = "922514152", future = "reject" }"#,
    ] {
        let row = format!(
            r#"{{ version = "2.5.0", backend = "packslip:github.com/jdx/hk", specifiers = ["2.5.0"], "platforms.macos-arm64" = {{ url = "https://example.test/hk.tar.gz", checksum = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", {ids} }} }}"#
        );
        let source = format!("lockfile_version = 3\n[tools]\nhk = [{row}]\n");
        let value = toml::from_str(&source).expect("valid TOML malformed-field fixture");
        let lock = parse_native_mise_lock(&value).expect("lock projection");
        let artifact = lock.tools["hk"][0].platforms.get("macos-arm64").unwrap();
        assert!(
            !artifact.unsupported_fields.is_empty(),
            "malformed repository identity must fail closed: {ids}"
        );
    }
}

#[test]
fn native_mise_lock_accepts_only_v3_with_the_known_root_fields() {
    let current = toml::from_str("lockfile_version = 3\n\n[tools]\n").expect("valid Mise v3 lock");
    let current = parse_native_mise_lock(&current).expect("lock projection");
    assert!(current.has_supported_root_shape());

    for unsupported in [
        "[tools]\n",
        "lockfile_version = 2\n[tools]\n",
        "lockfile_version = 4\n[tools]\n",
        "lockfile_version = \"3\"\n[tools]\n",
        "lockfile_version = 3\nfuture_root = true\n[tools]\n",
    ] {
        let value = toml::from_str(unsupported).expect("syntactically valid lock TOML");
        let lock = parse_native_mise_lock(&value).expect("lock projection");
        assert!(
            !lock.has_supported_root_shape(),
            "unsupported lock root must fail closed: {unsupported}"
        );
    }
}
