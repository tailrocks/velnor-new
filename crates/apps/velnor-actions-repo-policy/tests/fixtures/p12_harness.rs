//! P12 fixture harness: minimal repo roots plus script-runner helpers.
//!
//! Each builder returns a self-contained tree under a fresh tempdir that
//! `scripts/check-freshness.sh --root` validates. The passing root uses
//! the real reviewed pin values as fixture data; failing roots mutate one
//! aspect each. Date helpers derive "today" from the system clock so
//! evidence windows stay deterministic without external crates.

use std::error::Error;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// Monotonic counter keeping tempdir names unique within one test binary.
static COUNTER: AtomicU64 = AtomicU64::new(0);

/// Create a fresh unique directory under the system temp dir.
fn fresh_tempdir(prefix: &str) -> Result<PathBuf, Box<dyn Error>> {
    let id = COUNTER.fetch_add(1, Ordering::SeqCst);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| error.to_string())?
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "velnor-policy-{prefix}-{}-{id}-{nanos}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Best-effort tempdir removal; cleanup must never fail a test.
fn remove_dir(dir: &Path) {
    drop(std::fs::remove_dir_all(dir));
}

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
    Ok(date_iso(days))
}

/// UTC timestamp relative to the current instant, formatted for inventory evidence.
pub(crate) fn timestamp_iso(offset_seconds: i64) -> Result<String, Box<dyn Error>> {
    let epoch_seconds = i64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|err| err.to_string())?
            .as_secs(),
    )
    .map_err(|err| err.to_string())?;
    let seconds = epoch_seconds
        .checked_add(offset_seconds)
        .ok_or_else(|| "timestamp offset overflow".to_owned())?;
    let day_seconds = seconds.rem_euclid(86_400);
    let date = date_iso(seconds.div_euclid(86_400));
    let hour = day_seconds / 3_600;
    let minute = (day_seconds % 3_600) / 60;
    let second = day_seconds % 60;
    Ok(format!("{date}T{hour:02}:{minute:02}:{second:02}Z"))
}

fn date_iso(days: i64) -> String {
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
    format!("{year:04}-{month:02}-{day:02}")
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

/// A complete supported inventory tree; the gate must pass on it.
pub(crate) fn passing(prefix: &str) -> Result<Fixture, Box<dyn Error>> {
    let dir = fresh_tempdir(prefix)?;
    for (rel, body) in [
        (
            "crates/adapters/velnor-actions-mise-catalog/src/catalog.rs",
            include_str!("p12_catalog.txt"),
        ),
        (
            "crates/adapters/velnor-actions-actionlint/src/actions.rs",
            include_str!("p12_actions.txt"),
        ),
        (
            "crates/adapters/velnor-actions-actionlint/src/capabilities.rs",
            include_str!("p12_capabilities.txt"),
        ),
        (
            "crates/adapters/velnor-actions-actionlint/src/tools.rs",
            include_str!("p12_tools.txt"),
        ),
        (
            "crates/adapters/velnor-actions-actionlint/src/config.rs",
            include_str!("p12_config.txt"),
        ),
        (
            "crates/services/velnor-actions-workflow-renderer/src/render.rs",
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
    ] {
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

/// Add an excluded nested workspace with a dependency unique to its lock.
pub(crate) fn add_nested_workspace(fixture: &Fixture) -> Result<(), Box<dyn Error>> {
    mutate(
        &fixture.dir,
        "Cargo.toml",
        "[workspace]\n",
        "[workspace]\nexclude = [\"crates/runner\"]\n",
    )?;
    write(
        &fixture.dir,
        "crates/runner/Cargo.toml",
        "[workspace]\nmembers = [\"crates/worker\"]\nresolver = \"3\"\n",
    )?;
    write(
        &fixture.dir,
        "crates/runner/crates/worker/Cargo.toml",
        "[package]\nname = \"worker\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\nrunneronly = \"=1.2.3\"\n",
    )?;
    write(
        &fixture.dir,
        "crates/runner/Cargo.lock",
        "version = 4\n\n[[package]]\nname = \"worker\"\nversion = \"0.1.0\"\ndependencies = [\"runneronly\"]\n\n[[package]]\nname = \"runneronly\"\nversion = \"1.2.3\"\nsource = \"registry+https://github.com/rust-lang/crates.io-index\"\n",
    )?;
    write(
        &fixture.dir,
        "crates/runner/deny.toml",
        "[advisories]\nignore = []\n",
    )?;
    Ok(())
}

/// Remove a fixture tree; cleanup must never fail a test.
pub(crate) fn cleanup(fixture: &Fixture) {
    remove_dir(&fixture.dir);
}

const TOOL_PROBE_ROWS: &[(&str, &str, &str)] = &[
    (
        "https://api.github.com/repos/jdx/mise/releases/latest",
        "mise.json",
        "{\"tag_name\": \"v2026.9.16\"}",
    ),
    (
        "https://static.rust-lang.org/dist/channel-rust-stable.toml",
        "rust.toml",
        "[pkg.rust]\nversion = \"1.98.1 (48a229cea 2026-09-01)\"\n",
    ),
    (
        "https://api.github.com/repos/jdx/mr-boxington/releases/latest",
        "mbx.json",
        "[{\"tag_name\": \"v1.20.0-beta\", \"prerelease\": true}, {\"tag_name\": \"v1.19.0\"}]",
    ),
    (
        "https://api.github.com/repos/cli/cli/releases/latest",
        "gh.json",
        "{\"tag_name\": \"v2.101.0\"}",
    ),
    (
        "https://api.github.com/repos/rhysd/actionlint/releases/latest",
        "actionlint.json",
        "{\"tag_name\": \"v1.7.12\"}",
    ),
    (
        "https://api.github.com/repos/koalaman/shellcheck/releases/latest",
        "shellcheck.json",
        "{\"tag_name\": \"v0.11.0\"}",
    ),
    (
        "https://api.github.com/repos/zizmorcore/zizmor/releases/latest",
        "zizmor.json",
        "{\"tag_name\": \"v1.30.1\"}",
    ),
    (
        "https://crates.io/api/v1/crates/cargo-nextest",
        "nextest.json",
        "{\"crate\": {\"max_version\": \"0.9.148\"}}",
    ),
    (
        "https://api.github.com/repos/opentofu/opentofu/releases/latest",
        "opentofu.json",
        "{\"tag_name\": \"v1.13.1\"}",
    ),
];

const ACTION_PROBE_ROWS: &[(&str, &str, &str)] = &[
    (
        "https://api.github.com/repos/Swatinem/rust-cache/tags",
        "rust-cache.json",
        "[{\"name\": \"v2.9.2\"}]",
    ),
    (
        "https://api.github.com/repos/jdx/mise-action/releases/latest",
        "mise-action.json",
        "{\"tag_name\": \"v4.3.0\"}",
    ),
    (
        "https://api.github.com/repos/actions/checkout/releases/latest",
        "checkout.json",
        "{\"tag_name\": \"v7.0.1\"}",
    ),
    (
        "https://api.github.com/repos/actions/download-artifact/releases/latest",
        "download.json",
        "{\"tag_name\": \"v8.0.1\"}",
    ),
    (
        "https://api.github.com/repos/actions/upload-artifact/releases/latest",
        "upload.json",
        "{\"tag_name\": \"v7.0.1\"}",
    ),
    (
        "https://api.github.com/repos/actions/cache/releases/latest",
        "cache.json",
        "{\"tag_name\": \"v6.1.0\"}",
    ),
    (
        "https://api.github.com/repos/jdx/mr-boxington-action/releases",
        "mbx-action.json",
        "[{\"tag_name\": \"v1.5.0\"}]",
    ),
    (
        "https://api.github.com/repos/asamarts/alint/releases/latest",
        "alint.json",
        "{\"tag_name\": \"v0.16.1\"}",
    ),
];

/// (inventory source URL, canned file, canned body) for every probe row.
pub(crate) fn probe_rows() -> Vec<(&'static str, &'static str, &'static str)> {
    let mut rows = TOOL_PROBE_ROWS.to_vec();
    rows.extend_from_slice(ACTION_PROBE_ROWS);
    rows
}
