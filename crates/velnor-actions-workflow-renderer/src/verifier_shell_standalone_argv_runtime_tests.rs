use std::collections::BTreeMap;
use std::error::Error;
use std::process::Command;

use velnor_actions_contract::StepRole;

use super::{
    Scratch, assert_emitted_boundary, assert_verifier_output, hostile_startup_fixture, poison_path,
};
use crate::{VERIFY_PATH, yaml::Yaml};

#[test]
fn standalone_argv_ignores_hostile_startup_functions_and_search_path() -> Result<(), Box<dyn Error>>
{
    let root = Scratch::new()?;
    let (marker, bash_env) = hostile_startup_fixture(root.path())?;
    let poison_bin = poison_path(root.path(), &marker)?;
    let body = format!(
        "test \"$PATH\" = \"{VERIFY_PATH}\"; printf '%s\\n' \"$PATH\" > \"$RUNNER_TEMP/observed-path\"; command -v sh > \"$RUNNER_TEMP/observed-sh\"; env > \"$RUNNER_TEMP/observed-env\"; sh -c 'printf \"%s\\n\" nested-shell-ok > \"$RUNNER_TEMP/nested-shell\"'; printf '%s\\n' verifier-ok"
    );
    let step = crate::steps::shell_step(
        crate::VERIFY_RUST_TOOLCHAIN_STEP,
        vec!["sh".to_owned(), "-c".to_owned(), body],
        BTreeMap::new(),
    )?;
    let mut step = step;
    step.role = Some(StepRole::VerifyRustManifest);
    let argv = crate::verifier_composite::verifier_shell_argv(&step)?;
    let yaml = crate::verifier_composite::render_verifier_workflow_step(&step, &BTreeMap::new())?;
    assert_emitted_boundary(&yaml);

    let mut command = Command::new(&argv[0]);
    command
        .args(&argv[1..])
        .current_dir(root.path())
        .env_clear()
        .env("PATH", &poison_bin)
        .env("RUNNER_TEMP", root.path())
        .env("BASH_ENV", &bash_env);
    let hostile_function = format!("() {{ touch '{}'; }}", marker.display());
    for name in ["env", "sh"] {
        command.env(format!("BASH_FUNC_{name}%%"), &hostile_function);
    }
    if let Yaml::Map(fields) = yaml {
        let Yaml::Map(env) = fields
            .iter()
            .find(|(key, _)| key == "env")
            .map(|(_, value)| value)
            .expect("emitted verifier environment")
        else {
            panic!("emitted verifier env is a mapping");
        };
        for (key, value) in env {
            let Yaml::Str(value) = value else {
                panic!("verifier env {key} is not a string");
            };
            command.env(key, value);
        }
    } else {
        panic!("emitted verifier is a mapping");
    }
    for name in crate::toolchain_env::CREDENTIAL_UNSET_VARS {
        command.env(name, "runner-overrode-empty-step-env");
    }
    command.env("PATH", &poison_bin);

    let output = command.output()?;
    assert_verifier_output(root.path(), &marker, output)?;
    Ok(())
}
