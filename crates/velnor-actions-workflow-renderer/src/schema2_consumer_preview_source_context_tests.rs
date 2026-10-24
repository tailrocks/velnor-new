use super::*;

#[test]
fn original_run_tuple_is_bound_once_only_at_source_selection_and_finalization() {
    let spec = with_tap();
    let (_, eligibility) = super::super::source::job(&spec, false).expect("source job");
    let Yaml::Seq(steps) = field(&eligibility, "steps") else {
        panic!("source steps")
    };
    for operation in ["select-preview-source-v1", "finalize-preview-source-v1"] {
        let matching = steps
            .iter()
            .filter(|step| has_operation(step, operation))
            .collect::<Vec<_>>();
        assert_eq!(matching.len(), 1, "one {operation} step");
        let step = matching[0];
        let Yaml::Map(bindings) = field(step, "env") else {
            panic!("operation env")
        };
        for (key, value) in [
            (
                "VELNOR_RELEASE_ORIGINAL_RUN_ID",
                "${{ inputs.original_run_id }}",
            ),
            (
                "VELNOR_RELEASE_ORIGINAL_RUN_ATTEMPT",
                "${{ inputs.original_run_attempt }}",
            ),
        ] {
            let count = bindings
                .iter()
                .filter(|(name, _)| name.as_str() == key)
                .count();
            assert_eq!(count, 1, "{operation}: {key}");
            assert_eq!(
                env_value(step, key),
                Some(&Yaml::str(value)),
                "{operation}: {key}"
            );
        }
    }

    let yaml = render_preview(&spec).expect("preview").bytes;
    for (name, next) in [
        ("preview-payload", Some("preview-assemble")),
        ("preview-assemble", Some("preview-signer")),
        ("preview-signer", Some("preview-publisher")),
    ] {
        let rendered = job(&yaml, name, next);
        assert!(!rendered.contains("VELNOR_RELEASE_ORIGINAL_RUN_ID:"));
        assert!(!rendered.contains("VELNOR_RELEASE_ORIGINAL_RUN_ATTEMPT:"));
    }
    for (name, next) in [
        ("preview-recovery", None),
        ("preview-tap-writer", Some("preview-tap-merge")),
        ("preview-tap-merge", None),
    ] {
        let rendered = job(&yaml, name, next);
        assert!(rendered.contains("VELNOR_RELEASE_ORIGINAL_RUN_ID: ${{ inputs.original_run_id }}"));
        assert!(
            rendered.contains(
                "VELNOR_RELEASE_ORIGINAL_RUN_ATTEMPT: ${{ inputs.original_run_attempt }}"
            )
        );
    }
}
