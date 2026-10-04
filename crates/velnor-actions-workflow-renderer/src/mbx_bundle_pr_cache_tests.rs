use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use velnor_actions_contract::{PullRequestCachePolicy, StepKind};

use super::key_step;

const REPOSITORY: &str = "tailrocks/velnor-new";
const HEAD_SHA: &str = "abcdef0123456789abcdef0123456789abcdef01";
const BASE_SHA: &str = "0123456789abcdef0123456789abcdef01234567";

#[test]
fn same_repository_opt_in_uses_head_key_and_separate_prefix() -> Result<(), String> {
    let scratch = Scratch::new()?;
    let mut same_repo = valid_pr();
    let pr_key = execute_key(&scratch, "same-repo", same_repo.clone())?;
    assert_eq!(pr_key.value("pr-cache-allowed"), Some("true"));
    assert!(
        pr_key
            .scope
            .contains(&format!("same-repository-pr-731-{HEAD_SHA}")),
        "{}",
        pr_key.scope
    );
    assert!(
        pr_key
            .value("primary")
            .is_some_and(|key| key.ends_with(HEAD_SHA))
    );
    assert_eq!(pr_key.value("generation"), Some("velnor-mbx-1.21.1"));
    assert_eq!(pr_key.value("rustc_identity").map(str::len), Some(64));
    assert_eq!(pr_key.value("mbx_version"), Some("1.21.1"));
    assert!(
        pr_key
            .value("prefix")
            .is_some_and(|key| !key.contains(HEAD_SHA))
    );

    same_repo.insert("MBX_EVENT_NAME".to_owned(), "push".to_owned());
    same_repo.insert(
        "MBX_PR_CACHE_POLICY".to_owned(),
        "same-repository-scoped".to_owned(),
    );
    same_repo.insert("MBX_BASE_SHA".to_owned(), String::new());
    same_repo.insert("GITHUB_SHA".to_owned(), BASE_SHA.to_owned());
    let push_key = execute_key(&scratch, "trusted-push", same_repo)?;
    assert_eq!(push_key.value("pr-cache-allowed"), Some("false"));
    assert!(!push_key.scope.contains("same-repository-pr"));
    assert_ne!(pr_key.value("primary"), push_key.value("primary"));
    assert_ne!(pr_key.value("prefix"), push_key.value("prefix"));

    let mut next_commit = valid_pr();
    next_commit.insert(
        "MBX_PR_HEAD_SHA".to_owned(),
        "fedcba9876543210fedcba9876543210fedcba98".to_owned(),
    );
    let next_key = execute_key(&scratch, "next-commit", next_commit)?;
    assert_ne!(pr_key.value("prefix"), next_key.value("prefix"));
    Ok(())
}

#[test]
fn hostile_or_incomplete_event_metadata_falls_back_read_only() -> Result<(), String> {
    let scratch = Scratch::new()?;
    let baseline = execute_key(&scratch, "baseline", fallback_event("pull_request_target"))?;
    let mut hostile = Vec::new();
    for (name, value) in [
        ("fork-true", Some("true")),
        ("fork-empty", Some("")),
        ("fork-null", Some("null")),
        ("fork-string-false", Some("\"false\"")),
        ("repository-empty", Some("")),
        ("repository-mismatch", Some("attacker/fork")),
        ("head-repository-empty", Some("")),
        ("head-repository-mismatch", Some("attacker/fork")),
        ("base-repository-empty", Some("")),
        ("base-repository-mismatch", Some("attacker/fork")),
        ("pr-zero", Some("0")),
        ("pr-missing", Some("")),
        ("pr-invalid", Some("12x")),
        ("head-sha-empty", Some("")),
        (
            "head-sha-upper",
            Some("ABCDEF0123456789ABCDEF0123456789ABCDEF01"),
        ),
        ("head-sha-short", Some("abcdef")),
    ] {
        let mut event = valid_pr();
        match name {
            "repository-empty" | "repository-mismatch" => {
                event.insert(
                    "MBX_REPOSITORY".to_owned(),
                    value.unwrap_or_default().to_owned(),
                );
            }
            "head-repository-empty"
            | "head-repository-mismatch"
            | "base-repository-empty"
            | "base-repository-mismatch" => {
                let key = if name.starts_with("base-") {
                    "MBX_BASE_REPOSITORY"
                } else {
                    "MBX_HEAD_REPOSITORY"
                };
                event.insert(key.to_owned(), value.unwrap_or_default().to_owned());
            }
            "pr-zero" | "pr-missing" | "pr-invalid" => {
                event.insert(
                    "MBX_PR_NUMBER".to_owned(),
                    value.unwrap_or_default().to_owned(),
                );
            }
            "head-sha-empty" | "head-sha-upper" | "head-sha-short" => {
                event.insert(
                    "MBX_PR_HEAD_SHA".to_owned(),
                    value.unwrap_or_default().to_owned(),
                );
            }
            _ => {
                event.insert(
                    "MBX_HEAD_REPOSITORY_FORK".to_owned(),
                    value.unwrap_or_default().to_owned(),
                );
            }
        }
        hostile.push((name, event));
    }
    for event_name in [
        "",
        "pull_request_target",
        "workflow_dispatch",
        "push",
        "dispatch",
    ] {
        hostile.push((event_name, fallback_event(event_name)));
    }

    for (index, (name, event)) in hostile.into_iter().enumerate() {
        let output = execute_key(&scratch, &format!("hostile-{index}-{name}"), event)?;
        assert_eq!(output.value("pr-cache-allowed"), Some("false"), "{name}");
        assert_eq!(output.value("prefix"), baseline.value("prefix"), "{name}");
        assert_eq!(output.value("primary"), baseline.value("primary"), "{name}");
    }
    Ok(())
}

#[test]
fn read_only_policy_emits_no_pr_write_authorization() -> Result<(), String> {
    let scratch = Scratch::new()?;
    let output = execute_with_policy(
        &scratch,
        "read-only",
        {
            let mut event = fallback_event("pull_request");
            event.remove("MBX_PR_CACHE_POLICY");
            event
        },
        PullRequestCachePolicy::ReadOnly,
    )?;
    assert!(output.value("pr-cache-allowed").is_none());
    Ok(())
}

fn valid_pr() -> BTreeMap<String, String> {
    let mut env = fallback_event("pull_request");
    env.insert("MBX_HEAD_REPOSITORY_FORK".to_owned(), "false".to_owned());
    env.insert("MBX_REPOSITORY".to_owned(), REPOSITORY.to_owned());
    env.insert("MBX_HEAD_REPOSITORY".to_owned(), REPOSITORY.to_owned());
    env.insert("MBX_BASE_REPOSITORY".to_owned(), REPOSITORY.to_owned());
    env.insert("MBX_PR_NUMBER".to_owned(), "731".to_owned());
    env.insert("MBX_PR_HEAD_SHA".to_owned(), HEAD_SHA.to_owned());
    env
}

fn fallback_event(event: &str) -> BTreeMap<String, String> {
    let base_sha = if event.starts_with("pull_request") {
        BASE_SHA
    } else {
        ""
    };
    BTreeMap::from([
        (
            "MBX_PR_CACHE_POLICY".to_owned(),
            "same-repository-scoped".to_owned(),
        ),
        ("MBX_EVENT_NAME".to_owned(), event.to_owned()),
        ("MBX_HEAD_REPOSITORY_FORK".to_owned(), "false".to_owned()),
        ("MBX_REPOSITORY".to_owned(), REPOSITORY.to_owned()),
        ("MBX_HEAD_REPOSITORY".to_owned(), REPOSITORY.to_owned()),
        ("MBX_BASE_REPOSITORY".to_owned(), REPOSITORY.to_owned()),
        ("MBX_PR_NUMBER".to_owned(), "731".to_owned()),
        ("MBX_PR_HEAD_SHA".to_owned(), HEAD_SHA.to_owned()),
        ("MBX_BASE_SHA".to_owned(), base_sha.to_owned()),
        ("GITHUB_SHA".to_owned(), BASE_SHA.to_owned()),
    ])
}

fn execute_key(
    scratch: &Scratch,
    label: &str,
    mut event: BTreeMap<String, String>,
) -> Result<KeyOutput, String> {
    event.extend([
        (
            "MBX_PR_CACHE_POLICY".to_owned(),
            "same-repository-scoped".to_owned(),
        ),
        ("MBX_VERSION".to_owned(), "1.21.1".to_owned()),
        ("MBX_EXPECTED_VERSION".to_owned(), "1.21.1".to_owned()),
        ("MBX_GENERATION".to_owned(), "velnor-mbx-1.21.1".to_owned()),
        ("MBX_CACHE_SCOPE".to_owned(), "renderer-test".to_owned()),
        ("MBX_MATRIX_CONTEXT".to_owned(), "{}".to_owned()),
        ("RUSTUP_TOOLCHAIN".to_owned(), "1.98.1".to_owned()),
    ]);
    execute_with_policy(
        scratch,
        label,
        event,
        PullRequestCachePolicy::SameRepositoryScoped,
    )
}

fn execute_with_policy(
    scratch: &Scratch,
    label: &str,
    mut env: BTreeMap<String, String>,
    policy: PullRequestCachePolicy,
) -> Result<KeyOutput, String> {
    let step = key_step(
        "Prepare MBX bundle key",
        "velnor-mbx-1.21.1",
        "1.21.1",
        "renderer-test",
        &BTreeMap::from([("RUSTUP_TOOLCHAIN".to_owned(), "1.98.1".to_owned())]),
        false,
        policy,
    )
    .map_err(|error| format!("key step: {error}"))?;
    let StepKind::Shell { run, env: step_env } = step.kind else {
        return Err("key step is not shell".to_owned());
    };
    fill_base_env(&mut env, step_env);
    let script = run
        .get(2)
        .ok_or_else(|| "missing inline shell script".to_owned())?;
    let case_dir = scratch.0.join(label);
    fs::create_dir_all(case_dir.join("bin")).map_err(|error| display_error(&error))?;
    let mise = case_dir.join("bin/mise");
    fs::write(
        &mise,
        "#!/bin/sh\nprintf 'rustc 1.98.1\\ncommit-hash: test\\n'\n",
    )
    .map_err(|error| display_error(&error))?;
    let mut permissions = fs::metadata(&mise)
        .map_err(|error| display_error(&error))?
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&mise, permissions).map_err(|error| display_error(&error))?;
    let output_file = case_dir.join("github-output");
    fs::write(&output_file, "").map_err(|error| display_error(&error))?;
    env.extend([
        ("RUNNER_OS".to_owned(), "Linux".to_owned()),
        ("RUNNER_ARCH".to_owned(), "X64".to_owned()),
        (
            "GITHUB_WORKFLOW_REF".to_owned(),
            format!("{REPOSITORY}/.github/workflows/ci.yml@refs/pull/731/merge"),
        ),
        (
            "GITHUB_OUTPUT".to_owned(),
            output_file.display().to_string(),
        ),
        (
            "PATH".to_owned(),
            format!("{}:/usr/bin:/bin:/sbin", case_dir.join("bin").display()),
        ),
    ]);
    let result = Command::new("bash")
        .arg("-c")
        .arg(script)
        .env_clear()
        .envs(env)
        .output()
        .map_err(|error| display_error(&error))?;
    if !result.status.success() {
        return Err(format!(
            "key script failed: {}",
            String::from_utf8_lossy(&result.stderr)
        ));
    }
    let values = fs::read_to_string(output_file).map_err(|error| display_error(&error))?;
    let scope = fs::read_to_string(format!(
        "{}.mbx-scope",
        case_dir.join("github-output").display()
    ))
    .map_err(|error| display_error(&error))?;
    Ok(KeyOutput { values, scope })
}

fn fill_base_env(env: &mut BTreeMap<String, String>, step_env: BTreeMap<String, String>) {
    for (key, value) in [
        ("MBX_VERSION", "1.21.1"),
        ("MBX_EXPECTED_VERSION", "1.21.1"),
        ("MBX_GENERATION", "velnor-mbx-1.21.1"),
        ("MBX_CACHE_SCOPE", "renderer-test"),
        ("MBX_MATRIX_CONTEXT", "{}"),
        ("RUSTUP_TOOLCHAIN", "1.98.1"),
    ] {
        env.entry(key.to_owned())
            .or_insert_with(|| value.to_owned());
    }
    env.extend(
        step_env
            .into_iter()
            .filter(|(key, _)| !key.starts_with("MBX_") && key != "RUSTUP_TOOLCHAIN"),
    );
}

fn display_error(error: &std::io::Error) -> String {
    error.to_string()
}

struct KeyOutput {
    values: String,
    scope: String,
}

impl KeyOutput {
    fn value(&self, key: &str) -> Option<&str> {
        self.values.lines().find_map(|line| {
            let (name, value) = line.split_once('=')?;
            (name == key).then_some(value)
        })
    }
}

struct Scratch(std::path::PathBuf);

impl Scratch {
    fn new() -> Result<Self, String> {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| error.to_string())?
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "velnor-mbx-pr-policy-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&path).map_err(|error| display_error(&error))?;
        Ok(Self(path))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.0) {
            eprintln!("could not clean MBX PR-key test directory: {error}");
        }
    }
}
