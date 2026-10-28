// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{CODEX, Scenario, event_action, request_body, request_time, sonar_edit};
use serde_json::{Value, json};
use std::{
    error::Error,
    fs, io,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Output},
};

#[path = "review_completion_fixture/comment_editor.rs"]
mod comment_editor;
#[path = "review_completion_fixture/dispositions.rs"]
mod dispositions;
#[path = "review_completion_fixture/review_content.rs"]
mod review_content;

const REPOSITORY: &str = "jackin-project/homebrew-tap";
const REPOSITORY_ID: u64 = 1_200_197_694;
const NUMBER: u64 = 514;
const HEAD: &str = "07c2e751b1720fddb8c02649c08792eaa53e7bbe";
const BASE: &str = "921858f118256e16d3dc312c407814ab0fd8ef52";
const SONAR: &str = "## [![Quality Gate Passed](https://sonarsource.github.io/sonarcloud-github-static-resources/v2/checks/QualityGateBadge/qg-passed-20px.png 'Quality Gate Passed')](https://sonarcloud.io/dashboard?id=jackin-project_homebrew-tap&pullRequest=514) **Quality Gate passed**  \nIssues  \n![](https://sonarsource.github.io/sonarcloud-github-static-resources/v2/common/passed-16px.png '') [0 New issues](https://sonarcloud.io/project/issues?id=jackin-project_homebrew-tap&pullRequest=514&issueStatuses=OPEN,CONFIRMED&sinceLeakPeriod=true)  \n![](https://sonarsource.github.io/sonarcloud-github-static-resources/v2/common/accepted-16px.png '') [0 Accepted issues](https://sonarcloud.io/project/issues?id=jackin-project_homebrew-tap&pullRequest=514&issueStatuses=ACCEPTED)\n\nMeasures  \n![](https://sonarsource.github.io/sonarcloud-github-static-resources/v2/common/passed-16px.png '') [0 Security Hotspots](https://sonarcloud.io/project/security_hotspots?id=jackin-project_homebrew-tap&pullRequest=514&issueStatuses=OPEN,CONFIRMED&sinceLeakPeriod=true)  \n![](https://sonarsource.github.io/sonarcloud-github-static-resources/v2/common/passed-16px.png '') [0.0% Coverage on New Code](https://sonarcloud.io/component_measures?id=jackin-project_homebrew-tap&pullRequest=514&metric=new_coverage&view=list)  \n![](https://sonarsource.github.io/sonarcloud-github-static-resources/v2/common/passed-16px.png '') [0.0% Duplication on New Code](https://sonarcloud.io/component_measures?id=jackin-project_homebrew-tap&pullRequest=514&metric=new_duplicated_lines_density&view=list)  \n  \n<!-- sqra-placement-anchor -->\n[See analysis details on SonarQube Cloud](https://sonarcloud.io/dashboard?id=jackin-project_homebrew-tap&pullRequest=514)\n\n";

struct Fixture {
    _temp: tempfile::TempDir,
    root: PathBuf,
    home: PathBuf,
    state: PathBuf,
    runner: PathBuf,
    output: PathBuf,
    event: PathBuf,
    policy: PathBuf,
    path: String,
}

impl Fixture {
    fn new(scenario: Scenario) -> Result<Self, Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().canonicalize()?;
        let home = root.join("home");
        let state = home.join("provider");
        let bin = root.join("bin");
        let runner = root.join("runner");
        fs::create_dir_all(&state)?;
        fs::create_dir_all(&bin)?;
        fs::create_dir_all(runner.join("velnor"))?;
        write_fixture(&state, scenario)?;
        write_executable(&bin.join("mise"), MISE)?;
        write_executable(&bin.join("gh"), GH)?;
        let event = root.join("event.json");
        let value = json!({
            "action":event_action(scenario),
            "repository":{"id":REPOSITORY_ID,"full_name":REPOSITORY},
            "issue":{"number":NUMBER,"pull_request":{"url":"fixture"}},
            "comment":{"body":request_body(scenario),"updated_at":request_time(scenario)}
        });
        fs::write(&event, serde_json::to_vec(&value)?)?;
        let output = root.join("github-output");
        fs::write(&output, "")?;
        let policy = runner.join("velnor/review-completion-policy.json");
        fs::write(&policy, br#"{"check_name":"Review completion","default_branch":"main","provider":"codex-v1","repository":"jackin-project/homebrew-tap","schema":"velnor.review-completion-policy.v1"}"#)?;
        Ok(Self {
            _temp: temp,
            root,
            home,
            state,
            runner,
            output,
            event,
            policy,
            path: format!("{}:/usr/bin:/bin:/usr/sbin:/sbin", bin.display()),
        })
    }

    fn command(&self, operation: &str) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_velnor-actions"));
        command
            .env_clear()
            .env("PATH", &self.path)
            .env("HOME", &self.home)
            .env("TMPDIR", &self.root)
            .env("RUNNER_TEMP", &self.runner)
            .env("GITHUB_OUTPUT", &self.output)
            .env("GITHUB_EVENT_PATH", &self.event)
            .env("GITHUB_EVENT_NAME", "issue_comment")
            .env("GITHUB_JOB", "review-completion")
            .env(
                "GITHUB_WORKFLOW_REF",
                format!("{REPOSITORY}/.github/workflows/review-completion.yml@refs/heads/main"),
            )
            .env("GITHUB_REF", "refs/heads/main")
            .env("GITHUB_REPOSITORY", REPOSITORY)
            .env("GITHUB_REPOSITORY_ID", REPOSITORY_ID.to_string())
            .env("VELNOR_INTERNAL_OP", operation)
            .env("VELNOR_REQUEST_FILE", &self.policy)
            .env("VELNOR_COMPLETION_APP_ID", "5399768")
            .env("VELNOR_REVIEWER_APP_ID", "1144995")
            .env("GH_TOKEN", "fixture-nonsecret-token");
        command
    }
}

pub(crate) fn run(
    scenario: Scenario,
    expected_success: bool,
    expected_state: &str,
) -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new(scenario)?;
    let captured = fixture.command("capture-review-completion-v1").output()?;
    assert_transport_clean(&fixture.state)?;
    assert_success(&captured, "capture")?;
    let proposal = read_json(
        &fixture
            .runner
            .join("velnor/review-completion-proposal.json"),
    )?;
    assert_eq!(proposal["state"], expected_state, "scenario: {scenario:?}");
    if scenario.has_authenticated_codex() {
        assert!(
            proposal["provider_digest"]
                .as_str()
                .is_some_and(|digest| !digest.is_empty())
        );
    }
    let capture_output = fs::read_to_string(&fixture.output)?;
    let digest = proposal_digest(&capture_output)?;
    let emitted = fixture
        .command("emit-review-completion-v1")
        .env("VELNOR_REVIEW_COMPLETION_PROPOSAL_DIGEST", digest)
        .output()?;
    assert_transport_clean(&fixture.state)?;
    assert_success(&emitted, "emit")?;
    let check = read_json(&fixture.state.join("completion-check.json"))?;
    assert_completion(&check, expected_success)?;
    Ok(())
}

fn assert_completion(check: &Value, expected_success: bool) -> Result<(), Box<dyn Error>> {
    assert_eq!(check["head_sha"], HEAD);
    assert_eq!(check["status"], "completed");
    assert_eq!(
        check["conclusion"],
        if expected_success {
            "success"
        } else {
            "failure"
        }
    );
    if expected_success {
        let text = check["output"]["text"]
            .as_str()
            .ok_or_else(|| io::Error::other("successful check omitted payload"))?;
        let payload: Value = serde_json::from_str(text)?;
        assert_eq!(payload["verdict"], "complete");
        assert_eq!(payload["head_sha"], HEAD);
        assert_eq!(payload["reviewers"][0]["app_id"], 1_144_995);
        assert_eq!(payload["reviewers"][0]["complete"], true);
        assert!(
            payload["reviewers"][0]["evidence_digest"]
                .as_str()
                .is_some_and(|digest| !digest.is_empty())
        );
    }
    Ok(())
}

fn read_json(path: &Path) -> Result<Value, Box<dyn Error>> {
    Ok(serde_json::from_slice(&fs::read(path)?)?)
}

fn proposal_digest(output: &str) -> Result<String, io::Error> {
    output
        .lines()
        .find_map(|line| line.strip_prefix("proposal_digest="))
        .map(str::to_owned)
        .ok_or_else(|| io::Error::other("capture omitted proposal digest"))
}

fn write_fixture(state: &Path, scenario: Scenario) -> Result<(), Box<dyn Error>> {
    let mut sonar_body = SONAR.to_owned();
    if matches!(scenario, Scenario::SonarFinding) {
        sonar_body = sonar_body.replace("[0 New issues]", "[1 New issues]");
    }
    let (sonar_created, sonar_rest_updated, sonar_graphql_updated, sonar_last_edited, sonar_editor) =
        sonar_edit(scenario);
    write_comments(
        state,
        scenario,
        &sonar_body,
        &sonar_created,
        &sonar_rest_updated,
    )?;
    write_static_responses(state, scenario)?;
    comment_editor::write_responses(
        state,
        scenario,
        &sonar_body,
        &sonar_graphql_updated,
        &sonar_last_edited,
        &sonar_editor,
    )?;
    write_sonar_check(state, scenario)?;
    Ok(())
}

fn write_comments(
    state: &Path,
    scenario: Scenario,
    sonar_body: &str,
    sonar_created: &Value,
    sonar_updated: &Value,
) -> Result<(), Box<dyn Error>> {
    let (codex_comment_id, sonar_comment_id) = comment_editor::comment_ids(scenario);
    let mut comments = Vec::new();
    if scenario.has_live_request() {
        comments.push(json!({
            "id":6_402_121_074_u64,"node_id":"IC_contextual_request","user":{"id":139_017,"type":"User"},
            "created_at":"2026-10-27T14:50:00Z","updated_at":request_time(scenario),"body":request_body(scenario)
        }));
    }
    comments.push(json!({
        "id":codex_comment_id,"node_id":"IC_codex_summary","user":{"id":199_175_422,"type":"Bot"},
        "created_at":"2026-10-26T00:42:04Z","updated_at":"2026-10-27T15:03:25Z","body":CODEX,
        "performed_via_github_app":{"id":1_144_995,"slug":"chatgpt-codex-connector","owner":{"id":14_957_082}}
    }));
    if scenario.has_sonar() {
        comments.push(json!({
            "id":sonar_comment_id,"node_id":"IC_sonar_summary","user":{"id":39_514_782,"type":"Bot"},
            "created_at":sonar_created,"updated_at":sonar_updated,"body":sonar_body,
            "performed_via_github_app":{"id":12526,"slug":"sonarqubecloud","owner":{"id":545_988}}
        }));
    }
    put_json(state, "issue-comments.json", &Value::Array(comments))
}

fn write_static_responses(state: &Path, scenario: Scenario) -> Result<(), Box<dyn Error>> {
    put_json(state, "pull.json", &pull(scenario))?;
    put_json(
        state,
        "repository.json",
        &json!({"id":REPOSITORY_ID,"full_name":REPOSITORY,"default_branch":"main"}),
    )?;
    put_json(
        state,
        "codex-app.json",
        &json!({"id":1_144_995,"slug":"chatgpt-codex-connector","owner":{"id":14_957_082,"login":"OpenAI","type":"Organization"}}),
    )?;
    put_json(
        state,
        "sonar-app.json",
        &json!({"id":12526,"slug":"sonarqubecloud","owner":{"id":545_988,"login":"SonarSource","type":"Organization"}}),
    )?;
    put_json(state, "commits.json", &json!([{"sha":HEAD}]))?;
    put_json(state, "empty-array.json", &json!([]))?;
    put_json(
        state,
        "threads.json",
        &json!({"data":{"repository":{"pullRequest":{"reviewThreads":{"nodes":[],"pageInfo":{"hasNextPage":false,"endCursor":null}}}}}}),
    )?;
    put_json(
        state,
        "opinions.json",
        &json!({"data":{"repository":{"pullRequest":{"latestOpinionatedReviews":{"nodes":[],"pageInfo":{"hasNextPage":false,"endCursor":null}}}}}}),
    )?;
    review_content::write_responses(state, scenario)
}

fn write_sonar_check(state: &Path, scenario: Scenario) -> Result<(), Box<dyn Error>> {
    let mut sonar_text = concat!(
        "Issues  \n",
        "![](https://sonarsource.github.io/sonarcloud-github-static-resources/v2/common/passed.svg '') [0 New issues](https://sonarcloud.io/project/issues?id=jackin-project_homebrew-tap&pullRequest=514&issueStatuses=OPEN,CONFIRMED&sinceLeakPeriod=true)  \n",
        "![](https://sonarsource.github.io/sonarcloud-github-static-resources/v2/common/accepted.svg '') [0 Accepted issues](https://sonarcloud.io/project/issues?id=jackin-project_homebrew-tap&pullRequest=514&issueStatuses=ACCEPTED)\n\n",
        "Measures  \n",
        "![](https://sonarsource.github.io/sonarcloud-github-static-resources/v2/common/passed.svg '') [0 Security Hotspots](https://sonarcloud.io/project/security_hotspots?id=jackin-project_homebrew-tap&pullRequest=514&issueStatuses=OPEN,CONFIRMED&sinceLeakPeriod=true)  \n",
        "![](https://sonarsource.github.io/sonarcloud-github-static-resources/v2/common/passed.svg '') [0.0% Coverage on New Code](https://sonarcloud.io/component_measures?id=jackin-project_homebrew-tap&pullRequest=514&metric=new_coverage&view=list)  \n",
        "![](https://sonarsource.github.io/sonarcloud-github-static-resources/v2/common/passed.svg '') [0.0% Duplication on New Code](https://sonarcloud.io/component_measures?id=jackin-project_homebrew-tap&pullRequest=514&metric=new_duplicated_lines_density&view=list)  \n  \n",
        "[See analysis details on SonarQube Cloud](https://sonarcloud.io/dashboard?id=jackin-project_homebrew-tap&pullRequest=514)\n\n"
    ).to_owned();
    if matches!(scenario, Scenario::SonarFinding) {
        sonar_text = sonar_text.replace("[0 New issues]", "[1 New issues]");
    }
    let association = match scenario {
        Scenario::ForkEmptyAssociation
        | Scenario::SameRepositoryEmptyAssociation
        | Scenario::ForkWrongHead
        | Scenario::ForkWrongApp
        | Scenario::ForkMissingFlag
        | Scenario::ForkFalseFlag => json!([]),
        Scenario::SonarTargetWithForeignAssociation => json!([
            {"number":NUMBER,"head":{"sha":HEAD},"base":{"repo":{"id":REPOSITORY_ID}}},
            {"number":515,"head":{"sha":HEAD},"base":{"repo":{"id":REPOSITORY_ID}}}
        ]),
        Scenario::SonarDuplicateTargetAssociation => json!([
            {"number":NUMBER,"head":{"sha":HEAD},"base":{"repo":{"id":REPOSITORY_ID}}},
            {"number":NUMBER,"head":{"sha":HEAD},"base":{"repo":{"id":REPOSITORY_ID}}}
        ]),
        Scenario::SonarForeignAssociationOnly => json!([
            {"number":515,"head":{"sha":HEAD},"base":{"repo":{"id":REPOSITORY_ID}}}
        ]),
        _ => json!([{"number":NUMBER,"head":{"sha":HEAD},"base":{"repo":{"id":REPOSITORY_ID}}}]),
    };
    let check_head = if matches!(scenario, Scenario::ForkWrongHead) {
        "wrong-head"
    } else {
        HEAD
    };
    let check_app_id = if matches!(scenario, Scenario::ForkWrongApp) {
        12_527
    } else {
        12_526
    };
    let sonar_check = json!({
        "id":122_263_237_931_u64,"name":"SonarCloud Code Analysis","head_sha":check_head,"status":"completed","conclusion":"success",
        "started_at":"2026-10-27T14:52:13Z","completed_at":"2026-10-27T14:52:22Z",
        "details_url":"https://sonarcloud.io/dashboard?id=jackin-project_homebrew-tap&pullRequest=514",
        "output":{"title":"Quality Gate passed","summary":sonar_text,"annotations_count":0},
        "app":{"id":check_app_id,"slug":"sonarqubecloud","owner":{"id":545_988}},
        "pull_requests":association
    });
    if scenario.has_sonar() {
        put_json(state, "sonar-check.json", &sonar_check)?;
    }
    Ok(())
}

fn pull(scenario: Scenario) -> Value {
    let (head_repository_id, fork_flag) =
        super::fork_head_repository(scenario).unwrap_or((REPOSITORY_ID, Some(false)));
    let full_name = if head_repository_id == REPOSITORY_ID {
        REPOSITORY
    } else {
        "JonathanBrouwer/rust"
    };
    let mut head_repository = json!({"id":head_repository_id,"full_name":full_name});
    if let Some(fork) = fork_flag {
        head_repository["fork"] = json!(fork);
    }
    json!({"number":NUMBER,"state":"open","draft":false,"commits":1,
        "head":{"sha":HEAD,"ref":"fixture","repo":head_repository},
        "base":{"sha":BASE,"ref":"main","repo":{"id":REPOSITORY_ID,"full_name":REPOSITORY}}})
}

fn put_json(path: &Path, name: &str, value: &Value) -> Result<(), Box<dyn Error>> {
    fs::write(path.join(name), serde_json::to_vec(value)?)?;
    Ok(())
}

fn write_executable(path: &Path, contents: &str) -> Result<(), io::Error> {
    fs::write(path, contents)?;
    let mut permissions = fs::metadata(path)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions)
}

fn assert_success(output: &Output, label: &str) -> Result<(), io::Error> {
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "{label} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    Ok(())
}

fn assert_transport_clean(state: &Path) -> Result<(), io::Error> {
    let errors = state.join("transport-errors");
    if errors.exists() {
        return Err(io::Error::other(format!(
            "fixture transport rejected a request: {}",
            fs::read_to_string(errors)?
        )));
    }
    Ok(())
}

const MISE: &str = r#"#!/bin/sh
[ "$1" = "--no-config" ] && [ "$2" = "--no-env" ] && [ "$3" = "--no-hooks" ] && [ "$4" = "exec" ] || exit 91
case "$5" in gh@*) ;; *) exit 92 ;; esac
[ "$6" = "--" ] && [ "$7" = "gh" ] || exit 93
shift 6
exec "$@"
"#;

const GH: &str = include_str!("review_completion_fixture/gh_transport.sh");
