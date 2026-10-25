use super::{
    AuditResult, arms_in, dispatch_arm_is, function, invalid, operation_arm_is, parsed_production,
    pat_path_is, use_imports_symbol,
};

pub(super) fn validate_sources(
    dispatch_source: &str,
    internal_run_source: &str,
    operation_source: &str,
) -> AuditResult<()> {
    let dispatch = parsed_production(dispatch_source)?;
    let internal_run = parsed_production(internal_run_source)?;
    let run_internal = function(&internal_run, "run_internal")?;
    let dispatch_arms = arms_in(run_internal);
    for (operation, function_name) in [
        ("GitHubEvidenceFetch", "run_github_evidence_fetch"),
        ("GitHubEvidenceImport", "run_github_evidence_import"),
    ] {
        if !use_imports_symbol(
            &internal_run,
            &["velnor_actions_orchestrator", function_name],
            function_name,
        ) {
            return Err(invalid("evidence CLI API import changed"));
        }
        let matching = dispatch_arms
            .iter()
            .filter(|arm| pat_path_is(&arm.pat, &["InternalOp", operation]))
            .collect::<Vec<_>>();
        if matching.len() != 1 || !dispatch_arm_is(matching[0], operation, function_name) {
            return Err(invalid("evidence CLI operation arm changed"));
        }
        let dispatch_references = super::super::source_identifiers(dispatch_source)?
            .iter()
            .filter(|ident| ident.as_str() == function_name)
            .count();
        let handler_references = super::super::source_identifiers(internal_run_source)?
            .iter()
            .filter(|ident| ident.as_str() == function_name)
            .count();
        if dispatch_references + handler_references != 2 {
            return Err(invalid("evidence CLI callable has an extra reference"));
        }
    }
    let try_internal = function(&dispatch, "try_internal")?;
    if !super::first_local_is_call(try_internal, "request", &["gate_request"], true) {
        return Err(invalid("evidence CLI call moved outside the internal gate"));
    }

    let operation = parsed_production(operation_source)?;
    let from_env = function(&operation, "from_env")?;
    let operation_arms = arms_in(from_env);
    for (constant, variant) in [
        ("GITHUB_EVIDENCE_FETCH_OP", "GitHubEvidenceFetch"),
        ("GITHUB_EVIDENCE_IMPORT_OP", "GitHubEvidenceImport"),
    ] {
        if !use_imports_symbol(&operation, &["super", constant], constant) {
            return Err(invalid("evidence operation constant import changed"));
        }
        let matching = operation_arms
            .iter()
            .filter(|arm| operation_arm_is(arm, constant, variant))
            .count();
        if matching != 1
            || super::super::source_identifiers(operation_source)?
                .iter()
                .filter(|ident| ident.as_str() == constant)
                .count()
                != 2
        {
            return Err(invalid("evidence operation selection changed"));
        }
    }
    Ok(())
}
