use std::collections::BTreeMap;

use velnor_actions_contract::{Job, JobTimeout, Step, StepKind};

use crate::{
    RenderedFile,
    lane_share::job_workflow_extract::{
        JobWorkflowRelocation, PROTOTYPE_JOB_ID, prepare_job, source_bound_caller_inputs,
    },
    yaml::Yaml,
};

#[test]
fn binds_only_source_cache_outputs_consumed_by_original_steps() {
    let cases = [
        (false, false, "", ""),
        (true, false, "${{ needs.plan.outputs.sources_key }}", ""),
        (
            false,
            true,
            "",
            "${{ needs.plan.outputs.sources_restore_keys }}",
        ),
        (
            true,
            true,
            "${{ needs.plan.outputs.sources_key }}",
            "${{ needs.plan.outputs.sources_restore_keys }}",
        ),
    ];

    for (uses_sources_key, uses_restore_keys, expected_key, expected_restore_keys) in cases {
        let relocation = relocation(uses_sources_key, uses_restore_keys);
        let bindings = source_bound_caller_inputs(&relocation.prepared.source_job);
        assert_eq!(
            binding(&bindings, "sources_key"),
            expected_key,
            "sources_key usage: {uses_sources_key}"
        );
        assert_eq!(
            binding(&bindings, "sources_restore_keys"),
            expected_restore_keys,
            "restore-key usage: {uses_restore_keys}"
        );
        assert_eq!(
            binding(&bindings, "plan_result"),
            "${{ needs.plan.result }}"
        );
        assert_eq!(
            binding(&bindings, "qualification_campaign"),
            "${{ needs.plan.outputs.qualification_campaign }}"
        );
        assert_eq!(
            binding(&bindings, "run_key_attempt"),
            "${{ needs.plan.outputs.plan_attempt || github.run_attempt }}"
        );

        let caller = super::source_caller_yaml(&relocation.prepared)
            .expect("source-derived caller is valid");
        assert_eq!(input_value(&caller, "sources_key"), expected_key);
        assert_eq!(
            input_value(&caller, "sources_restore_keys"),
            expected_restore_keys
        );
        assert!(super::validate_caller(&parent_jobs(&relocation, caller), &relocation).is_ok());
    }
}

#[test]
fn rejects_empty_or_plan_reference_bindings_that_disagree_with_source_usage() {
    for (input_name, output_name) in [
        ("sources_key", "sources_key"),
        ("sources_restore_keys", "sources_restore_keys"),
    ] {
        let unused = relocation(false, false);
        let mut parent = unused.expected_caller.clone();
        set_input_value(
            &mut parent,
            input_name,
            &format!("${{{{ needs.plan.outputs.{output_name} }}}}"),
        );
        assert!(super::validate_caller(&parent_jobs(&unused, parent), &unused).is_err());

        let uses_input = relocation(
            input_name == "sources_key",
            input_name == "sources_restore_keys",
        );
        let mut parent = uses_input.expected_caller.clone();
        set_input_value(&mut parent, input_name, "");
        assert!(super::validate_caller(&parent_jobs(&uses_input, parent), &uses_input).is_err());
    }
}

#[test]
fn validator_rederives_cache_bindings_instead_of_trusting_expected_caller() {
    for (input_name, uses_input, output_name) in [
        ("sources_key", false, "sources_key"),
        ("sources_key", true, "sources_key"),
        ("sources_restore_keys", false, "sources_restore_keys"),
        ("sources_restore_keys", true, "sources_restore_keys"),
    ] {
        let mut relocation = relocation(
            uses_input && input_name == "sources_key",
            uses_input && input_name == "sources_restore_keys",
        );
        let changed_value = if uses_input {
            String::new()
        } else {
            format!("${{{{ needs.plan.outputs.{output_name} }}}}")
        };
        set_input_value(&mut relocation.expected_caller, input_name, &changed_value);
        let actual = relocation.expected_caller.clone();
        assert!(super::validate_caller(&parent_jobs(&relocation, actual), &relocation).is_err());
    }
}

fn relocation(uses_sources_key: bool, uses_restore_keys: bool) -> JobWorkflowRelocation {
    let mut run = vec!["printf ok".to_owned()];
    if uses_sources_key {
        run.push(source_reference("sources_key"));
    }
    if uses_restore_keys {
        run.push(source_reference("sources_restore_keys"));
    }
    let source_job = Job {
        display_name: "Rust CLI hosted".to_owned(),
        runs_on: "ubuntu-26.04".to_owned(),
        check_runner: None,
        timeout_minutes: JobTimeout::CRATE,
        needs: vec!["plan".to_owned()],
        condition: Some("${{ needs.plan.result == 'success' }}".to_owned()),
        permissions: None,
        cache_mode: None,
        environment: None,
        services: Vec::new(),
        steps: vec![Step {
            name: "task argv --digest source-digest cache-owner source-owner".to_owned(),
            id: None,
            role: None,
            condition: None,
            parallel: false,
            kind: StepKind::Shell {
                run,
                env: BTreeMap::new(),
            },
        }],
    };
    let prepared = prepare_job(PROTOTYPE_JOB_ID, &source_job).expect("valid source job");
    let expected_caller =
        super::source_caller_yaml(&prepared).expect("source caller binds exact used outputs");
    JobWorkflowRelocation {
        expected_caller,
        expected_callee_file: RenderedFile {
            path: prepared.callee_path.clone(),
            bytes: String::new(),
        },
        prepared,
    }
}

fn source_reference(output: &str) -> String {
    format!("printf '%s' '${{{{ needs.plan.outputs.{output} }}}}'")
}

fn binding(
    bindings: &[crate::lane_share::job_workflow_extract::CallerInput],
    name: &str,
) -> &'static str {
    bindings
        .iter()
        .find(|input| input.name == name)
        .map(|input| input.expression)
        .expect("caller input is present")
}

fn input_value<'a>(caller: &'a Yaml, name: &str) -> &'a str {
    let Yaml::Map(fields) = caller else {
        panic!("caller is a map");
    };
    let Some((_, Yaml::Map(inputs))) = fields.iter().find(|(key, _)| key == "with") else {
        panic!("caller has input bindings");
    };
    let Some((_, Yaml::Str(value))) = inputs.iter().find(|(key, _)| key == name) else {
        panic!("caller input {name} is a string");
    };
    value
}

fn parent_jobs(relocation: &JobWorkflowRelocation, caller: Yaml) -> Vec<(String, Yaml)> {
    vec![(relocation.prepared.source_id.clone(), caller)]
}

fn set_input_value(caller: &mut Yaml, name: &str, value: &str) {
    let Yaml::Map(fields) = caller else {
        panic!("caller is a map");
    };
    let Some((_, Yaml::Map(inputs))) = fields.iter_mut().find(|(key, _)| key == "with") else {
        panic!("caller has input bindings");
    };
    let Some((_, binding)) = inputs.iter_mut().find(|(key, _)| key == name) else {
        panic!("caller has input {name}");
    };
    *binding = Yaml::str(value);
}
