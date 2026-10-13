/// Check for one explicitly declared step id in a serialized job.
fn has_step_id(jobs: &[(String, Yaml)], id: &str, step_id: &str) -> bool {
    let steps = jobs
        .iter()
        .find_map(|(name, job)| (name == id).then_some(job))
        .and_then(|job| {
            if let Yaml::Map(entries) = job {
                entries
                    .iter()
                    .find_map(|(name, value)| (name == "steps").then_some(value))
            } else {
                None
            }
        });
    let Some(Yaml::Seq(items)) = steps else {
        return false;
    };
    items.iter().any(|item| {
        matches!(
            item,
            Yaml::Map(step) if step.iter().any(|(name, value)| {
                name == "id" && matches!(value, Yaml::Str(actual) if actual == step_id)
            })
        )
    })
}
