//! T04/T07 parity goldens: plan/matrix/report bytes across refactors.
//!
//! Each `fixtures/parity/<case>/input` corpus repo regenerates `plan` text,
//! `generate` YAML, the `plan-v1` response (plan JSON with obligations plus
//! the matrix), the merge expected-report set, and one task report into a
//! temp dir; every artifact must byte-match its golden (modulo documented
//! normalization: repo path, head SHA, generator target/SHA, and the
//! input digests that embed the generator host triple). Malformed
//! cases goldenize the failure (exit code plus the machine-readable
//! `malformed_manifest:` token; the trailing Cargo diagnostic is toolchain
//! wording, asserted non-empty but not byte-pinned).
//!
//! Regenerate goldens only deliberately, at a known-good commit, with
//! `VELNOR_UPDATE_GOLDENS=1`; the default mode compares and never writes.

use std::collections::BTreeMap;
use std::error::Error;
use std::path::{Path, PathBuf};

use crate::impl_cli_tmp::{cleanup, code, commit_all, fresh_tempdir, git_init, spawn};

/// Success cases: full artifact goldens.
const CASES: [&str; 3] = ["minimal-cargo", "multi-crate", "ignored-stack"];
/// Failure cases: `plan` must fail with the malformed-manifest token.
const FAIL_CASES: [&str; 2] = ["malformed", "malformed-ignored"];
/// Fixed run key so plan IDs and report IDs are deterministic.
const RUN_KEY: &str = "r424242-a1";
/// Golden refresh switch; comparison is the default.
const UPDATE_ENV: &str = "VELNOR_UPDATE_GOLDENS";

/// Corpus directory for one case.
fn corpus(case: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/parity")
        .join(case)
}

/// Recursively copy `src` into existing `dst`.
fn copy_dir(src: &Path, dst: &Path) -> Result<(), Box<dyn Error>> {
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let target = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            std::fs::create_dir_all(&target)?;
            copy_dir(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

/// Fresh git checkout of one corpus case; returns the repo and its head SHA.
fn checkout(case: &str) -> Result<(PathBuf, String), Box<dyn Error>> {
    let dir = fresh_tempdir(&format!("parity-{case}"))?;
    let repo = dir.join("repo");
    std::fs::create_dir_all(&repo)?;
    copy_dir(&corpus(case).join("input"), &repo)?;
    git_init(&repo)?;
    let head = commit_all(&repo)?;
    Ok((repo, head))
}

/// Compare `actual` against the golden, or refresh it under [`UPDATE_ENV`].
fn check_golden(case: &str, name: &str, actual: &[u8]) -> Result<(), Box<dyn Error>> {
    let golden = corpus(case).join("expected").join(name);
    if std::env::var(UPDATE_ENV).is_ok_and(|value| value == "1") {
        if let Some(parent) = golden.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&golden, actual)?;
        return Ok(());
    }
    let expected = std::fs::read(&golden)
        .map_err(|err| Box::<dyn Error>::from(format!("missing golden {case}/{name}: {err}")))?;
    if expected == actual {
        return Ok(());
    }
    let at = expected
        .iter()
        .zip(actual.iter())
        .position(|(left, right)| left != right)
        .unwrap_or_else(|| expected.len().min(actual.len()));
    Err(Box::<dyn Error>::from(format!(
        "golden mismatch {case}/{name}: expected {} bytes, actual {} bytes, first diff at {at}",
        expected.len(),
        actual.len()
    )))
}

/// `plan` stdout with the volatile repository path normalized.
fn normalized_plan(repo: &Path, head: &str, stdout: &[u8]) -> Result<Vec<u8>, Box<dyn Error>> {
    let text = String::from_utf8_lossy(stdout).into_owned();
    let canonical = repo.canonicalize().unwrap_or_else(|_| repo.to_path_buf());
    let repo_line = format!("Repository: {}", canonical.display());
    if !text.contains(&repo_line) {
        return Err("plan lacks the Repository line".into());
    }
    Ok(text
        .replace(&repo_line, "Repository: <repo>")
        .replace(head, "<head>")
        .into_bytes())
}

use crate::impl_cli_parity_golden_normalize::normalized_response;

/// Merge expected-report set derived from the response (mirrors the
/// plan-derived expectation: report ID per matrix leg plus obligation
/// digest, never from an aggregate summary).
///
/// Report-id formula mirroring the contract (`task-{run}-{matrix_key}`
/// plus the 16 digest chars after the `b3-` tag); the golden pins it.
fn report_id_for(matrix_key: &str, digest: &str) -> Result<String, Box<dyn Error>> {
    let core = digest.get(3..19).ok_or("task_digest too short")?;
    Ok(format!("task-{RUN_KEY}-{matrix_key}-{core}"))
}

fn expected_set(bytes: &[u8]) -> Result<Vec<u8>, Box<dyn Error>> {
    let value: serde_json::Value = serde_json::from_slice(bytes)?;
    let plan = value.get("plan").ok_or("response lacks plan")?;
    let mut digests = BTreeMap::new();
    let obligations = plan
        .get("obligations")
        .and_then(serde_json::Value::as_array)
        .ok_or("plan lacks obligations")?;
    for obligation in obligations {
        let id = obligation
            .get("task_id")
            .and_then(serde_json::Value::as_str)
            .ok_or("obligation lacks task_id")?;
        let digest = obligation
            .get("task_digest")
            .and_then(serde_json::Value::as_str)
            .ok_or("obligation lacks task_digest")?;
        digests.insert(id, digest);
    }
    let mut expected: BTreeMap<String, (String, String)> = BTreeMap::new();
    let entries = plan
        .pointer("/matrix/include")
        .and_then(serde_json::Value::as_array)
        .ok_or("plan lacks matrix.include")?;
    for entry in entries {
        let matrix_key = entry
            .get("matrix_key")
            .and_then(serde_json::Value::as_str)
            .ok_or("entry lacks matrix_key")?;
        let tasks = entry
            .get("execute_task_ids")
            .and_then(serde_json::Value::as_object)
            .ok_or("entry lacks execute_task_ids")?;
        for reference in tasks.values() {
            let mut ids = Vec::new();
            if let Some(id) = reference.as_str() {
                ids.push(id);
            }
            if let Some(shards) = reference.as_array() {
                for shard in shards {
                    ids.push(shard.as_str().ok_or("shard id not a string")?);
                }
            }
            for id in ids {
                if let Some(digest) = digests.get(id) {
                    let report_id = report_id_for(matrix_key, digest)?;
                    expected.insert(report_id, (id.to_owned(), matrix_key.to_owned()));
                }
            }
        }
    }
    Ok(serde_json::to_string(&expected).map(String::into_bytes)?)
}

/// One validated task report for the first obligation, proving the
/// task-identity-to-report path (IDs, digests, matrix coordinates).
fn task_report(bytes: &[u8]) -> Result<Vec<u8>, Box<dyn Error>> {
    let value: serde_json::Value = serde_json::from_slice(bytes)?;
    let plan = value.get("plan").ok_or("response lacks plan")?;
    let obligations = plan
        .get("obligations")
        .and_then(serde_json::Value::as_array)
        .ok_or("plan lacks obligations")?;
    let obligation = obligations.first().ok_or("plan has no obligations")?;
    let task_id = obligation
        .get("task_id")
        .and_then(serde_json::Value::as_str)
        .ok_or("obligation lacks task_id")?;
    let task_digest = obligation
        .get("task_digest")
        .and_then(serde_json::Value::as_str)
        .ok_or("obligation lacks task_digest")?;
    let entries = plan
        .pointer("/matrix/include")
        .and_then(serde_json::Value::as_array)
        .ok_or("plan lacks matrix.include")?;
    let entry = entries
        .iter()
        .find(|entry| entry.get("task_id").and_then(serde_json::Value::as_str) == Some(task_id))
        .ok_or("no matrix entry for the first obligation")?;
    let matrix_id = entry
        .get("id")
        .and_then(serde_json::Value::as_str)
        .ok_or("entry lacks id")?;
    let matrix_key = entry
        .get("matrix_key")
        .and_then(serde_json::Value::as_str)
        .ok_or("entry lacks matrix_key")?;
    // Binary-only report bytes: struct field order with JSON-escaped
    // fragments (serde_json sorts map keys, so `json!` cannot reproduce
    // the wire order). Spellings mirror the report contract; the golden
    // pins them.
    let report_id = report_id_for(matrix_key, task_digest)?;
    let quoted = |raw: &str| -> Result<String, Box<dyn Error>> { Ok(serde_json::to_string(raw)?) };
    let out = format!(
        "{{\"schema\":1,\"task_report_id\":{},\"run_key\":{},\"event\":\"local\",\"trust\":\"pr\",\"matrix_id\":{},\"matrix_key\":{},\"task_id\":{},\"task_digest\":{},\"status\":\"executed\",\"cache\":{{\"layer\":\"sources\",\"key\":\"parity-fixed-key\",\"result\":\"miss\",\"miss_reason\":\"no_entry\"}},\"exit_code\":0,\"duration_ms\":7,\"outputs\":[]}}",
        quoted(&report_id)?,
        quoted(RUN_KEY)?,
        quoted(matrix_id)?,
        quoted(matrix_key)?,
        quoted(task_id)?,
        quoted(task_digest)?,
    );
    Ok(out.into_bytes())
}

/// Run `plan-v1` through the binary's internal dispatch; return the response.
fn plan_v1(repo: &Path, head: &str) -> Result<Vec<u8>, Box<dyn Error>> {
    let internal = repo.join("parity-internal");
    std::fs::create_dir_all(&internal)?;
    let request = internal.join("plan-v1-request.json");
    std::fs::write(
        &request,
        serde_json::json!({
            "schema": 1,
            "run_key": RUN_KEY,
            "base": null,
            "head": head,
            "event": "local",
        })
        .to_string(),
    )?;
    let runner_temp = repo.join("runner-temp");
    std::fs::create_dir_all(&runner_temp)?;
    let github_output = internal.join("github-output.txt");
    std::fs::write(&github_output, "")?;
    let output = spawn(
        &[],
        &[
            ("VELNOR_INTERNAL_OP", "plan-v1"),
            (
                "VELNOR_REQUEST_FILE",
                request.to_str().ok_or("non-utf8 request path")?,
            ),
            (
                "RUNNER_TEMP",
                runner_temp.to_str().ok_or("non-utf8 temp path")?,
            ),
            (
                "GITHUB_OUTPUT",
                github_output.to_str().ok_or("non-utf8 output path")?,
            ),
        ],
        repo,
    )?;
    if code(&output) != 0 {
        return Err(format!("plan-v1 failed: {:?}", output.stderr).into());
    }
    Ok(std::fs::read(internal.join("plan-v1-response.json"))?)
}

/// Full artifact goldens for one success case.
fn check_case(case: &str) -> Result<(), Box<dyn Error>> {
    let (repo, head) = checkout(case)?;
    let plan = spawn(&["plan"], &[], &repo)?;
    if code(&plan) != 0 {
        return Err(format!("{case}: plan failed: {:?}", plan.stderr).into());
    }
    if !plan.stderr.is_empty() {
        return Err(format!("{case}: plan stderr not empty: {:?}", plan.stderr).into());
    }
    check_golden(
        case,
        "plan.txt",
        &normalized_plan(&repo, &head, &plan.stdout)?,
    )?;
    let preview = repo.parent().ok_or("repo lacks a parent")?.join("preview");
    let generate = spawn(
        &[
            "generate",
            "--output-dir",
            preview.to_str().ok_or("non-utf8 preview path")?,
        ],
        &[],
        &repo,
    )?;
    if code(&generate) != 0 {
        return Err(format!("{case}: generate failed: {:?}", generate.stderr).into());
    }
    check_golden(
        case,
        "ci.yml",
        &std::fs::read(preview.join(".github/workflows/ci.yml"))?,
    )?;
    check_golden(
        case,
        "actionlint.yaml",
        &std::fs::read(preview.join(".github/actionlint.yaml"))?,
    )?;
    let response = plan_v1(&repo, &head)?;
    check_golden(
        case,
        "plan-v1.json",
        &normalized_response(&repo, &head, &response)?,
    )?;
    check_golden(case, "expected-set.json", &expected_set(&response)?)?;
    if case != "ignored-stack" {
        check_golden(case, "task-report.json", &task_report(&response)?)?;
    }
    cleanup(repo.parent().ok_or("tempdir lacks a parent")?);
    Ok(())
}

/// Machine-readable failure token for one malformed case.
fn check_fail(case: &str) -> Result<(), Box<dyn Error>> {
    let (repo, _) = checkout(case)?;
    let plan = spawn(&["plan"], &[], &repo)?;
    let stderr = String::from_utf8_lossy(&plan.stderr).into_owned();
    let token = "malformed_manifest:Cargo.toml: ";
    let Some(rest) = stderr.split_once(token).map(|(_, tail)| tail) else {
        return Err(format!("{case}: plan stderr lacks the token: {stderr:?}").into());
    };
    if rest.trim().is_empty() {
        return Err(format!("{case}: malformed diagnostic is empty").into());
    }
    let normalized = stderr.replacen(rest, "<cargo-diagnostic>", 1);
    check_golden(case, "plan.exit", code(&plan).to_string().as_bytes())?;
    check_golden(case, "plan.stderr.txt", normalized.as_bytes())?;
    cleanup(repo.parent().ok_or("tempdir lacks a parent")?);
    Ok(())
}

#[test]
fn parity_artifacts_match_goldens() -> Result<(), Box<dyn Error>> {
    for case in CASES {
        check_case(case)?;
    }
    Ok(())
}

#[test]
fn parity_malformed_fails_with_token() -> Result<(), Box<dyn Error>> {
    for case in FAIL_CASES {
        check_fail(case)?;
    }
    Ok(())
}
