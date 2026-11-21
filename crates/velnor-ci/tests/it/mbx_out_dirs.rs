// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use velnor_test_support::{ci_safe_cargo_env, real_cargo};

type TestResult = Result<(), Box<dyn std::error::Error>>;

const SETTING: (&str, &str) = ("MBX_SHARE_OUT_DIR", "0");

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let root =
            std::env::temp_dir().join(format!("velnor-mbx-out-dirs-{}-{name}", std::process::id()));
        removable(&root);
        fs::remove_dir_all(&root).ok();
        fs::create_dir_all(&root)?;
        let root = fs::canonicalize(root)?;
        declare_mbx(&root)?;
        Ok(Self(root))
    }
}

/// Declares mbx for the scratch tree the way a real project does: the checkout's own `rust` and
/// `mr-boxington` tool entries in a `mise.toml`. The scratch tree sits outside the checkout, so
/// without this mise's cargo wrapper finds no config that names mbx and runs plain cargo (a
/// developer's global mise config hides that locally; a CI runner has none).
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

/// `MISE_TRUSTED_CONFIG_PATHS` with the scratch tree added, so mise reads its `mise.toml`.
fn trusted_with(root: &Path) -> std::ffi::OsString {
    let mut paths: Vec<PathBuf> = std::env::var_os("MISE_TRUSTED_CONFIG_PATHS")
        .map(|value| std::env::split_paths(&value).collect())
        .unwrap_or_default();
    paths.push(root.to_path_buf());
    std::env::join_paths(paths).unwrap_or_else(|_| root.into())
}

impl Drop for Scratch {
    fn drop(&mut self) {
        removable(&self.0);
        fs::remove_dir_all(&self.0).ok();
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

const SEEDED: [&str; 4] = [
    "cas",
    "executable-identities",
    "task-manifests",
    "action-results",
];

fn copy_seeded_subsets(from: &Path, to: &Path) -> TestResult {
    fs::create_dir_all(to.join("actions"))?;
    for subset in SEEDED {
        let source = from.join("actions").join(subset);
        if source.exists() {
            let status = Command::new("cp")
                .arg("-R")
                .arg(&source)
                .arg(to.join("actions"))
                .status()?;
            assert!(status.success(), "cp {}", source.display());
        }
    }
    Ok(())
}

fn rebuild_from_base(
    name: &str,
    setting: Option<(&str, &str)>,
    seeded: bool,
) -> Result<String, Box<dyn std::error::Error>> {
    let scratch = Scratch::new(name)?;
    let store = scratch.0.join("runner-temp/mbx");
    let krate = the_crate(&scratch.0)?;
    let cold = build(&krate, &store, setting)?;
    assert!(cold.contains("Compiling probe"), "{cold}");
    let base = scratch.0.join("base.tar").display().to_string();
    let target = fs::canonicalize(krate.join("target"))?;
    tar(&["-C", &target.display().to_string(), "-cf", &base, "."])?;
    let seed = scratch.0.join("seed");
    if seeded {
        copy_seeded_subsets(&store, &seed)?;
    }
    fs::remove_file(krate.join("target"))?;
    removable(&scratch.0.join("runner-temp"));
    fs::remove_dir_all(scratch.0.join("runner-temp"))?;
    if seeded {
        copy_seeded_subsets(&seed, &store)?;
    }
    fs::create_dir(krate.join("target"))?;
    tar(&[
        "-C",
        &krate.join("target").display().to_string(),
        "-xf",
        &base,
    ])?;
    build(&krate, &store, setting)
}

fn warm_build_in_an_empty_store(
    name: &str,
    setting: Option<(&str, &str)>,
) -> Result<String, Box<dyn std::error::Error>> {
    rebuild_from_base(name, setting, false)
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

#[test]
fn a_base_restored_into_a_seeded_store_compiles_nothing_when_outputs_stay_in_target() -> TestResult
{
    let warm = rebuild_from_base("seeded-kept", Some(SETTING), true)?;
    assert!(!warm.contains("Compiling"), "{warm}");
    assert!(warm.contains("Fresh probe"), "{warm}");
    Ok(())
}

#[test]
fn without_the_setting_a_seeded_store_still_recompiles_the_probe() -> TestResult {
    let warm = rebuild_from_base("seeded-default", None, true)?;
    assert!(warm.contains("Compiling probe"), "{warm}");
    Ok(())
}
