use std::error::Error;
use std::fs;
use std::path::Path;

use velnor_actions_contract::StepRole;

use super::{
    CREDENTIAL_UNSET_VARS, Scratch, assert_emitted_boundary, hostile_startup_fixture, poison_path,
    rendered_probe, run_emitted_step, write_executable,
};

#[test]
fn emitted_jactionlint_verifier_filters_startup_inputs_before_mise_wrappers()
-> Result<(), Box<dyn Error>> {
    let root = Scratch::new()?;
    let (marker, bash_env) = hostile_startup_fixture(root.path())?;
    poison_path(root.path(), &marker)?;
    fs::write(root.path().join("fake-jactionlint"), "test bytes\n")?;
    write_mise_bash_probe(root.path())?;
    let hostile_function = format!("() {{ touch '{}'; }}", marker.display());
    let hostile_env = [
        ("BASH_ENV", bash_env.to_str().expect("BASH_ENV path")),
        ("BASH_FUNC_env%%", &hostile_function),
        ("BASH_FUNC_sh%%", &hostile_function),
        ("GITHUB_TOKEN", "runner-overrode-empty-step-env"),
        (
            "ACTIONS_ID_TOKEN_REQUEST_TOKEN",
            "runner-overrode-empty-step-env",
        ),
        (
            "ACTIONS_ID_TOKEN_REQUEST_URL",
            "runner-overrode-empty-step-env",
        ),
        ("ACTIONS_RUNTIME_TOKEN", "runner-overrode-empty-step-env"),
        ("MISE_GITHUB_TOKEN", "runner-overrode-empty-step-env"),
        ("GH_TOKEN", "runner-overrode-empty-step-env"),
        ("GH_HOST", "runner-overrode-empty-step-env"),
        ("GH_CONFIG_DIR", "runner-overrode-empty-step-env"),
        ("RUNNER_OS", "Linux"),
        ("RUNNER_ARCH", "X64"),
    ];
    let script =
        crate::selected_mise_verifier::test_script(velnor_actions_mise::JACTIONLINT_VERSION);
    let step = rendered_probe(StepRole::VerifyJactionlint, &script);
    assert_emitted_boundary(&step);
    for (index, shell) in ["/bin/sh", "/bin/bash"].into_iter().enumerate() {
        let functions_path = root.path().join("mise-functions");
        drop(fs::remove_file(&functions_path));
        let mise_data = root.path().join(format!("selected-mise-{index}"));
        fs::create_dir_all(mise_data.join("bin"))?;
        write_jactionlint_mise_wrapper(&mise_data.join("bin/mise"), shell)?;
        let mise_data = mise_data.to_str().expect("Mise data path").to_owned();
        let mut extra = hostile_env.to_vec();
        extra.push(("MISE_DATA_DIR", &mise_data));
        let output = run_emitted_step(root.path(), &step, &extra)?;
        assert!(!output.status.success(), "fake executable digest must fail");
        assert!(
            String::from_utf8_lossy(&output.stderr)
                .contains("jactionlint_executable_digest_mismatch"),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(!marker.exists(), "hostile startup code must not run");
        assert_eq!(
            fs::read_to_string(functions_path)?,
            "env=missing\nsh=missing\n",
            "selected Mise wrappers must not import hostile functions"
        );
        assert_child_environment_is_scrubbed(root.path())?;
    }
    Ok(())
}

fn write_mise_bash_probe(root: &Path) -> Result<(), Box<dyn Error>> {
    fs::write(
        root.join("mise-bash-probe"),
        r#"if declare -F env >/dev/null; then printf 'env=imported\n' >> "$RUNNER_TEMP/mise-functions"; else printf 'env=missing\n' >> "$RUNNER_TEMP/mise-functions"; fi
if declare -F sh >/dev/null; then printf 'sh=imported\n' >> "$RUNNER_TEMP/mise-functions"; else printf 'sh=missing\n' >> "$RUNNER_TEMP/mise-functions"; fi
/usr/bin/env > "$RUNNER_TEMP/mise-env"
printf '%s\n' "$RUNNER_TEMP/fake-jactionlint"
"#,
    )?;
    Ok(())
}

fn write_jactionlint_mise_wrapper(path: &Path, shell: &str) -> std::io::Result<()> {
    let contents = format!(
        r#"#!{shell}
exec /bin/bash --noprofile --norc "$RUNNER_TEMP/mise-bash-probe"
"#
    );
    write_executable(path, &contents)
}

fn assert_child_environment_is_scrubbed(root: &Path) -> Result<(), Box<dyn Error>> {
    let env = fs::read_to_string(root.join("mise-env"))?;
    for name in CREDENTIAL_UNSET_VARS {
        assert!(
            !env.lines()
                .any(|line| line.starts_with(&format!("{name}=")))
        );
    }
    for name in ["BASH_ENV", "ENV"] {
        assert!(
            !env.lines()
                .any(|line| line.starts_with(&format!("{name}=")))
        );
    }
    assert!(!env.lines().any(|line| line.starts_with("BASH_FUNC_")));
    for name in ["CARGO_REGISTRY_TOKEN", "NPM_TOKEN", "NODE_AUTH_TOKEN"] {
        assert!(env.lines().any(|line| line == format!("{name}=")));
    }
    Ok(())
}
