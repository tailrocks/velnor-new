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
[tools]
"github:boltffi/boltffi" = [
  { version = "0.30.1", backend = "github:boltffi/boltffi", "platforms.macos-arm64" = { url = "https://github.com/boltffi/boltffi/releases/download/v0.30.1/boltffi-darwin-aarch64.tar.gz", checksum = "sha256:ce3a47b5c398cbb9c327098a612b431f30db15d353d62cee4e4637540fa8321a" } }
]
rust = [
  { version = "1.98.0", backend = "core:rust" }
]
"#,
    )
    .expect("valid lock TOML");
    let lock = parse_native_mise_lock(&value).expect("Mise lock projection");
    let selected = lock
        .tools
        .get("github:boltffi/boltffi")
        .expect("selected tool entry");
    let macos = selected.macos_arm64.as_ref().expect("macOS ARM64 entry");

    assert!(lock.valid_shape);
    assert!(selected.valid_shape);
    assert_eq!(selected.backend.as_deref(), Some("github:boltffi/boltffi"));
    assert_eq!(selected.version.as_deref(), Some("0.30.1"));
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
    assert!(selected.linux_x64.is_none());

    let rust = lock.tools.get("rust").expect("Rust lock entry");
    assert!(rust.valid_shape);
    assert_eq!(rust.backend.as_deref(), Some("core:rust"));
    assert_eq!(rust.version.as_deref(), Some("1.98.0"));
    assert!(rust.macos_arm64.is_none());
}

#[test]
fn mise_lock_projects_exact_linux_x64_artifact() {
    let value = toml::from_str(
        r#"
[tools]
"github:example/linter" = [
  { version = "1.2.3", backend = "github:example/linter", "platforms.linux-x64" = { url = "https://github.com/example/linter/releases/download/v1.2.3/linter-linux-x64.tar.gz", checksum = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa" } }
]
"#,
    )
    .expect("valid Linux lock TOML");
    let lock = parse_native_mise_lock(&value).expect("Mise lock projection");
    let tool = lock.tools.get("github:example/linter").expect("tool row");
    let linux = tool.linux_x64.as_ref().expect("Linux x64 artifact");
    assert!(tool.macos_arm64.is_none());
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
future_root = true

[tools]
rust = [
  { version = "1.98.0", backend = "core:rust", future_option = "reject" }
]
"github:boltffi/boltffi" = [
  { version = "0.30.1", backend = "github:boltffi/boltffi", "platforms.macos-arm64" = { url = "https://github.com/boltffi/boltffi/releases/download/v0.30.1/boltffi-darwin-aarch64.tar.gz", checksum = "sha256:ce3a47b5c398cbb9c327098a612b431f30db15d353d62cee4e4637540fa8321a", future_artifact_field = "reject" } }
]
malformed = [
  { version = "1.0.0", backend = "core:rust" },
  { version = "2.0.0", backend = "core:rust" }
]
"#,
    )
    .expect("valid TOML with unknown fields and a malformed selection");
    let lock = parse_native_mise_lock(&value).expect("Mise lock projection");

    assert!(lock.root_keys.contains(&"future_root".to_owned()));
    let rust = lock.tools.get("rust").expect("Rust entry");
    assert!(rust.valid_shape);
    assert_eq!(rust.unsupported_fields, ["future_option"]);

    let selected = lock
        .tools
        .get("github:boltffi/boltffi")
        .expect("selected tool entry");
    let macos = selected.macos_arm64.as_ref().expect("macOS ARM64 entry");
    assert_eq!(macos.unsupported_fields, ["future_artifact_field"]);

    let malformed = lock.tools.get("malformed").expect("malformed entry");
    assert!(!malformed.valid_shape);
    assert!(parse_native_mise_lock(&toml::Value::String("not a lock".to_owned())).is_none());
}
