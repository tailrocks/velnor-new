//! Cold MBX pre-seed tool installation coverage.

#[cfg(unix)]
mod unix {
    use std::collections::BTreeMap;
    use std::ffi::OsString;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;
    use std::process::{Command, Output};

    use velnor_actions_actionlint::actions::MR_BOXINGTON_ACTION_SHA;
    use velnor_actions_contract::{Step, StepKind};
    use velnor_actions_mise::{PinnedTool, PreparePinnedTools, ToolCatalog, ToolHomes};
    use velnor_actions_workflow_renderer::{CompileDriver, mbx_steps_for_driver};

    use crate::impl_common::TestResult;

    const MISE_STUB: &str = r#"#!/bin/sh
set -eu
operation=
while [ "$#" -gt 0 ]; do
    case "$1" in install|where) operation=$1; shift; break ;; esac
    shift
done
case "$operation" in
    install)
        for selector do
            printf 'install:%s\n' "$selector" >> "$VELNOR_MISE_STUB_LOG"
            case "$selector" in
                mr-boxington@*)
                    mkdir -p "$VELNOR_MISE_STATE/mbx-root"
                    cp "$VELNOR_MBX_TEMPLATE" "$VELNOR_MISE_STATE/mbx-root/mbx"
                    chmod +x "$VELNOR_MISE_STATE/mbx-root/mbx"
                    touch "$VELNOR_MISE_STATE/mbx-installed"
                    ;;
                rust@*)
                    mkdir -p "$VELNOR_MISE_STATE/rust-root"
                    cp "$VELNOR_RUST_TEMPLATE" "$VELNOR_MISE_STATE/rust-root/rustc"
                    chmod +x "$VELNOR_MISE_STATE/rust-root/rustc"
                    touch "$VELNOR_MISE_STATE/rust-installed"
                    ;;
            esac
        done
        ;;
    where)
        selector=$1
        printf 'where:%s\n' "$selector" >> "$VELNOR_MISE_STUB_LOG"
        case "$selector" in
            mr-boxington@*) marker=mbx-installed; root=mbx-root ;;
            rust@*) marker=rust-installed; root=rust-root ;;
            *) exit 3 ;;
        esac
        [ -e "$VELNOR_MISE_STATE/$marker" ] || exit 1
        printf '%s\n' "$VELNOR_MISE_STATE/$root"
        ;;
    *) exit 2 ;;
esac
"#;
    const MBX_STUB: &str = "#!/bin/sh\nprintf '%s\\n' 'mbx 1.21.1'\n";
    const RUSTC_STUB: &str = "#!/bin/sh\nprintf '%s\\n' 'rustc 1.98.1' 'host: x86_64-unknown-linux-gnu' 'release: 1.98.1'\n";

    #[test]
    fn cargo_only_preseed_installs_mbx_before_cold_preflight() -> TestResult {
        let temp = tempfile::tempdir()?;
        let state = temp.path().join("empty-mise-state");
        let stub_bin = temp.path().join("stub-bin");
        fs::create_dir_all(&state)?;
        fs::create_dir_all(&stub_bin)?;
        let log = temp.path().join("mise.log");
        let runner_temp = temp.path().join("runner-temp");
        fs::create_dir_all(&runner_temp)?;
        let github_path = temp.path().join("github-path");
        let mbx_template = temp.path().join("mbx-template");
        let rust_template = temp.path().join("rust-template");
        write_executable(&stub_bin.join("mise"), MISE_STUB)?;
        write_executable(&mbx_template, MBX_STUB)?;
        write_executable(&rust_template, RUSTC_STUB)?;

        let catalog = ToolCatalog::pinned();
        let rust_install = install_step("Prepare pinned tools", vec![PinnedTool::Rust], &catalog)?;
        let mbx_install = install_step(
            "Prepare pre-seed MBX",
            vec![PinnedTool::MrBoxington],
            &catalog,
        )?;
        let preflight = preflight_step(&catalog)?;
        let steps = [&rust_install, &mbx_install, &preflight];
        assert_eq!(
            steps.map(|step| step.name.as_str()),
            [
                "Prepare pinned tools",
                "Prepare pre-seed MBX",
                "Verify MBX and Rust toolchains"
            ]
        );
        assert!(
            !shell_run(&rust_install)?
                .join(" ")
                .contains("mr-boxington@")
        );
        assert!(
            shell_run(&mbx_install)?
                .join(" ")
                .contains("mr-boxington@1.21.1")
        );

        let env = runner_env(
            &stub_bin,
            &state,
            &log,
            &mbx_template,
            &rust_template,
            &runner_temp,
            &github_path,
        )?;
        assert!(
            !run_step(&preflight, &env)?.status.success(),
            "cold MBX lookup must fail"
        );
        assert!(
            run_step(&rust_install, &env)?.status.success(),
            "Cargo install"
        );
        assert!(
            !run_step(&preflight, &env)?.status.success(),
            "Rust-only setup must not satisfy pre-seed MBX preflight"
        );
        assert!(
            run_step(&mbx_install, &env)?.status.success(),
            "pinned MBX install"
        );
        let ready = run_step(&preflight, &env)?;
        assert!(ready.status.success(), "preflight after install: {ready:?}");
        assert_install_precedes_lookup(&log)?;
        Ok(())
    }

    fn install_step(
        name: &str,
        tools: Vec<PinnedTool>,
        catalog: &ToolCatalog,
    ) -> Result<Step, Box<dyn std::error::Error>> {
        let prepare = PreparePinnedTools::new(tools, ToolHomes::runner_temp())
            .map_err(|err| std::io::Error::other(err.to_string()))?;
        let run = strings(prepare.argv(catalog));
        let env = strings_map(prepare.env(catalog));
        Ok(velnor_actions_workflow_renderer::ambient_shell_step(
            name, run?, env?,
        )?)
    }

    fn preflight_step(catalog: &ToolCatalog) -> Result<Step, Box<dyn std::error::Error>> {
        let uses = format!("jdx/mr-boxington-action@{MR_BOXINGTON_ACTION_SHA}");
        let homes = ToolHomes::runner_temp();
        let env = strings_map(homes.env(catalog))?;
        let steps = mbx_steps_for_driver(
            &uses,
            CompileDriver::Mbx,
            catalog.version(PinnedTool::MrBoxington),
            catalog.version(PinnedTool::Rust),
            env,
        )?
        .ok_or_else(|| std::io::Error::other("MBX driver steps missing"))?;
        steps
            .into_iter()
            .next()
            .ok_or_else(|| std::io::Error::other("preflight step missing").into())
    }

    fn strings(values: Vec<OsString>) -> Result<Vec<String>, Box<dyn std::error::Error>> {
        values
            .into_iter()
            .map(|value| {
                value
                    .into_string()
                    .map_err(|_| "non UTF-8 generated argv".into())
            })
            .collect()
    }

    fn strings_map(
        values: Vec<(OsString, OsString)>,
    ) -> Result<BTreeMap<String, String>, Box<dyn std::error::Error>> {
        values
            .into_iter()
            .map(|(key, value)| {
                Ok((
                    key.into_string().map_err(|_| "non UTF-8 env key")?,
                    value.into_string().map_err(|_| "non UTF-8 env value")?,
                ))
            })
            .collect()
    }

    fn runner_env(
        stub_bin: &Path,
        state: &Path,
        log: &Path,
        mbx_template: &Path,
        rust_template: &Path,
        runner_temp: &Path,
        github_path: &Path,
    ) -> Result<Vec<(&'static str, OsString)>, Box<dyn std::error::Error>> {
        let mut paths = vec![stub_bin.to_owned()];
        paths.extend(std::env::split_paths(
            &std::env::var_os("PATH").ok_or("test PATH")?,
        ));
        Ok(vec![
            ("PATH", std::env::join_paths(paths)?),
            ("VELNOR_MISE_STATE", state.as_os_str().to_owned()),
            ("VELNOR_MISE_STUB_LOG", log.as_os_str().to_owned()),
            ("VELNOR_MBX_TEMPLATE", mbx_template.as_os_str().to_owned()),
            ("VELNOR_RUST_TEMPLATE", rust_template.as_os_str().to_owned()),
            ("RUNNER_TEMP", runner_temp.as_os_str().to_owned()),
            ("GITHUB_PATH", github_path.as_os_str().to_owned()),
        ])
    }

    fn run_step(
        step: &Step,
        env: &[(&str, OsString)],
    ) -> Result<Output, Box<dyn std::error::Error>> {
        let StepKind::Shell { run, env: step_env } = &step.kind else {
            return Err(
                std::io::Error::other(format!("{} must be a shell step", step.name)).into(),
            );
        };
        let mut command = Command::new(&run[0]);
        command
            .args(&run[1..])
            .envs(step_env)
            .envs(env.iter().cloned());
        Ok(command.output()?)
    }

    fn shell_run(step: &Step) -> Result<&[String], Box<dyn std::error::Error>> {
        let StepKind::Shell { run, .. } = &step.kind else {
            return Err(
                std::io::Error::other(format!("{} must be a shell step", step.name)).into(),
            );
        };
        Ok(run)
    }

    fn write_executable(path: &Path, contents: &str) -> std::io::Result<()> {
        fs::write(path, contents)?;
        let mut permissions = fs::metadata(path)?.permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions)
    }

    fn assert_install_precedes_lookup(log: &Path) -> Result<(), Box<dyn std::error::Error>> {
        let events = fs::read_to_string(log)?;
        let lines = events.lines().collect::<Vec<_>>();
        let install = lines
            .iter()
            .position(|line| *line == "install:mr-boxington@1.21.1")
            .ok_or("pinned MBX install event")?;
        let lookup = lines
            .iter()
            .rposition(|line| *line == "where:mr-boxington@1.21.1")
            .ok_or("final MBX lookup event")?;
        assert!(install < lookup, "install must precede lookup: {events}");
        Ok(())
    }
}
