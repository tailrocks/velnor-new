//! Cold pre-seed tool-owner and Rust preflight coverage.

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
            rust@*) marker=rust-installed; root=rust-root ;;
            *) exit 3 ;;
        esac
        [ -e "$VELNOR_MISE_STATE/$marker" ] || exit 1
        printf '%s\n' "$VELNOR_MISE_STATE/$root"
        ;;
    *) exit 2 ;;
esac
"#;
    const RUSTC_STUB: &str = "#!/bin/sh\nprintf '%s\\n' 'rustc 1.98.1' 'host: x86_64-unknown-linux-gnu' 'release: 1.98.1'\n";

    #[test]
    fn cold_preseed_preflight_needs_prepared_rust_and_not_a_warm_mise_cache() -> TestResult {
        let temp = tempfile::tempdir()?;
        // TMPDIR may itself traverse a symlink (macOS /var); the generated
        // preflight refuses symlinked RUNNER_TEMP by design, so canonicalize.
        let temp_root = std::fs::canonicalize(temp.path())?;
        let state = temp_root.join("empty-mise-state");
        let stub_bin = temp_root.join("stub-bin");
        fs::create_dir_all(&state)?;
        fs::create_dir_all(&stub_bin)?;
        let log = temp_root.join("mise.log");
        let rust_template = temp_root.join("rust-template");
        write_executable(&stub_bin.join("mise"), MISE_STUB)?;
        write_executable(&rust_template, RUSTC_STUB)?;

        let catalog = ToolCatalog::pinned();
        let rust_install = install_step("Prepare pinned tools", vec![PinnedTool::Rust], &catalog)?;
        let preflight = preflight_step(&catalog)?;
        let cold_env = runner_env(
            &temp_root.join("cold-runner"),
            &stub_bin,
            &state,
            &log,
            &rust_template,
        )?;
        assert!(
            !run_step(&preflight, &cold_env)?.status.success(),
            "cold pinned Rust lookup must fail closed"
        );
        let runner_temp = temp_root.join("prepared-runner");
        let ready_env = runner_env(&runner_temp, &stub_bin, &state, &log, &rust_template)?;
        assert!(
            run_step(&rust_install, &ready_env)?.status.success(),
            "the generated pinned Rust preparation must install the tool"
        );
        let ready = run_step(&preflight, &ready_env)?;
        assert!(
            ready.status.success(),
            "preflight after Rust preparation: {ready:?}"
        );
        assert_rust_install_precedes_lookup(&log)?;
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
        runner_temp: &Path,
        stub_bin: &Path,
        state: &Path,
        log: &Path,
        rust_template: &Path,
    ) -> Result<Vec<(&'static str, OsString)>, Box<dyn std::error::Error>> {
        fs::create_dir_all(runner_temp)?;
        let github_path = runner_temp.join("github-path");
        let github_env = runner_temp.join("github-env");
        fs::write(&github_env, "")?;
        let mut paths = vec![stub_bin.to_owned()];
        paths.extend(std::env::split_paths(
            &std::env::var_os("PATH").ok_or("test PATH")?,
        ));
        Ok(vec![
            ("PATH", std::env::join_paths(paths)?),
            ("VELNOR_MISE_STATE", state.as_os_str().to_owned()),
            ("VELNOR_MISE_STUB_LOG", log.as_os_str().to_owned()),
            ("VELNOR_RUST_TEMPLATE", rust_template.as_os_str().to_owned()),
            ("RUNNER_TEMP", runner_temp.as_os_str().to_owned()),
            (
                "MBX_CACHE_DIR",
                runner_temp.join("velnor/mbx").as_os_str().to_owned(),
            ),
            ("GITHUB_PATH", github_path.as_os_str().to_owned()),
            ("GITHUB_ENV", github_env.as_os_str().to_owned()),
            ("GITHUB_RUN_ID", OsString::from("1")),
            ("GITHUB_RUN_ATTEMPT", OsString::from("1")),
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

    fn write_executable(path: &Path, contents: &str) -> std::io::Result<()> {
        fs::write(path, contents)?;
        let mut permissions = fs::metadata(path)?.permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions)
    }

    fn assert_rust_install_precedes_lookup(log: &Path) -> Result<(), Box<dyn std::error::Error>> {
        let events = fs::read_to_string(log)?;
        let lines = events.lines().collect::<Vec<_>>();
        let install = lines
            .iter()
            .position(|line| *line == "install:rust@1.98.1")
            .ok_or("pinned Rust install event")?;
        let lookup = lines
            .iter()
            .rposition(|line| *line == "where:rust@1.98.1")
            .ok_or("final Rust lookup event")?;
        assert!(install < lookup, "install must precede lookup: {events}");
        Ok(())
    }
}
