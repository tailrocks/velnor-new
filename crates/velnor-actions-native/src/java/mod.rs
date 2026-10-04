//! Closed Java validation goals, wrapper evidence and source-bound projects.

use velnor_actions_contract::ContractError;
mod postgres;
pub use postgres::{DatabaseInitialization, FixtureBinding, FixtureValue, postgres_initialization};
mod settings;
mod wrapper;
pub use wrapper::{WrapperAuthority, WrapperEvidence, validate_wrapper};

/// Source evidence only; plugins and arbitrary build logic remain runtime obligations.
/// # Errors
/// Rejects unsupported settings syntax or projects absent from literal includes.
pub fn validate_settings(source: &str, project: Option<&str>) -> Result<(), ContractError> {
    settings::validate(source, project).map_err(invalid)
}

/// Canonical project selector to its conventional source directory.
/// # Errors
/// Rejects malformed or unsafe project selectors.
pub fn project_path(project: &str) -> Result<String, ContractError> {
    settings::project_path(project).map_err(invalid)
}

/// Fixed root or module check, optionally preceded by migration and code generation.
/// # Errors
/// Rejects selectors without a canonical colon prefix or safe module segments.
pub fn phases(
    project: Option<&str>,
    database: bool,
) -> Result<Vec<(&'static str, Vec<String>)>, ContractError> {
    if let Some(project) = project {
        if !project.starts_with(':') {
            return Err(invalid("gradle_project_selector_invalid"));
        }
        project_path(project)?;
    }
    let mut args = vec!["./gradlew".to_owned()];
    if database {
        args.extend(["--no-parallel", "flywayMigrate", "jooqCodegen"].map(str::to_owned));
    }
    args.push(project.map_or_else(|| "check".to_owned(), |name| format!("{name}:check")));
    args.push("--no-daemon".to_owned());
    Ok(vec![("gradle-check", args)])
}

pub(super) fn invalid(problem: &str) -> ContractError {
    ContractError::identity("gradle", problem)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_checks_preserve_root_and_module_vectors() -> Result<(), ContractError> {
        assert_eq!(
            phases(None, false)?[0].1,
            ["./gradlew", "check", "--no-daemon"]
        );
        assert_eq!(
            phases(Some(":domain"), false)?[0].1,
            ["./gradlew", ":domain:check", "--no-daemon"]
        );
        assert_eq!(
            phases(Some(":domain"), true)?[0].1,
            [
                "./gradlew",
                "--no-parallel",
                "flywayMigrate",
                "jooqCodegen",
                ":domain:check",
                "--no-daemon"
            ]
        );
        for project in [
            "domain",
            ":",
            "::domain",
            ":../domain",
            ":domain:test --scan",
        ] {
            assert!(phases(Some(project), false).is_err());
        }
        Ok(())
    }
}
