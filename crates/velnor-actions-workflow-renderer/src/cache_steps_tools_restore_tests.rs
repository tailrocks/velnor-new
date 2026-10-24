use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use super::TOOLS_CACHE_ADMISSION_SCRIPT;

fn scratch(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "velnor-tools-restore-{name}-{}",
        std::process::id()
    ));
    fs::remove_dir_all(&path).ok();
    fs::create_dir_all(&path).expect("scratch");
    path
}

/// Bytes of the binary Setup Mise installed before the restore runs.
const SETUP_MISE: &str = "verified-mise-binary";

fn sha256(bytes: &str) -> String {
    let root = scratch("sha256");
    let file = root.join("bytes");
    fs::write(&file, bytes).expect("digest input");
    let output = Command::new("sha256sum")
        .arg(&file)
        .output()
        .expect("sha256sum");
    fs::remove_dir_all(root).expect("cleanup digest");
    String::from_utf8(output.stdout).expect("utf-8")[..64].to_owned()
}

fn install_setup_mise(home: &Path) {
    let bin = home.join(".local/share/mise/bin");
    fs::create_dir_all(&bin).expect("Setup Mise bin");
    fs::write(bin.join("mise"), SETUP_MISE).expect("Setup Mise binary");
}

fn populate_cache_paths(root: &Path) -> (PathBuf, PathBuf) {
    let home = root.join("home");
    let runner_temp = root.join("runner-temp");
    install_setup_mise(&home);
    let paths = [
        home.join(".local/share/mise"),
        runner_temp.join("velnor/rustup"),
        runner_temp.join("velnor/cargo/bin"),
    ];
    for path in paths {
        fs::create_dir_all(path).expect("cache directory");
    }
    for path in [
        runner_temp.join("velnor/cargo/.crates.toml"),
        runner_temp.join("velnor/cargo/.crates2.json"),
    ] {
        fs::write(path, "seed-state").expect("cache file");
    }
    (home, runner_temp)
}

fn run_script(
    home: &Path,
    runner_temp: &Path,
    cache_hit: Option<&str>,
    expected_key: Option<&str>,
    matched_key: Option<&str>,
    seed_admitted: Option<&str>,
) -> Output {
    let mut command = Command::new("bash");
    command
        .arg("-c")
        .arg(TOOLS_CACHE_ADMISSION_SCRIPT)
        .env("HOME", home)
        .env("TOOLS_MISE_SHA256", sha256(SETUP_MISE))
        .env("RUNNER_TEMP", runner_temp);
    for (name, value) in [
        ("TOOLS_CACHE_HIT", cache_hit),
        ("TOOLS_EXPECTED_KEY", expected_key),
        ("TOOLS_MATCHED_KEY", matched_key),
        ("TOOLS_SEED_ADMITTED", seed_admitted),
    ] {
        if let Some(value) = value {
            command.env(name, value);
        } else {
            command.env_remove(name);
        }
    }
    command.output().expect("run cache admission")
}

fn assert_paths_kept(home: &Path, runner_temp: &Path) {
    assert!(home.join(".local/share/mise").is_dir());
    assert!(runner_temp.join("velnor/rustup").is_dir());
    assert!(runner_temp.join("velnor/cargo/bin").is_dir());
    assert!(runner_temp.join("velnor/cargo/.crates.toml").is_file());
    assert!(runner_temp.join("velnor/cargo/.crates2.json").is_file());
    assert_eq!(
        fs::read_to_string(runner_temp.join("velnor/cargo/.crates.toml"))
            .expect("cache file bytes"),
        "seed-state"
    );
}

fn assert_paths_removed(home: &Path, runner_temp: &Path) {
    let mise_root = fs::read_dir(home.join(".local/share/mise"))
        .expect("Setup Mise root survives")
        .map(|entry| entry.expect("entry").file_name())
        .collect::<Vec<_>>();
    assert_eq!(mise_root, ["bin"]);
    assert_eq!(
        fs::read_to_string(home.join(".local/share/mise/bin/mise")).expect("Setup Mise binary"),
        SETUP_MISE
    );
    assert!(!runner_temp.join("velnor/rustup").exists());
    assert!(!runner_temp.join("velnor/cargo/bin").exists());
    assert!(!runner_temp.join("velnor/cargo/.crates.toml").exists());
    assert!(!runner_temp.join("velnor/cargo/.crates2.json").exists());
}

fn assert_case(
    name: &str,
    cache_hit: Option<&str>,
    expected_key: Option<&str>,
    matched_key: Option<&str>,
    seed_admitted: Option<&str>,
    keep: bool,
) {
    let root = scratch(name);
    let (home, runner_temp) = populate_cache_paths(&root);
    let output = run_script(
        &home,
        &runner_temp,
        cache_hit,
        expected_key,
        matched_key,
        seed_admitted,
    );
    assert!(output.status.success(), "{output:?}");
    if keep {
        assert_paths_kept(&home, &runner_temp);
    } else {
        assert_paths_removed(&home, &runner_temp);
    }
    fs::remove_dir_all(root).expect("cleanup");
}

#[path = "cache_steps_tools_restore_tests/admission_tests.rs"]
mod admission_tests;

#[path = "cache_steps_tools_restore_contract_tests.rs"]
mod contract_tests;

#[test]
fn rendered_action_wires_seed_admission_and_unset_miss_outputs() {
    let file = super::action_file("0.1.0", &[]).expect("restore action");
    for expected in [
        "seed-admitted:",
        "default: \"false\"",
        "TOOLS_SEED_ADMITTED: ${{ inputs.seed-admitted }}",
        "cache_hit=\\\"${TOOLS_CACHE_HIT-}\\\"",
        "matched_key=\\\"${TOOLS_MATCHED_KEY-}\\\"",
        "elif [ -z \\\"$cache_hit\\\" ]",
        "[ \\\"$seed_admitted\\\" = true ]",
        "mise-sha256:",
        "TOOLS_MISE_SHA256: ${{ inputs.mise-sha256 }}",
    ] {
        assert!(file.bytes.contains(expected), "missing {expected}");
    }
}

#[test]
fn generated_action_tree_scopes_seed_input_to_the_restore_composite() {
    use crate::yaml::Yaml;

    fn field<'a>(node: &'a Yaml, key: &str) -> &'a Yaml {
        let Yaml::Map(entries) = node else {
            panic!("expected mapping containing {key}");
        };
        entries
            .iter()
            .find_map(|(name, value)| (name == key).then_some(value))
            .unwrap_or_else(|| panic!("missing mapping field {key}"))
    }

    let body = super::action_body(&[]).expect("restore action tree");
    let seed_input = field(field(&body, "inputs"), "seed-admitted");
    assert_eq!(field(seed_input, "required"), &Yaml::Bool(false));
    assert_eq!(
        field(seed_input, "default"),
        &Yaml::Quoted("false".to_owned())
    );

    let Yaml::Seq(steps) = field(field(&body, "runs"), "steps") else {
        panic!("composite steps are a sequence");
    };
    assert_eq!(field(&steps[0], "id"), &Yaml::Str("restore".to_owned()));
    let admission_env = field(&steps[1], "env");
    assert_eq!(
        field(admission_env, "TOOLS_SEED_ADMITTED"),
        &Yaml::Str("${{ inputs.seed-admitted }}".to_owned())
    );
}

#[test]
fn restore_call_requires_the_exact_prelude_output_input() {
    let mut wrong = restore_step();
    let velnor_actions_contract::StepKind::Action { with, .. } = &mut wrong.kind else {
        panic!("restore is an action");
    };
    with.insert(
        crate::cache_steps::TOOLS_SEED_ADMITTED_INPUT.to_owned(),
        "caller-value".to_owned(),
    );
    assert!(super::validate_call(&wrong).is_err());

    let mut missing = restore_step();
    let velnor_actions_contract::StepKind::Action { with, .. } = &mut missing.kind else {
        panic!("restore is an action");
    };
    with.remove(crate::cache_steps::TOOLS_SEED_ADMITTED_INPUT);
    assert!(super::validate_call(&missing).is_err());
}

fn restore_step() -> velnor_actions_contract::Step {
    crate::cache_steps::tools_cache_restore_step(
        crate::cache_p08::TOOLS_CACHE_KEY_EXPRESSION,
        &"b".repeat(64),
        Some(crate::cache_p08::TOOLS_CACHE_RESTORE_CONDITION.to_owned()),
    )
    .expect("typed restore step")
}

#[path = "cache_steps_tools_restore_seed_tests.rs"]
mod seed_tests;
