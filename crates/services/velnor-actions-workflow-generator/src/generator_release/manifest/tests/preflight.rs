use super::super::tag_preflight_script;
use super::scratch;
use std::error::Error;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::Command;

#[test]
fn tag_preflight_accepts_only_confirmed_not_found_responses() -> Result<(), Box<dyn Error>> {
    for (case, accepted) in [
        ("confirmed-404", true),
        ("forbidden", false),
        ("network", false),
        ("malformed", false),
        ("missing-status", false),
    ] {
        let scratch = scratch()?;
        let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../..")
            .canonicalize()?;
        let gh = scratch.0.join("gh");
        fs::write(
            &gh,
            r#"#!/bin/sh
printf '%s\n' "$*" >> "$GH_CALLS"
case "$GH_CASE" in
  confirmed-404) printf 'HTTP/2 404\r\ncontent-type: application/json\r\n\r\n{"message":"Not Found","status":"404"}\n'; exit 1 ;;
  forbidden) printf 'HTTP/2 403\r\n\r\n{"message":"Forbidden","status":"403"}\n'; exit 1 ;;
  network) exit 22 ;;
  malformed) printf 'not-an-http-response\n\nnot-json\n'; exit 1 ;;
  missing-status) printf 'HTTP/2 404\r\n\r\n{"message":"Not Found"}\n'; exit 1 ;;
esac
"#,
        )?;
        let mut permissions = fs::metadata(&gh)?.permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&gh, permissions)?;
        let mut paths = vec![scratch.0.clone()];
        paths.extend(std::env::split_paths(
            &std::env::var_os("PATH").ok_or("missing PATH")?,
        ));
        let path = std::env::join_paths(paths)?;
        let output = Command::new("bash")
            .arg("-c")
            .arg(tag_preflight_script())
            .current_dir(workspace)
            .env("PATH", path)
            .env("GH_CALLS", scratch.0.join("calls"))
            .env("GH_CASE", case)
            .env("GITHUB_REPOSITORY", "tailrocks/velnor-new")
            .output()?;
        let calls = fs::read_to_string(scratch.0.join("calls"))?;
        assert_eq!(
            output.status.success(),
            accepted,
            "case {case}; stderr: {}; calls: {calls}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            calls.lines().next(),
            Some("api --include repos/tailrocks/velnor-new/git/ref/tags/v0.1.7")
        );
        assert_eq!(calls.lines().count(), if accepted { 2 } else { 1 });
        if accepted {
            assert_eq!(
                calls.lines().nth(1),
                Some("api --include repos/tailrocks/velnor-new/releases/tags/v0.1.7")
            );
        }
    }
    Ok(())
}
