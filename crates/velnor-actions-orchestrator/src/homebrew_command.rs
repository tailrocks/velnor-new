use std::{
    ffi::{OsStr, OsString},
    path::{Component, Path, PathBuf},
};

use velnor_actions_mise::IsolatedCommand;

use crate::{
    OrchestratorError, homebrew::install::HOMEBREW_CANDIDATE_FORMULAE, internal::internal,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum HomebrewCommand {
    Version,
    Config,
    InstallPortableRuby,
    MovePrefix {
        source: PathBuf,
        destination: PathBuf,
    },
    Prefix,
    Cellar,
    InstallCandidate {
        recipe: PathBuf,
    },
    TrustFormula {
        formula: String,
    },
    UpgradeCandidate {
        recipe: PathBuf,
    },
    TestArchived {
        recipe: PathBuf,
    },
    InstalledBinaryVersion {
        executable: PathBuf,
    },
}

impl HomebrewCommand {
    pub(crate) const fn diagnostic_label(&self) -> &'static str {
        match self {
            Self::Version => "version",
            Self::Config => "config",
            Self::InstallPortableRuby => "install-portable-ruby",
            Self::MovePrefix { .. } => "move-prefix",
            Self::Prefix => "prefix",
            Self::Cellar => "cellar",
            Self::InstallCandidate { .. } => "install-candidate",
            Self::TrustFormula { .. } => "trust-formula",
            Self::UpgradeCandidate { .. } => "upgrade-candidate",
            Self::TestArchived { .. } => "test-archived",
            Self::InstalledBinaryVersion { .. } => "installed-binary-version",
        }
    }
}

pub(crate) fn homebrew_command(
    operation: HomebrewCommand,
    home: &Path,
    temp: &Path,
) -> Result<IsolatedCommand, OrchestratorError> {
    validate_absolute_path(home, "home")?;
    validate_absolute_path(temp, "temp")?;
    let mut args = Vec::<OsString>::new();
    let program = match operation {
        HomebrewCommand::Version => {
            args.push("--version".into());
            HOMEBREW_EXECUTABLE
        }
        HomebrewCommand::Config => {
            args.push("config".into());
            HOMEBREW_EXECUTABLE
        }
        HomebrewCommand::InstallPortableRuby => {
            args.extend(["vendor-install".into(), "ruby".into()]);
            HOMEBREW_EXECUTABLE
        }
        HomebrewCommand::MovePrefix {
            source,
            destination,
        } => {
            let runner_temp = std::env::var_os("RUNNER_TEMP")
                .map(PathBuf::from)
                .filter(|path| path.is_absolute())
                .ok_or_else(|| internal("homebrew_command_runner_temp"))?;
            validate_prefix_move(&source, &destination, &runner_temp)?;
            args.extend([
                "-n".into(),
                "/bin/mv".into(),
                source.into_os_string(),
                destination.into_os_string(),
            ]);
            return isolated_move_command(args, home, temp);
        }
        HomebrewCommand::Prefix => {
            args.push("--prefix".into());
            HOMEBREW_EXECUTABLE
        }
        HomebrewCommand::Cellar => {
            args.push("--cellar".into());
            HOMEBREW_EXECUTABLE
        }
        HomebrewCommand::InstallCandidate { recipe } => {
            validate_candidate_formula_path(&recipe)?;
            args.extend([
                "install".into(),
                "--formula".into(),
                recipe.into_os_string(),
            ]);
            HOMEBREW_EXECUTABLE
        }
        HomebrewCommand::TrustFormula { formula } => {
            validate_candidate_formula(&formula)?;
            args.extend(["trust".into(), "--formula".into(), formula.into()]);
            HOMEBREW_EXECUTABLE
        }
        HomebrewCommand::UpgradeCandidate { recipe } => {
            validate_candidate_formula_path(&recipe)?;
            args.extend([
                "upgrade".into(),
                "--formula".into(),
                "--no-ask".into(),
                recipe.into_os_string(),
            ]);
            HOMEBREW_EXECUTABLE
        }
        HomebrewCommand::TestArchived { recipe } => {
            validate_formula_path(&recipe)?;
            args.extend(["test".into(), "--force".into(), recipe.into_os_string()]);
            HOMEBREW_EXECUTABLE
        }
        HomebrewCommand::InstalledBinaryVersion { executable } => {
            validate_absolute_path(&executable, "executable")?;
            args.push("--version".into());
            return isolated_command(executable.as_os_str(), args, home, temp);
        }
    };
    isolated_command(std::ffi::OsStr::new(program), args, home, temp)
}

fn validate_prefix_move(
    source: &Path,
    destination: &Path,
    runner_temp: &Path,
) -> Result<(), OrchestratorError> {
    validate_absolute_path(source, "prefix_move_source")?;
    validate_absolute_path(destination, "prefix_move_destination")?;
    let prefix = Path::new("/opt/homebrew");
    let source_name = source.file_name().and_then(|value| value.to_str());
    let destination_name = destination.file_name().and_then(|value| value.to_str());
    let valid_temp_child = |path: &Path, prefix: &str| {
        path.parent() == Some(runner_temp)
            && path
                .file_name()
                .and_then(|value| value.to_str())
                .is_some_and(|name| {
                    name.strip_prefix(prefix).is_some_and(|suffix| {
                        !suffix.is_empty()
                            && suffix
                                .bytes()
                                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
                    })
                })
    };
    let source_is_stage = valid_temp_child(source, "velnor-homebrew-stage-");
    let destination_is_stage = valid_temp_child(destination, "velnor-homebrew-stage-");
    let source_is_previous = source_name == Some("previous")
        && source
            .parent()
            .is_some_and(|parent| valid_temp_child(parent, "velnor-homebrew-recovery-"));
    let destination_is_previous = destination_name == Some("previous")
        && destination
            .parent()
            .is_some_and(|parent| valid_temp_child(parent, "velnor-homebrew-recovery-"));
    let allowed = (source == prefix && destination_is_previous)
        || (source_is_stage && destination == prefix)
        || (source_is_previous && destination == prefix)
        || (source == prefix && destination_is_stage);
    if !allowed {
        return Err(internal("homebrew_command_prefix_move"));
    }
    Ok(())
}

fn isolated_move_command(
    args: Vec<OsString>,
    home: &Path,
    temp: &Path,
) -> Result<IsolatedCommand, OrchestratorError> {
    let env = [
        ("HOME", home.as_os_str().to_owned()),
        ("TMPDIR", temp.as_os_str().to_owned()),
        ("PATH", "/usr/bin:/bin:/usr/sbin:/sbin".into()),
    ]
    .into_iter()
    .map(|(key, value)| (OsString::from(key), value))
    .collect::<Vec<_>>();
    IsolatedCommand::repo_task("/usr/bin/sudo", args, &[])
        .and_then(|command| command.with_env(&env))
        .map_err(|_| internal("homebrew_command_rejected"))
}

const HOMEBREW_EXECUTABLE: &str = "/opt/homebrew/bin/brew";

fn isolated_command(
    program: &OsStr,
    args: Vec<OsString>,
    home: &Path,
    temp: &Path,
) -> Result<IsolatedCommand, OrchestratorError> {
    let program = program
        .to_str()
        .ok_or_else(|| internal("homebrew_command_program_not_utf8"))?;
    let env = [
        ("HOME", home.as_os_str().to_owned()),
        ("TMPDIR", temp.as_os_str().to_owned()),
        ("PATH", "/usr/bin:/bin:/usr/sbin:/sbin".into()),
        ("HOMEBREW_NO_AUTO_UPDATE", "1".into()),
        ("HOMEBREW_NO_INSTALL_CLEANUP", "1".into()),
        ("HOMEBREW_NO_ANALYTICS", "1".into()),
        ("HOMEBREW_NO_ENV_HINTS", "1".into()),
        ("HOMEBREW_NO_INSTALL_FROM_API", "1".into()),
        ("HOMEBREW_NO_COLOR", "1".into()),
        ("HOMEBREW_NO_EMOJI", "1".into()),
        ("HOMEBREW_GIT_PATH", "/usr/bin/git".into()),
        ("HOMEBREW_CURL_PATH", "/usr/bin/curl".into()),
        ("HOMEBREW_FORCE_VENDOR_RUBY", "1".into()),
        ("HOMEBREW_NO_SUDO", "1".into()),
    ]
    .into_iter()
    .map(|(key, value)| (OsString::from(key), value))
    .collect::<Vec<_>>();
    IsolatedCommand::repo_task(program, args, &[])
        .and_then(|command| command.with_env(&env))
        .map_err(|_| internal("homebrew_command_rejected"))
}

fn validate_formula_path(path: &Path) -> Result<(), OrchestratorError> {
    validate_absolute_path(path, "recipe")?;
    if path.extension().and_then(|extension| extension.to_str()) != Some("rb") {
        return Err(internal("homebrew_command_recipe_extension"));
    }
    Ok(())
}

fn validate_candidate_formula_path(path: &Path) -> Result<(), OrchestratorError> {
    validate_formula_path(path)?;
    let formula = path.file_stem().and_then(OsStr::to_str).unwrap_or_default();
    if path.parent() != Some(Path::new(HOMEBREW_CANDIDATE_FORMULAE))
        || !valid_candidate_formula_name(formula)
    {
        return Err(internal("homebrew_command_candidate_formula_path"));
    }
    Ok(())
}

fn validate_candidate_formula(formula: &str) -> Result<(), OrchestratorError> {
    let Some(name) = formula.strip_prefix("velnor/native-install/") else {
        return Err(internal("homebrew_command_candidate_formula"));
    };
    if !valid_candidate_formula_name(name) {
        return Err(internal("homebrew_command_candidate_formula"));
    }
    Ok(())
}

pub(super) fn valid_candidate_formula_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name.as_bytes()[0].is_ascii_lowercase()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

fn validate_absolute_path(path: &Path, field: &'static str) -> Result<(), OrchestratorError> {
    if !path.is_absolute()
        || path.as_os_str().is_empty()
        || path.to_str().is_none()
        || !path
            .components()
            .any(|component| matches!(component, Component::Normal(_)))
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::CurDir | Component::Prefix(_)
            )
        })
        || path
            .as_os_str()
            .to_string_lossy()
            .bytes()
            .any(|byte| byte.is_ascii_control())
    {
        return Err(internal(&format!("homebrew_command_invalid_{field}")));
    }
    Ok(())
}
