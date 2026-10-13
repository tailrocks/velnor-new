//! Fresh Mise homes prove each generated validator installs before execution.

#[cfg(unix)]
mod unix {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;
    use std::process::{Command, Output};

    use velnor_actions_contract::{Step, StepKind};
    use velnor_actions_orchestrator::prepare;

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
        Ok(command
            .args(&argv[1..])
            .env("PATH", path)
            .env("VELNOR_MISE_STUB_LOG", log)
            .env("MISE_AUTO_INSTALL", "false")
            .env("MISE_EXEC_AUTO_INSTALL", "false")
            .output()?)
    }

    fn verify_cold_pair(install: &[String], execute: &[String], root: &Path) -> TestResult {
        let bin = root.join("mise-bin");
        fs::create_dir_all(&bin)?;
        let stub = bin.join("mise");
        fs::write(
            &stub,
            "#!/bin/sh\nset -eu\nverb=\nselector=\nfor arg do\n  case $arg in install|exec) verb=$arg ;; *@*) selector=$arg ;; esac\ndone\ncase $verb in\n  install) printf '%s\\n' \"$selector\" >> \"$VELNOR_MISE_STUB_LOG\" ;;\n  exec) grep -Fqx \"$selector\" \"$VELNOR_MISE_STUB_LOG\" ;;\n  *) exit 2 ;;\nesac\n",
        )?;
        let mut mode = fs::metadata(&stub)?.permissions();
        mode.set_mode(0o755);
        fs::set_permissions(&stub, mode)?;
        let log = root.join("installed-tools");
        fs::write(&log, "")?;
        assert!(
            !run_with_mise(execute, &bin, &log)?.status.success(),
            "empty tool home must fail closed before install"
        );
        assert!(
            run_with_mise(install, &bin, &log)?.status.success(),
            "explicit pinned installation"
        );
        assert!(
            run_with_mise(execute, &bin, &log)?.status.success(),
            "validator runs after exact pinned installation"
        );
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
                    if command.prepare_argv.is_empty() {
                        continue;
                    }
                    let child = tempfile::tempdir()?;
                    verify_cold_pair(&command.prepare_argv, &command.argv, child.path())?;
                }
                Ok(())
            },
        )
    }
}
