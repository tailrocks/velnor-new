//! Fixed multiline scripts are emitted as native YAML before serialization.

use std::collections::BTreeMap;
use std::error::Error;
use std::io;

use super::super::{Phase, render, scripts};
use crate::schema2::MbxQualificationPins;
use crate::yaml::Yaml;
use velnor_actions_contract::{PermissionLevel, PullRequestCachePolicy};

#[test]
fn protected_environment_uses_native_job_field() -> Result<(), Box<dyn Error>> {
    let hosted = Yaml::str("ubuntu-26.04");
    let mut job = super::super::probe_steps::make_job(
        "MBX cancellation fixture".to_owned(),
        &hosted,
        "github.event_name == 'workflow_dispatch'".to_owned(),
        Vec::new(),
        PermissionLevel::None,
        Vec::new(),
        5,
    )?;
    job.environment = Some("mbx-cancel-during-save".to_owned());
    let (_, rendered) = render::render_raw_job(job, "mbx-cancel-fixture", &hosted, Vec::new())?;
    let Yaml::Map(fields) = rendered else {
        return Err(io::Error::other("rendered job is not a YAML mapping").into());
    };
    assert!(fields.iter().any(|(key, value)| {
        key == "environment" && value == &Yaml::str("mbx-cancel-during-save")
    }));
    assert!(!fields.iter().any(|(key, _)| key == "env"));
    Ok(())
}

#[test]
fn victim_multiline_scripts_render_as_fixed_native_yaml_bodies() -> Result<(), Box<dyn Error>> {
    for phase in [Phase::PreSave, Phase::DuringSave] {
        let request = request();
        let hosted = Yaml::str("ubuntu-26.04");
        let job = super::super::victim_job(&request, phase, &hosted)?;
        let mut jobs = BTreeMap::from([(phase.victim_id().to_owned(), job)]);
        crate::mbx_bundle::append_single_bundle_saves(&mut jobs, PullRequestCachePolicy::ReadOnly)?;
        let job = jobs.remove(phase.victim_id()).ok_or("victim job missing")?;
        let (_, rendered) =
            render::render_victim_job(job, phase.victim_id(), &hosted, phase, &request)?;
        let runs = rendered_runs(&rendered)?;
        assert_run_body(
            &runs,
            "Validate MBX cancellation victim identity",
            scripts::VICTIM_IDENTITY,
        )?;
        assert_run_body(
            &runs,
            "Write MBX cancellation readiness receipt",
            scripts::WRITE_VICTIM_RECEIPT,
        )?;
        if phase == Phase::PreSave {
            assert_run_body(
                &runs,
                "Confirm pre-save victim has no writer payload",
                scripts::PRE_SAVE_GUARD,
            )?;
            assert_run_body(
                &runs,
                "Wait at MBX pre-save cancellation point",
                scripts::PRE_SAVE_WAIT,
            )?;
        } else {
            assert_run_body(
                &runs,
                "Fetch exact protected-main source",
                scripts::FETCH_SOURCE,
            )?;
            assert_run_body(
                &runs,
                "Build pinned MBX workspace",
                scripts::BUILD_WORKSPACE,
            )?;
        }
        assert_typed_actions(&rendered, &request)?;
    }
    Ok(())
}

fn request() -> MbxQualificationPins {
    MbxQualificationPins {
        mise_setup: crate::setup::MiseSetup {
            uses: "jdx/mise-action@9149ea85001c7435d5a66bb127d6a1b6227cb0a5".to_owned(),
            version: "2025.9.5".to_owned(),
            sha256: "d".repeat(64),
        },
        mbx_action_uses: "jdx/mr-boxington-action@1687e54eb349cadf61fa38b5813a77875489e8e6"
            .to_owned(),
        mbx_version: "1.22.0".to_owned(),
        rust_version: "1.98.1".to_owned(),
    }
}

fn rendered_runs(job: &Yaml) -> Result<Vec<(String, String)>, Box<dyn Error>> {
    let Yaml::Map(fields) = job else {
        return Err("rendered job is not a map".into());
    };
    let steps = fields
        .iter()
        .find(|(key, _)| key == "steps")
        .ok_or("steps missing")?;
    let Yaml::Seq(steps) = &steps.1 else {
        return Err("steps are not a sequence".into());
    };
    steps
        .iter()
        .filter_map(|step| {
            let Yaml::Map(fields) = step else {
                return Some(Err("step is not a map".into()));
            };
            let name = &fields.iter().find(|(key, _)| key == "name")?.1;
            let run = &fields.iter().find(|(key, _)| key == "run")?.1;
            let (Yaml::Str(name), Yaml::Str(run)) = (name, run) else {
                return Some(Err("script step fields are not strings".into()));
            };
            Some(Ok((name.clone(), run.clone())))
        })
        .collect()
}

fn assert_run_body(
    runs: &[(String, String)],
    name: &str,
    source: &str,
) -> Result<(), Box<dyn Error>> {
    let body = runs
        .iter()
        .find(|(step, _)| step == name)
        .ok_or("script step missing")?;
    assert_eq!(body.1, source);
    Ok(())
}

fn assert_typed_actions(job: &Yaml, request: &MbxQualificationPins) -> Result<(), Box<dyn Error>> {
    let Yaml::Map(fields) = job else {
        return Err("rendered job is not a map".into());
    };
    let steps = fields
        .iter()
        .find(|(key, _)| key == "steps")
        .ok_or("steps missing")?;
    let Yaml::Seq(steps) = &steps.1 else {
        return Err("steps are not a sequence".into());
    };
    for (name, uses) in [
        ("Set up Mise", request.mise_setup.uses.as_str()),
        ("Setup MBX", request.mbx_action_uses.as_str()),
        (
            "Upload MBX cancellation receipt",
            crate::steps::UPLOAD_ARTIFACT_USES,
        ),
        (
            crate::mbx_bundle::MBX_BUNDLE_RESTORE_NAME,
            crate::steps::TOOLS_RESTORE_USES,
        ),
        (
            crate::mbx_bundle::MBX_BUNDLE_SAVE_NAME,
            crate::steps::TOOLS_SAVE_USES,
        ),
    ] {
        let step = steps
            .iter()
            .find(|step| step_name(step) == Some(name))
            .ok_or("typed action missing")?;
        let Yaml::Map(fields) = step else {
            return Err("typed step is not a map".into());
        };
        assert!(
            fields
                .iter()
                .any(|(key, value)| key == "uses" && value == &Yaml::str(uses))
        );
    }
    Ok(())
}

fn step_name(step: &Yaml) -> Option<&str> {
    let Yaml::Map(fields) = step else { return None };
    let (_, Yaml::Str(name)) = fields.iter().find(|(key, _)| key == "name")? else {
        return None;
    };
    Some(name)
}
