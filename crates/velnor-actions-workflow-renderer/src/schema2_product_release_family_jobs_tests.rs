use std::collections::BTreeSet;
use std::error::Error;

use crate::schema2::{ProductReleasePins, Schema2WorkflowRequest};
use crate::yaml::Yaml;

use super::super::Family;
use super::{compose_job, family_document, take_jobs};
use crate::schema2::product_release_family as family;
use crate::schema2::product_release_test_pins::test_pins;

fn request(pins: ProductReleasePins) -> Result<Schema2WorkflowRequest, crate::RenderError> {
    Ok(Schema2WorkflowRequest {
        version: "2.0.0".to_owned(),
        hosted_label: "ubuntu-26.04".to_owned(),
        scale_set: Schema2WorkflowRequest::canonical_scale_set()?,
        workflows: BTreeSet::new(),
        mbx_qualification: None,
        product_release: Some(pins),
    })
}

fn publisher_job(
    selected: Family,
    pins: &ProductReleasePins,
) -> Result<(String, Yaml), Box<dyn Error>> {
    let publish_id = selected
        .job_ids()
        .ok_or("generator uses its dedicated typed graph")?
        .2;
    let document = family_document(selected, &request(pins.clone())?)?;
    take_jobs(document)?
        .into_iter()
        .find(|(id, _)| id == publish_id)
        .ok_or_else(|| format!("missing publisher job {publish_id}").into())
}

fn steps_mut(job: &mut Yaml) -> &mut Vec<Yaml> {
    let Yaml::Map(fields) = job else {
        panic!("publisher job is a map");
    };
    let Some((_, Yaml::Seq(steps))) = fields.iter_mut().find(|(key, _)| key == "steps") else {
        panic!("publisher steps are a sequence");
    };
    steps
}

fn step_id(step: &Yaml) -> Option<&str> {
    let Yaml::Map(fields) = step else {
        return None;
    };
    fields.iter().find_map(|(key, value)| {
        if key == "id"
            && let Yaml::Str(id) = value
        {
            return Some(id.as_str());
        }
        None
    })
}

fn rename_publish_display(step: &mut Yaml) {
    let Yaml::Map(fields) = step else {
        panic!("publisher step is a map");
    };
    let Some((_, name)) = fields.iter_mut().find(|(key, _)| key == "name") else {
        panic!("publisher step has a display name");
    };
    *name = Yaml::str("Changed presentation label");
}

#[test]
fn publisher_role_survives_display_name_changes_for_each_family() -> Result<(), Box<dyn Error>> {
    let pins = test_pins();
    for selected in [Family::Images, Family::Binary] {
        let (id, mut job) = publisher_job(selected, &pins)?;
        let source_steps = steps_mut(&mut job);
        let publish = source_steps
            .iter_mut()
            .find(|step| step_id(step) == Some(family::PUBLISH_STEP_ID))
            .ok_or("source publisher step lacks its typed role ID")?;
        assert_eq!(
            super::identified_step_role(publish),
            Some(family::StepRole::Publish)
        );
        rename_publish_display(publish);

        let (composed_id, mut composed) = compose_job(id.clone(), job, selected, &pins)?;
        assert_eq!(composed_id, id);
        let publish = steps_mut(&mut composed)
            .iter()
            .find(|step| step_id(step) == Some(family::PUBLISH_STEP_ID))
            .ok_or("composed publisher step disappeared")?;
        let Yaml::Map(fields) = publish else {
            return Err("composed publisher step is not a map".into());
        };
        assert!(fields.contains(&("name".to_owned(), Yaml::str("Changed presentation label"))));
        assert!(fields.contains(&(
            "run".to_owned(),
            Yaml::str(family::publish_script(selected, &pins)?)
        )));
        let Some((_, Yaml::Map(environment))) = fields.iter().find(|(key, _)| key == "env") else {
            return Err("composed publisher environment is missing".into());
        };
        assert!(environment.contains(&(
            "VELNOR_RELEASE_ACTION".to_owned(),
            Yaml::str("${{ inputs.release_action }}")
        )));
    }
    Ok(())
}

#[test]
fn publishing_job_requires_exactly_one_publisher_role() -> Result<(), Box<dyn Error>> {
    let pins = test_pins();
    let (id, job) = publisher_job(Family::Images, &pins)?;

    let mut missing = job.clone();
    let publish = steps_mut(&mut missing)
        .iter_mut()
        .find(|step| step_id(step) == Some(family::PUBLISH_STEP_ID))
        .ok_or("source publisher step lacks its typed role ID")?;
    let Yaml::Map(fields) = publish else {
        return Err("source publisher step is not a map".into());
    };
    fields.retain(|(key, _)| key != "id");
    assert!(compose_job(id.clone(), missing, Family::Images, &pins).is_err());

    let mut duplicated = job;
    let publish = steps_mut(&mut duplicated)
        .iter()
        .find(|step| step_id(step) == Some(family::PUBLISH_STEP_ID))
        .cloned()
        .ok_or("source publisher step lacks its typed role ID")?;
    steps_mut(&mut duplicated).push(publish);
    assert!(compose_job(id, duplicated, Family::Images, &pins).is_err());
    Ok(())
}
