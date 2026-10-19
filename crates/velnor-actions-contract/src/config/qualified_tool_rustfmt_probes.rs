//! Formatter banners bind an independently versioned component to its compiler.
use super::{
    QualifiedToolBackend, QualifiedToolExecutableRecipe, QualifiedToolOptions, QualifiedToolProbe,
    QualifiedToolRecipe, QualifiedToolRecipePlatform,
};
use crate::errors::ContractError;

pub(super) fn validate(
    recipe: &QualifiedToolRecipe,
    platform: &QualifiedToolRecipePlatform,
    executable: &QualifiedToolExecutableRecipe,
    file: &str,
    key: &str,
) -> Result<(), ContractError> {
    let bad = || ContractError::config(file, key, "invalid_qualified_tool_probe");
    if !matches!(&recipe.backend, QualifiedToolBackend::Core { tool } if tool == "rust")
        || !matches!(&recipe.options, QualifiedToolOptions::Rust { components, .. }
            if components.iter().any(|component| component == "rustfmt"))
        || !matches!(executable.name.as_str(), "rustfmt" | "cargo-fmt")
        || executable.path != format!("bin/{}", executable.name)
    {
        return Err(bad());
    }
    let QualifiedToolProbe::RustfmtVersion { expected } = &executable.probe else {
        return Err(bad());
    };
    let (prefix, date) = formatter_identity(expected).ok_or_else(bad)?;
    let mut compilers = platform
        .executables
        .iter()
        .filter(|candidate| candidate.name == "rustc" && candidate.path == "bin/rustc");
    let compiler = compilers.next().ok_or_else(bad)?;
    if compilers.next().is_some() {
        return Err(bad());
    }
    super::probes::validate_recipe_probe(&recipe.version, platform.platform, compiler, file, key)?;
    let QualifiedToolProbe::RustcVerbose { expected } = &compiler.probe else {
        return Err(bad());
    };
    let commit = unique_field(expected, "commit-hash: ").ok_or_else(bad)?;
    let compiler_date = unique_field(expected, "commit-date: ").ok_or_else(bad)?;
    if commit.len() != 40
        || !lower_hex(commit)
        || !commit.starts_with(prefix)
        || compiler_date != date
        || !iso_date(compiler_date)
    {
        return Err(bad());
    }
    Ok(())
}

fn unique_field<'a>(output: &'a str, prefix: &str) -> Option<&'a str> {
    let mut values = output.lines().filter_map(|line| line.strip_prefix(prefix));
    let value = values.next()?;
    values.next().is_none().then_some(value)
}

fn lower_hex(value: &str) -> bool {
    value
        .bytes()
        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn iso_date(value: &str) -> bool {
    value.len() == 10
        && value.as_bytes()[4] == b'-'
        && value.as_bytes()[7] == b'-'
        && super::sources::valid_release_date(&value.replace('-', ""))
}

fn formatter_identity(output: &str) -> Option<(&str, &str)> {
    if output.len() > crate::config::MAX_CHECK_QUALIFIED_PROBE_EXPECTED_BYTES {
        return None;
    }
    let rest = output.strip_prefix("rustfmt ")?;
    let (version, identity) = rest.split_once("-stable (")?;
    if !super::recipe_validation::exact_version(version) {
        return None;
    }
    let (commit, date) = identity.strip_suffix(')')?.split_once(' ')?;
    (commit.len() == 10 && lower_hex(commit) && iso_date(date)).then_some((commit, date))
}

#[cfg(test)]
#[path = "qualified_tool_rustfmt_probes_tests.rs"]
mod tests;
