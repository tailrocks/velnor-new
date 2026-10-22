use std::error::Error;
use std::fs;
use std::path::Path;

use serde_json::{Value, json};
use tempfile::TempDir;
use velnor_actions_contract::ImagePlatform;
use velnor_actions_workflow_renderer::image_publish_scripts::download_script;

use super::{TestResult, sh, tools_present};

const ID: &str = "demo";
const SHA: &str = "1111111111111111111111111111111111111111";
const REPOSITORY: &str = "acme/widgets";

const FAKE_GH: &str = r#"
api_calls=0
gh() {
  case "$1" in
    api)
      api_calls=$((api_calls + 1))
      printf '%s\n' "$*" >> "$API_LOG"
      if [ "$api_calls" -eq "$API_FAIL_AT" ]; then return 31; fi
      cat "$FIXTURE_DIR/response-$api_calls.json"
      ;;
    run)
      printf '%s\n' "$*" >> "$DOWNLOAD_LOG"
      ;;
    *)
      return 99
      ;;
  esac
}
sleep() { printf '%s\n' "$1" >> "$SLEEP_LOG"; }"#;

struct Outcome {
    success: bool,
    stderr: String,
    script: String,
    api_calls: Vec<String>,
    sleeps: Vec<String>,
    downloads: Vec<String>,
}

fn run_download(responses: &[&str], fail_at: usize) -> Result<Outcome, Box<dyn Error>> {
    let temp = TempDir::new()?;
    let fixtures = temp.path().join("responses");
    fs::create_dir(&fixtures)?;
    for (index, response) in responses.iter().enumerate() {
        fs::write(
            fixtures.join(format!("response-{}.json", index + 1)),
            response,
        )?;
    }
    let api_log = temp.path().join("api.log");
    let sleep_log = temp.path().join("sleep.log");
    let download_log = temp.path().join("download.log");
    let fail_at = fail_at.to_string();
    let script = download_script(FAKE_GH, ID, "main", &[ImagePlatform::LinuxAmd64]);
    let output = sh(
        &script,
        temp.path(),
        &[
            ("RUNNER_TEMP", temp.path().to_str().ok_or("temp path")?),
            ("GITHUB_SHA", SHA),
            ("GITHUB_REPOSITORY", REPOSITORY),
            ("FIXTURE_DIR", fixtures.to_str().ok_or("fixture path")?),
            ("API_LOG", api_log.to_str().ok_or("api log path")?),
            ("SLEEP_LOG", sleep_log.to_str().ok_or("sleep log path")?),
            (
                "DOWNLOAD_LOG",
                download_log.to_str().ok_or("download log path")?,
            ),
            ("API_FAIL_AT", &fail_at),
        ],
    )?;
    Ok(Outcome {
        success: output.status.success(),
        stderr: String::from_utf8(output.stderr)?,
        script,
        api_calls: lines_if_present(&api_log)?,
        sleeps: lines_if_present(&sleep_log)?,
        downloads: lines_if_present(&download_log)?,
    })
}

fn lines_if_present(path: &Path) -> Result<Vec<String>, Box<dyn Error>> {
    match fs::read_to_string(path) {
        Ok(contents) => Ok(contents.lines().map(str::to_owned).collect()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => Err(error.into()),
    }
}

fn run_record(
    id: u64,
    sha: &str,
    event: &str,
    conclusion: Option<&str>,
    status: &str,
    branch: &str,
    repository: &str,
) -> Value {
    json!({
        "id": id,
        "head_sha": sha,
        "event": event,
        "conclusion": conclusion,
        "status": status,
        "head_branch": branch,
        "head_repository": {"full_name": repository}
    })
}

fn page(runs: impl IntoIterator<Item = Value>) -> Value {
    json!({"workflow_runs": runs.into_iter().collect::<Vec<_>>()})
}

fn slurped(pages: impl IntoIterator<Item = Value>) -> Result<String, serde_json::Error> {
    serde_json::to_string(&pages.into_iter().collect::<Vec<_>>())
}

fn matching_run(id: u64) -> Value {
    run_record(
        id,
        SHA,
        "push",
        Some("success"),
        "completed",
        "main",
        REPOSITORY,
    )
}

#[test]
fn paginated_matching_run_is_selected_without_waiting() -> TestResult {
    if !tools_present() {
        return Ok(());
    }
    let response = slurped(vec![
        page(vec![run_record(
            4,
            SHA,
            "workflow_dispatch",
            Some("success"),
            "completed",
            "main",
            REPOSITORY,
        )]),
        page(vec![matching_run(9)]),
    ])?;
    let outcome = run_download(&[&response], 0)?;

    assert!(outcome.success, "{}\n{}", outcome.stderr, outcome.script);
    assert_eq!(outcome.api_calls.len(), 1);
    assert!(
        outcome.api_calls[0].contains(&format!(
            "api --paginate --slurp repos/{REPOSITORY}/actions/workflows/ci.yml/runs?head_sha={SHA}&per_page=100"
        )),
        "{}",
        outcome.api_calls[0]
    );
    assert!(!outcome.api_calls[0].contains("&event="));
    assert!(!outcome.api_calls[0].contains("&status="));
    assert_eq!(outcome.sleeps, Vec::<String>::new());
    assert_eq!(outcome.downloads.len(), 1);
    assert!(outcome.downloads[0].contains("--name velnor-image-demo-amd64"));
    Ok(())
}

#[test]
fn a_valid_raw_empty_listing_retries_then_downloads() -> TestResult {
    if !tools_present() {
        return Ok(());
    }
    let empty = slurped(vec![page(Vec::new())])?;
    let available = slurped(vec![page(vec![matching_run(9)])])?;
    let outcome = run_download(&[&empty, &available], 0)?;

    assert!(outcome.success, "{}", outcome.stderr);
    assert_eq!(outcome.api_calls.len(), 2);
    assert_eq!(outcome.sleeps, ["3"]);
    assert_eq!(outcome.downloads.len(), 1);
    Ok(())
}

#[test]
fn six_valid_raw_empty_listings_exhaust_after_the_shared_delays() -> TestResult {
    if !tools_present() {
        return Ok(());
    }
    let empty = slurped(vec![page(Vec::new())])?;
    let outcome = run_download(&[empty.as_str(); 6], 0)?;

    assert!(!outcome.success);
    assert!(
        outcome.stderr.contains("ci_run_not_found"),
        "{}",
        outcome.stderr
    );
    assert_eq!(outcome.api_calls.len(), 6);
    assert_eq!(outcome.sleeps, ["3", "6", "12", "24", "45"]);
    assert_eq!(outcome.downloads, Vec::<String>::new());
    Ok(())
}

#[test]
fn every_nonempty_mismatch_is_terminal_without_retry() -> TestResult {
    if !tools_present() {
        return Ok(());
    }
    let rejected = [
        (
            "manual event",
            run_record(
                1,
                SHA,
                "workflow_dispatch",
                Some("success"),
                "completed",
                "main",
                REPOSITORY,
            ),
        ),
        (
            "pending push",
            run_record(2, SHA, "push", None, "in_progress", "main", REPOSITORY),
        ),
        (
            "failed push",
            run_record(
                3,
                SHA,
                "push",
                Some("failure"),
                "completed",
                "main",
                REPOSITORY,
            ),
        ),
        (
            "wrong branch",
            run_record(
                4,
                SHA,
                "push",
                Some("success"),
                "completed",
                "topic",
                REPOSITORY,
            ),
        ),
        (
            "wrong repository",
            run_record(
                5,
                SHA,
                "push",
                Some("success"),
                "completed",
                "main",
                "acme/fork",
            ),
        ),
        (
            "wrong source",
            run_record(
                6,
                "2222222222222222222222222222222222222222",
                "push",
                Some("success"),
                "completed",
                "main",
                REPOSITORY,
            ),
        ),
    ];

    for (case, record) in rejected {
        assert_nonempty_mismatch_fails(case, record)?;
    }
    Ok(())
}

fn assert_nonempty_mismatch_fails(case: &str, record: Value) -> TestResult {
    let response = slurped([page([record])])?;
    let outcome = run_download(&[&response], 0)?;

    assert!(!outcome.success, "{case}: unexpected success");
    assert!(
        outcome.stderr.contains("ci_run_not_found"),
        "{case}: {}",
        outcome.stderr
    );
    assert_eq!(outcome.api_calls.len(), 1, "{case}");
    assert_eq!(outcome.sleeps, Vec::<String>::new(), "{case}");
    assert_eq!(outcome.downloads, Vec::<String>::new(), "{case}");
    Ok(())
}

#[test]
fn invalid_slurped_shapes_fail_before_retry() -> TestResult {
    if !tools_present() {
        return Ok(());
    }
    let invalid = [
        "not-json",
        "null",
        "[]",
        "[{}]",
        r#"[{"workflow_runs":null}]"#,
        r#"[{"workflow_runs":[]},null]"#,
    ];

    for response in invalid {
        let outcome = run_download(&[response], 0)?;

        assert!(!outcome.success, "{response}: unexpected success");
        assert_eq!(outcome.api_calls.len(), 1, "{response}");
        assert_eq!(outcome.sleeps, Vec::<String>::new(), "{response}");
        assert_eq!(outcome.downloads, Vec::<String>::new(), "{response}");
    }
    Ok(())
}

#[test]
fn api_failure_is_terminal_without_retry() -> TestResult {
    if !tools_present() {
        return Ok(());
    }
    let response = slurped(vec![page(vec![matching_run(9)])])?;
    let outcome = run_download(&[&response], 1)?;

    assert!(!outcome.success);
    assert_eq!(outcome.api_calls.len(), 1);
    assert_eq!(outcome.sleeps, Vec::<String>::new());
    assert_eq!(outcome.downloads, Vec::<String>::new());
    Ok(())
}
