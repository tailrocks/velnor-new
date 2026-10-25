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
            "velnor-verify-local-cargo-dispatch-{}-{unique}",
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

fn write_executable(path: &Path, body: &str) -> Result<(), Box<dyn Error>> {
    fs::write(path, format!("#!/bin/sh\n{body}\n"))?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

fn script_function<'a>(script: &'a str, name: &str) -> Result<&'a str, Box<dyn Error>> {
    let start = script
        .find(&format!("{name}() {{"))
        .ok_or("The Nextest dispatcher helper is missing.")?;
    let body = &script[start..];
    let end = body
        .find("\n}\n")
        .ok_or("The Nextest dispatcher helper is incomplete.")?
        + 2;
    Ok(&body[..end])
}

fn cargo_path() -> Result<PathBuf, Box<dyn Error>> {
    let policy: toml::Value = toml::from_str(&fs::read_to_string(
        workspace_root().join(".velnor/version-policy.toml"),
    )?)?;
    let version = policy["tools"]["rust"]
        .as_str()
        .ok_or("The Rust toolchain pin is missing.")?;
    let output = Command::new("rustup")
        .args(["which", "--toolchain", version, "cargo"])
        .output()?;
    if !output.status.success() {
        return Err("Cargo is not available from the pinned Rust toolchain.".into());
    }
    Ok(PathBuf::from(String::from_utf8(output.stdout)?.trim()))
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn path_with(prefixes: &[&Path], suffix: &OsStr) -> Result<OsString, Box<dyn Error>> {
    let mut paths = prefixes
        .iter()
        .map(|path| path.to_path_buf())
        .collect::<Vec<_>>();
    paths.extend(std::env::split_paths(suffix));
    Ok(std::env::join_paths(paths)?)
}

fn prepare_dispatcher(script: &str, dispatcher: &Path, root: &Path) -> Result<(), Box<dyn Error>> {
    let helper = script_function(script, "prepare_nextest_dispatcher")?;
    let harness = root.join("prepare-nextest-dispatcher.sh");
    fs::write(
        &harness,
        format!("{helper}\nprepare_nextest_dispatcher \"$1\"\n"),
    )?;
    let prepared = Command::new("bash")
        .arg(&harness)
        .arg(dispatcher)
        .output()?;
    assert!(
        prepared.status.success(),
        "The dispatcher setup failed. Error: {}",
        String::from_utf8_lossy(&prepared.stderr)
    );
    assert_eq!(
        fs::metadata(dispatcher)?.permissions().mode() & 0o777,
        0o700
    );
    Ok(())
}

fn run(
    executable: &Path,
    args: &[&str],
    cwd: &Path,
    cargo_home: &Path,
    path: OsString,
    cargo_tool: &Path,
    nextest: &Path,
) -> Result<std::process::Output, Box<dyn Error>> {
    Ok(Command::new(executable)
        .args(args)
        .current_dir(cwd)
        .env("CARGO_HOME", cargo_home)
        .env("CARGO_TOOL_PATH", cargo_tool)
        .env("NEXTEST_BIN", nextest)
        .env("PATH", path)
        .output()?)
}

fn create_fixture(root: &Path) -> Result<(PathBuf, PathBuf, PathBuf), Box<dyn Error>> {
    let cargo_home = root.join("cargo-home");
    let shadow_bin = cargo_home.join("bin");
    let pinned_bin = root.join("pinned");
    fs::create_dir_all(cargo_home.join("registry"))?;
    fs::create_dir_all(cargo_home.join("git"))?;
    fs::create_dir(&shadow_bin)?;
    fs::create_dir(&pinned_bin)?;
    fs::write(cargo_home.join("config.toml"), "# fixture Cargo config\n")?;
    fs::write(cargo_home.join("credentials.toml"), "fixture credential\n")?;
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"cargo-dispatch-fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )?;
    fs::create_dir(root.join("src"))?;
    fs::write(root.join("src/main.rs"), "fn main() {}\n")?;

    let shadow = shadow_bin.join("cargo-nextest");
    let pinned = pinned_bin.join("cargo-nextest");
    write_executable(&shadow, "printf '%s\\n' 'CARGO_HOME shadow selected'")?;
    write_executable(
        &pinned,
        "printf '%s\\n' 'cargo-nextest 0.9.149 (pinned fixture)'",
    )?;
    Ok((cargo_home, shadow, pinned))
}

fn assert_global_home_precedence(
    cargo: &Path,
    root: &Path,
    cargo_home: &Path,
    path: OsString,
    pinned: &Path,
) -> Result<(), Box<dyn Error>> {
    let path_description = path.to_string_lossy().into_owned();
    let shadowed = run(
        cargo,
        &["nextest", "--version"],
        root,
        cargo_home,
        path,
        cargo,
        pinned,
    )?;
    let shadowed_stdout = String::from_utf8(shadowed.stdout)?;
    assert!(shadowed.status.success());
    assert!(
        shadowed_stdout.contains("CARGO_HOME shadow selected"),
        "Cargo must select the test executable in CARGO_HOME/bin. Diagnostics: stdout={shadowed_stdout}; cargo={}; home={}; PATH={path_description}",
        cargo.display(),
        cargo_home.display()
    );
    Ok(())
}

fn assert_dispatcher_contract(script: &str) {
    assert!(
        script.contains("\"CARGO=$CARGO_DISPATCHER\"")
            && script.contains("\"NEXTEST_BIN=$NEXTEST_BIN\"")
            && script.contains("\"CARGO_TOOL_PATH=$CARGO_TOOL_PATH\""),
        "MBX must receive the Cargo dispatcher and both exact executable paths."
    );
    assert!(
        script.contains("exec mbx nextest \"$@\""),
        "MBX must run the pinned Nextest executable."
    );
}

fn assert_pinned_dispatch(
    dispatcher: &Path,
    root: &Path,
    cargo_home: &Path,
    path: OsString,
    cargo: &Path,
    pinned: &Path,
) -> Result<(), Box<dyn Error>> {
    let selected = run(
        dispatcher,
        &["nextest", "--version"],
        root,
        cargo_home,
        path,
        cargo,
        pinned,
    )?;
    let selected_stdout = String::from_utf8(selected.stdout)?;
    assert!(selected.status.success());
    assert!(
        selected_stdout.contains("cargo-nextest 0.9.149"),
        "The dispatcher did not select the pinned executable. Output: {selected_stdout}"
    );
    assert!(
        !selected_stdout.contains("CARGO_HOME shadow selected"),
        "The dispatcher ran the test executable in CARGO_HOME/bin."
    );
    Ok(())
}

fn assert_cargo_delegation(
    dispatcher: &Path,
    root: &Path,
    cargo_home: &Path,
    path: OsString,
    cargo: &Path,
    pinned: &Path,
) -> Result<(), Box<dyn Error>> {
    let manifest = root
        .join("Cargo.toml")
        .to_str()
        .ok_or("The manifest path is not UTF-8.")?
        .to_owned();
    let delegated = run(
        dispatcher,
        &[
            "metadata",
            "--no-deps",
            "--format-version",
            "1",
            "--manifest-path",
            &manifest,
        ],
        root,
        cargo_home,
        path,
        cargo,
        pinned,
    )?;
    let metadata = String::from_utf8(delegated.stdout)?;
    assert!(
        delegated.status.success(),
        "The dispatcher did not send the metadata command to Cargo. Error: {}",
        String::from_utf8_lossy(&delegated.stderr)
    );
    assert!(metadata.contains("cargo-dispatch-fixture"));
    assert_eq!(
        fs::read_to_string(cargo_home.join("config.toml"))?,
        "# fixture Cargo config\n"
    );
    assert_eq!(
        fs::read_to_string(cargo_home.join("credentials.toml"))?,
        "fixture credential\n"
    );
    Ok(())
}

#[test]
fn verify_local_dispatcher_overrides_cargo_home_shadow_and_delegates_cargo()
-> Result<(), Box<dyn Error>> {
    let temporary = TemporaryDirectory::new()?;
    let root = temporary.path();
    let (cargo_home, shadow, pinned) = create_fixture(root)?;
    assert!(shadow.is_file());
    let cargo = cargo_path()?;
    let original_path = std::env::var_os("PATH").ok_or("PATH is missing")?;
    let prefix = [pinned
        .parent()
        .ok_or("The pinned executable path has no parent directory.")?];
    let candidate_path = path_with(&prefix, original_path.as_os_str())?;

    assert_global_home_precedence(&cargo, root, &cargo_home, candidate_path.clone(), &pinned)?;
    let script = super::entrypoint_text()?;
    assert_dispatcher_contract(&script);
    let dispatcher = root.join("cargo-dispatch");
    prepare_dispatcher(&script, &dispatcher, root)?;
    assert_pinned_dispatch(
        &dispatcher,
        root,
        &cargo_home,
        candidate_path.clone(),
        &cargo,
        &pinned,
    )?;
    assert_cargo_delegation(
        &dispatcher,
        root,
        &cargo_home,
        candidate_path,
        &cargo,
        &pinned,
    )?;
    Ok(())
}
