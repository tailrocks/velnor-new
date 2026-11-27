// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::process::Command;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};

use crate::mise_world::{
    SHA_PLACEHOLDER, VERSION, World, assert_failed, job_fixture, real_action, sha256_hex,
};

#[test]
fn mise_action_runs_locked_install_with_store_data_dir() -> Result<()> {
    let world = World::new()?;
    world.install_mise()?;
    world.init_workspace(true)?;
    let job = world.with_final_script(
        &job_fixture("mise_action_ok"),
        "echo \"later=$MISE_CACHE_DIR $MISE_AUTO_INSTALL $MISE_TRUSTED_CONFIG_PATHS $MISE_YES $MISE_SHIMS_DIR\"",
    )?;
    let ran = world.run(&job, &[])?;
    assert!(ran.has_line("job succeeded"), "{}", ran.stdout);
    let calls = world.calls();
    assert_eq!(
        calls.first().map(|call| call.argv.as_str()),
        Some("config ls --json")
    );
    assert_eq!(world.mise_installs(), vec!["argv=install --locked"]);
    for call in &calls {
        let root = call.root();
        assert_eq!(
            call.env_value("MISE_DATA_DIR"),
            format!("{}/tools/data", world.store.display())
        );
        assert_eq!(
            call.env_value("MISE_CACHE_DIR"),
            format!("{}/cache", root.display())
        );
        assert_eq!(
            call.env_value("MISE_TRUSTED_CONFIG_PATHS"),
            format!("{}/config", root.display())
        );
    }
    let later = format!(
        "later={}/mise-cache false {} 1 {}/mise-shims",
        world.temp().display(),
        world.workspace().display(),
        world.temp().display()
    );
    assert!(ran.has_line(&later), "{later}\n{}", ran.stdout);
    Ok(())
}

#[test]
fn mise_action_exports_path_and_env() -> Result<()> {
    let world = World::new()?;
    let binary = world.install_mise()?;
    world.init_workspace(true)?;
    let ran = world.run(&job_fixture("mise_action_ok"), &[])?;
    for expected in [
        "foo=bar".to_owned(),
        "tooly-ran".to_owned(),
        format!("data={}/tools/data", world.store.display()),
        "secret=***".to_owned(),
        binary.display().to_string(),
        "job succeeded".to_owned(),
    ] {
        assert!(ran.has_line(&expected), "{expected}\n{}", ran.stdout);
    }
    assert!(!ran.stdout.contains("s3cr3t-value"), "{}", ran.stdout);
    Ok(())
}

#[test]
fn mise_action_masks_a_secret_that_contains_command_escapes() -> Result<()> {
    let world = World::new()?;
    world.install_mise()?;
    world.init_workspace(true)?;
    world.configure_double(&[("SECRET", "tok%25en%0Aline")])?;
    let ran = world.run(&job_fixture("mise_action_ok"), &[])?;
    assert!(ran.has_line("job succeeded"), "{}", ran.stdout);
    assert!(ran.has_line("secret=***"), "{}", ran.stdout);
    assert!(!ran.stdout.contains("tok%25en"), "{}", ran.stdout);
    Ok(())
}

#[test]
fn mise_action_rejects_sha256_mismatch() -> Result<()> {
    let world = World::new()?;
    let binary = world.install_mise()?;
    world.init_workspace(true)?;
    let ran = world.run(&job_fixture("mise_action_bad_sha"), &[])?;
    assert_failed(&ran, &format!("mise {VERSION} sha256 mismatch"));
    assert_eq!(world.mise_calls(), Vec::<String>::new());
    let matching = world.with_text(
        &job_fixture("mise_action_bad_sha"),
        &[(&"0".repeat(64), &sha256_hex(&binary)?)],
    )?;
    let ran = world.run(&matching, &[])?;
    assert!(ran.has_line("job succeeded"), "{}", ran.stdout);
    Ok(())
}

#[test]
fn mise_action_skips_sha_check_for_empty_value() -> Result<()> {
    let world = World::new()?;
    world.install_mise()?;
    world.init_workspace(true)?;
    let ran = world.run(&job_fixture("mise_action_ok"), &[])?;
    assert!(ran.has_line("job succeeded"), "{}", ran.stdout);
    assert_eq!(world.mise_installs().len(), 1);
    Ok(())
}

#[test]
fn mise_action_fails_closed_on_the_no_digest_sentinel() -> Result<()> {
    let world = World::new()?;
    world.install_mise()?;
    world.init_workspace(true)?;
    let sentinel = world.with_text(
        &job_fixture("mise_action_ok"),
        &[(
            "\"lit\": \"\"",
            "\"lit\": \"no-mise-digest-for-this-runner\"",
        )],
    )?;
    let ran = world.run(&sentinel, &[])?;
    assert_failed(&ran, &format!("mise {VERSION} sha256 mismatch"));
    assert_eq!(world.mise_calls(), Vec::<String>::new());
    Ok(())
}

#[test]
fn mise_action_prepends_tool_dirs_in_mise_order_without_duplicating_the_inherited_path()
-> Result<()> {
    let world = World::new()?;
    let binary = world.install_mise()?;
    world.init_workspace(true)?;
    let job = world.with_final_script(&job_fixture("mise_action_ok"), "echo \"path=$PATH\"")?;
    world.configure_double(&[("EXTRA_DIR", "/zz-extra")])?;
    let ran = world.run(&job, &[("PATH", "/zz-inherited:/usr/bin:/bin")])?;
    assert!(ran.has_line("job succeeded"), "{}", ran.stdout);
    let directory = world.binary_dir();
    let expected = format!(
        "path={}/tool-bin:/zz-extra:{}/mise-shims:{}:/zz-inherited:/usr/bin:/bin",
        directory.display(),
        world.temp().display(),
        binary
            .parent()
            .map_or_else(String::new, |dir| dir.display().to_string())
    );
    assert!(ran.has_line(&expected), "{expected}\n{}", ran.stdout);
    Ok(())
}

#[test]
fn mise_action_keeps_an_inherited_entry_that_mise_promotes() -> Result<()> {
    for no_paths in ["0", "1"] {
        let world = World::new()?;
        let binary = world.install_mise()?;
        world.init_workspace(true)?;
        let job = world.with_final_script(&job_fixture("mise_action_ok"), "echo \"path=$PATH\"")?;
        world.configure_double(&[("PROMOTE", "/usr/bin"), ("NO_PATHS", no_paths)])?;
        let ran = world.run(&job, &[("PATH", "/zz-inherited:/usr/bin:/bin")])?;
        assert!(ran.has_line("job succeeded"), "{no_paths}\n{}", ran.stdout);
        let expected = format!(
            "path={}/tool-bin:/usr/bin:{}/mise-shims:{}:/zz-inherited:/usr/bin:/bin",
            world.binary_dir().display(),
            world.temp().display(),
            binary
                .parent()
                .map_or_else(String::new, |dir| dir.display().to_string())
        );
        assert!(
            ran.has_line(&expected),
            "{no_paths}\n{expected}\n{}",
            ran.stdout
        );
    }
    Ok(())
}

#[test]
fn mise_action_promotes_an_entry_mise_drops_from_its_inherited_position() -> Result<()> {
    let world = World::new()?;
    let binary = world.install_mise()?;
    world.init_workspace(true)?;
    let job = world.with_final_script(&job_fixture("mise_action_ok"), "echo \"path=$PATH\"")?;
    world.configure_double(&[("PROMOTE", "/b"), ("DROP_PROMOTED", "1")])?;
    let ran = world.run(&job, &[("PATH", "/a:/b:/usr/bin:/bin")])?;
    assert!(ran.has_line("job succeeded"), "{}", ran.stdout);
    let expected = format!(
        "path={}/tool-bin:/b:{}/mise-shims:{}:/a:/b:/usr/bin:/bin",
        world.binary_dir().display(),
        world.temp().display(),
        binary
            .parent()
            .map_or_else(String::new, |dir| dir.display().to_string())
    );
    assert!(ran.has_line(&expected), "{expected}\n{}", ran.stdout);
    Ok(())
}

#[test]
fn mise_action_publishes_cache_hit_false_when_caching_is_disabled() -> Result<()> {
    let world = World::new()?;
    world.install_mise()?;
    world.init_workspace(true)?;
    let job = world.with_final_script(
        &job_fixture("mise_action_ok"),
        "echo \"hit=${{ steps.mise.outputs.cache-hit }}\"",
    )?;
    let ran = world.run(&job, &[])?;
    assert!(ran.has_line("hit=false"), "{}", ran.stdout);
    Ok(())
}

#[test]
fn mise_action_cancelled_during_the_path_probe_cancels_the_job() -> Result<()> {
    let world = World::new()?;
    world.install_mise()?;
    world.init_workspace(true)?;
    let hung = world.temp().join("paths-hung");
    let hung_text = hung.to_string_lossy().into_owned();
    let job = world.only_first_step(&job_fixture("mise_action_ok"))?;
    world.configure_double(&[("HANG_PATHS", &hung_text)])?;
    let runner = world.start(&job, &[])?;
    let deadline = Instant::now() + Duration::from_secs(10);
    while !hung.exists() {
        if Instant::now() > deadline {
            bail!("the path probe never started");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let signalled = Command::new("kill")
        .args(["-TERM", &runner.id().to_string()])
        .status()
        .context("run kill")?;
    assert!(signalled.success());
    let output = runner
        .wait_with_output()
        .context("wait for velnor-runner")?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.contains("job succeeded"), "{stdout}");
    assert!(stdout.contains("cancelled"), "{stdout}");
    Ok(())
}

#[test]
fn mise_action_puts_the_job_shims_dir_on_the_path() -> Result<()> {
    let world = World::new()?;
    world.install_mise()?;
    world.init_workspace(true)?;
    let job = world.with_final_script(&job_fixture("mise_action_ok"), "echo \"path=$PATH\"")?;
    let ran = world.run(&job, &[])?;
    let shims = format!("{}/mise-shims", world.temp().display());
    let path = ran
        .lines()
        .into_iter()
        .find_map(|line| line.strip_prefix("path="))
        .map_or_default(str::to_owned);
    assert!(path.split(':').any(|entry| entry == shims), "{path}");
    Ok(())
}

#[test]
fn mise_action_keeps_the_callers_mise_environment() -> Result<()> {
    let world = World::new()?;
    world.install_mise()?;
    world.init_workspace(true)?;
    let job = world.with_job_env(
        &job_fixture("mise_action_ok"),
        &[("MISE_YES", "0"), ("MISE_TRUSTED_CONFIG_PATHS", "/x")],
    )?;
    let job =
        world.with_final_script(&job, "echo \"later=$MISE_YES $MISE_TRUSTED_CONFIG_PATHS\"")?;
    let ran = world.run(&job, &[])?;
    assert!(ran.has_line("job succeeded"), "{}", ran.stdout);
    assert!(ran.has_line("later=0 /x"), "{}", ran.stdout);
    let calls = world.calls();
    assert!(calls.len() > 1, "{} calls", calls.len());
    for call in &calls {
        assert_eq!(call.env_value("MISE_YES"), "1", "{}", call.argv);
        assert_eq!(
            call.env_value("MISE_TRUSTED_CONFIG_PATHS"),
            format!("{}/config", call.root().display()),
            "{}",
            call.argv
        );
    }
    Ok(())
}

#[test]
fn mise_action_fails_when_binary_not_in_store() -> Result<()> {
    let world = World::new()?;
    world.init_workspace(true)?;
    let ran = world.run(&job_fixture("mise_action_ok"), &[])?;
    let expected = format!(
        "mise {VERSION} is not in the tools store at {}/mise; run the tools store task first",
        world.binary_dir().display()
    );
    assert_failed(&ran, &expected);
    assert_eq!(world.mise_calls(), Vec::<String>::new());
    Ok(())
}

#[test]
fn mise_action_reports_a_tool_missing_from_the_store() -> Result<()> {
    let world = World::new()?;
    world.install_mise()?;
    world.init_workspace(true)?;
    world.configure_double(&[("INSTALL_EXIT", "1")])?;
    let ran = world.run(&job_fixture("mise_action_missing_tool"), &[])?;
    assert!(
        ran.has_line("mise ERROR: tool is missing"),
        "{}",
        ran.stdout
    );
    assert_failed(&ran, "tool is not installed in the tools store");
    let calls = world.mise_calls();
    assert_eq!(world.mise_installs().len(), 1, "{calls:?}");
    assert!(
        !calls.iter().any(|call| call.starts_with("argv=env")),
        "{calls:?}"
    );
    Ok(())
}

#[test]
fn mise_action_rejects_untracked_mise_lock() -> Result<()> {
    let world = World::new()?;
    world.install_mise()?;
    world.init_workspace(false)?;
    let ran = world.run(&job_fixture("mise_action_ok"), &[])?;
    assert_failed(
        &ran,
        "mise policy: mise.lock is missing from the trusted commit or is not a regular file",
    );
    assert_eq!(world.mise_calls(), Vec::<String>::new());
    Ok(())
}

#[test]
fn mise_action_honours_install_args() -> Result<()> {
    let world = World::new()?;
    world.install_mise()?;
    world.init_workspace(true)?;
    let ran = world.run(&job_fixture("mise_action_install_args"), &[])?;
    assert!(ran.has_line("job succeeded"), "{}", ran.stdout);
    let installs = world.mise_installs();
    assert_eq!(
        installs,
        vec!["argv=install --locked jactionlint shellcheck node"]
    );
    Ok(())
}

#[test]
fn setup_velnor_expands_the_real_action_into_its_run_step() -> Result<()> {
    let world = World::new()?;
    let sha = world.publish_action(&real_action()?)?;
    let job = world.with_text(
        &job_fixture("setup_velnor_real_action"),
        &[(SHA_PLACEHOLDER, &sha)],
    )?;
    let ran = world.run(&job, &[])?;
    assert_failed(
        &ran,
        "::error title=setup-velnor::version 'x' must be MAJOR.MINOR.PATCH, digits only",
    );
    assert!(
        ran.has_line("setup-velnor step: Install velnor-ci"),
        "{}",
        ran.stdout
    );
    assert!(!ran.stdout.contains("unsupported"), "{}", ran.stdout);
    Ok(())
}
