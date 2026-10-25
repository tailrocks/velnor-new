use crate::BunError;

pub fn check_package_manager(
    package_manager: Option<&str>,
    mise_bun: &str,
) -> Result<(), BunError> {
    let Some(declared) = package_manager else {
        return Ok(());
    };
    let Some(rest) = declared.strip_prefix("bun@") else {
        return Err(BunError::new("unsupported_package_manager", declared));
    };
    let version = rest.split_once('+').map_or(rest, |(version, _)| version);
    if version == mise_bun {
        Ok(())
    } else {
        Err(BunError::new("conflicting_project_tool_versions", "bun"))
    }
}
