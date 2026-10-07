use super::tree;
use super::*;
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use tempfile::TempDir;
use velnor_actions_contract_config::config::HostOrbStackSdk;
use velnor_actions_orchestrator_core::sha256::sha256_hex;

fn fixture() -> (TempDir, PathBuf, HostOrbStackSdk, PathBuf) {
    let temp = tempfile::tempdir().expect("temp");
    let home = temp.path().canonicalize().expect("canonical temp");
    let app = home.join("OrbStack.app");
    let cli = app.join("Contents/Resources/orbstack-cli.app");
    fs::create_dir_all(cli.join("Contents/MacOS")).expect("dirs");
    fs::write(app.join("Contents/Info.plist"), b"outer plist").expect("plist");
    fs::create_dir_all(app.join("Contents/MacOS")).expect("main dir");
    fs::write(app.join("Contents/MacOS/OrbStack"), b"outer executable").expect("main");
    fs::write(cli.join("Contents/Info.plist"), b"cli plist").expect("cli plist");
    fs::write(cli.join("Contents/MacOS/orbctl"), b"orbctl bytes").expect("orbctl");
    #[cfg(unix)]
    {
        fs::set_permissions(
            cli.join("Contents/MacOS/orbctl"),
            fs::Permissions::from_mode(0o755),
        )
        .expect("exec");
    }
    let source_entries = tree::collect_tree(&cli).expect("source tree");
    let source_hash = tree::tree_digest(&source_entries).expect("source digest");
    let expected = home.join("expected");
    tree::create_destination(&expected).expect("expected dir");
    tree::populate(&expected, &source_entries).expect("expected copy");
    let owned_hash = tree::tree_digest(&tree::collect_tree(&expected).expect("owned tree"))
        .expect("owned digest");
    fs::remove_dir_all(expected).expect("cleanup");
    let info = sha256_hex(b"outer plist");
    let main = sha256_hex(b"outer executable");
    let cli_hash = sha256_hex(b"orbctl bytes");
    let sdk = HostOrbStackSdk {
        app_bundle_path: app.display().to_string(),
        bundle_id: "com.example.orbstack".to_owned(),
        team_id: "TEAM123".to_owned(),
        version: "1".to_owned(),
        build: "1".to_owned(),
        info_plist_sha256: info,
        main_executable_path: "Contents/MacOS/OrbStack".to_owned(),
        main_executable_sha256: main,
        cli_bundle_path: cli.display().to_string(),
        source_tree_sha256: source_hash,
        owned_tree_sha256: owned_hash,
        cli_relative_path: "Contents/MacOS/orbctl".to_owned(),
        cli_sha256: cli_hash,
        cli_version: "1".to_owned(),
        cli_build: "1".to_owned(),
        cli_commit: "a".repeat(40),
        runtime_dir: home.join(".orbstack/run").display().to_string(),
        runtime_uid: 0,
    };
    (temp, home, sdk, cli)
}

mod bundle_tests;
