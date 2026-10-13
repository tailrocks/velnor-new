use std::{
    error::Error,
    ffi::{OsStr, OsString},
    path::Path,
};

use super::{mbx_child_environment, mbx_path_from_path, parent_env_value};

struct MbxEnvironmentFixture {
    parent: Vec<(OsString, OsString)>,
    additions: Vec<(OsString, OsString)>,
    target_override: OsString,
    rust_bin: &'static str,
    mbx_bin: &'static str,
}

#[test]
fn mbx_child_removes_inherited_tool_and_authority_overrides() -> Result<(), Box<dyn Error>> {
    let fixture = mbx_environment_fixture()?;
    let child = mbx_child_environment(&fixture.parent, &fixture.additions);

    assert_removed_overrides(&child);
    assert_retained_tool_homes(&child);
    assert_mise_isolation(&child);
    assert_target_and_path(&child, &fixture)?;
    Ok(())
}

fn mbx_environment_fixture() -> Result<MbxEnvironmentFixture, Box<dyn Error>> {
    let mise_data = "/opt/mise/data";
    let wrapper = format!("{mise_data}/command-wrappers/bin");
    let shims = format!("{mise_data}/shims");
    let rust_bin = "/opt/rust/bin";
    let mbx_bin = "/opt/mbx/bin";
    let original_path = join_path(&[
        Path::new(&wrapper),
        Path::new(&shims),
        Path::new(rust_bin),
        Path::new(mbx_bin),
    ])?;
    let parent = vec![
        env_pair("PATH", original_path),
        env_pair("MISE_DATA_DIR", mise_data),
        env_pair("MISE_CARGO_HOME", "/owned/cargo"),
        env_pair("MISE_RUSTUP_HOME", "/owned/rustup"),
        env_pair("CARGO_HOME", "/cargo/home"),
        env_pair("CARGO_TARGET_DIR", "/old/target"),
        env_pair("RUSTC_WRAPPER", "/mbx/wrapper"),
        env_pair("RUSTUP_TOOLCHAIN", "1.97.1"),
        env_pair("CARGO", "/raw/cargo"),
        env_pair("RUSTC", "/raw/rustc"),
        env_pair("RUSTDOC", "/raw/rustdoc"),
        env_pair("GITHUB_TOKEN", "secret"),
        env_pair("CARGO_REGISTRIES_PRIVATE_TOKEN", "secret"),
        env_pair("ACTIONS_ID_TOKEN_REQUEST_URL", "secret"),
        env_pair("GH_HOST", "attacker.example"),
        env_pair("GH_CONFIG_DIR", "/untrusted/gh"),
        env_pair("TF_VAR_password", "secret"),
        env_pair("TOFU_CLI_ARGS_apply", "-auto-approve"),
        env_pair("CHECKPOINT_DISABLE", "1"),
    ];
    let target_override = OsString::from("/nested/target");
    let mut additions = super::isolation_environment();
    additions.push(env_pair("CARGO_TARGET_DIR", target_override.clone()));

    Ok(MbxEnvironmentFixture {
        parent,
        additions,
        target_override,
        rust_bin,
        mbx_bin,
    })
}

fn assert_removed_overrides(child: &[(OsString, OsString)]) {
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
        assert!(parent_env_value(child, stripped).is_none(), "{stripped}");
    }
}

fn assert_retained_tool_homes(child: &[(OsString, OsString)]) {
    for retained in [
        "MISE_DATA_DIR",
        "MISE_CARGO_HOME",
        "MISE_RUSTUP_HOME",
        "CARGO_HOME",
        "RUSTC_WRAPPER",
    ] {
        assert!(parent_env_value(child, retained).is_some(), "{retained}");
    }
}

fn assert_mise_isolation(child: &[(OsString, OsString)]) {
    for (key, value) in [
        ("MISE_NO_CONFIG", "1"),
        ("MISE_NO_ENV", "1"),
        ("MISE_NO_HOOKS", "1"),
        ("MISE_LOCKFILE", "0"),
        ("MISE_AUTO_INSTALL", "false"),
        ("MISE_EXEC_AUTO_INSTALL", "false"),
    ] {
        assert_eq!(
            parent_env_value(child, key),
            Some(OsStr::new(value)),
            "{key}"
        );
    }
}

fn assert_target_and_path(
    child: &[(OsString, OsString)],
    fixture: &MbxEnvironmentFixture,
) -> Result<(), Box<dyn Error>> {
    assert_eq!(
        parent_env_value(child, "CARGO_TARGET_DIR"),
        Some(fixture.target_override.as_os_str())
    );
    let expected_path = join_path(&[Path::new(fixture.rust_bin), Path::new(fixture.mbx_bin)])?;
    assert_eq!(
        parent_env_value(child, "PATH"),
        Some(expected_path.as_os_str())
    );
    Ok(())
}

fn env_pair(name: &str, value: impl Into<OsString>) -> (OsString, OsString) {
    (OsString::from(name), value.into())
}

fn join_path(entries: &[&Path]) -> Result<OsString, Box<dyn Error>> {
    Ok(std::env::join_paths(entries.iter().copied())?)
}

#[test]
fn relative_mbx_path_is_resolved_before_child_changes_directory() -> Result<(), Box<dyn Error>> {
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
