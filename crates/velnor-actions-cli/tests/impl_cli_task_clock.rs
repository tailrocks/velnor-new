//! Actual-platform proof for the portable private task start operation.

use std::error::Error;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::impl_cli_gate::assert_identical;
use crate::impl_cli_tmp::{cleanup, code, fresh_tempdir, spawn_isolated};

#[test]
fn task_start_requires_the_report_environment() -> Result<(), Box<dyn Error>> {
    let tmp = fresh_tempdir("clock-gate")?;
    let bare = spawn_isolated(&[], &[], &tmp)?;
    let runner = tmp.to_str().ok_or("non-Unicode temp path")?;
    for env in [
        vec![("VELNOR_INTERNAL_OP", "write-task-start-v1")],
        vec![
            ("VELNOR_INTERNAL_OP", "write-task-start-v1"),
            ("RUNNER_TEMP", runner),
        ],
        vec![
            ("VELNOR_INTERNAL_OP", "write-task-start-v1"),
            ("GITHUB_RUN_ID", "1"),
        ],
    ] {
        let gated = spawn_isolated(&[], &env, &tmp)?;
        assert_identical(&bare, &gated);
    }
    cleanup(&tmp);
    Ok(())
}

#[test]
fn task_start_is_decimal_millis_on_the_actual_host() -> Result<(), Box<dyn Error>> {
    let tmp = fresh_tempdir("clock-host")?;
    let runner = tmp.to_str().ok_or("non-Unicode temp path")?;
    let before = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis();
    let output = spawn_isolated(
        &[],
        &[
            ("VELNOR_INTERNAL_OP", "write-task-start-v1"),
            ("RUNNER_TEMP", runner),
            ("GITHUB_RUN_ID", "1"),
        ],
        &tmp,
    )?;
    let after = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis();
    assert_eq!(code(&output), 0, "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    let text = String::from_utf8(output.stdout)?;
    let stamp = text.strip_suffix('\n').ok_or("missing stamp newline")?;
    assert!(stamp.bytes().all(|byte| byte.is_ascii_digit()), "{stamp}");
    let millis = stamp.parse::<u64>()?;
    assert!((before..=after).contains(&u128::from(millis)), "{stamp}");
    cleanup(&tmp);
    Ok(())
}

#[test]
#[cfg(target_os = "macos")]
fn bsd_date_is_not_a_millisecond_telemetry_source() -> Result<(), Box<dyn Error>> {
    let output = std::process::Command::new("/bin/date")
        .arg("+%s%3N")
        .output()?;
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout)?;
    assert!(text.trim().parse::<u64>().is_err(), "BSD date: {text}");
    Ok(())
}
