use std::ffi::{OsStr, OsString};

use super::{mbx_child_environment, mbx_path_from_path, parent_env_value};

#[test]
fn mbx_child_removes_inherited_tool_and_authority_overrides() {
    let mise_data = "/opt/mise/data";
    let wrapper = format!("{mise_data}/command-wrappers/bin");
    let shims = format!("{mise_data}/shims");
    let rust_bin = "/opt/rust/bin";
    let mbx_bin = "/opt/mbx/bin";
    let original_path = std::env::join_paths([
        std::path::Path::new(&wrapper),
        std::path::Path::new(&shims),
        std::path::Path::new(rust_bin),
        std::path::Path::new(mbx_bin),
    ])
    .unwrap();
    let parent = vec![
        (OsString::from("PATH"), original_path),
        (OsString::from("MISE_DATA_DIR"), OsString::from(mise_data)),
        (
            OsString::from("MISE_CARGO_HOME"),
            OsString::from("/owned/cargo"),
        ),
        (
            OsString::from("MISE_RUSTUP_HOME"),
            OsString::from("/owned/rustup"),
        ),
        (OsString::from("CARGO_HOME"), OsString::from("/cargo/home")),
        (
            OsString::from("CARGO_TARGET_DIR"),
            OsString::from("/old/target"),
        ),
        (
            OsString::from("RUSTC_WRAPPER"),
            OsString::from("/mbx/wrapper"),
        ),
        (OsString::from("RUSTUP_TOOLCHAIN"), OsString::from("1.97.1")),
        (OsString::from("CARGO"), OsString::from("/raw/cargo")),
        (OsString::from("RUSTC"), OsString::from("/raw/rustc")),
        (OsString::from("RUSTDOC"), OsString::from("/raw/rustdoc")),
        (OsString::from("GITHUB_TOKEN"), OsString::from("secret")),
        (
            OsString::from("CARGO_REGISTRIES_PRIVATE_TOKEN"),
            OsString::from("secret"),
        ),
        (
            OsString::from("ACTIONS_ID_TOKEN_REQUEST_URL"),
            OsString::from("secret"),
        ),
        (
            OsString::from("GH_HOST"),
            OsString::from("attacker.example"),
        ),
        (
            OsString::from("GH_CONFIG_DIR"),
            OsString::from("/untrusted/gh"),
        ),
        (OsString::from("TF_VAR_password"), OsString::from("secret")),
        (
            OsString::from("TOFU_CLI_ARGS_apply"),
            OsString::from("-auto-approve"),
        ),
        (OsString::from("CHECKPOINT_DISABLE"), OsString::from("1")),
    ];
    let target_override = OsString::from("/nested/target");
    let mut additions = super::isolation_environment();
    additions.push((OsString::from("CARGO_TARGET_DIR"), target_override.clone()));

    let child = mbx_child_environment(&parent, &additions);
    for stripped in [
        "CARGO",
        "RUSTC",
        "RUSTDOC",
        "RUSTUP_TOOLCHAIN",
        "GITHUB_TOKEN",
        "CARGO_REGISTRIES_PRIVATE_TOKEN",
        "ACTIONS_ID_TOKEN_REQUEST_URL",
        "GH_HOST",
        "GH_CONFIG_DIR",
        "TF_VAR_password",
        "TOFU_CLI_ARGS_apply",
        "CHECKPOINT_DISABLE",
    ] {
        assert!(parent_env_value(&child, stripped).is_none(), "{stripped}");
    }
    for retained in [
        "MISE_DATA_DIR",
        "MISE_CARGO_HOME",
        "MISE_RUSTUP_HOME",
        "CARGO_HOME",
        "RUSTC_WRAPPER",
    ] {
        assert!(parent_env_value(&child, retained).is_some(), "{retained}");
    }
    assert_eq!(
        parent_env_value(&child, "CARGO_TARGET_DIR"),
        Some(target_override.as_os_str())
    );
    for (key, value) in [
        ("MISE_NO_CONFIG", "1"),
        ("MISE_NO_ENV", "1"),
        ("MISE_NO_HOOKS", "1"),
        ("MISE_LOCKFILE", "0"),
        ("MISE_AUTO_INSTALL", "false"),
        ("MISE_EXEC_AUTO_INSTALL", "false"),
    ] {
        assert_eq!(
            parent_env_value(&child, key),
            Some(OsStr::new(value)),
            "{key}"
        );
    }
    let expected_path = std::env::join_paths([
        std::path::Path::new(rust_bin),
        std::path::Path::new(mbx_bin),
    ])
    .unwrap();
    assert_eq!(
        parent_env_value(&child, "PATH"),
        Some(expected_path.as_os_str())
    );
}

#[test]
fn relative_mbx_path_is_resolved_before_child_changes_directory()
-> Result<(), Box<dyn std::error::Error>> {
    let root = crate::impl_cli_tmp::fresh_tempdir("relative-mbx-path")?;
    let bin = root.join("relative/bin");
    std::fs::create_dir_all(&bin)?;
    std::fs::write(bin.join("mbx"), b"test executable placeholder")?;
    let path = OsString::from("relative/bin");
    let selected = mbx_path_from_path(&path, &root).ok_or("relative-path MBX was not found")?;
    assert!(selected.is_absolute());
    assert_eq!(selected, bin.join("mbx"));
    crate::impl_cli_tmp::cleanup(&root);
    Ok(())
}
