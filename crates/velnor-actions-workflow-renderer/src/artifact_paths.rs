use crate::RenderError;

pub const PRESEED_OUTPUT_DIR_EXPR: &str = "${{ runner.temp }}/velnor/preseed-output";
pub const PRESEED_STAGE_DIR_EXPR: &str = "${{ runner.temp }}/velnor/preseed";

pub fn candidate_artifact_name(target: &str) -> Result<String, RenderError> {
    let key = velnor_actions_contract::target_key(target).map_err(RenderError::Contract)?;
    Ok(format!(
        "velnor-candidate-{}-{key}",
        crate::steps::RUN_KEY_EXPR
    ))
}

pub(crate) fn check_artifact_path(path: &str) -> Result<(), RenderError> {
    let mut rest = path;
    while let Some(at) = rest.find('$') {
        if !rest[at..].starts_with("${{") {
            return Err(RenderError::BadActionRef(format!(
                "shell_artifact_path:{path}"
            )));
        }
        rest = &rest[at + 3..];
    }
    Ok(())
}
