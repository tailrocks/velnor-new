pub(super) fn value_has_unbound_needs(value: &saphyr::Yaml<'_>, condition: bool) -> bool {
    if condition {
        return value.as_str().is_some_and(|text| {
            crate::lane_share::job_workflow_extract::test_condition_has_unbound_needs(text)
                .expect("serialized condition is well formed")
        });
    }
    super::yaml_has_unbound_needs(value)
}
