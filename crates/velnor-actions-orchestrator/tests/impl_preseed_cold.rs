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

    const MISE_STUB: &str = r##"#!/bin/sh
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
"##;
    const MBX_STUB: &str = "#!/bin/sh\nprintf '%s\\n' 'mbx 1.21.1'\n";
    const RUSTC_STUB: &str = "#!/bin/sh\nprintf '%s\\n' 'rustc 1.98.1' 'host: x86_64-unknown-linux-gnu' 'release: 1.98.1'\n";

    #[test]
    fn cargo_only_preseed_installs_mbx_before_cold_preflight() {
        let temp = tempfile::tempdir().expect("temporary runner");
        let state = temp.path().join("empty-mise-state");
        let stub_bin = temp.path().join("stub-bin");
        fs::create_dir_all(&state).expect("empty state");
        fs::create_dir_all(&stub_bin).expect("stub bin");
        let log = temp.path().join("mise.log");
        let runner_temp = temp.path().join("runner-temp");
        fs::create_dir_all(&runner_temp).expect("runner temp");
        let github_path = temp.path().join("github-path");
        let mbx_template = temp.path().join("mbx-template");
        let rust_template = temp.path().join("rust-template");
        write_executable(&stub_bin.join("mise"), MISE_STUB);
        write_executable(&mbx_template, MBX_STUB);
        write_executable(&rust_template, RUSTC_STUB);

        let catalog = ToolCatalog::pinned();
        let rust_install = install_step("Prepare pinned tools", vec![PinnedTool::Rust], &catalog);
        let mbx_install = install_step(
            "Prepare pre-seed MBX",
            vec![PinnedTool::MrBoxington],
            &catalog,
        );
        let preflight = preflight_step(&catalog);
        let steps = [&rust_install, &mbx_install, &preflight];
        assert_eq!(
            steps.map(|step| step.name.as_str()),
            [
                "Prepare pinned tools",
                "Prepare pre-seed MBX",
                "Verify MBX and Rust toolchains"
            ]
        );
        assert!(!shell_run(&rust_install).join(" ").contains("mr-boxington@"));
        assert!(
            shell_run(&mbx_install)
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
        );
        assert!(
            !run_step(&preflight, &env).status.success(),
            "cold MBX lookup must fail"
        );
        assert!(
            run_step(&rust_install, &env).status.success(),
            "Cargo install"
        );
        assert!(
            !run_step(&preflight, &env).status.success(),
            "Rust-only setup must not satisfy pre-seed MBX preflight"
        );
        assert!(
            run_step(&mbx_install, &env).status.success(),
            "pinned MBX install"
        );
        let ready = run_step(&preflight, &env);
        assert!(ready.status.success(), "preflight after install: {ready:?}");
        assert_install_precedes_lookup(&log);
    }

    fn install_step(name: &str, tools: Vec<PinnedTool>, catalog: &ToolCatalog) -> Step {
        let prepare =
            PreparePinnedTools::new(tools, ToolHomes::runner_temp()).expect("typed tool install");
        let run = strings(prepare.argv(catalog));
        let env = strings_map(prepare.env(catalog));
        velnor_actions_workflow_renderer::ambient_shell_step(name, run, env).expect("install step")
    }

    fn preflight_step(catalog: &ToolCatalog) -> Step {
        let uses = format!("jdx/mr-boxington-action@{MR_BOXINGTON_ACTION_SHA}");
        let homes = ToolHomes::runner_temp();
        let env = strings_map(homes.env(catalog));
        let steps = mbx_steps_for_driver(
            &uses,
            CompileDriver::Mbx,
            catalog.version(PinnedTool::MrBoxington),
            catalog.version(PinnedTool::Rust),
            env,
        )
        .expect("preflight steps")
        .expect("MBX driver steps");
        steps[0].clone()
    }

    fn strings(values: Vec<OsString>) -> Vec<String> {
        values
            .into_iter()
            .map(|value| value.into_string().expect("UTF-8 generated argv"))
            .collect()
    }

    fn strings_map(values: Vec<(OsString, OsString)>) -> BTreeMap<String, String> {
        values
            .into_iter()
            .map(|(key, value)| {
                (
                    key.into_string().expect("UTF-8 generated env key"),
                    value.into_string().expect("UTF-8 generated env value"),
                )
            })
            .collect()
    }

    #[expect(clippy::too_many_arguments, reason = "isolated command test inputs")]
    fn runner_env(
        stub_bin: &Path,
        state: &Path,
        log: &Path,
        mbx_template: &Path,
        rust_template: &Path,
        runner_temp: &Path,
        github_path: &Path,
    ) -> Vec<(&'static str, OsString)> {
        let mut paths = vec![stub_bin.to_owned()];
        paths.extend(std::env::split_paths(
            &std::env::var_os("PATH").expect("test PATH"),
        ));
        vec![
            ("PATH", std::env::join_paths(paths).expect("stub PATH")),
            ("VELNOR_MISE_STATE", state.as_os_str().to_owned()),
            ("VELNOR_MISE_STUB_LOG", log.as_os_str().to_owned()),
            ("VELNOR_MBX_TEMPLATE", mbx_template.as_os_str().to_owned()),
            ("VELNOR_RUST_TEMPLATE", rust_template.as_os_str().to_owned()),
            ("RUNNER_TEMP", runner_temp.as_os_str().to_owned()),
            ("GITHUB_PATH", github_path.as_os_str().to_owned()),
        ]
    }

    fn run_step(step: &Step, env: &[(&str, OsString)]) -> Output {
        let StepKind::Shell { run, env: step_env } = &step.kind else {
            panic!("{} must be a shell step", step.name);
        };
        let mut command = Command::new(&run[0]);
        command
            .args(&run[1..])
            .envs(step_env)
            .envs(env.iter().cloned());
        command.output().expect("execute generated step")
    }

    fn shell_run(step: &Step) -> &[String] {
        let StepKind::Shell { run, .. } = &step.kind else {
            panic!("{} must be a shell step", step.name);
        };
        run
    }

    fn write_executable(path: &Path, contents: &str) {
        fs::write(path, contents).expect("write executable");
        let mut permissions = fs::metadata(path)
            .expect("executable metadata")
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions).expect("executable permissions");
    }

    fn assert_install_precedes_lookup(log: &Path) {
        let events = fs::read_to_string(log).expect("mise event log");
        let lines = events.lines().collect::<Vec<_>>();
        let install = lines
            .iter()
            .position(|line| *line == "install:mr-boxington@1.21.1")
            .expect("pinned MBX install event");
        let lookup = lines
            .iter()
            .rposition(|line| *line == "where:mr-boxington@1.21.1")
            .expect("final MBX lookup event");
        assert!(install < lookup, "install must precede lookup: {events}");
    }
}
