#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use velnor_actions_contract_workflow::StepKind;

use super::impl_renderer_fixtures::{TEST_MBX_VERSION, TEST_RUST_TOOLCHAIN, mbx_tool_steps};

#[path = "impl_renderer_mbx_preflight_store.rs"]
mod store;
#[path = "impl_renderer_mbx_preflight_version.rs"]
mod version;

struct TempRoot(PathBuf);

impl TempRoot {
    fn new() -> std::io::Result<Self> {
        static NEXT_ROOT: AtomicU64 = AtomicU64::new(0);

        for _ in 0..128 {
            let sequence = NEXT_ROOT.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "velnor-rust-preflight-{}-{sequence}",
                std::process::id()
            ));
            match fs::create_dir(&path) {
                Ok(()) => return Ok(Self(fs::canonicalize(path)?)),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error),
            }
        }

        Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "could not allocate a unique renderer test root",
        ))
    }
}

impl Drop for TempRoot {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.0)
            && error.kind() != std::io::ErrorKind::NotFound
        {
            eprintln!("failed to remove test root {}: {error}", self.0.display());
        }
    }
}

fn write_tool(path: &Path, script: &str) -> std::io::Result<()> {
    fs::write(path, script)?;
    let mut permissions = fs::metadata(path)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions)
}

fn version_check_script() -> Result<String, Box<dyn std::error::Error>> {
    let [_, _, check] = mbx_tool_steps(
        "jdx/mr-boxington-action@aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        TEST_MBX_VERSION,
        TEST_RUST_TOOLCHAIN,
    )?;
    let StepKind::Shell { run, .. } = check.kind else {
        return Err("version guard is not a shell step".into());
    };
    run.get(2)
        .cloned()
        .ok_or_else(|| "version-check script missing".into())
}

fn run_version_check(root: &Path, script: &str) -> std::io::Result<std::process::Output> {
    run_version_check_with_cache_dir_mode(root, script, "exact")
}

fn run_version_check_with_cache_dir_mode(
    root: &Path,
    script: &str,
    cache_dir_mode: &str,
) -> std::io::Result<std::process::Output> {
    let mbx_cache_dir = root.join("velnor/mbx");
    fs::create_dir_all(mbx_cache_dir.join("actions"))?;
    let fake_bin = root.join("fake-bin");
    let path = format!("{}:/usr/bin:/bin", fake_bin.display());
    Command::new("sh")
        .args(["-c", script])
        .env_clear()
        .env("PATH", path)
        .env("RUNNER_TEMP", root)
        .env("MBX_CACHE_DIR", mbx_cache_dir)
        .env("MBX_CACHE_DIR_MODE", cache_dir_mode)
        .env("GITHUB_RUN_ID", "9876")
        .env("GITHUB_RUN_ATTEMPT", "2")
        .env("GITHUB_JOB", "rust_linux")
        .output()
}

fn assert_version_scratch_removed(root: &Path) -> std::io::Result<()> {
    assert!(
        fs::read_dir(root)?.all(|entry| {
            entry.is_ok_and(|entry| {
                !entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("velnor-mbx-verify-")
            })
        }),
        "version-check scratch remains"
    );
    Ok(())
}

fn assert_preflight_step_shapes(
    preflight: &velnor_actions_contract_workflow::Step,
    action: &velnor_actions_contract_workflow::Step,
    version_check: &velnor_actions_contract_workflow::Step,
) -> Result<(), Box<dyn std::error::Error>> {
    let StepKind::Shell { run, .. } = &preflight.kind else {
        return Err("preflight is not shell".into());
    };
    let script = run.get(2).ok_or("preflight script missing")?;
    assert!(!script.contains("mr-boxington@"));
    assert!(script.contains("umask 077"));
    assert!(script.contains("$GITHUB_RUN_ID-$GITHUB_RUN_ATTEMPT"));
    assert!(
        !script.contains("$RUNNER_TEMP/velnor/mbx-preflight"),
        "the retired caller-owned fixed scratch leaf is not used"
    );
    let StepKind::Action { with, .. } = &action.kind else {
        return Err("MBX owner is not an action".into());
    };
    assert_eq!(
        with.get("version").map(String::as_str),
        Some(TEST_MBX_VERSION)
    );
    let StepKind::Shell { run, .. } = &version_check.kind else {
        return Err("native version check is not shell".into());
    };
    let version_script = run.get(2).ok_or("version-check script missing")?;
    assert!(version_script.contains("mbx --version"));
    assert!(version_script.contains("mbx 1.21.1"));
    Ok(())
}

#[test]
fn rust_preflight_exports_only_the_owned_toolchain_and_uses_no_scratch_leaf()
-> Result<(), Box<dyn std::error::Error>> {
    let root = TempRoot::new()?;
    let fake_bin = root.0.join("fake-bin");
    let rust_root = root.0.join("mise rust install");
    let velnor = root.0.join("velnor");
    let external = root.0.join("external");
    fs::create_dir_all(&fake_bin)?;
    fs::create_dir_all(&rust_root)?;
    fs::create_dir_all(&external)?;
    fs::write(external.join("sentinel"), "untouched")?;
    fs::create_dir(&velnor)?;
    symlink(&external, velnor.join("mbx-preflight"))?;
    write_tool(
        &fake_bin.join("mise"),
        "#!/bin/sh\nset -eu\n[ \"$4\" = where ] || exit 2\n[ \"$5\" = 'rust@1.98.1' ] || exit 3\nprintf '%s\\n' \"$RUST_ROOT\"\n",
    )?;
    write_tool(
        &rust_root.join("rustc"),
        "#!/bin/sh\n[ \"$1\" = '+1.98.1' ] || exit 2\n[ \"$2\" = -vV ] || exit 3\nprintf 'rustc 1.98.1 (abc)\\nrelease: 1.98.1\\nhost: x86_64-unknown-linux-gnu\\n'\n",
    )?;
    let path = std::env::var("PATH").unwrap_or_default();
    let path = format!("{}:{path}", fake_bin.display());
    let github_path = root.0.join("github-path");
    fs::write(&github_path, "")?;
    let github_env = root.0.join("github-env");
    fs::write(&github_env, "")?;
    let [preflight, action, version_check] = mbx_tool_steps(
        "jdx/mr-boxington-action@aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        TEST_MBX_VERSION,
        TEST_RUST_TOOLCHAIN,
    )?;
    let StepKind::Shell { run, .. } = &preflight.kind else {
        return Err("preflight is not shell".into());
    };
    let script = run.get(2).ok_or("preflight script missing")?;
    assert_preflight_step_shapes(&preflight, &action, &version_check)?;

    let result = Command::new("sh")
        .args(["-c", script])
        .env_clear()
        .env("PATH", path)
        .env("RUST_ROOT", &rust_root)
        .env("RUNNER_TEMP", &root.0)
        .env("MBX_CACHE_DIR", root.0.join("velnor/mbx"))
        .env("GITHUB_RUN_ID", "12345")
        .env("GITHUB_RUN_ATTEMPT", "1")
        .env("GITHUB_PATH", &github_path)
        .env("GITHUB_ENV", &github_env)
        .env("MISE_RUSTUP_HOME", root.0.join("rustup"))
        .env("MISE_CARGO_HOME", root.0.join("cargo"))
        .env("RUSTUP_HOME", root.0.join("rustup"))
        .env("CARGO_HOME", root.0.join("cargo"))
        .output()?;
    assert!(
        result.status.success(),
        "preflight failed: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(
        fs::read_to_string(github_path)?,
        format!("{}\n", rust_root.display())
    );
    assert_eq!(
        fs::read_to_string(github_env)?,
        format!("MBX_CACHE_DIR={}/velnor/mbx\n", root.0.display()),
        "subsequent task steps inherit the exact private store root"
    );
    assert!(root.0.join("velnor/mbx").is_dir());
    assert!(!root.0.join("velnor-mbx-preflight-12345-1").exists());
    assert_eq!(fs::read_to_string(external.join("sentinel"))?, "untouched");
    assert_eq!(fs::read_dir(external)?.count(), 1);
    assert!(
        velnor
            .join("mbx-preflight")
            .symlink_metadata()?
            .file_type()
            .is_symlink(),
        "the caller-owned legacy leaf remains untouched"
    );
    Ok(())
}

#[test]
fn preexisting_unique_scratch_symlink_fails_without_touching_its_target()
-> Result<(), Box<dyn std::error::Error>> {
    let root = TempRoot::new()?;
    let external = root.0.join("external");
    let fake_bin = root.0.join("fake-bin");
    fs::create_dir(&external)?;
    fs::create_dir(&fake_bin)?;
    fs::write(external.join("sentinel"), "untouched")?;
    write_tool(&fake_bin.join("mise"), "#!/bin/sh\nexit 99\n")?;
    let scratch = root.0.join("velnor-mbx-preflight-23456-1");
    symlink(&external, &scratch)?;
    let [preflight, _, _] = mbx_tool_steps(
        "jdx/mr-boxington-action@aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        TEST_MBX_VERSION,
        TEST_RUST_TOOLCHAIN,
    )?;
    let StepKind::Shell { run, .. } = preflight.kind else {
        return Err("preflight is not shell".into());
    };
    let script = run.get(2).ok_or("preflight script missing")?;
    let path = format!("{}:/usr/bin:/bin", fake_bin.display());
    let github_env = root.0.join("github-env");
    fs::write(&github_env, "")?;
    let result = Command::new("sh")
        .args(["-c", script])
        .env_clear()
        .env("PATH", path)
        .env("RUNNER_TEMP", &root.0)
        .env("MBX_CACHE_DIR", root.0.join("velnor/mbx"))
        .env("GITHUB_RUN_ID", "23456")
        .env("GITHUB_RUN_ATTEMPT", "1")
        .env("GITHUB_PATH", root.0.join("github-path"))
        .env("GITHUB_ENV", &github_env)
        .env("MISE_RUSTUP_HOME", root.0.join("rustup"))
        .env("MISE_CARGO_HOME", root.0.join("cargo"))
        .env("RUSTUP_HOME", root.0.join("rustup"))
        .env("CARGO_HOME", root.0.join("cargo"))
        .output()?;
    assert!(
        !result.status.success(),
        "preexisting leaf must fail closed"
    );
    assert_eq!(fs::read_to_string(external.join("sentinel"))?, "untouched");
    assert_eq!(fs::read_dir(&external)?.count(), 1);
    assert!(scratch.symlink_metadata()?.file_type().is_symlink());
    Ok(())
}

#[test]
fn rust_preflight_rejects_invalid_selected_install_facts() -> Result<(), Box<dyn std::error::Error>>
{
    for (name, mise_mode, rust_mode, rustc_present, rustc_executable) in [
        ("wrong named toolchain", "valid", "wrong", true, true),
        (
            "selected toolchain is missing",
            "valid",
            "missing-toolchain",
            true,
            true,
        ),
        ("relative install path", "relative", "valid", true, true),
        ("missing Rustup shim", "valid", "valid", false, false),
        ("non-executable Rustup shim", "valid", "valid", true, false),
        (
            "unterminated second Mise output",
            "unterminated-extra",
            "valid",
            true,
            true,
        ),
    ] {
        let root = TempRoot::new()?;
        let fake_bin = root.0.join("fake-bin");
        let rust_root = root.0.join("rust install");
        fs::create_dir_all(&fake_bin)?;
        fs::create_dir_all(&rust_root)?;
        write_tool(
            &fake_bin.join("mise"),
            "#!/bin/sh\nset -eu\n[ \"$4\" = where ] || exit 2\n[ \"$5\" = 'rust@1.98.1' ] || exit 3\ncase \"$MISE_MODE\" in\n  relative) printf '%s\\n' 'relative/rust' ;;\n  unterminated-extra) printf '%s\\n' \"$RUST_ROOT\"; printf '%s' extra ;;\n  *) printf '%s\\n' \"$RUST_ROOT\" ;;\nesac\n",
        )?;
        if rustc_present {
            let rustc = rust_root.join("rustc");
            fs::write(
                &rustc,
                "#!/bin/sh\n[ \"$1\" = '+1.98.1' ] || exit 2\n[ \"$2\" = -vV ] || exit 3\nif [ \"$RUST_MODE\" = missing-toolchain ]; then printf '%s\\n' 'error: toolchain is not installed' >&2; exit 4; fi\nif [ \"$RUST_MODE\" = wrong ]; then printf 'release: 1.98.0\\nhost: x86_64-unknown-linux-gnu\\n'; else printf 'release: 1.98.1\\nhost: x86_64-unknown-linux-gnu\\n'; fi\n",
            )?;
            let mut permissions = fs::metadata(&rustc)?.permissions();
            permissions.set_mode(if rustc_executable { 0o755 } else { 0o644 });
            fs::set_permissions(&rustc, permissions)?;
        }
        let path = format!("{}:/usr/bin:/bin", fake_bin.display());
        let github_path = root.0.join("github-path");
        fs::write(&github_path, "")?;
        let script = preflight_script()?;
        let result = Command::new("sh")
            .args(["-c", script.as_str()])
            .env_clear()
            .env("PATH", path)
            .env("RUST_ROOT", &rust_root)
            .env("RUST_MODE", rust_mode)
            .env("MISE_MODE", mise_mode)
            .env("RUNNER_TEMP", &root.0)
            .env("GITHUB_RUN_ID", "34567")
            .env("GITHUB_RUN_ATTEMPT", "1")
            .env("GITHUB_PATH", &github_path)
            .env("MISE_RUSTUP_HOME", root.0.join("rustup"))
            .env("MISE_CARGO_HOME", root.0.join("cargo"))
            .env("RUSTUP_HOME", root.0.join("rustup"))
            .env("CARGO_HOME", root.0.join("cargo"))
            .output()?;
        assert!(
            !result.status.success(),
            "{name} must fail closed: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert_eq!(fs::read_to_string(&github_path)?, "", "{name} PATH");
        assert!(
            !root.0.join("velnor-mbx-preflight-34567-1").exists(),
            "{name} scratch cleanup"
        );
    }
    Ok(())
}

#[test]
fn native_action_version_guard_preserves_preexisting_scratch_symlink()
-> Result<(), Box<dyn std::error::Error>> {
    let root = TempRoot::new()?;
    let fake_bin = root.0.join("fake-bin");
    let external = root.0.join("external");
    fs::create_dir(&fake_bin)?;
    fs::create_dir(&external)?;
    write_tool(&fake_bin.join("mise"), "#!/bin/sh\nexit 99\n")?;
    fs::write(external.join("sentinel"), "untouched")?;
    let scratch = root.0.join("velnor-mbx-verify-9876-2");
    symlink(&external, &scratch)?;
    let result = run_version_check(&root.0, &version_check_script()?)?;
    assert!(
        !result.status.success(),
        "preexisting scratch link must fail"
    );
    assert_eq!(fs::read_to_string(external.join("sentinel"))?, "untouched");
    assert_eq!(fs::read_dir(&external)?.count(), 1);
    assert!(scratch.symlink_metadata()?.file_type().is_symlink());
    Ok(())
}

fn preflight_script() -> Result<String, Box<dyn std::error::Error>> {
    let [preflight, _, _] = mbx_tool_steps(
        "jdx/mr-boxington-action@aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        TEST_MBX_VERSION,
        TEST_RUST_TOOLCHAIN,
    )?;
    let StepKind::Shell { run, .. } = preflight.kind else {
        return Err("preflight must be a shell step".into());
    };
    Ok(run.get(2).ok_or("preflight script missing")?.clone())
}
