//! The configuration disable decision must precede early miserc parsing.
use std::process::Command;

#[test]
fn no_config_skips_early_miserc_at_root_and_nested_cwd() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let project = root.path().join("project");
    let nested = project.join("nested");
    let system = root.path().join("system");
    let config = home.join(".config/mise");
    for dir in [&nested, &system, &config] {
        std::fs::create_dir_all(dir).unwrap();
    }
    for path in [
        project.join(".miserc.toml"),
        nested.join(".miserc.local.toml"),
        config.join("miserc.toml"),
        system.join("miserc.toml"),
    ] {
        std::fs::write(path, "this is invalid TOML = [").unwrap();
    }
    for cwd in [&project, &nested] {
        for via_env in [false, true] {
            let mut command = Command::new(env!("CARGO_BIN_EXE_mise"));
            command
                .env_clear()
                .env("HOME", &home)
                .env("MISE_SYSTEM_CONFIG_DIR", &system)
                .current_dir(cwd);
            if via_env {
                command.env("MISE_NO_CONFIG", "1");
            } else {
                command.arg("--no-config");
            }
            let output = command.arg("version").output().unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        let output = Command::new(env!("CARGO_BIN_EXE_mise"))
            .env_clear()
            .env("HOME", &home)
            .env("MISE_SYSTEM_CONFIG_DIR", &system)
            .current_dir(cwd)
            .arg("--version")
            .output()
            .unwrap();
        assert!(
            !output.status.success(),
            "malformed miserc must fail without NoConfig"
        );
    }
}
