use super::{EnvPolicy, IsolatedCommand, NO_AUTO_INSTALL_ENV, pairs_of};
use crate::MiseError;
use crate::checks::{QualifiedCheck, invalid};
use std::ffi::OsString;

impl IsolatedCommand {
    pub(crate) fn qualified_check_probe(
        program: OsString,
        args: Vec<OsString>,
        env: Vec<(OsString, OsString)>,
    ) -> Self {
        Self {
            program,
            args,
            cwd: None,
            extra_env: env,
            policy: EnvPolicy::QualifiedProbe,
        }
    }
    pub(crate) fn qualified_check_run(
        owned: &QualifiedCheck,
        deadline: crate::CheckDeadline,
    ) -> Result<Self, MiseError> {
        let found = crate::checks::file_read::read_text(
            &owned.config,
            velnor_actions_contract::MAX_CHECK_SOURCE_BYTES,
            Some(deadline),
            "check_config",
        )?;
        if found != owned.bound_projection()? {
            return Err(invalid("check_config", "projection_changed"));
        }
        let program = verified_mise_program(owned, deadline)?;
        let mut extra_env = owned.owned_env()?;
        extra_env.extend(pairs_of(&NO_AUTO_INSTALL_ENV));
        let mut args: Vec<OsString> = [
            "--no-env",
            "--no-hooks",
            "run",
            "--force",
            "--task-cache",
            "off",
            "--skip-tools",
        ]
        .into_iter()
        .map(OsString::from)
        .collect();
        args.push(OsString::from(&owned.check.check.task));
        Ok(Self {
            program,
            args,
            cwd: Some(owned.cwd.clone()),
            extra_env,
            policy: EnvPolicy::QualifiedCheck,
        })
    }
}

fn verified_mise_program(
    owned: &QualifiedCheck,
    deadline: crate::CheckDeadline,
) -> Result<OsString, MiseError> {
    let program = owned.mise_program().into_os_string();
    let probe = IsolatedCommand::qualified_check_probe(
        program.clone(),
        ["--no-config", "--no-env", "--no-hooks", "version"]
            .iter()
            .map(OsString::from)
            .collect(),
        owned.owned_env()?,
    );
    let output = probe.run_until(64 * 1024, deadline)?;
    let version = String::from_utf8_lossy(&output.stdout);
    if !output.success || version.split_whitespace().next() != Some(crate::MISE_VERSION) {
        return Err(invalid(
            "mise_binary",
            format!("expected_version:{}", crate::MISE_VERSION),
        ));
    }
    Ok(program)
}
/// Resolve an installed executable before consumer task evaluation.
/// # Errors
/// Fails for unavailable executable or missing parent search path.
pub fn resolve_program(name: &str) -> Result<OsString, MiseError> {
    let path = std::env::var_os("PATH").ok_or_else(|| invalid("mise_binary", "missing_path"))?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return candidate
                .canonicalize()
                .map(std::path::PathBuf::into_os_string)
                .map_err(|e| invalid("mise_binary", e.to_string()));
        }
    }
    Err(invalid("capability_binary", format!("not_found:{name}")))
}
