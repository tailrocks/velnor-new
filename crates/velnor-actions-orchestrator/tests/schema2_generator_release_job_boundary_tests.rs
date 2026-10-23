use std::error::Error;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;

pub(super) fn candidate_environment_writes_cannot_cross_job_boundary() -> Result<(), Box<dyn Error>>
{
    let qualifier = tempfile::TempDir::new()?;
    let attester = tempfile::TempDir::new()?;
    let qualifier_bin = qualifier.path().join("bin");
    let attester_bin = attester.path().join("bin");
    fs::create_dir_all(&qualifier_bin)?;
    fs::create_dir_all(&attester_bin)?;
    let malicious_gh = qualifier_bin.join("gh");
    write_executable(
        &malicious_gh,
        "#!/bin/sh\nprintf '%s\\n' leaked > \"$QUALIFIER_TOKEN_CAPTURE\"\n",
    )?;
    let candidate = qualifier.path().join("candidate.sh");
    write_executable(
        &candidate,
        "#!/bin/sh\nprintf '%s\\n' \"$QUALIFIER_BIN\" >> \"$GITHUB_PATH\"\nprintf '%s\\n' 'INJECTED=1' >> \"$GITHUB_ENV\"\n(sleep 0.05; : > \"$QUALIFIER_BACKGROUND\") >/dev/null 2>&1 &\n",
    )?;
    let qualifier_path = qualifier.path().join("github-path");
    let qualifier_env = qualifier.path().join("github-env");
    let background = qualifier.path().join("background-finished");
    fs::write(&qualifier_path, "")?;
    fs::write(&qualifier_env, "")?;
    let candidate_status = Command::new(&candidate)
        .env("QUALIFIER_BIN", &qualifier_bin)
        .env("GITHUB_PATH", &qualifier_path)
        .env("GITHUB_ENV", &qualifier_env)
        .env("QUALIFIER_BACKGROUND", &background)
        .status()?;
    assert!(candidate_status.success());

    let trusted_gh = attester_bin.join("gh");
    write_executable(
        &trusted_gh,
        "#!/bin/sh\nprintf '%s\\n' \"$GH_TOKEN\" > \"$ATTEST_TOKEN_CAPTURE\"\n",
    )?;
    let attest_path = attester.path().join("github-path");
    let attest_env = attester.path().join("github-env");
    let token_capture = attester.path().join("token-capture");
    fs::write(&attest_path, "")?;
    fs::write(&attest_env, "")?;
    let gh_status = Command::new("gh")
        .arg("api")
        .env("PATH", &attester_bin)
        .env("GITHUB_PATH", &attest_path)
        .env("GITHUB_ENV", &attest_env)
        .env("GH_TOKEN", "attest-job-token")
        .env("ATTEST_TOKEN_CAPTURE", &token_capture)
        .status()?;
    assert!(gh_status.success());
    assert_eq!(fs::read_to_string(&token_capture)?, "attest-job-token\n");
    assert!(!qualifier.path().join("token-capture").exists());
    assert_eq!(fs::read_to_string(&qualifier_env)?, "INJECTED=1\n");
    assert!(
        fs::read_to_string(&qualifier_path)?
            .contains(qualifier_bin.to_str().ok_or("non-UTF8 path")?)
    );
    assert_eq!(fs::read_to_string(&attest_path)?, "");
    assert_eq!(fs::read_to_string(&attest_env)?, "");
    for _ in 0..50 {
        if background.exists() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(
        background.exists(),
        "candidate background process did not run"
    );
    assert_eq!(fs::read_to_string(&attest_path)?, "");
    assert_eq!(fs::read_to_string(&attest_env)?, "");
    Ok(())
}

fn write_executable(path: &Path, content: &str) -> Result<(), Box<dyn Error>> {
    fs::write(path, content)?;
    let mut permissions = fs::metadata(path)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions)?;
    Ok(())
}
