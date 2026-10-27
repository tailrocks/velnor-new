// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{
    error::Error,
    fs,
    os::unix::fs::PermissionsExt,
    path::Path,
    process::Command,
    thread,
    time::{Duration, Instant},
};

use serde_json::{Value, json};
use tempfile::TempDir;
use velnor_actions_mise::CheckDeadline;

const REPOSITORY: &str = "test-owner/test-repo";
const ID: u64 = 817_263;
const A: &str = "a111111111111111111111111111111111111111";
const B: &str = "b222222222222222222222222222222222222222";
pub(crate) const EXPECTED_TIP: &str = "c333333333333333333333333333333333333333";
const C: &str = EXPECTED_TIP;

pub(crate) fn run(case: &str) -> Result<Result<String, String>, Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    setup(&temp, case)?;
    let root = temp.path().canonicalize()?;
    let mut command = Command::new(std::env::current_exe()?);
    let mut child = command
        .args(["--exact", "public_source_lineage_child", "--nocapture"])
        .env_clear()
        .env(
            "PATH",
            format!("{}:/usr/bin:/bin", root.join("bin").display()),
        )
        .env("HOME", root.join("home"))
        .env("GH_TOKEN", fixture_token(&root))
        .env("VELNOR_LINEAGE_CHILD", "1")
        .env("VELNOR_LINEAGE_CASE", case)
        .env("VELNOR_LINEAGE_ROOT", &root)
        .env("VELNOR_LINEAGE_RESULT", root.join("result.json"))
        .spawn()?;
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if started.elapsed() > Duration::from_secs(30) {
            child.kill()?;
            let _ = child.wait()?;
            return Err("lineage child exceeded 30 seconds".into());
        }
        thread::sleep(Duration::from_millis(10));
    };
    if !status.success() {
        return Err(format!("child failed: {status}").into());
    }
    let result: Value = serde_json::from_slice(&fs::read(root.join("result.json"))?)?;
    if result["ok"] == true {
        Ok(Ok(result["tip"].as_str().unwrap_or_default().to_owned()))
    } else {
        Ok(Err(result["error"]
            .as_str()
            .unwrap_or("unknown")
            .to_owned()))
    }
}

pub(crate) fn call_api() -> Result<String, String> {
    let root = std::env::var("VELNOR_LINEAGE_ROOT").map_err(|_| "fixture root missing")?;
    let path = Path::new(&root);
    if !path.is_dir() || path.canonicalize().ok().as_deref() != Some(path) {
        return Err("fixture root invalid".to_owned());
    }
    if std::env::var("GH_TOKEN").ok().as_deref() != Some(fixture_token(path).as_str()) {
        return Err("fixture token missing".to_owned());
    }
    let case = std::env::var("VELNOR_LINEAGE_CASE").unwrap_or_default();
    let source = if case == "malformed-source-input" {
        "not-a-sha"
    } else {
        A
    };
    let repository_id = ID;
    let deadline =
        CheckDeadline::after(Duration::from_secs(20)).map_err(|error| format!("{error:?}"))?;
    velnor_actions_orchestrator::verify_public_protected_main_lineage(
        REPOSITORY,
        repository_id,
        "main",
        source,
        B,
        deadline,
    )
    .map_err(|error| format!("{error:?}"))
}

fn setup(temp: &TempDir, case: &str) -> Result<(), Box<dyn Error>> {
    let root = temp.path();
    fs::create_dir_all(root.join("bin"))?;
    fs::create_dir_all(root.join("home"))?;
    let script = root.join("bin/gh");
    fs::write(&script, GH)?;
    let mut perms = fs::metadata(&script)?.permissions();
    perms.set_mode(0o755);
    fs::set_permissions(script, perms)?;
    let mise = root.join("bin/mise");
    fs::write(
        &mise,
        "#!/bin/sh\n[ \"$1\" = --no-config ] && [ \"$2\" = --no-env ] && [ \"$3\" = --no-hooks ] && [ \"$4\" = exec ] || exit 91\n[ \"$5\" = gh@2.103.3 ] && [ \"$6\" = -- ] && [ \"$7\" = gh ] || exit 92\nshift 6\nexec \"$@\"\n",
    )?;
    let mut perms = fs::metadata(&mise)?.permissions();
    perms.set_mode(0o755);
    fs::set_permissions(mise, perms)?;
    for (name, value) in responses(case) {
        fs::write(root.join(name), value.to_string())?;
    }
    if case == "malformed-json" {
        fs::write(root.join("source-git.json"), "{\"sha\":")?;
        fs::write(root.join("source-rest.json"), "{\"sha\":")?;
    }
    Ok(())
}

fn fixture_token(root: &Path) -> String {
    format!(
        "fixture_token_{}",
        root.file_name().unwrap_or_default().to_string_lossy()
    )
}

fn responses(case: &str) -> Vec<(&'static str, Value)> {
    let mut responses = identity_responses(case);
    responses.extend(commit_responses(case));
    responses
}

fn identity_responses(case: &str) -> Vec<(&'static str, Value)> {
    let repo_id = if case == "wrong-repository-id" {
        ID + 1
    } else {
        ID
    };
    let repo_name = if case == "wrong-repository-name" {
        "other/repo"
    } else {
        REPOSITORY
    };
    let default_branch = if case == "wrong-default-branch" {
        "trunk"
    } else {
        "main"
    };
    let branch_name = if case == "wrong-branch-name" {
        "trunk"
    } else {
        "main"
    };
    let protected = case != "unprotected-branch";
    let tip = if case == "malformed-tip" { "BAD" } else { C };
    let branch_tip = if case == "moving-final-tip" {
        "d444444444444444444444444444444444444444"
    } else {
        tip
    };
    let branch_first = json!({"name":branch_name,"protected":protected,"commit":{"sha":tip}});
    let branch_second = json!({"name":"main","protected":true,"commit":{"sha":branch_tip}});
    let mut compare_ab = compare(A, B, 1);
    let mut compare_bc = compare(B, C, 2);
    match case {
        "wrong-base" => compare_ab["base_commit"]["sha"] = json!(B),
        "wrong-merge-base" => compare_ab["merge_base_commit"]["sha"] = json!(B),
        "wrong-status" => compare_ab["status"] = json!("behind"),
        "wrong-ahead-count" => compare_ab["ahead_by"] = json!(2),
        "zero-count" => {
            compare_ab["ahead_by"] = json!(0);
            compare_ab["total_commits"] = json!(0);
        }
        "excessive-count" => {
            compare_ab["ahead_by"] = json!(1_000_001);
            compare_ab["total_commits"] = json!(1_000_001);
        }
        "missing-compare-fields" => compare_ab = json!({"status":"ahead"}),
        "wrong-second-leg-base" => compare_bc["base_commit"]["sha"] = json!(C),
        "wrong-second-leg-status" => compare_bc["status"] = json!("behind"),
        "wrong-final-page-head" => compare_bc["commits"][0]["sha"] = json!(B),
        _ => {}
    }
    vec![
        (
            "repository.json",
            json!({"id":repo_id,"full_name":repo_name,"default_branch":default_branch}),
        ),
        ("branch-first.json", branch_first),
        ("branch-second.json", branch_second),
        ("compare-ab.json", compare_ab),
        ("compare-bc-first.json", compare_bc),
    ]
}

fn commit_responses(case: &str) -> Vec<(&'static str, Value)> {
    let source_large = case == "large-source-rest";
    let authority_large = case == "large-authority-rest";
    let tip_large = case == "large-tip-rest";
    let source_commit = commit(A, None, source_large);
    let authority_rest = commit(B, None, authority_large);
    let tip_commit = commit(C, None, tip_large);
    let mut parent = json!([{"sha":A}]);
    if case == "missing-parent" {
        parent = json!([]);
    }
    if case == "wrong-parent" {
        parent = json!([{"sha":C}]);
    }
    let git_authority = commit(B, Some(parent), case == "large-git-data-b-capped");
    let mut source_commit = source_commit;
    let mut authority_rest = authority_rest;
    let mut tip_commit = tip_commit;
    let mut source_git = commit(A, None, false);
    let mut authority_git = git_authority;
    let mut tip_git = commit(C, None, false);
    match case {
        "source-sha-mismatch" => {
            source_commit["sha"] = json!(C);
            source_git["sha"] = json!(C);
        }
        "authority-sha-mismatch" => {
            authority_rest["sha"] = json!(C);
            authority_git["sha"] = json!(C);
        }
        "tip-sha-mismatch" => {
            tip_commit["sha"] = json!(B);
            tip_git["sha"] = json!(B);
        }
        "malformed-json" => source_commit = json!({"sha":"broken"}),
        _ => {}
    }
    vec![
        ("source-rest.json", source_commit),
        ("source-git.json", source_git),
        ("authority-rest.json", authority_rest),
        ("tip-rest.json", tip_commit),
        ("tip-git.json", tip_git),
        ("authority-git.json", authority_git),
    ]
}

fn compare(base: &str, head: &str, count: u64) -> Value {
    json!({"base_commit":{"sha":base},"merge_base_commit":{"sha":base},"status":"ahead","ahead_by":count,"total_commits":count,"commits":[{"sha":head}]})
}

fn commit(sha: &str, parents: Option<Value>, large: bool) -> Value {
    let mut value = json!({"sha":sha,"tree":{"sha":"e555555555555555555555555555555555555555"},"parents":parents.unwrap_or(json!([]))});
    if large {
        value["padding"] = json!("x".repeat(1_100_000));
    }
    value
}

const GH: &str = r#"#!/bin/sh
[ "$#" -eq 10 ] && [ "$1" = api ] && [ "$2" = -H ] && [ "$3" = "Accept: application/vnd.github+json" ] || exit 83
[ "$4" = -H ] && [ "$5" = "X-GitHub-Api-Version: 2026-03-10" ] || exit 85
[ "$7" = -H ] && [ "$8" = "Cache-Control: no-cache, no-store" ] || exit 86
[ "$9" = -H ] && [ "${10}" = "Pragma: no-cache" ] || exit 87
route=$6
case "$route" in *\&_velnor_read_nonce=*) route=${route%\&_velnor_read_nonce=*};; *\?_velnor_read_nonce=*) route=${route%\?_velnor_read_nonce=*};; esac
case "$route" in
  repos/test-owner/test-repo) file=repository.json ;;
  repos/test-owner/test-repo/branches/main) count=$(cat "$VELNOR_LINEAGE_ROOT/branch-count" 2>/dev/null || echo 0); count=$((count+1)); echo "$count" > "$VELNOR_LINEAGE_ROOT/branch-count"; if [ "$count" -eq 1 ]; then file=branch-first.json; else file=branch-second.json; fi ;;
  repos/test-owner/test-repo/commits/*) sha=${route##*/}; case "$sha" in a*) file=source-rest.json;; b*) file=authority-rest.json;; c*) file=tip-rest.json;; esac ;;
  repos/test-owner/test-repo/git/commits/*) sha=${route##*/}; case "$sha" in a*) file=source-git.json;; b*) file=authority-git.json;; c*) file=tip-git.json;; esac ;;
  repos/test-owner/test-repo/compare/a*...b*\?per_page=1\&page=2) file=compare-ab.json ;;
  repos/test-owner/test-repo/compare/b*...c*\?per_page=1\&page=2) file=compare-bc-first.json ;;
  *) exit 84 ;;
esac
cat "$VELNOR_LINEAGE_ROOT/$file"
"#;
