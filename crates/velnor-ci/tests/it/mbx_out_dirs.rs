// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant, UNIX_EPOCH};

use velnor_test_support::{ci_safe_cargo_env, real_cargo};

type TestResult = Result<(), Box<dyn std::error::Error>>;

const SOURCE_MTIME_SECS: u64 = 1_600_000_000;

const SETTING: (&str, &str) = ("MBX_SHARE_OUT_DIR", "0");

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let root =
            std::env::temp_dir().join(format!("velnor-mbx-out-dirs-{}-{name}", std::process::id()));
        Self::at(root)
    }

    fn at(root: PathBuf) -> Result<Self, Box<dyn std::error::Error>> {
        remove_tree(&root).ok();
        fs::create_dir_all(&root)?;
        let root = fs::canonicalize(root)?;
        declare_mbx(&root)?;
        Ok(Self(root))
    }
}

fn declare_mbx(root: &Path) -> TestResult {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../mise.toml");
    let repo: toml::Table = fs::read_to_string(repo)?.parse()?;
    let tools = repo
        .get("tools")
        .and_then(toml::Value::as_table)
        .ok_or("the checkout's mise.toml has no [tools]")?;
    let mut declared = toml::Table::new();
    for tool in ["rust", "mr-boxington"] {
        let entry = tools
            .get(tool)
            .ok_or_else(|| format!("the checkout's mise.toml pins no {tool}"))?;
        declared.insert(tool.to_owned(), entry.clone());
    }
    let config = toml::Table::from_iter([("tools".to_owned(), toml::Value::Table(declared))]);
    fs::write(root.join("mise.toml"), toml::to_string(&config)?)?;
    Ok(())
}

fn trusted_with(root: &Path) -> std::ffi::OsString {
    let mut paths: Vec<PathBuf> = std::env::var_os("MISE_TRUSTED_CONFIG_PATHS")
        .map_or_default(|value| std::env::split_paths(&value).collect());
    paths.push(root.to_path_buf());
    std::env::join_paths(paths).unwrap_or_else(|_| root.into())
}

impl Drop for Scratch {
    fn drop(&mut self) {
        if !velnor_test_support::keeping() {
            remove_tree(&self.0).ok();
        }
    }
}

const PROBE_ENV: &str = "VELNOR_MBX_KEEP_PROBE";

#[test]
fn a_scratch_tree_survives_its_guard_when_the_keep_flag_is_set() -> TestResult {
    let probe = std::env::temp_dir().join(format!("velnor-mbx-keep-probe-{}", std::process::id()));
    remove_tree(&probe).ok();
    let status = std::process::Command::new(std::env::current_exe()?)
        .args([
            "--exact",
            "mbx_out_dirs::a_scratch_tree_is_written_for_the_keep_probe",
            "--nocapture",
        ])
        .env(PROBE_ENV, &probe)
        .env("VELNOR_KEEP_TEST_DIRS", "1")
        .status()?;
    let kept = probe.join("mise.toml").is_file();
    remove_tree(&probe).ok();
    assert!(status.success(), "probe child failed: {status}");
    assert!(
        kept,
        "the scratch tree must survive its guard when VELNOR_KEEP_TEST_DIRS=1"
    );
    Ok(())
}

#[test]
fn a_scratch_tree_is_written_for_the_keep_probe() -> TestResult {
    let Some(probe) = std::env::var_os(PROBE_ENV) else {
        return Ok(());
    };
    Scratch::at(PathBuf::from(probe))?;
    Ok(())
}

fn remove_tree(path: &Path) -> std::io::Result<()> {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        removable(path);
        match fs::remove_dir_all(path) {
            Err(error)
                if error.kind() == std::io::ErrorKind::DirectoryNotEmpty
                    && Instant::now() < deadline =>
            {
                std::thread::yield_now();
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            other => return other,
        }
    }
}

fn removable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return;
    };
    if metadata.is_dir() {
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).ok();
        for entry in fs::read_dir(path).into_iter().flatten().flatten() {
            removable(&entry.path());
        }
    }
}

fn the_crate(root: &Path) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let krate = root.join("probe");
    fs::create_dir_all(krate.join("src"))?;
    fs::write(
        krate.join("Cargo.toml"),
        "[package]\nname = \"probe\"\nversion = \"0.0.0\"\nedition = \"2021\"\nbuild = \"build.rs\"\n",
    )?;
    fs::write(
        krate.join("build.rs"),
        "fn main() {\n    let out = std::env::var(\"OUT_DIR\").unwrap();\n    std::fs::write(format!(\"{out}/x.rs\"), \"pub fn value() -> u32 { 7 }\").unwrap();\n}\n",
    )?;
    fs::write(
        krate.join("src/lib.rs"),
        "include!(concat!(env!(\"OUT_DIR\"), \"/x.rs\"));\n",
    )?;
    let settled = UNIX_EPOCH + Duration::from_secs(SOURCE_MTIME_SECS);
    for source in ["Cargo.toml", "build.rs", "src/lib.rs"] {
        fs::File::options()
            .write(true)
            .open(krate.join(source))?
            .set_modified(settled)?;
    }
    Ok(krate)
}

fn build(
    krate: &Path,
    store: &Path,
    setting: Option<(&str, &str)>,
) -> Result<String, Box<dyn std::error::Error>> {
    let mut cargo = Command::new(real_cargo());
    cargo.args(["build", "-v", "--offline"]).current_dir(krate);
    cargo.env_clear();
    for (name, value) in std::env::vars_os() {
        let text = name.to_string_lossy();
        if matches!(
            text.as_ref(),
            "PATH" | "HOME" | "USER" | "LANG" | "TMPDIR" | "CARGO_HOME"
        ) || text.starts_with("MISE_")
            || text.starts_with("RUSTUP_")
        {
            cargo.env(name, value);
        }
    }
    ci_safe_cargo_env(&mut cargo);
    if let Some(root) = krate.parent() {
        cargo.env("MISE_TRUSTED_CONFIG_PATHS", trusted_with(root));
    }
    cargo.env("MBX_CACHE_DIR", store);
    if let Some((name, value)) = setting {
        cargo.env(name, value);
    }
    let output = cargo.output()?;
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(output.status.success(), "{stderr}");
    assert!(
        stderr.contains("mbx[cache]"),
        "the build did not run under mbx: {stderr}"
    );
    Ok(stderr)
}

fn tar(args: &[&str]) -> TestResult {
    let status = Command::new("tar").args(args).status()?;
    assert!(status.success(), "tar {args:?}");
    Ok(())
}

fn warm_build_in_an_empty_store(
    name: &str,
    setting: Option<(&str, &str)>,
) -> Result<String, Box<dyn std::error::Error>> {
    let scratch = Scratch::new(name)?;
    let store = scratch.0.join("runner-temp/mbx");
    let krate = the_crate(&scratch.0)?;
    let cold = build(&krate, &store, setting)?;
    assert!(cold.contains("Compiling probe"), "{cold}");
    let base = scratch.0.join("base.tar").display().to_string();
    let target = fs::canonicalize(krate.join("target"))?;
    tar(&[
        "--format=ustar",
        "-C",
        &target.display().to_string(),
        "-cf",
        &base,
        ".",
    ])?;
    fs::remove_file(krate.join("target"))?;
    remove_tree(&scratch.0.join("runner-temp"))?;
    fs::create_dir(krate.join("target"))?;
    tar(&[
        "-C",
        &krate.join("target").display().to_string(),
        "-xf",
        &base,
    ])?;
    build(&krate, &store, setting)
}

#[test]
fn a_base_restored_into_an_empty_mbx_store_compiles_nothing_when_outputs_stay_in_target()
-> TestResult {
    let warm = warm_build_in_an_empty_store("kept", Some(SETTING))?;
    assert!(!warm.contains("Compiling"), "{warm}");
    assert!(warm.contains("Fresh probe"), "{warm}");
    Ok(())
}

#[test]
fn without_the_setting_mbx_points_dep_info_into_a_store_the_next_job_does_not_have() -> TestResult {
    let warm = warm_build_in_an_empty_store("default", None)?;
    assert!(
        warm.contains("Compiling probe"),
        "mbx no longer keeps build-script outputs in the cache directory; drop {}=0 (#2373): {warm}",
        SETTING.0
    );
    Ok(())
}
