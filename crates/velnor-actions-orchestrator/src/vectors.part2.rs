/// Fixed pre-seed MBX route probe through pinned Mise.
///
/// Runs `mbx --version` under exact Rust Mise while retaining the native action's
/// earlier PATH entry. The action owns the MBX installation and object cache.
/// # Errors
///
/// Returns a contract error when the Mise adapter rejects the vector.
pub(crate) fn mbx_probe_argv(catalog: &ToolCatalog) -> Result<Vec<String>, OrchestratorError> {
    let exec = RouteDriver::Mbx
        .probe_exec(vec![OsString::from("--version")])
        .map_err(|err| OrchestratorError::Contract {
            problem: err.to_string(),
        })?;
    strings_of(exec.argv(catalog)).map_err(|problem| OrchestratorError::Contract { problem })
}

/// Fixed bootstrap §4 build vector through pinned Mise.
///
/// Shared by the candidate build and pre-seed helper build, so both use
/// the exact Rust selector and MBX binary installed by the native action.
pub(crate) fn candidate_build_argv(
    catalog: &ToolCatalog,
) -> Result<Vec<String>, OrchestratorError> {
    let build = CandidateBuild::new().map_err(|err| OrchestratorError::Contract {
        problem: err.to_string(),
    })?;
    strings_of(build.argv(catalog)).map_err(|problem| OrchestratorError::Contract { problem })
}

/// Fixed candidate build plus qualification vectors.
pub(crate) fn candidate_spec(catalog: &ToolCatalog) -> Result<CandidateSpec, OrchestratorError> {
    Ok(CandidateSpec {
        build: candidate_build_argv(catalog)?,
        qualify: QualifyRequest::staged().argv()?,
    })
}

/// Convert fixed argv to UTF-8 strings.
fn strings_of(argv: Vec<OsString>) -> Result<Vec<String>, String> {
    argv.into_iter()
        .map(|arg| arg.into_string().map_err(|_| "non_utf8_argv".to_owned()))
        .collect()
}

#[cfg(test)]
#[path = "vectors_tests.rs"]
mod vectors_tests;
