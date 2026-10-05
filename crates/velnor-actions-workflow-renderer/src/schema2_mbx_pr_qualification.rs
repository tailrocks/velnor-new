//! Same-repository pull-request qualification for the held MBX release.

use std::collections::BTreeMap;

use super::features::{CHECKOUT_USES, finish, gated, lane_base, run_step};
use super::{MbxQualificationTarget, RunnerSpec, mbx_qualification};
use crate::cache_steps::MBX_ACTION_NAME;
use crate::yaml::Yaml;
use crate::{RenderError, steps::validate_uses};

const PR_ADMISSION: &str = "github.event_name == 'pull_request' && github.repository == github.event.pull_request.head.repo.full_name && github.repository == github.event.pull_request.base.repo.full_name && github.event.pull_request.head.repo.fork == false";
const PR_CACHE_KEY_SCRIPT: &str = r#"set -euo pipefail
test "$MBX_EVENT_NAME" = pull_request
test -n "$MBX_REPOSITORY"
test "$MBX_HEAD_REPOSITORY" = "$MBX_REPOSITORY"
test "$MBX_BASE_REPOSITORY" = "$MBX_REPOSITORY"
test "$MBX_HEAD_REPOSITORY_FORK" = false
case "$MBX_PR_NUMBER" in ''|0*|*[!0-9]*) exit 1 ;; esac
case "$MBX_PR_HEAD_SHA" in *[!0-9a-f]*) exit 1 ;; esac
test "${#MBX_PR_HEAD_SHA}" -eq 40
case "$GITHUB_RUN_ID:$GITHUB_RUN_ATTEMPT" in *[!0-9:]*|:*|*:) exit 1 ;; esac
case "$RUNNER_OS:$RUNNER_ARCH" in Linux:X64) ;; *) exit 1 ;; esac
key="qualification-mbx-pr-${MBX_VERSION}-action-${MBX_ACTION_SHA}-pr-${MBX_PR_NUMBER}-${MBX_PR_HEAD_SHA}-run-${GITHUB_RUN_ID}-attempt-${GITHUB_RUN_ATTEMPT}"
printf 'key=%s\nno_fallback=%s-no-fallback-\n' "$key" "$key" >> "$GITHUB_OUTPUT"
"#;
const OBJECTS_PROBE: &str = "set -e -o pipefail; mbx cache stats --json | tee \"$RUNNER_TEMP/mbx-pr-object-stats.json\"; jq -e '.objects > 0' \"$RUNNER_TEMP/mbx-pr-object-stats.json\"";
const REUSE_PROBE: &str = "set -e -o pipefail; mbx stats --json | tee \"$RUNNER_TEMP/mbx-pr-reuse-stats.json\"; jq -e '.savings.cached_compilations > 0' \"$RUNNER_TEMP/mbx-pr-reuse-stats.json\"";

/// Emit one cold writer and one exact-key reader for an admitted PR.
pub(super) fn jobs(
    target: &MbxQualificationTarget,
    hosted: &RunnerSpec,
) -> Result<Vec<(String, Yaml)>, RenderError> {
    mbx_qualification::validate_target(target)?;
    validate_uses(CHECKOUT_USES)?;
    Ok(vec![
        job(target, hosted, true)?,
        job(target, hosted, false)?,
    ])
}

fn job(
    target: &MbxQualificationTarget,
    hosted: &RunnerSpec,
    writer: bool,
) -> Result<(String, Yaml), RenderError> {
    let (id, title, needed) = job_identity(writer);
    let mut fields = lane_base(title, hosted, 45);
    append_needs(&mut fields, needed);
    fields.push(("permissions".to_owned(), permissions(writer)));
    fields.push(("env".to_owned(), qualification_env(target, writer)));
    let steps = qualification_steps(target, writer)?;
    Ok(gated(finish(id, fields, steps), PR_ADMISSION))
}

fn job_identity(writer: bool) -> (&'static str, &'static str, Option<&'static str>) {
    if writer {
        (
            "mbx-pr-candidate-write",
            "MBX 1.22 PR cache candidate / writer",
            None,
        )
    } else {
        (
            "mbx-pr-candidate-read",
            "MBX 1.22 PR cache candidate / reuse reader",
            Some("mbx-pr-candidate-write"),
        )
    }
}

fn append_needs(fields: &mut Vec<(String, Yaml)>, needed: Option<&str>) {
    if let Some(needed) = needed {
        fields.push(("needs".to_owned(), Yaml::Seq(vec![Yaml::str(needed)])));
    }
}

fn permissions(writer: bool) -> Yaml {
    let actions = if writer { "write" } else { "read" };
    mapping(&[("contents", "read"), ("actions", actions)])
}

fn qualification_steps(
    target: &MbxQualificationTarget,
    writer: bool,
) -> Result<Vec<Yaml>, RenderError> {
    let mut steps = vec![
        checkout_step(),
        mbx_qualification::mise_setup_step(target),
        mbx_qualification::mise_install_step(target),
        cache_key_step(target),
        action_step(target, writer)?,
        verify_action_step(target, writer),
    ];
    if !writer {
        steps.push(run_step("Require imported MBX objects", OBJECTS_PROBE));
    }
    steps.push(mbx_qualification::build_step());
    steps.push(run_step(
        "Sample runner disk after MBX build",
        "df -B1 -P \"$RUNNER_TEMP\"; df -i -P \"$RUNNER_TEMP\"",
    ));
    if !writer {
        steps.push(run_step("Require reused MBX compilation", REUSE_PROBE));
    }
    Ok(steps)
}

fn checkout_step() -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str("Checkout PR merge ref")),
        ("uses".to_owned(), Yaml::str(CHECKOUT_USES)),
        (
            "with".to_owned(),
            mapping(&[("persist-credentials", "false")]),
        ),
    ])
}

fn cache_key_step(target: &MbxQualificationTarget) -> Yaml {
    let action_sha = target
        .action_uses
        .strip_prefix(&format!("{MBX_ACTION_NAME}@"))
        .unwrap_or_default();
    let mut env = BTreeMap::from([
        ("MBX_VERSION".to_owned(), target.mbx_version.clone()),
        ("MBX_ACTION_SHA".to_owned(), action_sha.to_owned()),
        (
            "MBX_EVENT_NAME".to_owned(),
            "${{ github.event_name }}".to_owned(),
        ),
        (
            "MBX_REPOSITORY".to_owned(),
            "${{ github.repository }}".to_owned(),
        ),
        (
            "MBX_HEAD_REPOSITORY".to_owned(),
            "${{ github.event.pull_request.head.repo.full_name }}".to_owned(),
        ),
        (
            "MBX_BASE_REPOSITORY".to_owned(),
            "${{ github.event.pull_request.base.repo.full_name }}".to_owned(),
        ),
        (
            "MBX_HEAD_REPOSITORY_FORK".to_owned(),
            "${{ toJSON(github.event.pull_request.head.repo.fork) }}".to_owned(),
        ),
        (
            "MBX_PR_NUMBER".to_owned(),
            "${{ github.event.pull_request.number }}".to_owned(),
        ),
        (
            "MBX_PR_HEAD_SHA".to_owned(),
            "${{ github.event.pull_request.head.sha }}".to_owned(),
        ),
    ]);
    env.insert("RUNNER_TEMP".to_owned(), "${{ runner.temp }}".to_owned());
    Yaml::Map(vec![
        (
            "name".to_owned(),
            Yaml::str("Bind same-repository candidate cache key"),
        ),
        ("id".to_owned(), Yaml::str("mbx-pr-key")),
        ("env".to_owned(), mapping_owned(env)),
        ("run".to_owned(), Yaml::str(PR_CACHE_KEY_SCRIPT)),
    ])
}

fn action_step(target: &MbxQualificationTarget, writer: bool) -> Result<Yaml, RenderError> {
    validate_uses(&target.action_uses)?;
    let save = writer.to_string();
    let mode = if writer { "write" } else { "read" };
    let with = mapping(&[
        ("version", &target.mbx_version),
        ("github-cache-mode", "objects"),
        ("isolate-objects-cache", "true"),
        ("save-on-pull-request", &save),
        ("save-on-workflow-dispatch", "false"),
        (
            "cache-generation",
            &format!(
                "qualification-mbx-pr-v1-{}-{}",
                target.mbx_version, target.action_uses
            ),
        ),
        ("cache-key", "${{ steps.mbx-pr-key.outputs.key }}"),
        (
            "restore-keys",
            "${{ steps.mbx-pr-key.outputs.no_fallback }}",
        ),
        ("toolchain", &target.rust_version),
    ]);
    Ok(Yaml::Map(vec![
        (
            "name".to_owned(),
            Yaml::str("Restore MBX PR candidate objects"),
        ),
        ("id".to_owned(), Yaml::str("mbx_pr_cache")),
        ("uses".to_owned(), Yaml::str(target.action_uses.clone())),
        ("with".to_owned(), with),
        (
            "env".to_owned(),
            mapping(&[
                ("ACTIONS_CACHE_MODE", mode),
                ("MBX_GC_AUTO", "1"),
                ("MISE_NO_CONFIG", "1"),
                ("MISE_NO_ENV", "1"),
                ("MISE_NO_HOOKS", "1"),
            ]),
        ),
    ]))
}

fn verify_action_step(target: &MbxQualificationTarget, writer: bool) -> Yaml {
    let (eligible, reason, hit) = if writer {
        ("true", "same-repository pull request", "false")
    } else {
        ("false", "pull request; save-on-pull-request is off", "true")
    };
    Yaml::Map(vec![
        (
            "name".to_owned(),
            Yaml::str("Verify isolated MBX candidate cache policy"),
        ),
        (
            "env".to_owned(),
            mapping(&[
                (
                    "MBX_VERSION",
                    "${{ steps.mbx_pr_cache.outputs.mbx-version }}",
                ),
                (
                    "CACHE_SAVE_ELIGIBLE",
                    "${{ steps.mbx_pr_cache.outputs.cache-save-eligible }}",
                ),
                (
                    "CACHE_SAVE_REASON",
                    "${{ steps.mbx_pr_cache.outputs.cache-save-reason }}",
                ),
                ("CACHE_HIT", "${{ steps.mbx_pr_cache.outputs.cache-hit }}"),
            ]),
        ),
        (
            "run".to_owned(),
            Yaml::str(format!(
                "test \"$MBX_VERSION\" = '{}' && test \"$CACHE_SAVE_ELIGIBLE\" = '{}' && test \"$CACHE_SAVE_REASON\" = '{}' && test \"$CACHE_HIT\" = '{}'",
                target.mbx_version, eligible, reason, hit
            )),
        ),
    ])
}

fn qualification_env(target: &MbxQualificationTarget, writer: bool) -> Yaml {
    let home = "${{ github.workspace }}/.velnor-mbx-pr-qualification";
    mapping(&[
        ("ACTIONS_CACHE_MODE", if writer { "write" } else { "read" }),
        ("CARGO_HOME", &format!("{home}/cargo")),
        ("MISE_AUTO_INSTALL", "false"),
        ("MISE_CARGO_HOME", &format!("{home}/cargo")),
        ("MISE_EXEC_AUTO_INSTALL", "false"),
        ("MISE_LOCKFILE", "0"),
        ("MISE_NO_CONFIG", "1"),
        ("MISE_NO_ENV", "1"),
        ("MISE_NO_HOOKS", "1"),
        ("MISE_RUSTUP_HOME", &format!("{home}/rustup")),
        ("RUSTUP_HOME", &format!("{home}/rustup")),
        ("RUSTUP_TOOLCHAIN", &target.rust_version),
    ])
}

fn mapping(pairs: &[(&str, &str)]) -> Yaml {
    Yaml::Map(
        pairs
            .iter()
            .map(|(key, value)| ((*key).to_owned(), Yaml::str(*value)))
            .collect(),
    )
}

fn mapping_owned(pairs: BTreeMap<String, String>) -> Yaml {
    Yaml::Map(
        pairs
            .into_iter()
            .map(|(key, value)| (key, Yaml::str(value)))
            .collect(),
    )
}
