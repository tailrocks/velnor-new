use std::process::Command;
use velnor_actions_workflow_renderer::quote_run_arg;

#[test]
fn parameter_fallback_quotes_preserve_posix_argv() -> Result<(), Box<dyn std::error::Error>> {
    const RUNNER: &str = "/tmp/Runner Space";
    const HOME: &str = "/tmp/Home Space";
    let cases = [
        (r#"${RUNNER_TEMP:-"a}b"}"#, Some(RUNNER), Some(HOME), RUNNER),
        (r#"${RUNNER_TEMP:-"a}b"}"#, None, Some(HOME), "a}b"),
        (r"${RUNNER_TEMP:-'a}b'}", Some(RUNNER), Some(HOME), RUNNER),
        (r"${RUNNER_TEMP:-'a}b'}", None, Some(HOME), "a}b"),
        (r"${RUNNER_TEMP:-a\}b}", Some(RUNNER), Some(HOME), RUNNER),
        (r"${RUNNER_TEMP:-a\}b}", None, Some(HOME), "a}b"),
        (r"${RUNNER_TEMP:-a{b}", Some(RUNNER), Some(HOME), RUNNER),
        (r"${RUNNER_TEMP:-a{b}", None, Some(HOME), "a{b"),
        (r"${RUNNER_TEMP:-a\ b}", Some(RUNNER), Some(HOME), RUNNER),
        (r"${RUNNER_TEMP:-a\ b}", None, Some(HOME), "a b"),
        (r"${RUNNER_TEMP:-a\\b}", None, Some(HOME), r"a\b"),
        (r"${RUNNER_TEMP:-a\\b}", Some(RUNNER), Some(HOME), RUNNER),
        (r"${RUNNER_TEMP:-\$HOME}", None, Some(HOME), "$HOME"),
        (r"${RUNNER_TEMP:-\$HOME}", Some(RUNNER), Some(HOME), RUNNER),
        (r"${RUNNER_TEMP:-'$HOME'}", None, Some(HOME), "$HOME"),
        (r"${RUNNER_TEMP:-'$HOME'}", Some(RUNNER), Some(HOME), RUNNER),
        (r"${RUNNER_TEMP:-'a b'}", None, Some(HOME), "a b"),
        (r#"${RUNNER_TEMP:-"$HOME"}"#, None, Some(HOME), HOME),
        (r"${RUNNER_TEMP:-${HOME:-a{b}}", None, Some(HOME), HOME),
        (r"${RUNNER_TEMP:-${HOME:-a{b}}", None, None, "a{b"),
        (r"${RUNNER_TEMP:-}", None, Some(HOME), ""),
        (r"${RUNNER_TEMP:-}", Some(""), Some(HOME), ""),
        (r"${RUNNER_TEMP:-}", Some(RUNNER), Some(HOME), RUNNER),
        ("$RUNNER_TEMP", None, Some(HOME), ""),
        ("$RUNNER_TEMP", Some(""), Some(HOME), ""),
        ("$RUNNER_TEMP", Some(RUNNER), Some(HOME), RUNNER),
    ];

    for (source, runner_temp, home, expected) in cases {
        let quoted = quote_run_arg(source)?;
        assert_eq!(
            shell_argv(&quoted, runner_temp, home)?,
            vec![expected],
            "source={source:?}, quoted={quoted:?}"
        );
    }
    Ok(())
}

#[test]
fn unsupported_parameter_expansions_fail_closed() {
    for source in [
        "$@",
        "${}",
        "${RUNNER_TEMP!}",
        "${1RUNNER_TEMP:-x}",
        "${RUNNER_TEMP:}",
        "${RUNNER_TEMP:-${HOME!}}",
        "${RUNNER_TEMP:-${}}",
        "${RUNNER_TEMP:-$@}",
        "${RUNNER_TEMP:-missing close",
        "${RUNNER_TEMP:-'missing quote}",
        "${RUNNER_TEMP:-a\\",
    ] {
        assert!(
            quote_run_arg(source).is_err(),
            "malformed expansion was accepted: {source:?}"
        );
    }
}

fn shell_argv(
    quoted: &str,
    runner_temp: Option<&str>,
    home: Option<&str>,
) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let script = format!("set -- {quoted}; printf '%s\\0' \"$@\"");
    let mut command = Command::new("/bin/sh");
    command.arg("-c").arg(script).env_clear();
    if let Some(value) = runner_temp {
        command.env("RUNNER_TEMP", value);
    }
    if let Some(value) = home {
        command.env("HOME", value);
    }
    let output = command.output()?;
    assert!(output.status.success(), "shell failed: {quoted:?}");
    if output.stdout.is_empty() {
        return Ok(Vec::new());
    }
    let body = output
        .stdout
        .strip_suffix(&[0])
        .ok_or_else(|| std::io::Error::other("missing argv terminator"))?;
    Ok(std::str::from_utf8(body)?
        .split('\0')
        .map(ToOwned::to_owned)
        .collect())
}
