//! Shared release-job identity, permissions, and output fields.

use crate::yaml::Yaml;

pub(super) fn artifact_outputs(step_id: &str) -> Yaml {
    Yaml::Map(vec![
        (
            "artifact_id".to_owned(),
            Yaml::str(format!("${{{{ steps.{step_id}.outputs.artifact-id }}}}")),
        ),
        (
            "artifact_digest".to_owned(),
            Yaml::str(format!(
                "${{{{ steps.{step_id}.outputs.artifact-digest }}}}"
            )),
        ),
    ])
}

pub(super) fn job_base(
    name: &str,
    runs_on: Yaml,
    timeout: i64,
    pairs: &[(&str, &str)],
) -> Vec<(String, Yaml)> {
    vec![
        ("name".to_owned(), Yaml::str(name)),
        ("runs-on".to_owned(), runs_on),
        ("timeout-minutes".to_owned(), Yaml::Int(timeout)),
        ("if".to_owned(), Yaml::str(super::VERSION_GUARD)),
        ("permissions".to_owned(), permissions(pairs)),
    ]
}

fn permissions(pairs: &[(&str, &str)]) -> Yaml {
    Yaml::Map(
        pairs
            .iter()
            .map(|(key, value)| ((*key).to_owned(), Yaml::str(*value)))
            .collect(),
    )
}

pub(super) fn needs(jobs: &[&str]) -> Vec<(String, Yaml)> {
    vec![(
        "needs".to_owned(),
        Yaml::Seq(jobs.iter().map(|job| Yaml::str(*job)).collect()),
    )]
}

pub(super) fn with_runner(mut fields: Vec<(String, Yaml)>, runs_on: Yaml) -> Vec<(String, Yaml)> {
    if let Some((_, value)) = fields.iter_mut().find(|(key, _)| key == "runs-on") {
        *value = runs_on;
    }
    fields
}

pub(super) fn finish_job(
    id: &str,
    mut fields: Vec<(String, Yaml)>,
    steps: Vec<Yaml>,
) -> (String, Yaml) {
    fields.push(("steps".to_owned(), Yaml::Seq(steps)));
    (id.to_owned(), Yaml::Map(fields))
}
