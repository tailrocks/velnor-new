// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn release_discovery_stops_when_main_advances() -> Result<(), Box<dyn Error>> {
    use std::{fs, process::Command};
    let dir = tempfile::tempdir()?;
    let sha = "a".repeat(40);
    write_fixture(dir.path(), &sha)?;
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let script = r#"sleep() { n=$(cat "$FIXTURE/elapsed"); n=$((n + 1)); echo "$n" > "$FIXTURE/elapsed"; if (( n == 1 )); then SECONDS=$((SECONDS + 5)); else SECONDS=$((SECONDS + 21000)); fi; }
gh() { case "$*" in
  *git/ref/heads/main*) echo main >> "$FIXTURE/calls"; n=$(cat "$FIXTURE/count"); n=$((n + 1)); echo "$n" > "$FIXTURE/count"; case "$MODE" in error) return 1;; malformed) echo '{}'; return;; already) value="$OTHER_SHA";; advance) if (( n >= 2 )); then value="$OTHER_SHA"; else value="$GITHUB_SHA"; fi;; after) if (( n >= 3 )); then value="$OTHER_SHA"; else value="$GITHUB_SHA"; fi;; after_artifact) if (( n >= 4 )); then value="$OTHER_SHA"; else value="$GITHUB_SHA"; fi;; *) value="$GITHUB_SHA";; esac; printf '{"ref":"refs/heads/main","object":{"type":"commit","sha":"%s"}}\n' "$value";;
  *workflows/ci.yml/runs*) echo workflow >> "$FIXTURE/calls"; if [[ "$MODE" == advance ]]; then cat "$FIXTURE/pending"; else cat "$FIXTURE/run"; fi;;
  *runs/31/artifacts*) echo artifact >> "$FIXTURE/calls"; cat "$FIXTURE/artifact";; *) return 1;; esac; }
source "$RESOLVER"
"#;
    for mode in [
        "stable",
        "already",
        "advance",
        "after",
        "after_artifact",
        "malformed",
        "error",
    ] {
        let output = dir.path().join("output");
        fs::write(&output, "")?;
        fs::write(dir.path().join("calls"), "")?;
        fs::write(dir.path().join("count"), "0")?;
        fs::write(dir.path().join("elapsed"), "0")?;
        let result = Command::new("bash")
            .arg("-c")
            .arg(script)
            .env("FIXTURE", dir.path())
            .env(
                "RESOLVER",
                root.join("scripts/resolve-runner-family-publication.sh"),
            )
            .env("MODE", mode)
            .env("GH_TOKEN", "fixture")
            .env("GITHUB_REPOSITORY", "tailrocks/velnor-new")
            .env("GITHUB_SHA", &sha)
            .env("OTHER_SHA", "b".repeat(40))
            .env("GITHUB_OUTPUT", &output)
            .env("VELNOR_FAMILY_DISCOVERY_PHASE", "producer")
            .env("VELNOR_FAMILY_DISCOVERY_TIMEOUT_SECONDS", "21000")
            .output()?;
        assert_eq!(
            result.status.success(),
            mode == "stable",
            "{mode}: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        let content = fs::read_to_string(output)?;
        let calls = fs::read_to_string(dir.path().join("calls"))?;
        if mode == "stable" {
            assert_eq!(
                content,
                "producer_run_id=31\nproducer_run_attempt=2\nproducer_artifact_id=53\n"
            );
            assert!(calls.contains("artifact\n"));
        } else {
            assert_eq!(content, "");
            assert_eq!(calls.contains("artifact\n"), mode == "after_artifact");
            if mode == "advance" {
                assert_eq!(fs::read_to_string(dir.path().join("elapsed"))?.trim(), "1");
            }
            if mode == "already" {
                assert!(!calls.contains("workflow\n"));
            }
        }
    }
    Ok(())
}

fn write_fixture(dir: &std::path::Path, sha: &str) -> Result<(), Box<dyn Error>> {
    use std::fs;
    let run = format!(
        r#"[{{"total_count":1,"workflow_runs":[{{"path":".github/workflows/ci.yml","event":"push","head_branch":"main","head_sha":"{sha}","head_repository":{{"full_name":"tailrocks/velnor-new"}},"id":31,"run_number":7,"run_attempt":2,"status":"completed","conclusion":"success"}}]}}]"#
    );
    fs::write(dir.join("run"), &run)?;
    fs::write(
        dir.join("pending"),
        run.replace("\"completed\"", "\"in_progress\""),
    )?;
    fs::write(
        dir.join("artifact"),
        format!(
            r#"[{{"total_count":1,"artifacts":[{{"id":53,"name":"image-assets-2","expired":false,"size_in_bytes":10,"workflow_run":{{"id":31,"head_sha":"{sha}","head_branch":"main"}}}}]}}]"#
        ),
    )?;
    Ok(())
}
