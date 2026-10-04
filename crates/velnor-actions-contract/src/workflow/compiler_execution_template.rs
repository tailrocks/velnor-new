//! Canonical source for the compiled Rust report wrapper.

const INTERNAL_OP_ENV: &str = "VELNOR_INTERNAL_OP";
const TASK_ID_ENV: &str = "VELNOR_TASK_ID";
const TASK_DIGEST_ENV: &str = "VELNOR_TASK_DIGEST";
const FRAME_ARGV_ENV: &str = "VELNOR_RUST_FRAME_ARGV_JSON";
const FRAME_TOOLCHAIN_ENV: &str = "VELNOR_RUST_FRAME_TOOLCHAIN";
const START_OP: &str = "write-task-start-v1";
const REPORT_OP: &str = "write-task-report-v1";
const PREEXEC_OP: &str = "validate-rust-report-preexec-v1";
const EXIT_CODE_ENV: &str = "VELNOR_EXIT_CODE";
const START_MS_ENV: &str = "VELNOR_START_MS";
const STAGED_BINARY_PREFIX: &str = "$RUNNER_TEMP/velnor/bin/velnor-actions-";

pub(super) fn source(
    recipe: &super::CompiledRustReportRecipe,
    frame: &super::RustReportFrame,
) -> Result<String, crate::ContractError> {
    let argv_json = serde_json::to_string(&recipe.compiler_argv)
        .map_err(|_| crate::ContractError::identity("rust_report_wrapper", "frame_json"))?;
    let assignments = [
        (TASK_ID_ENV, frame.task_id.as_str()),
        (TASK_DIGEST_ENV, recipe.expected_task_digest.as_str()),
        (FRAME_ARGV_ENV, argv_json.as_str()),
        (FRAME_TOOLCHAIN_ENV, recipe.toolchain_id.as_str()),
    ]
    .into_iter()
    .map(|(key, value)| format!("{key}={}", quote_literal_run_arg(value)))
    .collect::<Vec<_>>()
    .join(" ");
    let helper = format!("{STAGED_BINARY_PREFIX}{}", frame.version);
    let start_path = format!("$RUNNER_TEMP/velnor/start-{}", frame.matrix_key);
    let arguments = recipe
        .compiler_argv
        .iter()
        .skip(1)
        .map(|argument| quote_literal_run_arg(argument))
        .collect::<Vec<_>>()
        .join(" ");
    let compiler = format!("\"$RUNNER_TEMP/velnor/mise/bin/mise\" {arguments}");
    let body = format!(
        "{assignments} {INTERNAL_OP_ENV}={PREEXEC_OP} \"{helper}\" || exit $?; {INTERNAL_OP_ENV}={START_OP} \"{helper}\" > \"{start_path}\"; stamp_code=$?; if [ \"$stamp_code\" -ne 0 ]; then true > \"{start_path}\"; fi; {compiler}; code=$?; start_ms=; if [ \"$stamp_code\" -eq 0 ]; then read -r start_ms rest < \"{start_path}\"; fi; {EXIT_CODE_ENV}=\"$code\" {START_MS_ENV}=\"$start_ms\" {INTERNAL_OP_ENV}={REPORT_OP} \"{helper}\"; helper_code=$?; if [ \"$code\" -ne 0 ]; then exit \"$code\"; fi; exit \"$helper_code\"\n"
    );
    crate::generated_source(&frame.version, &body)
}

/// POSIX literal argument quoting; shell expansions remain immutable bytes.
#[must_use]
pub fn quote_literal_run_arg(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}
