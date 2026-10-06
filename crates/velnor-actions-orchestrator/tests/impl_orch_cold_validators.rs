//! Fresh Mise homes prove each generated validator installs before execution.

#[cfg(unix)]
mod unix {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};
    use std::process::{Command, Output};

    use velnor_actions_contract::{Step, StepKind, ValidatorKind};
    use velnor_actions_orchestrator::prepare;
    use velnor_actions_workflow_renderer::shell_step;

    use crate::impl_common::{TestResult, without_ambient_identity};
    use crate::impl_orch_install_sets::velnor_workspace;

    fn shell_argv(steps: &[Step], name: &str) -> Result<Vec<String>, Box<dyn std::error::Error>> {
        let step = steps
            .iter()
            .find(|step| step.name == name)
            .ok_or_else(|| std::io::Error::other(format!("missing {name}")))?;
        let StepKind::Shell { run, .. } = &step.kind else {
            return Err(std::io::Error::other(format!("{name} is not a shell step")).into());
        };
        Ok(run.clone())
    }

    fn run_with_mise(
        argv: &[String],
        bin: &Path,
        log: &Path,
    ) -> Result<Output, Box<dyn std::error::Error>> {
        let mut paths = vec![bin.to_owned()];
        paths.extend(std::env::split_paths(
            &std::env::var_os("PATH").ok_or("test PATH")?,
        ));
        let path = std::env::join_paths(paths)?;
        let mut command = Command::new(&argv[0]);
        // The combined argv stages under $RUNNER_TEMP; hermetic scratch keeps
        // it off the ambient filesystem (and off a read-only /).
        let runner_temp = log.parent().unwrap_or(log).join("runner-temp");
        Ok(command
            .args(&argv[1..])
            .env("PATH", path)
            .env("RUNNER_TEMP", runner_temp)
            .env("VELNOR_MISE_STUB_LOG", log)
            .env("MISE_AUTO_INSTALL", "false")
            .env("MISE_EXEC_AUTO_INSTALL", "false")
            .env("GH_TOKEN", "")
            .env("MISE_GITHUB_TOKEN", "")
            .output()?)
    }

    fn mise_stub(root: &Path) -> Result<(PathBuf, PathBuf), Box<dyn std::error::Error>> {
        let bin = root.join("mise-bin");
        fs::create_dir_all(&bin)?;
        let stub = bin.join("mise");
        fs::write(
            &stub,
            "#!/bin/sh\nset -eu\nverb=\nscan=0\nfor arg do\n  case $arg in\n    install|exec) verb=$arg; scan=1 ;;\n    --|\\&\\&|\\;|\\|\\|) scan=0 ;;\n    *@*) if [ \"$scan\" = 1 ]; then\n      case $verb in\n        install) printf '%s\\n' \"$arg\" >> \"$VELNOR_MISE_STUB_LOG\" ;;\n        exec) grep -Fqx \"$arg\" \"$VELNOR_MISE_STUB_LOG\" ;;\n      esac\n    fi ;;\n  esac\ndone\ncase $verb in\n  install) [ \"${GH_TOKEN+x}\" = x ]; [ \"${MISE_GITHUB_TOKEN+x}\" = x ] ;;\n  exec) [ \"${GH_TOKEN+x}\" != x ]; [ \"${MISE_GITHUB_TOKEN+x}\" != x ] ;;\n  *) exit 2 ;;\nesac\n",
        )?;
        let mut mode = fs::metadata(&stub)?.permissions();
        mode.set_mode(0o755);
        fs::set_permissions(&stub, mode)?;
        let log = root.join("installed-tools");
        fs::write(&log, "")?;
        Ok((bin, log))
    }

    fn verify_cold_pair(install: &[String], execute: &[String], root: &Path) -> TestResult {
        let (bin, log) = mise_stub(root)?;
        assert!(
            !run_with_mise(execute, &bin, &log)?.status.success(),
            "empty tool home with empty GH_TOKEN must fail before install"
        );
        assert!(
            run_with_mise(install, &bin, &log)?.status.success(),
            "pinned installation keeps its inherited job environment"
        );
        assert!(
            run_with_mise(execute, &bin, &log)?.status.success(),
            "scrubbed validator runs after the exact install with tokens unset"
        );
        Ok(())
    }

    fn verify_combined_cargo_deny(argv: &[String], root: &Path) -> TestResult {
        let (bin, log) = mise_stub(root)?;
        let output = run_with_mise(argv, &bin, &log)?;
        assert!(
            output.status.success(),
            "combined Cargo Deny installs before unsetting tokens: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(fs::read_to_string(log)?, "cargo-deny@0.20.2\n");
        Ok(())
    }

    #[test]
    fn cold_validator_homes_require_emitted_pinned_installs() -> TestResult {
        without_ambient_identity(
            "cold_validator_homes_require_emitted_pinned_installs",
            || {
                let repo = velnor_workspace()?;
                let prep = prepare(repo.path())?;
                let root = tempfile::tempdir()?;
                let lint = prep.workflow.ir.jobs.get("actionlint").ok_or("lint job")?;
                verify_cold_pair(
                    &shell_argv(&lint.steps, "Prepare pinned tools")?,
                    &shell_argv(&lint.steps, "Run actionlint")?,
                    root.path(),
                )?;
                for command in &prep.workflow.context.validator_commands {
                    if command.validator == ValidatorKind::CargoDeny {
                        let child = tempfile::tempdir()?;
                        verify_combined_cargo_deny(&command.argv, child.path())?;
                    } else {
                        let child = tempfile::tempdir()?;
                        let step = shell_step(
                            &command.name,
                            command.argv.clone(),
                            std::collections::BTreeMap::new(),
                        )?;
                        let execute = shell_argv(std::slice::from_ref(&step), &command.name)?;
                        verify_cold_pair(&command.prepare_argv, &execute, child.path())?;
                    }
                }
                Ok(())
            },
        )
    }
}
