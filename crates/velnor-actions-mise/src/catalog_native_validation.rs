//! Closed SDK tool and environment recipes; native adapters own source programs.
use crate::{MISE_GLOBAL_FLAGS, MiseError, PinnedTool, ToolCatalog};
use std::collections::BTreeMap;
use velnor_actions_contract::{
    NativeValidationDescriptor, workflow::native_tools::CompiledNativeExecRecipe,
};

/// Exact installed tools and anonymous launch for a closed native program.
/// # Errors
/// Rejects invalid semantic descriptors or foreign catalogs.
pub fn recipe_for_descriptor(
    descriptor: &NativeValidationDescriptor,
    catalog: &ToolCatalog,
) -> Result<CompiledNativeExecRecipe, MiseError> {
    descriptor.validate().map_err(|error| contract(&error))?;
    if catalog != &ToolCatalog::pinned() {
        return Err(invalid("native_validation_foreign_catalog"));
    }
    let (tools, identity) = match descriptor {
        NativeValidationDescriptor::PackageUpdateFixture { .. } => (
            vec![PinnedTool::Ruby, PinnedTool::Jq],
            "package-update-fixture-v1".to_owned(),
        ),
        NativeValidationDescriptor::HomebrewPreparation { .. } => (
            Vec::new(),
            format!(
                "homebrew={}\nsource={}\nportable-ruby={}\nx86_64-linux={}\narm64-linux={}\naudit-targets=indexed-regular-source-v1;bare+explicit-scoped;skip-style",
                super::homebrew::VERSION,
                super::homebrew::SOURCE_SHA,
                super::homebrew::PORTABLE_RUBY_VERSION,
                super::homebrew::PORTABLE_RUBY_X86_64_LINUX_SHA256,
                super::homebrew::PORTABLE_RUBY_ARM64_LINUX_SHA256
            ),
        ),
    };
    let mut env = environment(!tools.is_empty());
    env.insert(
        "VELNOR_NATIVE_VALIDATION_RECIPE".to_owned(),
        velnor_actions_contract::digest_b3(identity.as_bytes()),
    );
    env.insert(
        "VELNOR_NATIVE_VALIDATION_SOURCE_SHA".to_owned(),
        "${{ github.sha }}".to_owned(),
    );
    execution(catalog, &tools, env)
}

/// Verify the complete SDK launch against the closed catalog owner.
/// # Errors
/// Rejects altered selector, prefix, credential scope or environment authority.
pub fn validate_recipe(
    descriptor: &NativeValidationDescriptor,
    catalog: &ToolCatalog,
    recipe: &CompiledNativeExecRecipe,
) -> Result<(), MiseError> {
    if &recipe_for_descriptor(descriptor, catalog)? != recipe {
        return Err(invalid("native_validation_recipe_mismatch"));
    }
    Ok(())
}

fn environment(managed_tools: bool) -> BTreeMap<String, String> {
    let mut env: BTreeMap<_, _> = [
        ("HOME", "${{ runner.temp }}/velnor/native-validation-home"),
        ("PATH", "/usr/bin:/bin:/usr/sbin:/sbin"),
        ("RUNNER_TEMP", "${{ runner.temp }}"),
        ("TMPDIR", "${{ runner.temp }}"),
        ("LC_ALL", "C"),
    ]
    .into_iter()
    .map(|(key, value)| (key.to_owned(), value.to_owned()))
    .collect();
    if managed_tools {
        env.extend(
            crate::command::ISOLATION_ENV
                .into_iter()
                .chain(crate::command::NO_AUTO_INSTALL_ENV)
                .map(|(key, value)| (key.to_owned(), value.to_owned())),
        );
        env.insert(
            "MISE_DATA_DIR".to_owned(),
            crate::runtime_paths::MISE_DATA_DIR.to_owned(),
        );
    }
    env
}

fn execution(
    catalog: &ToolCatalog,
    tools: &[PinnedTool],
    environment: BTreeMap<String, String>,
) -> Result<CompiledNativeExecRecipe, MiseError> {
    let selectors = catalog.tool_specs(tools)?;
    let mut prefix = vec!["/usr/bin/env".to_owned(), "-i".to_owned()];
    prefix.extend(environment.iter().map(|(key, value)| {
        format!(
            "{key}={}",
            value
                .replace("${{ runner.temp }}", "$RUNNER_TEMP")
                .replace("${{ github.sha }}", "$VELNOR_NATIVE_VALIDATION_SOURCE_SHA")
        )
    }));
    if tools.is_empty() {
        // The inner fixed env consumes the terminal separator after the outer
        // env has consumed its assignments; Bash is appended by the launcher.
        prefix.push("/usr/bin/env".to_owned());
    } else {
        prefix.push("$RUNNER_TEMP/velnor/mise/bin/mise".to_owned());
        prefix.extend(MISE_GLOBAL_FLAGS.into_iter().map(str::to_owned));
        prefix.push("exec".to_owned());
        prefix.extend(selectors.iter().cloned());
    }
    prefix.push("--".to_owned());
    if tools.is_empty() {
        CompiledNativeExecRecipe::compiled_homebrew_foundation(prefix, environment)
            .map_err(|error| contract(&error))
    } else {
        CompiledNativeExecRecipe::compiled(prefix, environment, selectors)
            .map_err(|error| contract(&error))
    }
}

fn contract(error: &velnor_actions_contract::ContractError) -> MiseError {
    invalid(&error.to_string())
}
fn invalid(problem: &str) -> MiseError {
    MiseError::Contract {
        problem: problem.to_owned(),
    }
}
#[cfg(test)]
#[path = "catalog_native_validation_tests.rs"]
mod tests;
