use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, Output};

use super::super::{ScriptKey, ShellDialect, approved_factory_script, script_file};
use super::{context, shared_fixture};
use crate::{steps, toolchain_env};
use support::{CasePaths, Outcome, OwnedTempDir, case_paths, collect_outcome, new_root};

#[path = "document_shared_scripts_execution_signal.rs"]
mod signal;
#[path = "document_shared_scripts_execution_support.rs"]
mod support;

#[derive(Debug, Clone, Copy)]
enum ScriptRole {
    Preflight,
    VersionCheck,
}

impl ScriptRole {
    fn name(self) -> &'static str {
        match self {
            Self::Preflight => steps::MBX_PREFLIGHT_NAME,
            Self::VersionCheck => steps::MBX_VERSION_CHECK_NAME,
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum CaseMode {
    Success,
    Failure,
    Signal(&'static str),
}

fn factory_body(role: ScriptRole) -> String {
    let ctx = context();
    let fixture = shared_fixture(&ctx);
    let job = fixture.jobs.get("linux").expect("Linux fixture");
    let (index, step) = job
        .steps
        .iter()
        .enumerate()
        .find(|(_, step)| step.name == role.name())
        .expect("factory shell step");
    let key = approved_factory_script(job, index, step).expect("exact source factory body");
    assert_eq!(key.dialect, ShellDialect::Sh);
    key.body
}

fn run_case(role: ScriptRole, source: bool, mode: CaseMode) -> Outcome {
    let mut temp = new_root().expect("create exclusive temporary case");
    let root = temp.path().to_path_buf();
    let paths = case_paths(&root);
    prepare_case(&paths, role, source, mode);
    let body = factory_body(role);
    let command = step_command(&body, source);
    let output = run_process(&paths, &command, mode, &mut temp);
    let outcome = collect_outcome(&paths, output, matches!(mode, CaseMode::Signal(_)));
    temp.cleanup().expect("remove owned temporary case");
    outcome
}

fn prepare_case(paths: &CasePaths, role: ScriptRole, source: bool, mode: CaseMode) {
    fs::create_dir_all(&paths.runner).expect("runner temp directory");
    fs::create_dir_all(&paths.rust_root).expect("fake Rust install");
    fs::create_dir_all(paths.tools.join("bin")).expect("fake tool directory");
    fs::write(&paths.github_env, "").expect("GITHUB_ENV fixture");
    fs::write(&paths.github_path, "").expect("GITHUB_PATH fixture");
    if matches!(role, ScriptRole::VersionCheck) {
        fs::create_dir_all(paths.runner.join("velnor/mbx/actions"))
            .expect("prior MBX action output");
    }
    install_executable(
        &paths.rust_root.join("rustc"),
        if matches!(mode, CaseMode::Failure) && matches!(role, ScriptRole::Preflight) {
            "#!/bin/sh\nprintf 'rustc 1.97.0\\nrelease: 1.97.0\\n'\n"
        } else {
            "#!/bin/sh\nprintf 'rustc 1.98.1\\nrelease: 1.98.1\\n'\n"
        },
    );
    install_executable(&paths.tools.join("bin/mise"), &fake_mise_script());
    if source {
        let key = ScriptKey {
            dialect: ShellDialect::Sh,
            body: factory_body(role),
        };
        let file = script_file(&key, "0.1.0").expect("marked body");
        let script_path = paths.root.join(&file.path);
        fs::create_dir_all(script_path.parent().expect("script parent"))
            .expect("shared script directory");
        fs::write(script_path, file.bytes).expect("shared script file");
    }
}

fn install_executable(path: &Path, body: &str) {
    fs::write(path, body).expect("write fake executable");
    let mut mode = fs::metadata(path)
        .expect("stat fake executable")
        .permissions();
    mode.set_mode(0o700);
    fs::set_permissions(path, mode).expect("set fake executable mode");
}

fn fake_mise_script() -> String {
    "#!/bin/sh\nset -eu\ntrap 'printf HUP > \"$SIGNAL_CAPTURE\"; exit 129' HUP\ntrap 'printf INT > \"$SIGNAL_CAPTURE\"; exit 130' INT\ntrap 'printf TERM > \"$SIGNAL_CAPTURE\"; exit 143' TERM\nif [ \"${GITHUB_TOKEN+x}\" = x ] || [ \"${ACTIONS_RUNTIME_TOKEN+x}\" = x ]; then exit 91; fi\nif [ \"${BLOCK_MISE:-0}\" = 1 ]; then printf '%s\\n' \"$$\" > \"$CHILD_PID_CAPTURE\"; : > \"$READY_CAPTURE\"; while :; do sleep 1; done; fi\ncase \" $* \" in\n  *\" where rust@\"*) printf '%s\\n' \"$RUST_INSTALL_ROOT\" ;;\n  *\" mbx --version \"*) if [ \"${FAIL_MISE_VERSION:-0}\" = 1 ]; then printf '%s\\n' 'mbx 1.0.0'; else printf '%s\\n' 'mbx 1.21.1'; fi ;;\n  *\" mbx cache dir \"*) printf '%s\\n' \"$MBX_CACHE_DIR/actions\" ;;\n  *) printf '%s\\n' 'unexpected mise arguments' >&2; exit 92 ;;\nesac\n".to_owned()
}

fn step_command(body: &str, source: bool) -> String {
    let invocation = if source {
        let file = script_file(
            &ScriptKey {
                dialect: ShellDialect::Sh,
                body: body.to_owned(),
            },
            "0.1.0",
        )
        .expect("marked body");
        format!(". './{}'", file.path)
    } else {
        body.to_owned()
    };
    toolchain_env::with_credential_unset_script(&format!(
        "{invocation}; printf '%s\\n' \"$PWD\" > \"$PWD_CAPTURE\""
    ))
}

fn run_process(
    paths: &CasePaths,
    command: &str,
    mode: CaseMode,
    temp: &mut OwnedTempDir,
) -> Output {
    let mut process = Command::new("sh");
    process
        .arg("-c")
        .arg(command)
        .current_dir(&paths.root)
        .env_clear()
        .env(
            "PATH",
            format!("{}:/usr/bin:/bin", paths.tools.join("bin").display()),
        )
        .env("RUNNER_TEMP", &paths.runner)
        .env("GITHUB_ENV", &paths.github_env)
        .env("GITHUB_PATH", &paths.github_path)
        .env("GITHUB_RUN_ID", "147")
        .env("GITHUB_RUN_ATTEMPT", "3")
        .env("RUSTUP_TOOLCHAIN", "1.98.1")
        .env("MBX_CACHE_DIR", paths.runner.join("velnor/mbx"))
        .env("RUST_INSTALL_ROOT", &paths.rust_root)
        .env("PWD_CAPTURE", &paths.cwd_capture)
        .env("READY_CAPTURE", &paths.ready)
        .env("CHILD_PID_CAPTURE", &paths.child_pid)
        .env("SIGNAL_CAPTURE", &paths.signal_capture)
        .env("GITHUB_TOKEN", "ambient-token")
        .env("ACTIONS_RUNTIME_TOKEN", "ambient-runtime-token")
        .env("CARGO_REGISTRY_TOKEN", "");
    match mode {
        CaseMode::Success => {
            process.env("FAIL_MISE_VERSION", "0").env("BLOCK_MISE", "0");
        }
        CaseMode::Failure => {
            process.env("FAIL_MISE_VERSION", "1").env("BLOCK_MISE", "0");
        }
        CaseMode::Signal(_) => {
            process.env("FAIL_MISE_VERSION", "0").env("BLOCK_MISE", "1");
        }
    }
    if let CaseMode::Signal(signal) = mode {
        return signal::run_signalled(process, paths, signal, temp);
    }
    process.output().expect("run shell step")
}

#[test]
fn sourced_factory_scripts_match_inline_success_and_early_failure() {
    for role in [ScriptRole::Preflight, ScriptRole::VersionCheck] {
        for mode in [CaseMode::Success, CaseMode::Failure] {
            let inline = run_case(role, false, mode);
            let sourced = run_case(role, true, mode);
            assert_eq!(inline, sourced, "{role:?} {mode:?}");
            assert!(!inline.child_survives);
            assert_eq!(inline.cwd_unchanged, matches!(mode, CaseMode::Success));
            if matches!(mode, CaseMode::Success) {
                assert_eq!(inline.status_code, Some(0));
                assert_eq!(inline.signal, None);
            } else {
                assert_eq!(inline.status_code, Some(1));
                assert_eq!(inline.signal, None);
            }
        }
    }
}

#[test]
fn sourced_factory_scripts_match_inline_hup_int_and_term_cleanup() {
    for role in [ScriptRole::Preflight, ScriptRole::VersionCheck] {
        for signal in ["HUP", "INT", "TERM"] {
            let inline = run_case(role, false, CaseMode::Signal(signal));
            let sourced = run_case(role, true, CaseMode::Signal(signal));
            assert_eq!(inline, sourced, "{role:?} {signal}");
            assert!(!inline.cwd_unchanged);
            assert!(!inline.child_survives);
        }
    }
}
