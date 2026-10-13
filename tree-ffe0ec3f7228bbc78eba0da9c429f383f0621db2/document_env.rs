//! Workflow/job environment factoring.

use std::collections::BTreeMap;

/// Remove job values already inherited unchanged from workflow scope.
pub(super) fn exclude_inherited(
    job: &BTreeMap<String, String>,
    workflow: &BTreeMap<String, String>,
) -> BTreeMap<String, String> {
    job.iter()
        .filter_map(|(key, value)| {
            (workflow.get(key) != Some(value)).then(|| (key.clone(), value.clone()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::exclude_inherited;
    use std::collections::BTreeMap;

    #[test]
    fn retains_job_overrides_and_job_only_values() {
        let workflow = BTreeMap::from([
            ("shared".to_owned(), String::new()),
            ("override".to_owned(), String::new()),
        ]);
        let job = BTreeMap::from([
            ("shared".to_owned(), String::new()),
            ("override".to_owned(), "specific".to_owned()),
            ("job_only".to_owned(), "value".to_owned()),
        ]);

        assert_eq!(
            exclude_inherited(&job, &workflow),
            BTreeMap::from([
                ("job_only".to_owned(), "value".to_owned()),
                ("override".to_owned(), "specific".to_owned()),
            ])
        );
    }
}
