use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::{
    Job, JobTimeout, StepId, StepRole, workflow::step_identity::validate_step_sequence,
};

use crate::yaml::Yaml;

use super::*;

fn payload(runs_on: &str) -> Result<super::super::ToolsCachePayload, RenderError> {
    payload_with_source(runs_on, None)
}

fn payload_with_source(
    runs_on: &str,
    scale_set_image_source_sha: Option<&str>,
) -> Result<super::super::ToolsCachePayload, RenderError> {
    let setup = crate::setup::MiseSetup {
        uses: "jdx/mise-action@fc42bcc7ddbf51124c30f95a877a0c5168a3bfde".to_owned(),
        version: "2026.10.4".to_owned(),
        sha256: "a".repeat(64),
    };
    super::super::ToolsCachePayload::new(crate::cache_p08::ToolsCacheInputs {
        runs_on,
        target: "x86_64-unknown-linux-gnu",
        mise_setup: &setup,
        tool_specs: &["rust@1.98.1".to_owned(), "actionlint@1.7.12".to_owned()],
        rustup_toolchain: Some("1.98.1"),
        rustup_components: &[],
        scale_set_image_source_sha,
    })
}

fn verified_acquire_step() -> Result<velnor_actions_contract::Step, RenderError> {
    let helper = format!("{}0.1.0", crate::steps::STAGED_BINARY_PREFIX);
    crate::steps::acquire_velnor_step(
        vec![
            "sh".to_owned(),
            "-c".to_owned(),
            format!(
                "curl -fsSL \"$VELNOR_ASSET_URL\" -o {helper} && echo \"$VELNOR_ASSET_SHA256  {helper}\" | sha256sum -c - && chmod +x {helper}"
            ),
        ],
        &BTreeMap::from([
            (
                crate::steps::ASSET_URL_ENV.to_owned(),
                "https://example.invalid/velnor".to_owned(),
            ),
            (crate::steps::ASSET_SHA_ENV.to_owned(), "a".repeat(64)),
            (crate::steps::RELEASE_COMMIT_ENV.to_owned(), "b".repeat(40)),
        ]),
    )
}

#[test]
fn scale_set_prelude_threads_the_validated_image_source_pin() -> Result<(), RenderError> {
    let source_sha = "a".repeat(40);
    let payload =
        payload_with_source("scale-set:velnor+ubuntu-26.04-scale-set", Some(&source_sha))?;
    assert!(
        !payload.runtime_identity_supported(),
        "an expected source pin is not an observed runner image identity"
    );
    let step = payload.runtime_prelude_step()?;
    assert_eq!(
        step.condition.as_deref(),
        Some(cache_p08::TOOLS_CACHE_NORMAL_RUN_CONDITION)
    );
    let StepKind::Action { uses, with, env } = &step.kind else {
        return Err(RenderError::InvalidWorkflow(
            "scale_set_prelude_not_action".to_owned(),
        ));
    };
    assert_eq!(uses, "./.github/actions/velnor-tools-prelude-scale-set");
    assert_eq!(with.get("d"), Some(&payload.static_digest().to_owned()));
    assert_eq!(with.get("scale-set-image-source-sha"), Some(&source_sha));
    assert!(env.is_empty());

    let file = action_file("scale-set:velnor+ubuntu-26.04-scale-set", "0.1.0")?;
    assert_eq!(
        file.path,
        ".github/actions/velnor-tools-prelude-scale-set/action.yml"
    );
    assert!(file.bytes.contains("scale-set-image-source-sha:"));
    assert!(
        file.bytes
            .contains("${{ inputs.scale-set-image-source-sha }}")
    );
    assert!(file.bytes.contains("uses: ./.github/actions/scale-set"));
    Ok(())
}

#[test]
fn fixed_composite_forwards_identity_and_runs_seed_before_outer_restore() -> Result<(), RenderError>
{
    for (lane, helper, wrapper) in [
        (
            "ubuntu-22.04",
            "./.github/actions/u22",
            "./.github/actions/velnor-tools-prelude-u22",
        ),
        (
            "ubuntu-24.04",
            "./.github/actions/u24",
            "./.github/actions/velnor-tools-prelude-u24",
        ),
        (
            "ubuntu-26.04",
            "./.github/actions/u26",
            "./.github/actions/velnor-tools-prelude-u26",
        ),
    ] {
        let payload = payload(lane)?;
        let step = payload.runtime_prelude_step()?;
        assert_eq!(step.id, Some(StepId::ToolsCacheIdentity));
        assert_eq!(step.role, Some(StepRole::ToolsCacheIdentity));
        assert_eq!(
            step.condition.as_deref(),
            Some(cache_p08::TOOLS_CACHE_NORMAL_RUN_CONDITION)
        );
        let StepKind::Action { uses, with, env } = &step.kind else {
            return Err(RenderError::InvalidWorkflow(
                "prelude_not_action".to_owned(),
            ));
        };
        assert_eq!(uses, wrapper);
        assert_eq!(with.get("d"), Some(&payload.static_digest().to_owned()));
        assert_eq!(env.len(), 0);
        validate_step_sequence(std::slice::from_ref(&step), "tools-prelude-test")
            .map_err(RenderError::Contract)?;
        assert!(
            validate_step_sequence(&[step.clone(), step.clone()], "tools-prelude-test").is_err()
        );

        let file = action_file(lane, "0.1.0")?;
        assert_eq!(
            file.path,
            format!("{}/action.yml", wrapper.trim_start_matches("./"))
        );
        let identity_at = file
            .bytes
            .find(&format!("uses: {helper}"))
            .expect("identity child");
        let seed_at = file
            .bytes
            .find(&format!("uses: {}", crate::tool_seed::TOOL_SEED_USES))
            .expect("seed child");
        let enabled_gate = file
            .bytes
            .find(&format!(
                "if: {}",
                cache_p08::TOOLS_CACHE_IDENTITY_CONDITION
            ))
            .expect("seed enabled gate");
        assert!(identity_at < enabled_gate && enabled_gate < seed_at);
        assert!(file.bytes.contains("id: v2"));
        assert!(file.bytes.contains("d:"));
        assert!(file.bytes.contains("inputs.d"));
        assert!(
            file.bytes
                .contains("value: ${{ steps.v2.outputs.identity }}"),
            "{}",
            file.bytes
        );
        assert!(
            file.bytes
                .contains("value: ${{ steps.v2.outputs.enabled }}"),
            "{}",
            file.bytes
        );
        assert!(
            file.bytes
                .contains("mise-tools-v2-payload-v3-${{steps.v2.outputs.identity}}")
        );
    }
    Ok(())
}

#[test]
fn prelude_exposes_the_validated_seed_admission_output() -> Result<(), RenderError> {
    let file = action_file("ubuntu-26.04", "0.1.0")?;
    assert!(file.bytes.contains("id: seed"), "{}", file.bytes);
    assert!(file.bytes.contains("seed_admitted:"), "{}", file.bytes);
    assert!(
        file.bytes
            .contains("value: ${{ steps.seed.outputs.seed_admitted }}"),
        "{}",
        file.bytes
    );

    let Yaml::Map(seed_entries) = super::seed_step() else {
        return Err(RenderError::InvalidWorkflow(
            "prelude_seed_step_not_mapping".to_owned(),
        ));
    };
    assert!(seed_entries.contains(&("id".to_owned(), Yaml::Str("seed".to_owned()))));
    assert!(seed_entries.contains(&(
        "if".to_owned(),
        Yaml::Str(cache_p08::TOOLS_CACHE_IDENTITY_CONDITION.to_owned()),
    )));
    let Yaml::Map(outputs) = super::output_map() else {
        return Err(RenderError::InvalidWorkflow(
            "prelude_outputs_not_mapping".to_owned(),
        ));
    };
    assert!(outputs.contains(&(
        "seed_admitted".to_owned(),
        Yaml::Map(vec![(
            "value".to_owned(),
            Yaml::Str("${{ steps.seed.outputs.seed_admitted }}".to_owned()),
        )]),
    )));
    Ok(())
}

#[test]
fn normal_tools_cache_requires_a_successful_non_qualification_plan() -> Result<(), RenderError> {
    let payload = payload("ubuntu-26.04")?;
    let prelude = payload.runtime_prelude_step()?;
    let restore = payload.restore_step()?;

    assert_eq!(
        prelude.condition.as_deref(),
        Some("needs.plan.result == 'success' && needs.plan.outputs.qualification_campaign == ''")
    );
    assert_eq!(
        restore.condition.as_deref(),
        Some(
            "needs.plan.result == 'success' && needs.plan.outputs.qualification_campaign == '' && steps.v2.outputs.enabled == 'true'"
        )
    );

    let mut unguarded_prelude = prelude.clone();
    unguarded_prelude.condition = None;
    let StepKind::Action { uses, with, env } = &unguarded_prelude.kind else {
        return Err(RenderError::InvalidWorkflow(
            "prelude_not_action".to_owned(),
        ));
    };
    assert!(
        crate::cache_p08::validate_runtime_identity_action(
            &unguarded_prelude,
            uses,
            "ubuntu-26.04",
            with,
            env,
        )
        .is_err()
    );

    Ok(())
}

#[test]
fn tools_cache_conditions_follow_the_job_plan_dependency() -> Result<(), RenderError> {
    for (needs, expected_identity, expected_restore) in [
        (
            Vec::new(),
            cache_p08::TOOLS_CACHE_PLAN_RUN_CONDITION,
            cache_p08::TOOLS_CACHE_PLAN_RESTORE_CONDITION,
        ),
        (
            vec!["plan".to_owned()],
            cache_p08::TOOLS_CACHE_NORMAL_RUN_CONDITION,
            cache_p08::TOOLS_CACHE_RESTORE_CONDITION,
        ),
        (
            vec!["suite-build-capture".to_owned()],
            cache_p08::TOOLS_CACHE_PLAN_RUN_CONDITION,
            cache_p08::TOOLS_CACHE_PLAN_RESTORE_CONDITION,
        ),
    ] {
        let checkout = crate::steps::checkout_step(
            "actions/checkout@0000000000000000000000000000000000000000",
        )?;
        let install = crate::steps::shell_step(
            "Install actionlint",
            vec![
                "mise".to_owned(),
                "--no-config".to_owned(),
                "--no-env".to_owned(),
                "--no-hooks".to_owned(),
                "install".to_owned(),
                "actionlint@1.7.12".to_owned(),
            ],
            BTreeMap::new(),
        )?;
        let mut job = Job {
            display_name: "Suite check".to_owned(),
            runs_on: "ubuntu-26.04".to_owned(),
            check_runner: None,
            timeout_minutes: JobTimeout::CRATE,
            needs,
            condition: None,
            permissions: None,
            environment: None,
            steps: vec![checkout, verified_acquire_step()?, install],
            cache_mode: None,
        };
        assert_eq!(
            cache_p08::infer_job_tools(&job),
            ["actionlint@1.7.12".to_owned()]
        );

        cache_p08::ensure_tools_cache_v2_with_source(
            "suite-check",
            &mut job,
            &crate::setup::MiseSetup {
                uses: "jdx/mise-action@9b200bcecbfd6303e69a4a261b2e09e3757440a7".to_owned(),
                version: "2026.10.13".to_owned(),
                sha256: "a".repeat(64),
            },
            true,
            "x86_64-unknown-linux-gnu",
            None,
            "actions/checkout@0000000000000000000000000000000000000000",
        )?;

        let identity = job
            .steps
            .iter()
            .find(|step| step.role == Some(StepRole::ToolsCacheIdentity))
            .ok_or_else(|| RenderError::InvalidWorkflow("tools_identity_missing".to_owned()))?;
        let restore = job
            .steps
            .iter()
            .find(|step| step.role == Some(StepRole::ToolsCacheRestore))
            .ok_or_else(|| RenderError::InvalidWorkflow("tools_restore_missing".to_owned()))?;

        assert_eq!(identity.condition.as_deref(), Some(expected_identity));
        assert_eq!(restore.condition.as_deref(), Some(expected_restore));
        cache_p08::elect_cache_writers(&mut BTreeMap::from([("suite-check".to_owned(), job)]))?;
    }
    Ok(())
}

#[test]
fn generated_files_exist_only_for_real_v2_consumers() -> Result<(), RenderError> {
    let payload = payload("ubuntu-26.04")?;
    let prelude = payload.runtime_prelude_step()?;
    let job = Job {
        display_name: "fixture".to_owned(),
        runs_on: "ubuntu-26.04".to_owned(),
        check_runner: None,
        timeout_minutes: JobTimeout::CRATE,
        needs: Vec::new(),
        condition: None,
        permissions: None,
        environment: None,
        steps: vec![prelude],
        cache_mode: None,
    };
    let jobs = BTreeMap::from([("fixture".to_owned(), job)]);
    let files = crate::render_cache_files::with_runtime_identity_files(Vec::new(), &jobs, "0.1.0")?;
    let paths = files
        .iter()
        .map(|file| file.path.as_str())
        .collect::<BTreeSet<_>>();
    assert_eq!(files.len(), 5);
    assert!(paths.contains(".github/actions/u26/action.yml"));
    assert!(paths.contains(".github/actions/velnor-tools-prelude-u26/action.yml"));
    assert!(paths.contains(".github/actions/velnor-tool-seed/action.yml"));
    assert!(paths.contains(".github/scripts/velnor-tools-cache-identity.sh"));

    let no_cache_files = crate::render_cache_files::with_runtime_identity_files(
        Vec::new(),
        &BTreeMap::new(),
        "0.1.0",
    )?;
    assert_eq!(no_cache_files, [] as [crate::RenderedFile; 0]);
    Ok(())
}

#[path = "cache_p08_helper_policy_tests.rs"]
mod helper_policy_tests;
