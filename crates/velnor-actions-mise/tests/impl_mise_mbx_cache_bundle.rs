//! Real MBX 1.21.1 CLI qualification for its directory bundle contract.
//!
//! Ignored tests require the official Linux `x86_64` binary at
//! `VELNOR_MBX_CACHE_QUALIFICATION_BINARY`; its extracted bytes are verified.
#![cfg(all(target_os = "linux", target_arch = "x86_64"))]

use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
const BINARY_ENV: &str = "VELNOR_MBX_CACHE_QUALIFICATION_BINARY";
const MBX_VERSION: &str = "1.21.1";
const OFFICIAL_RELEASE_SOURCE: &str = "commit=a0a44c61ca6aaa8da41d59deeebdfc46fc9d3313 archive=https://github.com/jdx/mr-boxington/releases/download/v1.21.1/mbx-x86_64-unknown-linux-gnu.tar.gz archive_sha256=1ecb4d55582a40a1227e8ca3450da054ea464e5ff976bb5762943fb1ce6f31da manifest_sha256=38e681992b87d182b8a3468bf7ae00dfc1aa9ae800210d8dede63a1b6a5bca3f";
const MBX_BINARY_SHA256: &str = "97984b8c92953cefc027014d24c8abf8773f2d12ded0385156d2050da8d5fb8c";
const PROBE_LOCK: &str = "version = 4\n\n[[package]]\nname = \"mbx-cache-probe-app\"\nversion = \"0.1.0\"\ndependencies = [\n \"mbx-cache-probe-lib\",\n]\n\n[[package]]\nname = \"mbx-cache-probe-lib\"\nversion = \"0.1.0\"\n";
static NEXT_STAGE: AtomicU64 = AtomicU64::new(0);
struct Stage {
    root: PathBuf,
    project: PathBuf,
    binary: PathBuf,
    path: OsString,
    rustup_home: OsString,
    group: OsString,
}
impl Stage {
    fn new() -> Result<Self, String> {
        let root = new_root()?;
        let project = root.join("project");
        write_probe_project(&project)?;
        let binary = official_binary()?;
        let path = std::env::var_os("PATH").ok_or_else(|| "PATH is unset".to_owned())?;
        let rustup_home = std::env::var_os("RUSTUP_HOME")
            .ok_or_else(|| "RUSTUP_HOME must name the installed Rust toolchain".to_owned())?;
        let id = root
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| "temporary stage has no UTF-8 name".to_owned())?
            .to_owned();
        Ok(Self {
            root,
            project,
            binary,
            path,
            rustup_home,
            group: OsString::from(format!("velnor-mbx-cli-{id}")),
        })
    }
    fn store_root(&self, name: &str) -> PathBuf {
        self.root.join(format!("store-{name}"))
    }
    fn command(&self, store: &str) -> Result<Command, String> {
        let home = self.root.join(format!("home-{store}"));
        let config = home.join("config");
        let cache = home.join("cache");
        let cargo = self.root.join(format!("cargo-{store}"));
        let mbx_cache = self.store_root(store);
        for path in [&home, &config, &cache, &cargo, &mbx_cache] {
            fs::create_dir_all(path).map_err(|err| err.to_string())?;
        }
        let mut command = Command::new(&self.binary);
        command
            .env_clear()
            .env("HOME", home)
            .env("XDG_CONFIG_HOME", config)
            .env("XDG_CACHE_HOME", cache)
            .env("CARGO_HOME", cargo)
            .env("CARGO_BUILD_JOBS", "2")
            .env("CARGO_NET_OFFLINE", "true")
            .env("MBX_CACHE_DIR", mbx_cache)
            .env("MBX_CACHE_EXPORT_GROUP", &self.group)
            .env("PATH", &self.path)
            .env("RUSTUP_HOME", &self.rustup_home)
            .env("RUSTUP_TOOLCHAIN", "1.98.1")
            .current_dir(&self.project)
            .stdin(Stdio::null());
        Ok(command)
    }
    fn run(&self, store: &str, args: &[OsString]) -> Result<Output, String> {
        self.command(store)?
            .args(args)
            .output()
            .map_err(|err| format!("cannot run official MBX: {err}"))
    }
    fn run_success(&self, store: &str, args: &[OsString]) -> Result<Output, String> {
        let output = self.run(store, args)?;
        if !output.status.success() {
            return Err(format!(
                "official MBX {args:?} failed: {}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            ));
        }
        eprintln!(
            "mbx {args:?}: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        Ok(output)
    }
    fn assert_store_path(&self, name: &str) -> Result<(), String> {
        let args = strings(&["cache", "dir", "--json"]);
        let output = self.run_success(name, &args)?;
        let expected = self.store_root(name).join("actions");
        let report = String::from_utf8_lossy(&output.stdout);
        if !report.contains(&expected.display().to_string()) {
            return Err(format!(
                "MBX store path did not match isolated MBX_CACHE_DIR: {report}"
            ));
        }
        Ok(())
    }
}
impl Drop for Stage {
    fn drop(&mut self) {
        drop(fs::remove_dir_all(&self.root));
    }
}
fn new_root() -> Result<PathBuf, String> {
    for _ in 0..16 {
        let id = NEXT_STAGE.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!("velnor-mbx-cli-{}-{id}", std::process::id()));
        match fs::create_dir(&root) {
            Ok(()) => return Ok(root),
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(err) => return Err(format!("cannot create {}: {err}", root.display())),
        }
    }
    Err("cannot allocate unique MBX qualification directory".to_owned())
}
fn write_probe_project(project: &Path) -> Result<(), String> {
    let app = project.join("app");
    let lib = project.join("probe");
    for path in [app.join("src"), lib.join("src")] {
        fs::create_dir_all(path).map_err(|err| err.to_string())?;
    }
    for (path, content) in [
        (
            project.join("Cargo.toml"),
            "[workspace]\nmembers = [\"app\", \"probe\"]\nresolver = \"3\"\n",
        ),
        (project.join("Cargo.lock"), PROBE_LOCK),
        (
            app.join("Cargo.toml"),
            "[package]\nname = \"mbx-cache-probe-app\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\nmbx-cache-probe-lib = { path = \"../probe\" }\n",
        ),
        (
            app.join("src/main.rs"),
            "fn main() { println!(\"{}\", mbx_cache_probe_lib::cached_value()); }\n",
        ),
        (
            lib.join("Cargo.toml"),
            "[package]\nname = \"mbx-cache-probe-lib\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        ),
        (
            lib.join("src/lib.rs"),
            "pub fn cached_value() -> u64 { 42 }\n",
        ),
    ] {
        write_text(path, content)?;
    }
    Ok(())
}
fn write_text(path: impl AsRef<Path>, content: &str) -> Result<(), String> {
    fs::write(path, content).map_err(|err| err.to_string())
}
fn sha256(path: &Path) -> Result<String, String> {
    let output = Command::new("/usr/bin/sha256sum")
        .arg(path)
        .env_clear()
        .output()
        .map_err(|err| format!("cannot hash {}: {err}", path.display()))?;
    if !output.status.success() {
        return Err(format!("sha256sum failed for {}", path.display()));
    }
    String::from_utf8_lossy(&output.stdout)
        .split_whitespace()
        .next()
        .map(str::to_owned)
        .ok_or_else(|| format!("sha256sum returned no digest for {}", path.display()))
}
fn official_binary() -> Result<PathBuf, String> {
    let supplied = std::env::var_os(BINARY_ENV)
        .map(PathBuf::from)
        .ok_or_else(|| format!("required verified MBX binary is unset: {BINARY_ENV}"))?;
    let metadata = fs::symlink_metadata(&supplied).map_err(|err| err.to_string())?;
    if !supplied.is_absolute() || !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(format!(
            "{BINARY_ENV} must be an absolute regular file: {}",
            supplied.display()
        ));
    }
    let binary = supplied.canonicalize().map_err(|err| err.to_string())?;
    let digest = sha256(&binary)?;
    if digest != MBX_BINARY_SHA256 {
        return Err(format!(
            "MBX binary digest mismatch: expected {MBX_BINARY_SHA256}, got {digest}"
        ));
    }
    let output = Command::new(&binary)
        .arg("--version")
        .env_clear()
        .output()
        .map_err(|err| format!("cannot run {}: {err}", binary.display()))?;
    let version = String::from_utf8_lossy(&output.stdout);
    if !output.status.success() || version.trim() != format!("mbx {MBX_VERSION}") {
        return Err(format!(
            "expected official MBX {MBX_VERSION}, got {version}"
        ));
    }
    eprintln!(
        "official MBX identity: version={MBX_VERSION} {OFFICIAL_RELEASE_SOURCE} binary_sha256={digest} path={}",
        binary.display()
    );
    Ok(binary)
}
fn largest_regular_file(root: &Path) -> Result<(PathBuf, u64), String> {
    let mut largest: Option<(PathBuf, u64)> = None;
    visit_files(root, &mut largest)?;
    largest.ok_or_else(|| format!("MBX export has no regular files: {}", root.display()))
}
fn visit_files(root: &Path, largest: &mut Option<(PathBuf, u64)>) -> Result<(), String> {
    for entry in fs::read_dir(root).map_err(|err| err.to_string())? {
        let entry = entry.map_err(|err| err.to_string())?;
        let kind = entry.file_type().map_err(|err| err.to_string())?;
        if kind.is_dir() {
            visit_files(&entry.path(), largest)?;
        } else if kind.is_file() {
            let len = entry.metadata().map_err(|err| err.to_string())?.len();
            if largest.as_ref().is_none_or(|(_, old_len)| len > *old_len) {
                *largest = Some((entry.path(), len));
            }
        }
    }
    Ok(())
}
fn strings(values: &[&str]) -> Vec<OsString> {
    values.iter().map(OsString::from).collect()
}
fn json_number(output: &Output, name: &str) -> Result<u64, String> {
    let text = std::str::from_utf8(&output.stdout).map_err(|err| err.to_string())?;
    let key = format!("\"{name}\":");
    let rest = text
        .split_once(&key)
        .map(|(_, rest)| rest.trim_start())
        .ok_or_else(|| format!("JSON field {name} is missing: {text}"))?;
    let number = rest
        .chars()
        .take_while(char::is_ascii_digit)
        .collect::<String>();
    number
        .parse()
        .map_err(|err| format!("JSON field {name}: {err}"))
}
const BUILD_ARGS: [&str; 3] = ["build", "--locked", "--offline"];
#[test]
#[ignore = "selected by the mandatory official MBX 1.21.1 cache CLI qualification lane"]
fn empty_group_export_fails_without_creating_a_bundle() -> Result<(), String> {
    let stage = Stage::new()?;
    let bundle = stage.root.join("empty-bundle");
    let mut args = strings(&["cache", "export", "--group"]);
    args.push(stage.group.clone());
    args.extend(strings(&["--format", "directory"]));
    args.push(bundle.clone().into_os_string());
    let output = stage.run("empty", &args)?;
    assert!(!output.status.success(), "empty export must fail");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("no completed mbx builds"),
        "unexpected empty-export error: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!bundle.exists(), "empty export created a bundle path");
    stage.assert_store_path("empty")
}
#[test]
#[ignore = "selected by the mandatory official MBX 1.21.1 cache CLI qualification lane"]
fn directory_bundle_import_reuses_compilation_in_a_fresh_isolated_store() -> Result<(), String> {
    let stage = Stage::new()?;
    let writer_store = stage.store_root("writer");
    let reader_store = stage.store_root("reader");
    let bundle = stage.root.join("mbx-single-bundle");
    assert!(!bundle.starts_with(&writer_store));
    assert!(!bundle.starts_with(&reader_store));

    stage.run_success("writer", &strings(&BUILD_ARGS))?;
    stage.assert_store_path("writer")?;
    let mut export = strings(&["cache", "export", "--group"]);
    export.push(stage.group.clone());
    export.extend(strings(&["--format", "directory"]));
    export.push(bundle.clone().into_os_string());
    stage.run_success("writer", &export)?;
    assert!(largest_regular_file(&bundle)?.1 > 0);

    remove_target_link(&stage.project)?;
    let mut import = strings(&["cache", "import"]);
    import.push(bundle.clone().into_os_string());
    stage.run_success("reader", &import)?;
    assert!(
        !bundle.exists(),
        "directory import did not consume its bundle"
    );
    stage.assert_store_path("reader")?;
    let imported = stage.run_success("reader", &strings(&["cache", "stats", "--json"]))?;
    assert!(
        json_number(&imported, "objects")? > 0,
        "reader store is empty"
    );
    let source = stage.project.join("probe/src/lib.rs");
    let contents = fs::read(&source).map_err(|err| err.to_string())?;
    fs::write(source, contents).map_err(|err| err.to_string())?;
    stage.run_success("reader", &strings(&BUILD_ARGS))?;
    let stats = stage.run_success("reader", &strings(&["stats", "--json"]))?;
    assert!(
        json_number(&stats, "cached_compilations")? > 0,
        "reader did not reuse a cached compilation: {}",
        String::from_utf8_lossy(&stats.stdout)
    );
    assert_target_is_owned_by(&stage.project, &reader_store)
}
#[test]
#[ignore = "selected by the mandatory official MBX 1.21.1 cache CLI qualification lane"]
fn corrupt_directory_import_publishes_nothing_and_a_cold_build_succeeds() -> Result<(), String> {
    let stage = Stage::new()?;
    let reader_store = stage.store_root("reader");
    let bundle = stage.root.join("mbx-single-bundle");
    stage.run_success("writer", &strings(&BUILD_ARGS))?;
    let mut export = strings(&["cache", "export", "--group"]);
    export.push(stage.group.clone());
    export.extend(strings(&["--format", "directory"]));
    export.push(bundle.clone().into_os_string());
    stage.run_success("writer", &export)?;

    let (member, length) = largest_regular_file(&bundle)?;
    assert!(length > 1, "bundle member is too short to truncate");
    OpenOptions::new()
        .write(true)
        .open(&member)
        .and_then(|file| file.set_len(1))
        .map_err(|err| format!("cannot truncate {}: {err}", member.display()))?;
    remove_target_link(&stage.project)?;

    let mut import = strings(&["cache", "import"]);
    import.push(bundle.clone().into_os_string());
    let rejected = stage.run("reader", &import)?;
    assert!(
        !rejected.status.success(),
        "corrupt directory import passed"
    );
    let error = String::from_utf8_lossy(&rejected.stderr);
    assert!(
        error.contains("corrupt"),
        "unexpected import error: {error}"
    );
    assert!(
        bundle.exists(),
        "failed directory import consumed the bundle"
    );

    stage.assert_store_path("reader")?;
    let empty = stage.run_success("reader", &strings(&["cache", "stats", "--json"]))?;
    assert_eq!(
        json_number(&empty, "objects")?,
        0,
        "failed import published objects"
    );
    stage.run_success("reader", &strings(&BUILD_ARGS))?;
    let stats = stage.run_success("reader", &strings(&["stats", "--json"]))?;
    assert_eq!(json_number(&stats, "cached_compilations")?, 0);
    assert!(
        json_number(&stats, "objects")? > 0,
        "cold build produced no objects"
    );
    assert_target_is_owned_by(&stage.project, &reader_store)
}
fn remove_target_link(project: &Path) -> Result<(), String> {
    let target = project.join("target");
    let metadata = fs::symlink_metadata(&target).map_err(|err| err.to_string())?;
    if !metadata.file_type().is_symlink() {
        return Err(format!(
            "MBX target is not a managed link: {}",
            target.display()
        ));
    }
    fs::remove_file(target).map_err(|err| err.to_string())
}
fn assert_target_is_owned_by(project: &Path, store: &Path) -> Result<(), String> {
    let target = fs::read_link(project.join("target")).map_err(|err| err.to_string())?;
    if !target.starts_with(store) {
        return Err(format!(
            "MBX target {} is outside isolated store {}",
            target.display(),
            store.display()
        ));
    }
    Ok(())
}
