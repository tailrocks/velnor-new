//! Nextest routing guard extraction and failure cases.

use std::error::Error;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn verify_local_nextest_route_rejects_missing_or_wrong_pins() -> Result<(), Box<dyn Error>> {
    let script = workspace_root().join("scripts/verify-local.sh");
    let source = fs::read_to_string(&script)?;
    let route_check = source
        .split("if ! nextest_route_is_pinned ")
        .nth(1)
        .and_then(|body| body.split("\nfi").next())
        .ok_or("Nextest version guard call is missing")?;
    assert!(
        route_check.contains("exit 1"),
        "version failures exit early"
    );

    let function = extract_shell_function(&source, "nextest_route_is_pinned")?;
    let executable = std::env::current_exe()?;
    let guard = format!(
        r#"
{function}
case "$1" in
  valid) nextest_route_is_pinned "$2" pinned pinned pinned ;;
  missing) ! nextest_route_is_pinned "" pinned pinned pinned ;;
  binary-version) ! nextest_route_is_pinned "$2" wrong pinned pinned ;;
  route-version) ! nextest_route_is_pinned "$2" pinned wrong pinned ;;
  *) exit 2 ;;
esac
"#
    );

    for case in ["valid", "missing", "binary-version", "route-version"] {
        let output = Command::new("bash")
            .arg("-c")
            .arg(&guard)
            .arg("verify-local-nextest-guard")
            .arg(case)
            .arg(&executable)
            .output()?;
        assert!(
            output.status.success(),
            "Nextest version guard rejected {case}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(())
}

/// Extract one complete shell function without external tools.
fn extract_shell_function<'a>(source: &'a str, name: &str) -> Result<&'a str, Box<dyn Error>> {
    let marker = format!("{name}() {{");
    let start = source
        .lines()
        .scan(0_usize, |offset, line| {
            let current = *offset;
            *offset += line.len() + 1;
            Some((current, line))
        })
        .find_map(|(offset, line)| (line == marker).then_some(offset))
        .ok_or("shell function definition is missing")?;
    let body = &source[start..];
    let end = body
        .find("\n}\n")
        .map(|offset| offset + 2)
        .or_else(|| body.ends_with("\n}").then_some(body.len()))
        .ok_or("shell function definition is not closed")?;
    Ok(&body[..end])
}

#[test]
fn verify_local_shell_function_extraction_fails_closed() {
    assert!(extract_shell_function("echo no function\n", "nextest_route_is_pinned").is_err());
    assert!(
        extract_shell_function(
            "nextest_route_is_pinned() {\n  true\n",
            "nextest_route_is_pinned"
        )
        .is_err()
    );
}

#[test]
fn verify_local_shell_function_extraction_accepts_closing_brace_at_eof()
-> Result<(), Box<dyn Error>> {
    let source = "nextest_route_is_pinned() {\n  true\n}";
    assert_eq!(
        extract_shell_function(source, "nextest_route_is_pinned")?,
        source
    );
    Ok(())
}
