//! Fixed Rust compiler command and ordinary report-frame authority.

use std::collections::BTreeMap;

use crate::{ContractError, matrix_id_for_task_group, matrix_key_for_id, validate_task_id};

use super::{CompiledSourceHelper, HelperInvocation, SourceBoundHelper, SourceBoundOperation};

#[path = "compiler_execution_environment.rs"]
mod environment;
#[path = "compiler_execution_template.rs"]
mod template;
pub use template::quote_literal_run_arg;
#[cfg(test)]
#[path = "compiler_execution_binding_tests.rs"]
mod binding_tests;
#[cfg(test)]
#[path = "compiler_execution_tests.rs"]
mod tests;

/// Finite compiler selection; this metadata is never executable authority alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompilerDriver {
    /// Rust's Cargo compiler route.
    Cargo,
    /// The qualified MBX compiler route.
    Mbx,
}

impl CompilerDriver {
    /// Exact compiler program name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Cargo => "cargo",
            Self::Mbx => "mbx",
        }
    }
}

/// Closed compiler operations; formatting has no compiler authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RustCompilerOperation {
    /// Compile and lint through Clippy.
    Clippy,
    /// Compile and execute Cargo tests.
    Test,
    /// Compile and execute Nextest tests.
    Nextest,
    /// Compile documentation tests.
    Doctest,
    /// Compile documentation.
    Doc,
    /// Compile test binaries or build artifacts.
    Build,
}

impl RustCompilerOperation {
    /// Exact task kind bound to this operation.
    #[must_use]
    pub const fn task_kind(self) -> &'static str {
        match self {
            Self::Clippy => "clippy",
            Self::Test => "test",
            Self::Nextest => "nextest",
            Self::Doctest => "doctest",
            Self::Doc => "doc",
            Self::Build => "build",
        }
    }
    fn accepts(self, payload: &[String]) -> bool {
        if !payload.iter().any(|v| v == "--locked")
            || !payload.iter().any(|v| v == "--offline")
            || payload
                .iter()
                .any(|v| matches!(v.as_str(), "--help" | "--version"))
        {
            return false;
        }
        match (self, payload.first().map(String::as_str)) {
            (Self::Clippy, Some("clippy")) => true,
            (Self::Test, Some("test")) => !payload
                .iter()
                .any(|v| matches!(v.as_str(), "--doc" | "--no-run")),
            (Self::Doctest, Some("test")) => payload.iter().any(|v| v == "--doc"),
            (Self::Doc, Some("doc")) => payload.iter().any(|v| v == "--no-deps"),
            (Self::Build, Some("test")) => payload.iter().any(|v| v == "--no-run"),
            (Self::Nextest, Some("nextest")) => payload.get(1).is_some_and(|v| v == "run"),
            (Self::Build, Some("nextest")) => payload.get(1).is_some_and(|v| v == "list"),
            _ => false,
        }
    }
}

/// Finite tool slots selected by the original compiler adapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RustCompilerTools {
    /// Exact Rust selector.
    pub rust: String,
    /// Exact MBX selector, present only on the MBX route.
    pub mbx: Option<String>,
    /// Exact Nextest selector where the original profile requires it.
    pub nextest: Option<String>,
}

/// Canonical compiler recipe, without arbitrary shell or source inputs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompiledRustReportRecipe {
    driver: CompilerDriver,
    operation: RustCompilerOperation,
    tools: RustCompilerTools,
    payload: Vec<String>,
    compiler_argv: Vec<String>,
    toolchain_id: String,
    expected_task_digest: String,
}

impl CompiledRustReportRecipe {
    /// Rebuild the canonical direct Mise command from finite compiler slots.
    /// # Errors
    /// Rejects foreign selectors, payload heads, shell expansion or identities.
    pub fn compiled(
        driver: CompilerDriver,
        operation: RustCompilerOperation,
        tools: RustCompilerTools,
        payload: Vec<String>,
        toolchain_id: String,
        expected_task_digest: String,
    ) -> Result<Self, ContractError> {
        validate_rust_selector(&tools.rust)?;
        match (driver, &tools.mbx) {
            (CompilerDriver::Mbx, Some(selector)) => validate_selector(selector, "mr-boxington@")?,
            (CompilerDriver::Cargo, None) => {}
            _ => return Err(invalid("driver_tools")),
        }
        if let Some(selector) = &tools.nextest {
            validate_selector(selector, "aqua:nextest-rs/nextest/cargo-nextest@")?;
        }
        if payload.is_empty() || !operation.accepts(&payload) || payload.iter().any(|v| !literal(v))
        {
            return Err(invalid("compiler_recipe"));
        }
        if payload.first().is_some_and(|v| v == "nextest") && tools.nextest.is_none() {
            return Err(invalid("nextest_tools"));
        }
        if payload.len() > 2030
            || payload.iter().map(String::len).sum::<usize>()
                > super::SOURCE_HELPER_ARGUMENT_BYTES_MAX
        {
            return Err(invalid("compiler_bounds"));
        }
        crate::validate_digest(&toolchain_id)?;
        crate::validate_digest(&expected_task_digest)?;
        let original_tools = tools.clone();
        let original_payload = payload.clone();
        let mut argv = ["mise", "--no-config", "--no-env", "--no-hooks", "exec"]
            .map(str::to_owned)
            .to_vec();
        argv.push(tools.rust);
        argv.extend(tools.mbx);
        argv.extend(tools.nextest);
        argv.push("--".to_owned());
        argv.push(driver.as_str().to_owned());
        argv.extend(payload);
        Ok(Self {
            driver,
            operation,
            tools: original_tools,
            payload: original_payload,
            compiler_argv: argv,
            toolchain_id,
            expected_task_digest,
        })
    }
    fn validate(&self) -> Result<(), ContractError> {
        let expected = Self::compiled(
            self.driver,
            self.operation,
            self.tools.clone(),
            self.payload.clone(),
            self.toolchain_id.clone(),
            self.expected_task_digest.clone(),
        )?;
        if expected != *self {
            return Err(invalid("compiler_recipe_changed"));
        }
        Ok(())
    }
    /// Complete canonical command, compared independently with the adapter vector.
    #[must_use]
    pub fn compiler_argv(&self) -> &[String] {
        &self.compiler_argv
    }
    /// Exact source invocation frame, including compiler identity.
    #[must_use]
    pub fn invocation_arguments(&self) -> Vec<String> {
        let mut args = vec![
            self.driver.as_str().to_owned(),
            self.toolchain_id.clone(),
            self.expected_task_digest.clone(),
        ];
        args.extend(self.compiler_argv.clone());
        args
    }
}

/// Validated report coordinates, separate from the task digest's preimage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RustReportFrame {
    version: String,
    task_id: String,
    matrix_key: String,
}

impl RustReportFrame {
    /// Bind the fixed helper version and matrix coordinates to one Rust task.
    /// # Errors
    /// Rejects invalid versions or task/matrix identities.
    pub fn compiled(version: &str, task_id: &str, matrix_key: &str) -> Result<Self, ContractError> {
        crate::generated_source(version, "")?;
        validate_task_id(task_id)?;
        let id = matrix_id_for_task_group("rust", task_id)?;
        if task_id.split('/').nth(1) != Some("rust") || matrix_key_for_id(&id)? != matrix_key {
            return Err(invalid("report_frame"));
        }
        Ok(Self {
            version: version.to_owned(),
            task_id: task_id.to_owned(),
            matrix_key: matrix_key.to_owned(),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct CompiledCompilerBinding {
    recipe: CompiledRustReportRecipe,
    frame: RustReportFrame,
    invocation: HelperInvocation,
    source: String,
    environment: BTreeMap<String, String>,
}

impl CompiledSourceHelper {
    /// Issue compiler authority only for the internally rebuilt guarded template.
    /// # Errors
    /// Rejects mismatched operation, compiler frame or owned environment.
    pub fn rust_report_wrapper(
        recipe: CompiledRustReportRecipe,
        frame: RustReportFrame,
        environment: BTreeMap<String, String>,
    ) -> Result<Self, ContractError> {
        recipe.validate()?;
        environment::validate(&recipe, &environment)?;
        let base = crate::split_shard_suffix(&frame.task_id)
            .map_or(frame.task_id.as_str(), |(base, _, _)| base);
        if base.rsplit('/').nth(1) != Some(recipe.operation.task_kind()) {
            return Err(invalid("operation_task"));
        }
        if crate::canonical_task_digest(
            &frame.task_id,
            &recipe.compiler_argv,
            &recipe.toolchain_id,
            None,
            None,
        )? != recipe.expected_task_digest
        {
            return Err(invalid("task_digest"));
        }
        let argv =
            serde_json::to_string(recipe.compiler_argv()).map_err(|_| invalid("frame_json"))?;
        let matrix_id = crate::matrix_id_for_task_group("rust", &frame.task_id)?;
        for (key, value) in [
            ("VELNOR_TASK_ID", frame.task_id.as_str()),
            ("VELNOR_TASK_DIGEST", recipe.expected_task_digest.as_str()),
            ("VELNOR_MATRIX_KEY", frame.matrix_key.as_str()),
            ("VELNOR_MATRIX_ID", matrix_id.as_str()),
            ("VELNOR_RUST_FRAME_ARGV_JSON", argv.as_str()),
            ("VELNOR_RUST_FRAME_TOOLCHAIN", recipe.toolchain_id.as_str()),
        ] {
            if environment.get(key).map(String::as_str) != Some(value) {
                return Err(invalid("frame_environment"));
            }
        }
        let source = template::source(&recipe, &frame)?;
        let operation = SourceBoundOperation::RustReportWrapper;
        let digest = super::compiled_source_sha256(source.as_bytes());
        let path = format!("{}{digest}.sh", operation.path());
        let descriptor = SourceBoundHelper::compiled(operation, &path, &digest)?;
        let invocation =
            HelperInvocation::compiled(descriptor, recipe.invocation_arguments(), Vec::new())?;
        let mut record = Self::compiled(invocation.clone(), source.clone())?
            .with_environment(environment.clone());
        record.compiler_binding = Some(CompiledCompilerBinding {
            recipe,
            frame,
            invocation,
            source,
            environment,
        });
        record.validate_binding()?;
        Ok(record)
    }
    /// Finite execution authority held by the named compiled owner.
    #[must_use]
    pub fn compiler_driver(&self) -> Option<CompilerDriver> {
        self.compiler_binding
            .as_ref()
            .map(|binding| binding.recipe.driver)
    }
}

pub(super) fn validate(record: &CompiledSourceHelper) -> Result<(), ContractError> {
    if let Some(binding) = &record.compiler_binding {
        binding.recipe.validate()?;
        if binding.frame
            != RustReportFrame::compiled(
                &binding.frame.version,
                &binding.frame.task_id,
                &binding.frame.matrix_key,
            )?
            || binding.source != template::source(&binding.recipe, &binding.frame)?
            || binding.invocation.args() != binding.recipe.invocation_arguments()
        {
            return Err(invalid("compiler_template_changed"));
        }
        if record.invocation != binding.invocation
            || record.source != binding.source
            || record.environment != binding.environment
        {
            return Err(invalid("compiler_binding"));
        }
    } else if record.invocation.descriptor().operation() == SourceBoundOperation::RustReportWrapper
    {
        return Err(invalid("compiler_authority_missing"));
    }
    Ok(())
}

fn validate_selector(selector: &str, family: &str) -> Result<(), ContractError> {
    let version = selector
        .strip_prefix(family)
        .ok_or_else(|| invalid("tool_selector"))?;
    if version.is_empty()
        || !version.bytes().all(|b| b.is_ascii_digit() || b == b'.')
        || version.split('.').any(str::is_empty)
    {
        return Err(invalid("tool_version"));
    }
    Ok(())
}

fn validate_rust_selector(selector: &str) -> Result<(), ContractError> {
    let (prefix, version) = selector
        .rsplit_once('@')
        .ok_or_else(|| invalid("rust_selector"))?;
    if prefix != "rust[profile=minimal,components=clippy,rustfmt]" {
        return Err(invalid("rust_options"));
    }
    validate_selector(&format!("rust@{version}"), "rust@")
}

fn literal(value: &str) -> bool {
    !value.is_empty()
        && !value.chars().any(char::is_control)
        && !value.contains(['$', '`', '\0'])
        && !value.contains("${{")
}

fn invalid(reason: &str) -> ContractError {
    ContractError::identity("rust_report_wrapper", reason)
}
