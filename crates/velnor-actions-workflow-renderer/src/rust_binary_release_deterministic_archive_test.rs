use super::scripts;
use std::error::Error;
use std::fs::{self, FileTimes};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use velnor_actions_contract::RustBinaryReleaseConfig;

const PACKAGE: &str = "demo-package";

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Result<Self, Box<dyn Error>> {
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let root = std::env::temp_dir().join(format!(
            "velnor-binary-release-archive-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&root)?;
        Ok(Self(root))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        drop(fs::remove_dir_all(&self.0));
    }
}

#[test]
fn binary_build_archives_are_reproducible_for_long_names_across_source_mtimes_and_temp_dirs()
-> Result<(), Box<dyn Error>> {
    const SOURCE_SHA: &str = "0123456789abcdef0123456789abcdef01234567";
    let target = "x86_64-unknown-linux-gnu";
    let binary = format!("demo-binary-{}", "x".repeat(100));
    assert!(binary.len() > 100);
    let config = RustBinaryReleaseConfig {
        enabled: true,
        manifest_path: "Cargo.toml".to_owned(),
        package: PACKAGE.to_owned(),
        binary: Some(binary.clone()),
        source_commit_env: None,
    };
    config.validate("test")?;
    let rustc = format!("printf 'host: {target}\\n'");
    let cargo_build = format!(
        "printf '%s\\n' '{{\"reason\":\"compiler-artifact\",\"target\":{{\"name\":\"{binary}\",\"kind\":[\"bin\"]}},\"executable\":\"fixture-bin\"}}' > \"$RUNNER_TEMP/cargo-build.json\""
    );
    let script = scripts::build(&binary, target, &cargo_build, &rustc);
    let scratch = Scratch::new()?;
    let mut archive_bytes = Vec::with_capacity(2);

    for (run, modified_seconds) in [("first", 1_577_836_800), ("second", 1_640_995_200)] {
        let build_root = scratch.0.join(run);
        let runner_temp = scratch.0.join(format!("runner-temp-{run}"));
        let bin = scratch.0.join("bin");
        fs::create_dir_all(&build_root)?;
        fs::create_dir_all(&runner_temp)?;
        fs::create_dir_all(&bin)?;
        install_mock_git(&bin.join("git"), SOURCE_SHA)?;
        install_mock_file(&bin.join("file"))?;

        let fixture_binary = build_root.join("fixture-bin");
        fs::write(&fixture_binary, b"same executable bytes across builds")?;
        fs::set_permissions(&fixture_binary, fs::Permissions::from_mode(0o755))?;
        let times =
            FileTimes::new().set_modified(UNIX_EPOCH + Duration::from_secs(modified_seconds));
        fs::File::open(&fixture_binary)?.set_times(times)?;

        let path = format!("{}:{}", bin.display(), std::env::var("PATH")?);
        let output = Command::new("bash")
            .args(["-c", &script])
            .current_dir(&build_root)
            .env("PATH", path)
            .env("SOURCE_SHA", SOURCE_SHA)
            .env("RELEASE_VERSION", "1.10.0")
            .env("RUNNER_TEMP", &runner_temp)
            .env("MOCK_TARGET", target)
            .output()?;
        assert!(
            output.status.success(),
            "{run}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        archive_bytes.push(fs::read(
            build_root
                .join("dist")
                .join(format!("{binary}-1.10.0-{target}.tar.gz")),
        )?);

        if run == "first" {
            // The old tar path stored staged-copy mtimes at whole-second precision.
            std::thread::sleep(Duration::from_millis(1_100));
        }
    }

    let first_digest = sha256(&archive_bytes[0])?;
    let second_digest = sha256(&archive_bytes[1])?;
    assert_eq!(
        first_digest, second_digest,
        "identical executable bytes produced different archive digests"
    );
    assert_eq!(archive_bytes[0], archive_bytes[1]);
    assert_eq!(&archive_bytes[0][4..8], &[0, 0, 0, 0]);
    Ok(())
}

fn sha256(bytes: &[u8]) -> Result<String, Box<dyn Error>> {
    let scratch = Scratch::new()?;
    let path = scratch.0.join("archive.tar.gz");
    fs::write(&path, bytes)?;
    let output = if cfg!(target_os = "macos") {
        Command::new("shasum")
            .args(["-a", "256"])
            .arg(path)
            .output()?
    } else {
        Command::new("sha256sum").arg(path).output()?
    };
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned().into());
    }
    Ok(String::from_utf8(output.stdout)?
        .split_whitespace()
        .next()
        .ok_or("SHA-256 utility returned no digest")?
        .to_owned())
}

fn install_mock_git(path: &Path, source_sha: &str) -> Result<(), Box<dyn Error>> {
    fs::write(
        path,
        format!(
            "#!/bin/sh\nset -eu\n[ \"$1\" = rev-parse ] && [ \"$2\" = HEAD ] || exit 80\nprintf '%s\\n' '{source_sha}'\n"
        ),
    )?;
    let mut permissions = fs::metadata(path)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions)?;
    Ok(())
}

fn install_mock_file(path: &Path) -> Result<(), Box<dyn Error>> {
    fs::write(
        path,
        r#"#!/bin/sh
set -eu
[ "$1" = -b ] || exit 80
case "$MOCK_TARGET" in
  x86_64-unknown-linux-gnu) printf 'ELF 64-bit LSB executable, x86-64\n' ;;
  *) exit 81 ;;
esac
"#,
    )?;
    let mut permissions = fs::metadata(path)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions)?;
    Ok(())
}
