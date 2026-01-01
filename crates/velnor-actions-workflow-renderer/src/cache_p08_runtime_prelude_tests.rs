use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::{
    Job, JobTimeout, StepId, StepRole, workflow::step_identity::validate_step_sequence,
};

use crate::yaml::Yaml;

use super::*;

fn payload(runs_on: &str) -> Result<super::super::ToolsCachePayload, RenderError> {
    let setup = crate::setup::MiseSetup {
        uses: "jdx/mise-action@2d8d4cafcbd33be2ea37d2b6f5ad595363d1f1ca".to_owned(),
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
    })
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
        let StepKind::Action { uses, with, env } = &step.kind else {
            return Err(RenderError::InvalidWorkflow(
                "prelude_not_action".to_owned(),
            ));
        };
        assert_eq!(uses, wrapper);
        assert_eq!(with.get("d"), Some(&payload.static_digest().to_owned()));
        assert!(env.is_empty());
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
            .find(&format!("if: {}", cache_p08::TOOLS_CACHE_RESTORE_CONDITION))
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
                .contains("mise-tools-v2-${{steps.v2.outputs.identity}}")
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
    assert_eq!(no_cache_files, [] as [crate::tree::RenderedFile; 0]);
    Ok(())
}
