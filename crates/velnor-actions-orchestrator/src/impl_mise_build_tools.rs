//! Parser-shape tests for native Mise projections.
//!
//! These fixtures test parsing only. They do not prove tool installation,
//! artifact provenance, or resolver acceptance.

use super::*;

#[test]
fn mise_config_projects_task_dependencies_and_nested_task_calls() {
    let value = toml::from_str(
        r#"
[tools]
rust = "1.98.0"

[tasks.format]
run = "cargo fmt --all -- --check"

[tasks.unit]
run = "cargo test --workspace"

[tasks.verify]
run = ["cargo check --workspace", "mise run unit"]
depends = ["format"]
"#,
    )
    .expect("valid Mise TOML");
    let config = parse_native_mise_config(&value).expect("Mise config projection");
    let verify = config.tasks.get("verify").expect("verify task");

    assert!(verify.valid_shape);
    assert!(verify.run_field_present);
    assert_eq!(
        verify.run_commands,
        Some(vec![
            "cargo check --workspace".to_owned(),
            "mise run unit".to_owned()
        ])
    );
    assert_eq!(verify.dependencies, ["format"]);
}

#[test]
fn mise_config_preserves_env_source_overlay_for_allowlist_rejection() {
    // This sentinel path is data only: the parser never evaluates Mise env scripts.
    let value = toml::from_str(
        r#"
[env]
_.source = "./sentinel.sh"
"#,
    )
    .expect("valid TOML with an env source directive");
    let config = parse_native_mise_config(&value).expect("Mise config projection");

    assert!(config.root_keys.contains(&"env".to_owned()));
    assert_eq!(config.root_keys, ["env"]);
}

#[test]
fn mise_config_exposes_invalid_task_shapes_and_unknown_fields() {
    let value = toml::from_str(
        r#"
[tasks.bad_shape]
run = 7

[tasks.bad_nested]
run = ["mise install cargo@1.98.0"]
depends = ["format", 7]
custom = "unsupported"
"#,
    )
    .expect("valid TOML with unsupported task shapes");
    let config = parse_native_mise_config(&value).expect("Mise config projection");
    let bad_shape = config.tasks.get("bad_shape").expect("bad-shape task");
    assert!(bad_shape.run_field_present);
    assert!(bad_shape.run_commands.is_none());

    let bad_nested = config.tasks.get("bad_nested").expect("bad-nested task");
    assert!(bad_nested.valid_shape);
    assert!(bad_nested.run_field_present);
    assert_eq!(
        bad_nested.run_commands,
        Some(vec!["mise install cargo@1.98.0".to_owned()])
    );
    assert!(bad_nested.unsupported_fields.contains(&"custom".to_owned()));
    assert!(
        bad_nested
            .unsupported_fields
            .contains(&"depends.shape".to_owned())
    );
}

#[test]
fn mise_lock_projects_exact_selected_tool_and_macos_arm64_artifact() {
    // Parser-shape fixture only. It makes no install-acceptance or provenance claim.
    let value = toml::from_str(
        r#"
lockfile_version = 3

[tools]
"github:boltffi/boltffi" = [
  { version = "0.30.1", backend = "github:boltffi/boltffi", specifiers = ["0.30.1"], "platforms.macos-arm64" = { url = "https://github.com/boltffi/boltffi/releases/download/v0.30.1/boltffi-darwin-aarch64.tar.gz", checksum = "sha256:ce3a47b5c398cbb9c327098a612b431f30db15d353d62cee4e4637540fa8321a" } }
]
rust = [
  { version = "1.98.0", backend = "core:rust", specifiers = ["1.98.0"] }
]
"#,
    )
    .expect("valid lock TOML");
    let lock = parse_native_mise_lock(&value).expect("Mise lock projection");
    let selected_rows = lock
        .tools
        .get("github:boltffi/boltffi")
        .expect("selected tool rows");
    assert_eq!(selected_rows.len(), 1, "fixture has one exact selected row");
    let selected = selected_rows.first().expect("selected tool entry");
    let macos = selected
        .platforms
        .get("macos-arm64")
        .expect("macOS ARM64 entry");

    assert!(lock.valid_shape);
    assert!(lock.has_supported_root_shape());
    assert_eq!(lock.lockfile_version, Some(3));
    assert!(selected.valid_shape);
    assert_eq!(selected.backend.as_deref(), Some("github:boltffi/boltffi"));
    assert_eq!(selected.version.as_deref(), Some("0.30.1"));
    assert_eq!(
        selected.specifiers.as_deref(),
        Some(&["0.30.1".to_owned()][..])
    );
    assert!(macos.valid_shape);
    assert_eq!(
        macos.url.as_deref(),
        Some(
            "https://github.com/boltffi/boltffi/releases/download/v0.30.1/boltffi-darwin-aarch64.tar.gz"
        )
    );
    assert_eq!(
        macos.checksum.as_deref(),
        Some("sha256:ce3a47b5c398cbb9c327098a612b431f30db15d353d62cee4e4637540fa8321a")
    );
    assert!(!selected.platforms.contains_key("linux-x64"));

    let rust = lock
        .tools
        .get("rust")
        .and_then(|entries| entries.first())
        .expect("Rust lock entry");
    assert!(rust.valid_shape);
    assert_eq!(rust.backend.as_deref(), Some("core:rust"));
    assert_eq!(rust.version.as_deref(), Some("1.98.0"));
    assert_eq!(rust.specifiers.as_deref(), Some(&["1.98.0".to_owned()][..]));
    assert!(rust.platforms.is_empty());
}

#[test]
fn mise_lock_projects_exact_linux_x64_artifact() {
    let value = toml::from_str(
        r#"
lockfile_version = 3

[tools]
"github:example/linter" = [
  { version = "1.2.3", backend = "github:example/linter", specifiers = ["1.2.3"], "platforms.linux-x64" = { url = "https://github.com/example/linter/releases/download/v1.2.3/linter-linux-x64.tar.gz", checksum = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa" } }
]
"#,
    )
    .expect("valid Linux lock TOML");
    let lock = parse_native_mise_lock(&value).expect("Mise lock projection");
    let tool = lock
        .tools
        .get("github:example/linter")
        .and_then(|entries| entries.first())
        .expect("tool row");
    let linux = tool.platforms.get("linux-x64").expect("Linux x64 artifact");
    assert!(!tool.platforms.contains_key("macos-arm64"));
    assert_eq!(tool.version.as_deref(), Some("1.2.3"));
    assert_eq!(
        linux.url.as_deref(),
        Some("https://github.com/example/linter/releases/download/v1.2.3/linter-linux-x64.tar.gz")
    );
}

#[test]
fn mise_lock_exposes_unknown_fields_and_rejects_malformed_shapes() {
    let value = toml::from_str(
        r#"
lockfile_version = 3
future_root = true

[tools]
rust = [
  { version = "1.98.0", backend = "core:rust", specifiers = ["1.98.0"], future_option = "reject" }
]
"github:boltffi/boltffi" = [
  { version = "0.30.1", backend = "github:boltffi/boltffi", specifiers = ["0.30.1"], "platforms.macos-arm64" = { url = "https://github.com/boltffi/boltffi/releases/download/v0.30.1/boltffi-darwin-aarch64.tar.gz", checksum = "sha256:ce3a47b5c398cbb9c327098a612b431f30db15d353d62cee4e4637540fa8321a", future_artifact_field = "reject" } }
]
malformed = [
  { version = "1.0.0", backend = "core:rust", specifiers = ["1.0.0"] },
  { version = "2.0.0", backend = "core:rust", specifiers = ["2.0.0"] }
]
"#,
    )
    .expect("valid TOML with unknown fields and a malformed selection");
    let lock = parse_native_mise_lock(&value).expect("Mise lock projection");

    assert!(lock.root_keys.contains(&"future_root".to_owned()));
    assert!(!lock.has_supported_root_shape());
    let rust = lock
        .tools
        .get("rust")
        .and_then(|entries| entries.first())
        .expect("Rust entry");
    assert!(rust.valid_shape);
    assert_eq!(rust.unsupported_fields, ["future_option"]);

    let selected_rows = lock
        .tools
        .get("github:boltffi/boltffi")
        .expect("selected tool rows");
    assert_eq!(selected_rows.len(), 1, "fixture has one exact selected row");
    let selected = selected_rows.first().expect("selected tool entry");
    let macos = selected
        .platforms
        .get("macos-arm64")
        .expect("macOS ARM64 entry");
    assert_eq!(macos.unsupported_fields, ["future_artifact_field"]);

    let variants = lock.tools.get("malformed").expect("version variants");
    assert_eq!(variants.len(), 2);
    assert!(variants.iter().all(|entry| entry.valid_shape));
    assert!(
        lock.selected_tool(
            "malformed",
            "1.0.0",
            "1.0.0",
            &std::collections::BTreeMap::new(),
        )
        .is_some()
    );
    assert!(
        lock.selected_tool(
            "malformed",
            "2.0.0",
            "2.0.0",
            &std::collections::BTreeMap::new(),
        )
        .is_some()
    );
    assert!(parse_native_mise_lock(&toml::Value::String("not a lock".to_owned())).is_none());
}

#[test]
fn mise_lock_specifiers_must_be_nonempty_unique_string_lists() {
    for row in [
        r#"{ version = "1.2.3", backend = "core:rust" }"#,
        r#"{ version = "1.2.3", backend = "core:rust", specifiers = "1.2.3" }"#,
        r#"{ version = "1.2.3", backend = "core:rust", specifiers = [] }"#,
        r#"{ version = "1.2.3", backend = "core:rust", specifiers = [1] }"#,
        r#"{ version = "1.2.3", backend = "core:rust", specifiers = [""] }"#,
        r#"{ version = "1.2.3", backend = "core:rust", specifiers = ["1.2.3", "1.2.3"] }"#,
    ] {
        let value = toml::from_str(&format!("lockfile_version = 3\n[tools]\nrust = [{row}]\n"))
            .expect("syntactically valid lock row");
        let lock = parse_native_mise_lock(&value).expect("lock projection");
        let rust = lock
            .tools
            .get("rust")
            .and_then(|entries| entries.first())
            .expect("Rust row");
        assert!(
            rust.unsupported_fields
                .iter()
                .any(|field| field.starts_with("specifiers.")),
            "invalid specifier field must be rejected: {row}"
        );
    }
}

#[test]
fn mise_lock_selects_exact_option_variants_and_rejects_ambiguous_rows() {
    let value = toml::from_str(
        r#"
lockfile_version = 3

[tools]
rust = [
  { version = "1.99.0", backend = "core:rust", specifiers = ["1.99.0"], options = { profile = "plain" } },
  { version = "1.99.0", backend = "core:rust", specifiers = ["=1.99.0"], options = { profile = "plain" } },
  { version = "1.99.0", backend = "core:rust", specifiers = ["1.99.0"], options = { profile = "workspace" } }
]
"#,
    )
    .expect("valid duplicate-version option variants");
    let lock = parse_native_mise_lock(&value).expect("lock projection");
    assert_eq!(lock.tools["rust"].len(), 3);
    assert!(
        lock.selected_tool(
            "rust",
            "1.99.0",
            "1.99.0",
            &std::collections::BTreeMap::from([("profile".to_owned(), "plain".to_owned())]),
        )
        .is_some()
    );
    assert!(
        lock.selected_tool(
            "rust",
            "1.99.0",
            "1.99",
            &std::collections::BTreeMap::from([("profile".to_owned(), "plain".to_owned())]),
        )
        .is_none(),
        "a lock row must bind the exact requested specifier"
    );
    assert!(
        lock.selected_tool(
            "rust",
            "1.99.0",
            "=1.99.0",
            &std::collections::BTreeMap::from([("profile".to_owned(), "plain".to_owned())]),
        )
        .is_some()
    );
    assert!(
        lock.selected_tool(
            "rust",
            "1.99.0",
            "1.99.0",
            &std::collections::BTreeMap::from([("profile".to_owned(), "workspace".to_owned())]),
        )
        .is_some()
    );

    let ambiguous = toml::from_str(
        r#"
lockfile_version = 3

[tools]
rust = [
  { version = "1.99.0", backend = "core:rust", specifiers = ["1.99.0"] },
  { version = "1.99.0", backend = "aqua:unexpected/tool", specifiers = ["1.99.0"] }
]
"#,
    )
    .expect("valid duplicated rows");
    let lock = parse_native_mise_lock(&ambiguous).expect("lock projection");
    assert!(
        lock.selected_tool(
            "rust",
            "1.99.0",
            "1.99.0",
            &std::collections::BTreeMap::new(),
        )
        .is_none()
    );
}

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
        r#"repository_ids = { repository = 922514152 }"#,
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
