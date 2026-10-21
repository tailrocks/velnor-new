//! P12 fixture harness: minimal repo roots plus script-runner helpers.
//!
//! Each builder returns a self-contained tree under a fresh tempdir that
//! `scripts/check-freshness.sh --root` validates. The passing root uses
//! an intentional historical pin baseline as fixture data; failing roots mutate one
//! aspect each. Date helpers derive "today" from the system clock so
//! evidence windows stay deterministic without external crates.

use std::error::Error;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

/// Self-contained fixture catalog; independent of current live tool pins.
pub(crate) const FIXTURE_CATALOG: &str = include_str!("p12_catalog.txt");

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
    /// Exact inventory timestamp written when the fixture was built.
    pub(crate) checked_at: String,
}

/// Days since the Unix epoch, plus `offset`, as `YYYY-MM-DD`.
pub(crate) fn days_iso(offset: i64) -> Result<String, Box<dyn Error>> {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|err| err.to_string())?
        .as_secs();
    let days = i64::try_from(secs / 86_400).map_err(|err| err.to_string())? + offset;
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
    let (h, m, s) = (
        day_seconds / 3_600,
        (day_seconds % 3_600) / 60,
        day_seconds % 60,
    );
    Ok(format!("{date}T{h:02}:{m:02}:{s:02}Z"))
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

/// Install a strict `mise` executable for script fallback tests. It checks the
/// exact policy-selected tool pins and command, then runs Cargo directly so
/// ambient user command wrappers cannot replace the test's toolchain.
pub(crate) fn use_fixture_mise(
    command: &mut Command,
    fixture: &Fixture,
) -> Result<(), Box<dyn Error>> {
    let bin = fixture.dir.join("fixture-bin");
    std::fs::create_dir_all(&bin)?;
    let mise = bin.join("mise");
    write(
        &fixture.dir,
        "fixture-bin/mise",
        "#!/bin/sh\nset -eu\n[ \"$#\" -eq 12 ] || exit 90\n[ \"$1\" = exec ] || exit 91\n[ \"$2\" = rust@1.99.1 ] || exit 92\n[ \"$3\" = mr-boxington@1.24.1 ] || exit 93\n[ \"$4\" = -- ] || exit 94\nshift 4\n[ \"$*\" = 'cargo run --quiet --locked -p velnor-actions-cli --bin velnor-actions' ] || exit 95\n[ \"$1\" = cargo ] || exit 96\nshift\nexec \"$VELNOR_TEST_CARGO\" \"$@\"\n",
    )?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = std::fs::metadata(&mise)?.permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&mise, permissions)?;
    }
    let mut paths = vec![bin];
    if let Some(path) = std::env::var_os("PATH") {
        paths.extend(std::env::split_paths(&path));
    }
    command
        .env("PATH", std::env::join_paths(paths)?)
        .env("VELNOR_TEST_CARGO", env!("CARGO"))
        .env("RUSTUP_TOOLCHAIN", "1.99.1");
    Ok(())
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
    command
        .arg(script)
        .arg("--root")
        .arg(dir)
        .args(extra)
        .env("VELNOR_ACTIONS_BIN", env!("CARGO_BIN_EXE_velnor-actions"));
    let output = command.output()?;
    Ok(Run {
        code: output.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    })
}

#[test]
fn relative_root_is_resolved_before_the_script_changes_directory() -> Result<(), Box<dyn Error>> {
    let _target_lock = crate::impl_cli_tmp::nested_target::lock_nested_cargo_target()?;
    let fixture = passing("p12-relative-root")?;
    let caller_dir = fixture.dir.parent().ok_or("fixture parent missing")?;
    let relative_root = fixture.dir.file_name().ok_or("fixture name missing")?;
    let script = crate::impl_repo_policy::repo_root().join("scripts/check-freshness.sh");
    let mut command = Command::new("bash");
    command
        .arg(script)
        .arg("--root")
        .arg(relative_root)
        .env(
            "CARGO_TARGET_DIR",
            crate::impl_cli_tmp::nested_target::nested_cargo_target_dir()?,
        )
        .env("CDPATH", caller_dir)
        .current_dir(caller_dir);
    use_fixture_mise(&mut command, &fixture)?;
    let output = command.output()?;
    let run = Run {
        code: output.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    };
    assert_clean(&run);
    cleanup(&fixture);
    Ok(())
}

#[test]
fn script_builds_cli_in_shared_nested_target() -> Result<(), Box<dyn Error>> {
    let _target_lock = crate::impl_cli_tmp::nested_target::lock_nested_cargo_target()?;
    let fixture = passing("p12-nested-target")?;
    let nested_target = crate::impl_cli_tmp::nested_target::nested_cargo_target_dir()?;
    let binary = nested_target.join("debug/velnor-actions");
    if binary.exists() {
        std::fs::remove_file(&binary)?;
    }
    let script = crate::impl_repo_policy::repo_root().join("scripts/check-freshness.sh");
    let mut command = Command::new("bash");
    command
        .arg(script)
        .arg("--root")
        .arg(&fixture.dir)
        .env("CARGO_TARGET_DIR", &nested_target)
        .env_remove("VELNOR_ACTIONS_BIN");
    use_fixture_mise(&mut command, &fixture)?;
    let output = command.output()?;
    let run = Run {
        code: output.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    };
    assert_clean(&run);
    assert!(
        binary.is_file(),
        "check-freshness Cargo output was not written under {}",
        nested_target.display()
    );
    cleanup(&fixture);
    Ok(())
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
    let dir = crate::impl_cli_tmp::fresh_tempdir(prefix)?;
    for (rel, body) in [
        ("crates/velnor-actions-mise/src/catalog.rs", FIXTURE_CATALOG),
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
            "crates/velnor-actions-workflow-renderer/src/steps.rs",
            include_str!("p12_steps.txt"),
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
    let checked_at = timestamp_iso(0)?;
    let inventory = include_str!("p12_inventory.json").replace("{CHECKED}", &checked_at);
    write(&dir, ".velnor/freshness-inventory.json", &inventory)?;
    write(&dir, "deny.toml", "[advisories]\nignore = []\n")?;
    write(
        &dir,
        ".cargo/mutants.toml",
        "# pinned: cargo-mutants = \"27.1.0\"\nexamine_globs = [\n    \"crates/aaa/Cargo.toml\",\n]\n",
    )?;
    Ok(Fixture { dir, checked_at })
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
    crate::impl_cli_tmp::cleanup(&fixture.dir);
}

const PROBE_DATA: &[&str] = &[
    "https://api.github.com/repos/jdx/mise/releases/latest\tmise.json\t{\"tag_name\": \"v2026.10.14\"}",
    "https://static.rust-lang.org/dist/channel-rust-stable.toml\trust.toml\t[pkg.rust]\nversion = \"1.99.1 (48a229cea 2026-09-01)\"\n",
    "https://api.github.com/repos/jdx/mr-boxington/releases/latest\tmbx.json\t[{\"tag_name\": \"v1.20.0-beta\", \"prerelease\": true}, {\"tag_name\": \"v1.24.1\"}]",
    "https://api.github.com/repos/cli/cli/releases/latest\tgh.json\t{\"tag_name\": \"v2.102.0\"}",
    "https://api.github.com/repos/rhysd/actionlint/releases/latest\tactionlint.json\t{\"tag_name\": \"v1.7.12\"}",
    "https://api.github.com/repos/jdx/jactionlint/releases/latest\tjactionlint.json\t{\"tag_name\": \"v2.3.0\"}",
    "https://api.github.com/repos/koalaman/shellcheck/releases/latest\tshellcheck.json\t{\"tag_name\": \"v0.11.0\"}",
    "https://api.github.com/repos/zizmorcore/zizmor/releases/latest\tzizmor.json\t{\"tag_name\": \"v1.31.1\"}",
    "https://crates.io/api/v1/crates/cargo-nextest\tnextest.json\t{\"crate\": {\"max_version\": \"0.9.148\"}}",
    "https://crates.io/api/v1/crates/release-plz\trelease-plz.json\t{\"crate\": {\"max_version\": \"0.3.172\"}}",
    "https://api.github.com/repos/opentofu/opentofu/releases/latest\topentofu.json\t{\"tag_name\": \"v1.13.1\"}",
    "https://api.github.com/repos/jdx/mise-action/releases/latest\tmise-action.json\t{\"tag_name\": \"v5.2.0\"}",
    "https://api.github.com/repos/actions/checkout/releases/latest\tcheckout.json\t{\"tag_name\": \"v7.0.1\"}",
    "https://api.github.com/repos/actions/download-artifact/releases/latest\tdownload.json\t{\"tag_name\": \"v8.0.1\"}",
    "https://api.github.com/repos/actions/upload-artifact/releases/latest\tupload.json\t{\"tag_name\": \"v7.0.1\"}",
    "https://api.github.com/repos/actions/cache/releases/latest\tcache.json\t{\"tag_name\": \"v6.1.0\"}",
    "https://api.github.com/repos/jdx/mr-boxington-action/releases\tmbx-action.json\t[{\"tag_name\": \"v1.5.0\"}]",
    "https://api.github.com/repos/asamarts/alint/releases/latest\talint.json\t{\"tag_name\": \"v0.19.0\"}",
    "https://api.github.com/repos/aws-actions/configure-aws-credentials/releases/latest\taws-credentials.json\t{\"tag_name\": \"v6.3.0\"}",
    "https://api.github.com/repos/google/go-containerregistry/releases/latest\tcrane.json\t{\"tag_name\": \"v0.22.1\"}",
    "https://api.github.com/repos/actions/attest/releases/latest\tattest.json\t{\"tag_name\": \"v4.2.2\"}",
    "https://api.github.com/repos/oven-sh/bun/releases/latest\tbun.json\t{\"tag_name\": \"bun-v1.4.3\"}",
    "https://api.github.com/repos/sigstore/cosign/releases/latest\tcosign.json\t{\"tag_name\": \"v3.1.3\"}",
    "https://api.github.com/repos/actions/attest-build-provenance/releases/latest\tattest-build-provenance.json\t{\"tag_name\": \"v4.2.2\"}",
    "https://api.github.com/repos/actions/create-github-app-token/releases/latest\tcreate-github-app-token.json\t{\"tag_name\": \"v3.3.0\"}",
    "https://api.github.com/repos/actions/github-script/releases/latest\tgithub-script.json\t{\"tag_name\": \"v9.0.0\"}",
];

/// (inventory source URL, canned file, canned body) for every probe row.
pub(crate) fn probe_rows() -> Vec<(&'static str, &'static str, &'static str)> {
    PROBE_DATA
        .iter()
        .map(|row| {
            let mut parts = row.split('\t');
            (
                parts.next().unwrap_or_default(),
                parts.next().unwrap_or_default(),
                parts.next().unwrap_or_default(),
            )
        })
        .collect()
}

#[path = "p12_harness_tests.rs"]
mod tests;
