//! The generator seed copy runs the shipped acquire script.
//!
//! Orchestrator `src/` must not spawn a process. This test executes
//! `acquire_script_argv` from the integration crate.

use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, Output};

use velnor_actions_contract_release::ReleaseTarget;
use velnor_actions_orchestrator::acquire_script_argv;

#[test]
fn generator_seed_hit_skips_curl_and_a_bad_hash_does_not_copy()
-> Result<(), Box<dyn std::error::Error>> {
    let root = std::env::temp_dir().join(format!("velnor-gen-seed-{}", std::process::id()));
    std::fs::remove_dir_all(&root).ok();
    let name = "velnor-actions-0.1.0";
    let root_text = root.to_str().ok_or("root")?;
    for unsafe_staged in [
        "/tmp/$(id)/velnor-actions-0.1.0",
        "$RUNNER_TEMP/velnor/bin/$(id)",
    ] {
        assert!(acquire_script_argv(unsafe_staged, root_text, ReleaseTarget::LinuxX86_64).is_err());
    }
    assert!(
        acquire_script_argv(
            "$RUNNER_TEMP/velnor/bin/velnor-actions-0.1.0",
            root_text,
            ReleaseTarget::LinuxX86_64
        )
        .is_ok()
    );
    let seed_file = root.join("generator").join(name);
    let parent = seed_file.parent().ok_or("seed parent")?;
    std::fs::create_dir_all(parent)?;
    std::fs::write(&seed_file, b"generator-bytes")?;
    let staged = root.join("stage").join(name);
    let argv = acquire_script_argv(
        staged.to_str().ok_or("staged")?,
        root_text,
        ReleaseTarget::LinuxX86_64,
    )?;
    let bin = root.join("bin");
    std::fs::create_dir_all(&bin)?;
    let curl = bin.join("curl");
    std::fs::write(&curl, "#!/bin/sh\necho \"$@\" >> \"$CURL_LOG\"\nexit 22\n")?;
    let mut mode = std::fs::metadata(&curl)?.permissions();
    mode.set_mode(0o755);
    std::fs::set_permissions(&curl, mode)?;
    let hash = file_sha256(&seed_file)?;
    let hit = run_acquire(&argv[2], &bin, &hash)?;
    assert!(hit.status.success(), "{hit:?}");
    assert_eq!(std::fs::read(&staged)?, b"generator-bytes");
    assert!(read_log(&bin).is_empty(), "{}", read_log(&bin));
    std::fs::remove_file(&staged)?;
    let miss = run_acquire(&argv[2], &bin, &"a".repeat(64))?;
    assert!(!miss.status.success(), "bad hash must not succeed");
    assert!(!staged.exists(), "bad seed is not staged");
    assert!(read_log(&bin).contains("https://example.invalid/generator"));
    assert_eq!(std::fs::read(&seed_file)?, b"generator-bytes");
    std::fs::remove_dir_all(&root).ok();
    Ok(())
}

fn file_sha256(path: &Path) -> Result<String, Box<dyn std::error::Error>> {
    // Ubuntu keeps sha256sum in /usr/bin. macOS coreutils can put it in /sbin.
    // The acquire script also resolves sha256sum from PATH.
    let output = Command::new("sha256sum").arg(path).output()?;
    if !output.status.success() {
        return Err(format!("sha256sum:{output:?}").into());
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let hash = text.split_whitespace().next().ok_or("hash")?;
    Ok(hash.to_owned())
}

fn run_acquire(script: &str, bin: &Path, hash: &str) -> Result<Output, Box<dyn std::error::Error>> {
    let path = format!("{}:/sbin:/usr/bin:/bin", bin.display());
    Ok(Command::new("sh")
        .arg("-c")
        .arg(script)
        .env("PATH", &path)
        .env("VELNOR_ASSET_SHA256", hash)
        .env("VELNOR_ASSET_URL", "https://example.invalid/generator")
        .env("CURL_LOG", bin.join("curl.log"))
        .output()?)
}

fn read_log(bin: &Path) -> String {
    std::fs::read_to_string(bin.join("curl.log")).unwrap_or_default()
}
