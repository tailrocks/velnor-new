//! Restored toolchain payload verification and owner repair fixtures.
#![cfg(unix)]

use std::{fs, os::unix::fs::PermissionsExt, path::Path, process::Command};
use velnor_actions_mise::catalog::{
    RUST_VERSION, RustInstallOptions, ToolCatalog, qualification::DistributionHost,
    rust_desktop::DESKTOP_RUST_VERSION, rust_health::RustToolchainHealth,
};

#[test]
fn compiler_role_selects_version_independently_of_host() -> Result<(), Box<dyn std::error::Error>> {
    let root = ToolCatalog::pinned();
    let desktop = root.for_native_kind("native_xcode_project_ci")?;
    let release = ToolCatalog::for_release_host(DistributionHost::MacosArm64)?;
    for (catalog, version, host) in [
        (root, RUST_VERSION, "x86_64-unknown-linux-gnu"),
        (desktop, DESKTOP_RUST_VERSION, "aarch64-apple-darwin"),
        (release, RUST_VERSION, "aarch64-apple-darwin"),
    ] {
        let health = RustToolchainHealth::for_catalog(&catalog, catalog.rust_install_options());
        assert!(
            health
                .prepare_script()?
                .contains(&format!("toolchain='{version}-{host}'"))
        );
        assert!(
            health
                .finalize_script()?
                .contains(&format!("'rustc {version} '*"))
        );
    }
    Ok(())
}

const MANAGER: &str = "#!/bin/bash\nset -eu\nprintf '%s\\n' \"$*\" >> \"$PROOF_ROOT/calls\"\ncase \"$1 $2\" in\n 'toolchain list') printf '1.98.1-x86_64-unknown-linux-gnu (active, default) %s/tree\\n' \"$RUSTUP_HOME\" ;;\n 'toolchain uninstall') printf 'uninstalled\\n' >> \"$PROOF_ROOT/repairs\" ;;\n 'component list') printf '%s\\n' cargo-x86_64-unknown-linux-gnu rustc-x86_64-unknown-linux-gnu rust-std-x86_64-unknown-linux-gnu clippy-x86_64-unknown-linux-gnu rustfmt-x86_64-unknown-linux-gnu ;;\n 'target list') printf '%s\\n' x86_64-unknown-linux-gnu ;;\n 'run 1.98.1-x86_64-unknown-linux-gnu') printf '%s\\n' 'rustc 1.98.1 (fixture)' ;;\n *) exit 1 ;;\nesac\n";

fn run(root: &Path, script: &str) -> Result<(), Box<dyn std::error::Error>> {
    let output = execute(root, script)?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!String::from_utf8_lossy(&output.stderr).contains("private-metadata-canary"));
    assert!(!String::from_utf8_lossy(&output.stdout).contains("private-metadata-canary"));
    Ok(())
}

fn execute(root: &Path, script: &str) -> std::io::Result<std::process::Output> {
    Command::new("bash")
        .args(["-c", script])
        .env("PROOF_ROOT", root)
        .env("CARGO_HOME", root.join("cargo"))
        .env("RUSTUP_HOME", root.join("rustup"))
        .output()
}

#[test]
fn terminal_restore_verifies_before_execution_without_repair_or_writes()
-> Result<(), Box<dyn std::error::Error>> {
    let root = std::env::temp_dir().join(format!("velnor-rust-terminal-{}", std::process::id()));
    fs::create_dir_all(root.join("cargo/bin"))?;
    fs::create_dir_all(root.join("rustup/tree"))?;
    let manager = root.join("cargo/bin/rustup");
    fs::write(&manager, MANAGER)?;
    fs::set_permissions(&manager, fs::Permissions::from_mode(0o755))?;
    let payload = root.join("rustup/tree/payload");
    fs::write(&payload, "qualified")?;
    let health =
        RustToolchainHealth::for_catalog(&ToolCatalog::pinned(), RustInstallOptions::required());
    run(&root, &health.finalize_script()?)?;
    let marker = root.join("rustup/velnor-integrity/1.98.1-x86_64-unknown-linux-gnu.sha256");
    let original = fs::read(&marker)?;
    let modified = fs::metadata(&marker)?.modified()?;
    fs::write(root.join("calls"), "")?;
    run(&root, &health.terminal_restore_script()?)?;
    assert_eq!(fs::read(&marker)?, original);
    assert_eq!(fs::metadata(&marker)?.modified()?, modified);
    assert!(!root.join("repairs").exists());
    for damage in ["payload", "marker", "extra-newline", "missing"] {
        fs::write(&payload, "qualified")?;
        fs::write(&marker, &original)?;
        match damage {
            "payload" => fs::write(&payload, "corrupt")?,
            "marker" => fs::write(&marker, "corrupt")?,
            "extra-newline" => fs::write(&marker, [original.as_slice(), b"\n"].concat())?,
            _ => fs::remove_file(&marker)?,
        }
        fs::write(root.join("calls"), "")?;
        assert!(
            !execute(&root, &health.terminal_restore_script()?)?
                .status
                .success()
        );
        let calls = fs::read_to_string(root.join("calls"))?;
        assert!(!calls.contains("run "));
        assert!(!calls.contains("uninstall"));
        assert!(!calls.contains("install"));
    }
    fs::remove_dir_all(root.join("rustup/velnor-integrity"))?;
    assert!(
        !execute(&root, &health.terminal_restore_script()?)?
            .status
            .success()
    );
    assert!(!root.join("rustup/velnor-integrity").exists());
    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn shared_inventory_detects_hardlink_groups_and_private_metadata()
-> Result<(), Box<dyn std::error::Error>> {
    let root = std::env::temp_dir().join(format!(
        "velnor-rust-health-metadata-{}",
        std::process::id()
    ));
    fs::create_dir_all(root.join("cargo/bin"))?;
    fs::create_dir_all(root.join("rustup/tree"))?;
    let manager = root.join("cargo/bin/rustup");
    fs::write(&manager, MANAGER)?;
    fs::set_permissions(&manager, fs::Permissions::from_mode(0o755))?;
    let payload = root.join("rustup/tree/payload");
    let alias = root.join("rustup/tree/alias");
    fs::write(&payload, "qualified")?;
    fs::hard_link(&payload, &alias)?;
    let health =
        RustToolchainHealth::for_catalog(&ToolCatalog::pinned(), RustInstallOptions::required());
    run(&root, &health.finalize_script()?)?;
    fs::remove_file(&alias)?;
    fs::write(&alias, "qualified")?;
    run(&root, &health.prepare_script()?)?;
    assert!(root.join("repairs").exists());
    fs::remove_file(root.join("repairs"))?;
    run(&root, &health.finalize_script()?)?;
    let status = if cfg!(target_os = "macos") {
        Command::new("/usr/bin/xattr")
            .args(["-w", "user.velnor-proof", "private-metadata-canary"])
            .arg(&payload)
            .status()?
    } else {
        Command::new("/usr/bin/python3")
            .args(["-I", "-S", "-c", "import os,sys; os.setxattr(sys.argv[1], b\"user.velnor-proof\", b\"private-metadata-canary\")"])
            .arg(&payload).status()?
    };
    assert!(status.success());
    run(&root, &health.prepare_script()?)?;
    assert!(root.join("repairs").exists());
    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn inventory_detects_payload_topology_and_marker_damage_before_execution()
-> Result<(), Box<dyn std::error::Error>> {
    let root = std::env::temp_dir().join(format!("velnor-rust-health-{}", std::process::id()));
    fs::create_dir_all(root.join("cargo/bin"))?;
    fs::create_dir_all(root.join("rustup/tree"))?;
    let manager = root.join("cargo/bin/rustup");
    fs::write(&manager, MANAGER)?;
    fs::set_permissions(&manager, fs::Permissions::from_mode(0o755))?;
    let payload = root.join("rustup/tree/payload");
    fs::write(&payload, "qualified")?;
    let health =
        RustToolchainHealth::for_catalog(&ToolCatalog::pinned(), RustInstallOptions::required());
    run(&root, &health.finalize_script()?)?;
    fs::write(root.join("calls"), "")?;
    run(&root, &health.prepare_script()?)?;
    assert!(!root.join("repairs").exists());
    assert!(!fs::read_to_string(root.join("calls"))?.contains("run "));
    fs::write(&payload, "corrupt")?;
    run(&root, &health.prepare_script()?)?;
    assert_eq!(fs::read_to_string(root.join("repairs"))?, "uninstalled\n");
    fs::remove_file(root.join("repairs"))?;
    run(&root, &health.prepare_script()?)?;
    assert!(root.join("repairs").exists());
    fs::remove_file(root.join("repairs"))?;
    run(&root, &health.finalize_script()?)?;
    fs::write(root.join("rustup/tree/new-file"), "unexpected")?;
    run(&root, &health.prepare_script()?)?;
    assert!(root.join("repairs").exists());
    fs::remove_file(root.join("repairs"))?;
    run(&root, &health.finalize_script()?)?;
    fs::write(
        root.join("rustup/velnor-integrity/1.98.1-x86_64-unknown-linux-gnu.sha256"),
        "corrupt",
    )?;
    run(&root, &health.prepare_script()?)?;
    assert!(root.join("repairs").exists());
    fs::remove_file(root.join("repairs"))?;
    run(&root, &health.finalize_script()?)?;
    fs::remove_file(&payload)?;
    run(&root, &health.prepare_script()?)?;
    assert!(root.join("repairs").exists());
    fs::remove_file(root.join("repairs"))?;
    fs::write(&payload, "qualified")?;
    run(&root, &health.finalize_script()?)?;
    std::os::unix::fs::symlink("/etc/passwd", root.join("rustup/tree/escape"))?;
    run(&root, &health.prepare_script()?)?;
    assert!(root.join("repairs").exists());
    fs::remove_file(root.join("repairs"))?;
    fs::remove_file(root.join("rustup/tree/escape"))?;
    let sentinel = root.join("sentinel");
    fs::write(&sentinel, "unchanged")?;
    let marker = root.join("rustup/velnor-integrity/1.98.1-x86_64-unknown-linux-gnu.sha256");
    std::os::unix::fs::symlink(&sentinel, marker)?;
    run(&root, &health.prepare_script()?)?;
    assert!(root.join("repairs").exists());
    assert_eq!(fs::read_to_string(sentinel)?, "unchanged");
    fs::remove_dir_all(root)?;
    Ok(())
}
