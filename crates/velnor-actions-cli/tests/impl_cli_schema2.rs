//! `config migrate` preview and write through the built binary.

use std::error::Error;

use crate::impl_cli_tmp::{cleanup, code, fresh_tempdir, git_init, spawn};

const SOURCE: &str = r#"schema = 1
[workflow]
name = "Kept"
default_branch = "main"
runner_label = "ubuntu-24.04"
[resources]
compiler_process_budget = 4
test_process_budget = 5
[test_sharding]
default_shards = 1
[test_sharding.by_manifest]
"crates/demo/Cargo.toml" = 2
[stacks]
ignore = ["tofu"]
[discovery]
exclude = ["vendor/**"]
[actions.overrides."actions/checkout"]
sha = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
version = "v1.2.3"
"#;

#[test]
fn config_migrate_preview_does_not_write() -> Result<(), Box<dyn Error>> {
    let tmp = fresh_tempdir("migrate")?;
    git_init(&tmp)?;
    let path = tmp.join(".velnor").join("config.toml");
    std::fs::create_dir_all(path.parent().ok_or("parent")?)?;
    std::fs::write(&path, SOURCE)?;
    let before = std::fs::read(&path)?;
    let preview = spawn(&["config", "migrate", "--to", "2"], &[], &tmp)?;
    assert_eq!(code(&preview), 0, "stderr {:?}", preview.stderr);
    assert_eq!(std::fs::read(&path)?, before);
    let stdout = String::from_utf8(preview.stdout)?;
    assert!(stdout.contains("schema = 2"), "{stdout}");
    assert!(
        stdout.contains("runner_label = \"ubuntu-24.04\""),
        "{stdout}"
    );
    assert!(stdout.contains("default_profile = \"hosted\""), "{stdout}");
    assert!(!stdout.contains("mode ="), "{stdout}");
    assert!(!stdout.contains("profiles.ubuntu-24.04"), "{stdout}");
    assert!(stdout.contains("Kept"), "{stdout}");
    assert!(stdout.contains("crates/demo/Cargo.toml"), "{stdout}");
    let bad = spawn(&["config", "migrate", "--to", "9", "--write"], &[], &tmp)?;
    assert_ne!(code(&bad), 0);
    let err = String::from_utf8_lossy(&bad.stderr);
    assert!(err.contains("unsupported_migration_target"), "{err}");
    assert_eq!(std::fs::read(&path)?, before);
    let written = spawn(&["config", "migrate", "--to", "2", "--write"], &[], &tmp)?;
    assert_eq!(code(&written), 0, "stderr {:?}", written.stderr);
    let again = String::from_utf8(written.stdout)?;
    assert_eq!(again, stdout);
    assert_eq!(std::fs::read_to_string(&path)?, stdout);
    cleanup(&tmp);
    Ok(())
}
