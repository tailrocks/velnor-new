use std::error::Error;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

struct TemporaryDirectory(PathBuf);

impl TemporaryDirectory {
    fn new() -> Result<Self, Box<dyn Error>> {
        let unique = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let path = std::env::temp_dir().join(format!(
            "velnor-verify-local-cargo-home-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir(&path)?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700))?;
        Ok(Self(path))
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TemporaryDirectory {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.0) {
            eprintln!("could not remove test directory: {error}");
        }
    }
}

fn write_executable(path: &Path, version: &str) -> Result<(), Box<dyn Error>> {
    fs::write(
        path,
        format!("#!/bin/sh\nprintf '%s\\n' 'cargo-nextest {version} (fixture)'\n"),
    )?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

fn script_function<'a>(script: &'a str, name: &str) -> Result<&'a str, Box<dyn Error>> {
    let start = script
        .find(&format!("{name}() {{"))
        .ok_or("CARGO_HOME helper is missing")?;
    let body = &script[start..];
    let end = body
        .find("\n}\n")
        .ok_or("CARGO_HOME helper is incomplete")?
        + 2;
    Ok(&body[..end])
}

fn cargo_path() -> Result<PathBuf, Box<dyn Error>> {
    let output = Command::new("sh")
        .args(["-c", "command -v cargo", "verify-local-cargo"])
        .output()?;
    if !output.status.success() {
        return Err("Cargo is not on PATH".into());
    }
    Ok(PathBuf::from(String::from_utf8(output.stdout)?.trim()))
}

fn path_with(prefixes: &[&Path], suffix: &OsStr) -> Result<OsString, Box<dyn Error>> {
    let mut paths = prefixes
        .iter()
        .map(|path| path.to_path_buf())
        .collect::<Vec<_>>();
    paths.extend(std::env::split_paths(suffix));
    Ok(std::env::join_paths(paths)?)
}

fn cargo_nextest_version(
    cargo: &Path,
    cwd: &Path,
    cargo_home: &Path,
    path: OsString,
) -> Result<String, Box<dyn Error>> {
    let output = Command::new(cargo)
        .args(["nextest", "--version"])
        .current_dir(cwd)
        .env("CARGO_HOME", cargo_home)
        .env("PATH", path)
        .output()?;
    if !output.status.success() {
        return Err(format!(
            "Cargo Nextest command failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn create_fixture(
    global_home: &Path,
    pinned_bin: &Path,
) -> Result<(PathBuf, PathBuf), Box<dyn Error>> {
    fs::create_dir_all(global_home.join("registry"))?;
    fs::create_dir_all(global_home.join("git"))?;
    let shadow_bin = global_home.join("bin");
    fs::create_dir(&shadow_bin)?;
    fs::create_dir(pinned_bin)?;
    fs::write(
        global_home.join("config.toml"),
        "# synthetic global config\n",
    )?;
    fs::write(
        global_home.join("credentials.toml"),
        "# synthetic credential file\n",
    )?;
    let shadowed_nextest = shadow_bin.join("cargo-nextest");
    let pinned_nextest = pinned_bin.join("cargo-nextest");
    write_executable(&shadowed_nextest, "0.9.143")?;
    write_executable(&pinned_nextest, "0.9.148")?;
    Ok((shadow_bin, pinned_nextest))
}

fn prepare_cargo_home(
    script: &str,
    global_home: &Path,
    isolated_home: &Path,
    pinned_nextest: &Path,
    root: &Path,
) -> Result<(), Box<dyn Error>> {
    let helper = script_function(script, "prepare_isolated_cargo_home")?;
    let harness = root.join("prepare-cargo-home.sh");
    fs::write(
        &harness,
        format!("{helper}\nprepare_isolated_cargo_home \"$1\" \"$2\" \"$3\"\n"),
    )?;
    let prepared = Command::new("bash")
        .arg(&harness)
        .arg(global_home)
        .arg(isolated_home)
        .arg(pinned_nextest)
        .output()?;
    assert!(
        prepared.status.success(),
        "CARGO_HOME setup failed: {}",
        String::from_utf8_lossy(&prepared.stderr)
    );
    Ok(())
}

fn assert_private_cargo_home(
    global_home: &Path,
    isolated_home: &Path,
    pinned_nextest: &Path,
) -> Result<(), Box<dyn Error>> {
    assert_eq!(
        fs::canonicalize(isolated_home.join("bin/cargo-nextest"))?,
        fs::canonicalize(pinned_nextest)?
    );
    assert_eq!(
        fs::canonicalize(isolated_home.join("registry"))?,
        fs::canonicalize(global_home.join("registry"))?
    );
    assert_eq!(
        fs::canonicalize(isolated_home.join("git"))?,
        fs::canonicalize(global_home.join("git"))?
    );
    assert!(!isolated_home.join("config.toml").exists());
    assert!(!isolated_home.join("credentials.toml").exists());
    assert_eq!(
        fs::metadata(isolated_home)?.permissions().mode() & 0o777,
        0o700
    );
    Ok(())
}

fn assert_route_selection(
    root: &Path,
    global_home: &Path,
    isolated_home: &Path,
    shadow_bin: &Path,
    pinned_bin: &Path,
) -> Result<(), Box<dyn Error>> {
    let cargo = cargo_path()?;
    let original_path = std::env::var_os("PATH").ok_or("PATH is missing")?;
    let baseline_path = path_with(&[shadow_bin, pinned_bin], original_path.as_os_str())?;
    let baseline = cargo_nextest_version(&cargo, root, global_home, baseline_path)?;
    assert!(
        baseline.contains("0.9.143"),
        "older executable not selected: {baseline}"
    );

    let isolated_bin = isolated_home.join("bin");
    let isolated_path = path_with(
        &[&isolated_bin, pinned_bin, shadow_bin],
        original_path.as_os_str(),
    )?;
    let isolated = cargo_nextest_version(&cargo, root, isolated_home, isolated_path)?;
    assert!(
        isolated.contains("0.9.148"),
        "pinned executable not selected: {isolated}"
    );
    Ok(())
}

#[test]
fn verify_local_isolates_the_pinned_nextest_from_the_global_cargo_home()
-> Result<(), Box<dyn Error>> {
    let temporary = TemporaryDirectory::new()?;
    let root = temporary.path();
    let global_home = root.join("global-cargo-home");
    let isolated_home = root.join("isolated-cargo-home");
    let pinned_bin = root.join("pinned");
    let (shadow_bin, pinned_nextest) = create_fixture(&global_home, &pinned_bin)?;
    let script = super::entrypoint_text()?;
    assert!(
        script.contains("PATH=\"$CARGO_HOME/bin:$NEXTEST_DIR:$PATH\""),
        "Nextest command must put the private CARGO_HOME before other command directories"
    );
    prepare_cargo_home(&script, &global_home, &isolated_home, &pinned_nextest, root)?;
    assert_private_cargo_home(&global_home, &isolated_home, &pinned_nextest)?;
    assert_route_selection(root, &global_home, &isolated_home, &shadow_bin, &pinned_bin)?;

    let root_path = root.to_path_buf();
    drop(temporary);
    assert!(
        !root_path.exists(),
        "temporary CARGO_HOME directories were not removed"
    );
    Ok(())
}
