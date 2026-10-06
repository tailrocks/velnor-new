//! P12 fixture harness: minimal repo roots plus script-runner helpers.
//!
//! Each builder returns a self-contained tree under a fresh tempdir that
//! `scripts/check-freshness.sh --root` validates. The passing root uses
//! the real reviewed pin values as fixture data; failing roots mutate one
//! aspect each. Date helpers derive "today" from the system clock so
//! evidence windows stay deterministic without external crates.

#[path = "p12_probe_rows.rs"]
mod probe_data;

use std::error::Error;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

/// One script run: exit code plus captured streams.
pub(crate) struct Run {
    /// Exit code, or -1 when the child died by signal.
    pub(crate) code: i32,
    /// Captured stdout text.
    pub(crate) stdout: String,
    /// Captured stderr text.
    pub(crate) stderr: String,
}

/// A fixture repo root; removed best-effort by callers via [`cleanup`].
pub(crate) struct Fixture {
    /// Root directory of the fixture tree.
    pub(crate) dir: PathBuf,
}

/// Days since the Unix epoch, plus `offset`, as `YYYY-MM-DD`.
pub(crate) fn days_iso(offset: i64) -> Result<String, Box<dyn Error>> {
    let whole_days = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|err| err.to_string())?
        .as_secs()
        / 86_400;
    let days = i64::try_from(whole_days).map_err(|err| err.to_string())? + offset;
    // Howard Hinnant's civil-from-days algorithm (shift to civil epoch).
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month + 2) / 5 + 1;
    let month = if month < 10 { month + 3 } else { month - 9 };
    let year = if month <= 2 { year + 1 } else { year };
    Ok(format!("{year:04}-{month:02}-{day:02}"))
}

/// Write `body` to `dir/rel`, creating parent directories.
pub(crate) fn write(dir: &Path, rel: &str, body: &str) -> Result<(), Box<dyn Error>> {
    let path = dir.join(rel);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, body)?;
    Ok(())
}

/// Replace exactly one occurrence of `old` with `new` in `dir/rel`.
pub(crate) fn mutate(dir: &Path, rel: &str, old: &str, new: &str) -> Result<(), Box<dyn Error>> {
    let path = dir.join(rel);
    let body = std::fs::read_to_string(&path)?;
    assert_eq!(body.matches(old).count(), 1, "{rel} anchor {old:?}");
    std::fs::write(path, body.replacen(old, new, 1))?;
    Ok(())
}

/// Run `check-freshness.sh --root dir` plus `extra` args.
pub(crate) fn run_script(dir: &Path, extra: &[&str]) -> Result<Run, Box<dyn Error>> {
    let script = crate::impl_repo_policy::repo_root().join("scripts/check-freshness.sh");
    let mut command = Command::new("bash");
    command.arg(script).arg("--root").arg(dir).args(extra);
    let output = command.output()?;
    Ok(Run {
        code: output.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    })
}

/// Stdout lines carrying a machine-readable `row:` payload.
pub(crate) fn rows(run: &Run) -> Vec<&str> {
    run.stdout
        .lines()
        .filter(|line| line.starts_with("row: "))
        .collect()
}

/// True when some fail row contains `needle`.
pub(crate) fn has_fail(run: &Run, needle: &str) -> bool {
    rows(run)
        .iter()
        .any(|line| line.contains("\"status\":\"fail\"") && line.contains(needle))
}

/// Require exit 0 with no fail rows; panic with full output otherwise.
pub(crate) fn assert_clean(run: &Run) {
    assert_eq!(
        run.code, 0,
        "stdout:\n{}\nstderr:\n{}",
        run.stdout, run.stderr
    );
    assert!(
        !rows(run)
            .iter()
            .any(|line| line.contains("\"status\":\"fail\"")),
        "fail row present:\n{}",
        run.stdout
    );
}

/// Require nonzero exit plus a fail row containing `needle`.
pub(crate) fn assert_fail(run: &Run, needle: &str) {
    assert_ne!(run.code, 0, "expected nonzero:\n{}", run.stdout);
    assert!(has_fail(run, needle), "missing {needle}:\n{}", run.stdout);
}

/// Source and policy files shared by every complete fixture.
const FIXTURE_FILES: &[(&str, &str)] = &[
    (
        "crates/velnor-actions-mise/src/catalog.rs",
        include_str!("p12_catalog.txt"),
    ),
    (
        "crates/velnor-actions-mise/src/catalog_qualification_semver.rs",
        include_str!("p12_catalog_qualification_semver.txt"),
    ),
    (
        "crates/velnor-actions-mise/src/catalog_pins.rs",
        include_str!("p12_catalog_pins.txt"),
    ),
    (
        "crates/velnor-actions-mise/src/catalog_workloads.rs",
        include_str!("p12_catalog_workloads.txt"),
    ),
    (
        "crates/velnor-actions-mise/src/catalog_rust_desktop.rs",
        include_str!("p12_catalog_rust_desktop.txt"),
    ),
    (
        "crates/velnor-actions-mise/src/catalog_homebrew.rs",
        include_str!("p12_catalog_homebrew.txt"),
    ),
    (
        "crates/velnor-actions-mise/src/catalog_qualification.rs",
        include_str!("p12_catalog_qualification.txt"),
    ),
    (
        "crates/velnor-actions-mise/src/catalog_qualification_java.rs",
        include_str!("p12_catalog_qualification_java.txt"),
    ),
    (
        "crates/velnor-actions-mise/src/catalog_qualification_gradle.rs",
        include_str!("p12_catalog_qualification_gradle.txt"),
    ),
    (
        "crates/velnor-actions-mise/src/catalog_source_build_bootstrap.rs",
        include_str!("p12_catalog_source_build_bootstrap.txt"),
    ),
    (
        "crates/velnor-actions-mise/src/catalog_qualification_records.rs",
        include_str!("p12_catalog_qualification_records.txt"),
    ),
    (
        "crates/velnor-actions-mise/src/catalog_gradle.rs",
        include_str!("p12_catalog_gradle.txt"),
    ),
    (
        "crates/velnor-actions-actionlint/src/actions.rs",
        include_str!("p12_actions.txt"),
    ),
    (
        "crates/velnor-actions-actionlint/src/capabilities.rs",
        include_str!("p12_capabilities.txt"),
    ),
    (
        "crates/velnor-actions-actionlint/src/tools.rs",
        include_str!("p12_tools.txt"),
    ),
    (
        "crates/velnor-actions-actionlint/src/config.rs",
        include_str!("p12_config.txt"),
    ),
    (
        "crates/velnor-actions-workflow-renderer/src/render.rs",
        include_str!("p12_render.txt"),
    ),
    (
        ".velnor/version-policy.toml",
        include_str!("p12_policy.toml"),
    ),
    ("Cargo.toml", include_str!("p12_workspace.toml")),
    ("crates/aaa/Cargo.toml", include_str!("p12_aaa.toml")),
    ("crates/bbb/Cargo.toml", include_str!("p12_bbb.toml")),
    ("Cargo.lock", include_str!("p12_lock.toml")),
];

/// A complete supported inventory tree; the gate must pass on it.
pub(crate) fn passing(prefix: &str) -> Result<Fixture, Box<dyn Error>> {
    let dir = crate::impl_cli_tmp::fresh_tempdir(prefix)?;
    for &(rel, body) in FIXTURE_FILES {
        write(&dir, rel, body)?;
    }
    let inventory = include_str!("p12_inventory.json").replace("{CHECKED}", &days_iso(0)?);
    write(&dir, ".velnor/freshness-inventory.json", &inventory)?;
    write(&dir, "deny.toml", "[advisories]\nignore = []\n")?;
    write(
        &dir,
        ".cargo/mutants.toml",
        "# pinned: cargo-mutants = \"27.1.0\"\nexamine_globs = [\n    \"crates/aaa/Cargo.toml\",\n]\n",
    )?;
    Ok(Fixture { dir })
}

/// Remove a fixture tree; cleanup must never fail a test.
pub(crate) fn cleanup(fixture: &Fixture) {
    crate::impl_cli_tmp::cleanup(&fixture.dir);
}

/// (inventory source URL, canned file, canned body) for every probe row.
pub(crate) fn probe_rows() -> Vec<(&'static str, &'static str, &'static str)> {
    probe_data::probe_rows()
}
